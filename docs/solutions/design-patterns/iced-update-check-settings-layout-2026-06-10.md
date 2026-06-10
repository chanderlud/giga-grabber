---
title: Iced Update-Check Flow and Settings Layout
date: 2026-06-10
category: design-patterns
module: update_check
problem_type: design_pattern
component: tooling
severity: medium
related_components:
  - app
  - settings
  - config
  - release-checking
applies_when:
  - Adding automatic or manual update checks to an Iced app
  - Exposing update preferences and a manual check action in settings
  - Showing an update modal with a browser-open action for the release page
tags:
  - iced
  - update-check
  - settings
  - modal
  - layout
  - reqwest
---

# Iced Update-Check Flow and Settings Layout

## Context

Issue chanderlud/giga-grabber#26 added latest-version checking to the Rust/Iced GUI. The feature looked simple at first: call GitHub, compare the latest tag to the installed version, notify the user, and expose a setting. In practice, the durable learning was about keeping the flow split across the right layers so the UI stayed predictable.

The final branch separates the work into four concerns:

- `src/update_check.rs` owns GitHub release fetching and version comparison.
- `src/config.rs` persists the user's automatic-check preference without breaking existing `config.json` files.
- `src/app.rs` schedules async checks, decides which results are silent or visible, and owns modal state.
- `src/app/screens/settings.rs` and `src/app/components/error_modal.rs` present the settings controls and update modal.

The branch also surfaced a few practical pitfalls: Iced 0.14 uses `checkbox(state).label(...).on_toggle(...)`, not a label-first checkbox helper; update-available should be modeled as structured app state rather than folded into generic error text; and a raw release URL in modal copy is less useful than a clear `Open release` button.

## Guidance

Keep release-checking logic out of the Iced view layer. The checker should return typed outcomes, and the app should convert those outcomes into UI state.

```rust
pub(crate) enum UpdateStatus {
    Available(ReleaseInfo),
    Current,
}

pub(crate) async fn check_latest_release() -> Result<UpdateStatus> {
    let release: GitHubRelease = reqwest::Client::new()
        .get(LATEST_RELEASE_API_URL)
        .header(USER_AGENT, concat!("giga-grabber/", env!("CARGO_PKG_VERSION")))
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(status_for_versions(env!("CARGO_PKG_VERSION"), release))
}
```

Use `Task::perform` as the bridge between async work and app messages. Carry whether a check was manual so startup checks can stay quiet while user-triggered checks report current/error states.

```rust
fn check_for_updates(manual: bool) -> Task<Message> {
    Task::perform(
        async {
            update_check::check_latest_release()
                .await
                .map_err(UpdateCheckError::new)
        },
        move |result| Message::UpdateCheckFinished { manual, result },
    )
}
```

Handle the result in app state instead of pushing every outcome through one string modal. Available updates need structured release data because the modal has an action; current/error states can remain plain messages.

```rust
match result {
    Ok(UpdateStatus::Available(release)) => {
        self.update_release = Some(release);
    }
    Ok(UpdateStatus::Current) if manual => {
        self.error_modal = Some("Giga Grabber is up to date".to_string());
    }
    Err(error) if manual => {
        self.error_modal = Some(format!("Failed to check for updates: {error}"));
    }
    Ok(UpdateStatus::Current) | Err(_) => {}
}
```

For settings layout, place the update preference and manual action together as one final settings row above the generic Save/Reset controls. Keep the button fixed-width so the label does not wrap or jitter.

```rust
Row::new()
    .height(Length::Fixed(30_f32))
    .push(space::horizontal().width(Length::Fixed(8_f32)))
    .push(
        checkbox(self.config.check_for_updates)
            .label("Automatically check for updates")
            .on_toggle(Message::CheckForUpdatesChanged),
    )
    .push(space::horizontal())
    .push(
        button("Check now")
            .width(Length::Fixed(120_f32))
            .style(styles::button::primary)
            .on_press(Message::CheckForUpdates),
    )
```

For update-available UI, use a dedicated modal shape with explicit actions. The modal should say that a version is available, then offer `Open release` and `Ok`; it should not display the URL as body text.

```rust
if let Some(release) = &self.update_release {
    error_modal::update_modal(
        &release.version,
        body.into(),
        Message::OpenUrl(release.url.clone()),
        Message::CloseModal,
    )
} else if let Some(error_message) = &self.error_modal {
    error_modal::error_modal(error_message, body.into()).map(|_| Message::CloseModal)
}
```

