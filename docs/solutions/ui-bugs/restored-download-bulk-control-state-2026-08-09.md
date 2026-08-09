---
title: Derive Bulk Download Controls From Live Pause State
date: 2026-08-09
category: ui-bugs
module: home-screen
problem_type: ui_bug
component: tooling
symptoms:
  - Restored paused downloads show Pause All instead of Resume All
  - Per-transfer pause changes can leave the bulk control stale
root_cause: logic_error
resolution_type: code_fix
severity: medium
related_components:
  - app
  - worker
  - session
tags:
  - pause-resume
  - restored-downloads
  - home-screen
  - ui-state
---

# Derive Bulk Download Controls From Live Pause State

## Problem

Restored downloads correctly entered Home paused, but the global control still displayed `Pause All` because Home stored a separate `all_paused` boolean initialized to `false`.

## Symptoms

- All restored downloads are paused but the global button offers `Pause All`.
- Pausing the last running transfer through an individual control can leave the global control in the wrong state.

## What Didn't Work

- Updating the cached boolean only from bulk pause and resume actions leaves direct restore, individual controls, and lifecycle removals as desynchronization paths.
- Adding another restore-specific boolean assignment would only fix one entry path and would retain the stale-cache design.

## Solution

Remove the cache and derive the bulk action from visible downloads whenever Home renders.

```rust
fn bulk_control_message(&self) -> Message {
    if self.active_downloads.values().all(Download::is_paused) {
        Message::ResumeDownloads
    } else {
        Message::PauseDownloads
    }
}
```

The view maps `ResumeDownloads` to `Resume All`; otherwise it maps to `Pause All`.

## Why This Works

`Download::restore` deliberately pauses each reconstructed download before Home receives it. Evaluating `Download::is_paused()` from Home's current visible transfers reflects that state regardless of whether it came from restore, an individual action, or a bulk action. The nonempty download-list guard means the empty-set behavior is never rendered as a bulk control.

## Prevention

- Derive presentation decisions from authoritative transfer state instead of duplicating lifecycle flags in UI models.
- Test all-paused, all-running, and mixed visible-transfer cases as action enums rather than label text.
- Include restored paused downloads in state-matrix coverage because they enter Home outside the normal runner-active event path.

## Related Issues

- `src/app/screens/home.rs` - Home state and bulk button rendering.
- `src/app/screens/home/tests.rs` - pause-state action matrix.
- `src/worker/mod.rs` - `Download::is_paused` and restore pause behavior.
