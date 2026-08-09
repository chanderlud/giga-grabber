---
title: Preserve Iced Session Snapshots on Native Window Close
date: 2026-08-09
category: ui-bugs
module: app-shutdown
problem_type: ui_bug
component: tooling
symptoms:
  - Native window close exits without creating download-session.json
  - Active downloads are absent after reopening the GUI
root_cause: wrong_api
resolution_type: code_fix
severity: high
related_components:
  - app
  - session-persistence
  - iced
tags:
  - iced
  - window-close
  - session-persistence
  - async-shutdown
---

# Preserve Iced Session Snapshots on Native Window Close

## Problem

The GUI queued a download-session snapshot from `window::close_requests()`, but closing through the native title-bar control exited before that subscription could run.

## Symptoms

- A saved persistence setting and active download still produced no `download-session.json` file on normal window close.
- Reopening the application had no download record to restore.

## What Didn't Work

- Verifying only the saved `persist_download_sessions` configuration did not explain the missing file. The setting was enabled, but the close message was never delivered.
- Scheduling `Task::perform` from the existing close-message branch could not help while the native event bypassed the branch entirely.

## Solution

Disable Iced's automatic exit on close requests, then let the existing message chain save the snapshot before explicitly closing the window.

```rust
iced::application(App::new, App::update, App::view)
    .subscription(App::subscription)
    .exit_on_close_request(false)
```

`App::update` receives `CloseRequested`, starts the session save task, and calls `close_window()` only after `CloseSnapshotFinished(Ok(()))`. `close_window()` then requests the final Iced window close.

## Why This Works

Iced 0.14 enables `exit_on_close_request` by default. With that setting enabled, its native runner closes the window directly instead of publishing the close event that `window::close_requests()` listens for. Setting it to `false` preserves the event for the application subscription, allowing the asynchronous write to finish before the app issues its own close command.

## Prevention

- For any Iced close handler that must flush state, set `exit_on_close_request(false)` on the application builder.
- Keep shutdown ordering explicit: close request, durable write, completion message, then `window::close`.
- Test the persisted artifact through a native-close smoke test when desktop GUI automation is available: save `persist_download_sessions = true`, queue an accepted unfinished transfer, close the native window, wait for process exit, verify `download-session.json`, relaunch, then verify a paused restored row.

## Related Issues

- `src/app.rs` - application builder and close-message state machine.
- `src/session_persistence.rs` - atomic session-file writer.
- `docs/solutions/design-patterns/iced-update-check-settings-layout-2026-06-10.md` - related Iced `Task::perform` guidance.
