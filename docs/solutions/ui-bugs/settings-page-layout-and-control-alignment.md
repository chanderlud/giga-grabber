---
title: Fix Settings Page Layout and Control Alignment
date: 2026-08-09
category: ui-bugs
module: settings
problem_type: ui_bug
component: tooling
severity: medium
symptoms:
  - "Settings rows and spacers spread across the entire window instead of staying in a compact form column"
  - "The settings scrollbar appeared inside the content instead of at the far-right edge of the pane"
  - "Checkbox controls were not aligned with their labels and the Theme row used a different left inset"
  - "The manual update action was presented inline with the automatic-update checkbox"
root_cause: scope_issue
resolution_type: code_fix
related_components:
  - app
  - iced
  - config
tags:
  - iced
  - settings-ui
  - layout
  - scrollable
  - checkbox
  - ui-customization
---

# Fix Settings Page Layout and Control Alignment

## Problem

The Settings screen needed to add Network, General, and UI Customization sections without losing the compact control-column layout. Iterative fixes exposed a sizing conflict: the scrollbar needed the full content pane width, while the actual settings rows needed a stable maximum width.

## Symptoms

- A fixed-width outer settings column kept the scrollbar well inside a wide window rather than at the pane edge.
- Making the entire settings column fill available width moved the scrollbar correctly but caused row spacers to expand with the window, pushing fixed-width controls to the far edge.
- Checkbox builders with embedded labels placed the checkboxes before the text instead of matching the label-left/control-right layout used by other settings rows.
- Checkbox controls sat high in their rows until the row used vertical alignment.
- The Theme row started without the 8px inset used by sliders, pick lists, and checkbox rows.
- The manual update button was initially combined with the automatic-update checkbox and used the shorter label `Check now`.

## What Didn't Work

- **Fixed-width outer pane:** Constraining the whole Settings view to `Length::Fixed(350_f32)` kept controls compact, but the scrollbar belonged to that same narrow element and therefore did not reach the right edge of the available pane.
- **Full-width control column:** Changing the outer column to `Length::Fill` fixed scrollbar placement, but its child rows also resolved their flexible spacers against the full window width. Fixed-width controls were pushed to the far edge, leaving excessive separation and a visually stretched form.
- **Labeled checkbox widgets:** Using `checkbox(value).label(label)` preserved the default widget composition, but it could not produce the required label-left/control-right arrangement.
- **Coordinate-only visual debugging:** During this session, the native Iced app launched under Xvfb, but reliable automated navigation from Home to Settings was unavailable in this environment. The final change was therefore validated with source inspection and automated Rust checks rather than an unverified rendered Settings capture.

## Solution

Keep the scrollable viewport full width, but cap only the content column that contains the settings sections:

```rust
const SETTINGS_CONTENT_MAX_WIDTH: f32 = 350.0;

scrollable(
    Column::new().width(Length::Fill).spacing(16).push(
        container(
            Column::new()
                .width(Length::Fill)
                .push(network_settings)
                .push(general_settings)
                .push(ui_customization),
        )
        .width(Length::Fill)
        .max_width(SETTINGS_CONTENT_MAX_WIDTH),
    ),
)
.width(Length::Fill)
.height(Length::Fill)
```

This keeps the scrollbar attached to the full pane while resolving each settings row against the capped inner column. Keep Save/Apply/Reset outside the scrollable.

Use a shared row shape for checkbox settings instead of embedding labels in the checkbox widget:

```rust
Row::new()
    .height(Length::Fixed(30_f32))
    .align_y(Alignment::Center)
    .push(space::horizontal().width(Length::Fixed(8_f32)))
    .push(text(label).align_y(Vertical::Center).height(Length::Fill))
    .push(space::horizontal())
    .push(checkbox(is_checked).on_toggle(on_toggle))
```

Give Theme the same initial inset, and keep the manual update action as a separate right-aligned button row:

```rust
Row::new()
    .height(Length::Fixed(30_f32))
    .push(space::horizontal())
    .push(
        button("Check for updates")
            .width(Length::Fixed(170_f32))
            .on_press(Message::CheckForUpdates),
    )
```

The layout and widget-message implementation lives in `src/app/screens/settings.rs:239-459`; `src/app.rs:409-420` converts the resulting `Action::CheckForUpdates` into the existing manual update task.

## Why This Works

Scrollable geometry and content geometry have separate responsibilities. The scrollable must fill the available pane so its scrollbar is positioned at the pane boundary. The inner content container must be capped so flexible row spacers resolve against a predictable form width instead of the entire window.

The checkbox helper makes alignment explicit: the label stays at the left inset, a spacer consumes remaining row space, and the unlabeled checkbox stays at the right control edge. `Row::align_y(Alignment::Center)` centers the checkbox's intrinsic height against the text row. Applying the same 8px inset to Theme removes the last one-off row geometry.

## Prevention

- Treat scrollable viewport width and form content width as separate layout decisions in Iced screens.
- Keep a named max-width constant for compact settings forms rather than repeating an anonymous fixed width.
- Use one helper for repeated label-left/control-right checkbox rows so those settings remain consistent with sibling pick-list and slider geometry.
- Add a manual wide-window check whenever a settings pane combines a right-edge scrollbar with fixed-width controls.
- Preserve a characterization test for action-only settings messages, such as `Message::CheckForUpdates -> Action::CheckForUpdates`, while changing layout.
- For native UI QA, prefer a deterministic route/startup hook or a framework-specific inspector. Do not infer rendered success from a launch-only smoke test.

## Related Issues

- `../design-patterns/iced-update-check-settings-layout-2026-06-10.md` covers the earlier update-check flow. This learning supersedes its recommendation to keep the automatic checkbox and manual action in one row, while extending the remaining guidance with scroll ownership, max-width geometry, and control alignment.
- `src/app/screens/settings.rs` is the source of truth for the current layout and message bindings.