Open release pages with a platform-specific command when avoiding a new dependency is preferable:

```rust
#[cfg(target_os = "macos")]
fn browser_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
    command
}

#[cfg(target_os = "windows")]
fn browser_command(url: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn browser_command(url: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
}
```

For persisted settings, add a serde default and keep `Default` in sync. This avoids breaking existing JSON config files that were written before the field existed.

```rust
#[cfg(feature = "gui")]
#[serde(default = "default_check_for_updates")]
pub(crate) check_for_updates: bool,
```

## Why This Matters

This pattern keeps a background update feature understandable as it crosses several boundaries: HTTP, version comparison, config persistence, async GUI scheduling, settings layout, modal presentation, and OS integration. If any of those concerns are collapsed together, later UI polish becomes harder: a text-only error modal cannot easily grow an `Open release` action, and a settings checkbox inserted mid-form reads like another tuning value instead of a final app-level preference.

The manual/automatic distinction matters too. Automatic startup checks should not annoy users when GitHub is unreachable or when the installed version is current. Manual checks should always respond because the user explicitly asked for status.

Finally, Iced API details are easy to misremember. In Iced 0.14, checkbox labels are configured with a builder-style `.label(...)`; buttons get their action through `.on_press(...)`; and async work returns to the app through `Task::perform`. Capturing those exact shapes prevents small layout and compile-time detours the next time a settings control is added.

## When to Apply

- When adding background HTTP checks to an Iced app.
- When a settings toggle controls whether an automatic action runs at startup.
- When a manual settings action should reuse the same async code path but report more status than the automatic path.
- When a modal needs an action button tied to structured app state rather than plain text.
- When adding fields to persisted config that existing users already have on disk.

## Examples

### Separate typed status from presentation

Avoid returning UI strings from the release checker. Return `UpdateStatus`, then let `App::update` decide whether that status should be silent, an error modal, or an actionable update modal.

```rust
pub(crate) enum UpdateStatus {
    Available(ReleaseInfo),
    Current,
}
```

### Keep UI messages cloneable without `String`-only errors

Iced messages are cloned through widget trees, so wrapping async errors in a small cloneable `Display` type keeps the app message clean without losing error text.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateCheckError {
    message: String,
}

impl UpdateCheckError {
    pub(crate) fn new(error: impl Display) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}
```

### Put update controls where users expect app-level preferences

The update checkbox and manual action belong together as the last settings row, not between existing transfer/proxy tuning controls and not split between the form body and the bottom Save/Reset row.

```rust
.push(self.proxy_selector())
.push(space::vertical().height(Length::Fixed(8_f32)))
.push(update_controls_row)
.push(space::vertical().height(Length::Fill))
.push(save_reset_row)
```

### Prefer an actionable modal over raw URLs

The update modal is a distinct UI state because it needs both version text and a release action. Generic error modals can still remain string-based.

```rust
button(" Open release ")
    .style(button::primary)
    .on_press(Message::OpenUrl(release.url.clone()))
```

## Related

- chanderlud/giga-grabber#26 - original feature request for latest-version checking, notification, settings toggle, and manual check button.
- `src/update_check.rs` - GitHub API fetch, `v`-prefix normalization, version comparison, and tests.
- `src/config.rs` - persisted `check_for_updates` defaulting.
- `src/app.rs` and `src/app/helpers.rs` - Iced task routing, app-owned modal state, and message definitions.
- `src/app/screens/settings.rs` - final-row settings placement and fixed-width action button.
- `src/app/components/error_modal.rs` - existing generic modal plus update-specific modal.
- `docs/solutions/best-practices/session-centered-transfer-core-2026-04-18.md` - adjacent pattern for keeping surfaces thin over shared logic; overlap is low, but the same principle applies here.

External references that informed the captured pattern:

- Iced 0.14 `Task::perform`: https://docs.iced.rs/iced/struct.Task.html
- Iced 0.14 checkbox API: https://docs.rs/iced/0.14.0/iced/widget/struct.Checkbox.html
- Iced 0.14 release notes for checkbox `.label(...)` and `Task::perform` `FnOnce`: https://github.com/iced-rs/iced/releases/tag/0.14.0
- Rust `std::process::Command`: https://doc.rust-lang.org/std/process/struct.Command.html
