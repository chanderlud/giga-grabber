---
title: Centralize Transfer Lifecycle in a Shared Session Core
date: 2026-04-18
category: best-practices
module: session-core
problem_type: best_practice
component: service_object
severity: medium
related_components:
  - cli
  - gui
  - worker
  - testing
applies_when:
  - CLI and GUI need to share transfer lifecycle rules without duplicating state logic
  - New download behavior affects queueing, draining, pause or retry semantics
  - Focused lifecycle tests are needed without full surface integration
tags:
  - session
  - transfer-lifecycle
  - cli-gui-parity
  - worker-events
  - testability
---

# Centralize Transfer Lifecycle in a Shared Session Core

## Context

`giga-grabber` previously spread downloader lifecycle behavior across the GUI app, CLI flow, and worker plumbing. That made core rules such as "when is a session still alive?", "what counts as queued versus active?", and "how do new transfers join in-flight work?" harder to reason about and easier to drift between surfaces.

The `session-core` work introduced one explicit in-memory transfer session so CLI and GUI could consume the same lifecycle model while keeping restart persistence and broader flow redesign out of scope for v1.

## Guidance

Put lifecycle ownership in a dedicated core type and make surfaces react to its events instead of owning parallel state machines.

In this repo, that core is `TransferSession` in `src/session.rs`. It owns queue state, worker lifetime, and translation from worker messages into surface-facing session events:

```rust
pub(crate) enum SessionEvent {
    TransferActive(Download),
    TransferTerminal(String),
    Error(String),
    Drained,
}
```

`add_downloads()` makes append behavior idempotent per handle and starts workers only when needed. `handle_runner_message()` is the single place where worker activity becomes `TransferActive`, `TransferTerminal`, `Error`, or `Drained`.

The worker layer now includes `session_id` on per-download runner messages so a fresh session can safely ignore stale events from an older one:

```rust
pub(crate) enum RunnerMessage {
    Active { session_id: u64, download: Download },
    Inactive { session_id: u64, handle: String },
    Error { session_id: u64, error: String },
    Finished,
}
```

With that contract in place:

- `src/cli.rs` creates one `TransferSession`, feeds it downloads, and renders progress from returned `SessionEvent`s.
- `src/app.rs` stores `Option<TransferSession<MegaClient>>`, appends newly chosen downloads into the current session, normally clears it after `SessionEvent::Drained`, and also aborts it immediately when the user explicitly stops work.
- `src/session.rs` tests the lifecycle directly instead of pushing every scenario through full CLI or GUI integration.

## Why This Matters

This pattern keeps lifecycle rules defined once. New behavior around append, drain, pause, retry, or shutdown can be implemented in the session core and immediately apply to both surfaces.

It also makes the codebase safer to change. The session tests cover the scenarios that were previously implicit: appending into a running session, ignoring stale worker messages, keeping paused work alive, and snapshotting config for the life of a session. That gives future refactors a tighter feedback loop than surface-level manual checks alone.

## When to Apply

- When both CLI and GUI need the same underlying execution semantics but different presentation layers.
- When worker messages are starting to leak domain rules into multiple call sites.
- When a new feature would otherwise add one more surface-specific flag or lifecycle branch.
- When you need to test queue and drain behavior without booting the full app.

## Examples

Before this pattern, the natural tendency is to let each surface decide when work starts, when it ends, and how queued items behave. That creates subtle divergence over time.

After this pattern, surfaces stay thin and react to session events:

```rust
while let Some(msg) = message_receiver.recv().await {
    for event in session.handle_runner_message(msg) {
        match event {
            SessionEvent::TransferActive(download) => pb.println(format!(
                "→ {} ({})",
                download.node.name,
                format_size(download.node.size)
            )),
            SessionEvent::Error(err) => pb.println(format!("Error: {err}")),
            SessionEvent::Drained => break,
            SessionEvent::TransferTerminal(_) => {}
        }
    }
}
```

The same pattern shows up in the GUI path, where `App::handle_runner_message()` updates `home` state from `SessionEvent`s and clears the session only after drain.

The tests in `src/session.rs` are part of the guidance too. If a lifecycle rule matters, add coverage there first so both surfaces inherit the behavior.

## Related

- `docs/brainstorms/2026-04-18-session-centered-core-requirements.md` - requirements and scope boundaries for the refactor
- `docs/ideation/2026-04-18-open-ideation.md` - earlier "engine-first core with thin surfaces" direction that this work realized
- `src/session.rs` - session ownership, event mapping, and lifecycle tests
- `src/worker/mod.rs` - session-scoped runner messages
- `src/cli.rs` and `src/app.rs` - thin surface integrations over the shared core
