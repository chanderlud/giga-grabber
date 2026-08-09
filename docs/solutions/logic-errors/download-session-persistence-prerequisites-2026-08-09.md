---
title: Persist Accepted Transfers and Retain Retryable Restore Records
date: 2026-08-09
category: logic-errors
module: session-persistence
problem_type: logic_error
component: tooling
symptoms:
  - No session file is retained when no current transfer or retryable restore record remains
  - Session restoration skips stale completed files and invalid source nodes
root_cause: missing_workflow_step
resolution_type: code_fix
severity: medium
related_components:
  - app
  - session
  - worker
  - config
tags:
  - session-persistence
  - download-restore
  - graceful-shutdown
  - working-directory
---

# Persist Accepted Transfers and Retain Retryable Restore Records

## Problem

Download-session persistence must save enough provenance to reconstruct unfinished GUI work, while avoiding records for rejected, completed, or cancelled transfers. A failed source refetch initially retains its record for a later restore attempt, unless bulk cancellation or a later session drain clears it.

## Symptoms

- A session file is intentionally absent when the active-record set is empty.
- A saved record may not restore when its final output file already exists, its source cannot be fetched, or its node metadata no longer matches.

## What Didn't Work

- Treating the settings checkbox as immediate state is incorrect. The runtime shutdown gate changes after the settings save action succeeds.
- Treating a selected download as persistable is incorrect. A record is valid only after `TransferSession::add_downloads` accepts its handle.
- Persisting progress in JSON is unnecessary. Resume progress comes from the partial output file when `Download::restore` reconstructs the download.

## Solution

Maintain `DownloadSessionRecord` entries for accepted GUI downloads, snapshot those records on graceful close, and remove records as transfers become terminal or the session drains. When a source refetch fails during restore, retain its record for a later launch unless bulk cancellation or a later session drain clears it. Store only source URL, node handle, destination directory, and expected size.

```rust
let records: Vec<_> = self.download_records.values().cloned().collect();
if records.is_empty() {
    session_persistence::remove()
} else {
    session_persistence::save(&records)
}
```

On startup, load the records, refetch their public source nodes, validate the node handle, kind, and expected size, then call `Download::restore`. That constructor pauses valid restored downloads and derives displayed progress from the partial file length.

## Why This Works

The session file represents durable transfer identity, not volatile worker state. Accepted handles prevent records for rejected queue entries. Terminal and drained cleanup prevent replaying completed work, while a failed source refetch initially retains a retryable record. Restore verifies the stored handle, file kind, and expected size before reconstructing a download. The partial file remains the source of truth for resumable progress.

Both `config.json` and `download-session.json` are relative paths, so they are resolved from the GUI process working directory. Inspect the actual launch directory when a local file appears missing.

## Prevention

- Save the settings form before expecting the runtime persistence gate to change.
- Add records only after queue acceptance; remove them on terminal transfer events and drain, and document that retryable restore records are also cleared by bulk cancellation or a later drain.
- Restore downloads paused, then require an explicit resume before transfer work begins.
- Cover serialization round trips, missing and corrupt session files, queue acceptance provenance, and paused restored downloads.

## Related Issues

- `src/session_persistence.rs` - JSON session format and atomic replacement.
- `src/app.rs` - record ownership, shutdown snapshot, and restore orchestration.
- `src/worker/mod.rs` - paused restoration and partial-file progress recovery.
- `docs/solutions/best-practices/session-centered-transfer-core-2026-04-18.md` - shared transfer lifecycle ownership.
