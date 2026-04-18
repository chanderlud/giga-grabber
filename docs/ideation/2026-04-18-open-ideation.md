---
date: 2026-04-18
topic: open
mode: repo-grounded
---

# Ideation: Open Improvements for Giga Grabber

## Grounding Context

### Codebase Context
- Compact Rust app with `tokio`, `reqwest`, `clap`, and optional/default `iced` GUI.
- One crate currently mixes downloader logic, worker orchestration, CLI, GUI screens/components/styles, and settings.
- Top-level docs are product-facing and light on architecture, failure modes, and development workflow.

### Past Learnings
- No formal `docs/solutions/` or prior ideation docs were present.
- The strongest implicit values in the repo are resumability, retries/backoff, worker-state correctness, config recovery, and CLI/GUI parity.

### External Context
- Strong prior art points to engine/control-plane separation, staged link intake, repair-first partial recovery, and clearer performance/reliability tradeoffs.
- Adjacent ideas from `aria2`, `Motrix`, `JDownloader`, `rclone`, and AWS CRT-style transfer recovery reinforced those directions.

## Ranked Ideas

### 1. Engine-first core with thin surfaces
**Description:** Split downloader/session logic from CLI and GUI so both surfaces talk to one clearer engine/control plane.
**Rationale:** This is the highest-leverage direction because it makes future parity, testing, automation, and reliability work cheaper instead of repeatedly threading logic through two entry points.
**Downsides:** High migration risk and easy to over-engineer if done too early.
**Confidence:** 88%
**Complexity:** High
**Status:** Explored

### 2. Durable transfer journal / intent ledger
**Description:** Persist queued/running intent, retry history, and recovery context rather than relying mostly on in-memory runtime state plus `.partial` files.
**Rationale:** Directly strengthens the repo’s existing promise around crash recovery and resumability, and creates a foundation for better UX and debugging.
**Downsides:** Durable state design is easy to get wrong and needs careful corruption handling.
**Confidence:** 84%
**Complexity:** High
**Status:** Unexplored

### 3. Repair-first partial recovery
**Description:** Inspect and heal damaged or incomplete spans of partial files rather than falling back to coarse rewind/retry behavior.
**Rationale:** This is the sharpest product-level reliability improvement in the app’s core differentiator.
**Downsides:** Protocol and integrity-check complexity may be substantial.
**Confidence:** 79%
**Complexity:** High
**Status:** Unexplored

### 4. Staged link intake inbox
**Description:** Add a pre-download workspace for inspecting, deduping, labeling, and batch-shaping links before they become active jobs.
**Rationale:** Fits the current import/choose-files flow and reduces accidental work before live worker state is involved.
**Downsides:** Adds more UI/UX surface area and could feel slower for simple downloads if forced too early.
**Confidence:** 82%
**Complexity:** Medium
**Status:** Unexplored

### 5. Reliability/performance profiles
**Description:** Replace raw tuning knobs with named operating modes like balanced, aggressive, fragile-network, or resume-first, while keeping advanced controls available.
**Rationale:** A pragmatic improvement that helps both CLI and GUI quickly by making existing power easier to understand.
**Downsides:** Profiles can become hand-wavy unless they are grounded in real workloads.
**Confidence:** 86%
**Complexity:** Medium
**Status:** Unexplored

### 6. Explainability and diagnostics layer
**Description:** Expose why a transfer is waiting, retrying, paused, resumed, stalled, or recovered in one coherent model across surfaces.
**Rationale:** Makes the existing reliability machinery easier to trust and debug, while lowering support cost.
**Downsides:** Can turn noisy if the explanation model is too granular.
**Confidence:** 83%
**Complexity:** Medium
**Status:** Unexplored

## Rejection Summary

| # | Idea | Reason Rejected |
|---|------|-----------------|
| 1 | Adaptive scheduling modes | Strong idea, but less urgent than fixing observability and durable state first; better as a later optimization layer. |
| 2 | Architecture + failure-mode handbook | Valuable, but mostly a byproduct of stronger structural/reliability work rather than a top standalone direction. |
| 3 | Crash recovery as a product surface | Overlaps heavily with the durable transfer journal plus diagnostics ideas. |
| 4 | Shared capability model for CLI and GUI | Useful, but largely subsumed by the engine-first core direction. |
| 5 | Portable download manifests | Interesting but premature relative to the more foundational journal/intake directions. |
| 6 | Background download service | Bold, but too expensive relative to likely near-term value for the current repo shape. |
| 7 | Recovery command center | Combination idea that mostly repackages stronger underlying survivors. |
| 8 | Policy-driven engine | Combination idea that overlaps with engine-first core and profiles. |
| 9 | Manifest-based staged intake | Combination idea that overlaps with staged intake plus portable manifests. |
