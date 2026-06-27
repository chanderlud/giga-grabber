---
title: MEGA 509 Bandwidth Limit Pauses Downloads Without Retry
date: 2026-06-27
category: integration-issues
module: mega-download-worker
problem_type: integration_issue
component: service_object
symptoms:
  - "MEGA file downloads retried after HTTP 509 Bandwidth Limit Exceeded"
  - "Log showed HTTP status server error (509 Bandwidth Limit Exceeded) for url ()"
  - "The GUI error log showed a generic MEGA file download HTTP error instead of a clear quota message"
root_cause: logic_error
resolution_type: code_fix
severity: high
related_components:
  - worker
  - session
  - gui
  - cli
  - testing
tags:
  - mega
  - bandwidth-limit
  - http-509
  - retry
  - pause
  - worker-events
  - session-core
---

# MEGA 509 Bandwidth Limit Pauses Downloads Without Retry

## Problem

Giga Grabber treated MEGA's HTTP `509 Bandwidth Limit Exceeded` response like any other HTTP status failure. That made the worker retry a daily quota condition that cannot be fixed by retrying, while the GUI showed only a generic download error instead of telling the user they were out of bandwidth.

## Symptoms

- Runtime logs included `HTTP status server error (509 Bandwidth Limit Exceeded) for url ()`.
- The worker logged `Error downloading file: MEGA file download HTTP error`.
- The failed download was requeued through the normal retry path.
- Active downloads kept running or waiting as though this were a transient per-file failure.

## What Didn't Work

- Leaving the failure as a generic `reqwest::Response::error_for_status()` error did not preserve enough domain meaning for the worker to choose a different lifecycle path.
- Handling the condition only in the GUI would have required string matching on presentation text and would not have fixed CLI behavior or worker retry semantics.
- Treating the error as "max retries reached" was misleading because the daily transfer limit is not solved by exhausting retry attempts.

## Solution

Classify the HTTP status while the MEGA file-download response is still available, before converting it into a generic reqwest status error:

```rust
let response = result.context("MEGA file download request failed")?;
if is_out_of_bandwidth_status(response.status()) {
    return Err(OutOfBandwidthError.into());
}
response.error_for_status().context("MEGA file download HTTP error")?
```

The worker now recognizes `OutOfBandwidthError` before the generic retry branch. It pauses the affected download, emits a typed runner message, waits for resume or cancellation, and does not increment retry count:

```rust
if error.downcast_ref::<OutOfBandwidthError>().is_some() {
    download.pause();
    download.mark_paused_if_requested();
    message_sender
        .send(RunnerMessage::OutOfBandwidth {
            session_id,
            error: error.to_string(),
        })
        .await?;
    // wait for resume or cancellation, then requeue
}
```

The session core handles the typed runner event once for all surfaces:

```rust
RunnerMessage::OutOfBandwidth { session_id, error } if session_id == self.id => {
    self.pause_all_transfers();
    events.push(SessionEvent::OutOfBandwidth(error));
}
```

The GUI maps `SessionEvent::OutOfBandwidth` to the existing modal system and the home screen pause-all behavior. The CLI prints the clear error and avoids finishing with a misleading "Download complete" message.

## Why This Works

The key is preserving domain intent at the layer that has the HTTP status code. Once `.error_for_status()` wraps the response, downstream code only sees a generic error string. By converting status `509` into a typed `OutOfBandwidthError`, the worker can choose a non-retry lifecycle path without brittle string matching.

Putting the surface-facing behavior through `RunnerMessage::OutOfBandwidth` and `SessionEvent::OutOfBandwidth` keeps the existing session-core pattern intact: worker messages describe transfer lifecycle facts, `TransferSession` owns shared lifecycle rules such as pausing all tracked downloads, and GUI/CLI only decide presentation.

## Prevention

- Classify provider-specific HTTP statuses before converting responses into generic status errors.
- Use typed worker/session events for lifecycle-changing conditions; avoid matching on formatted error strings in UI code.
- Keep non-retryable quota conditions out of the normal retry branch so retry counters and "max retries" messages stay meaningful.
- Add integration-style regression tests around the real HTTP client path when provider behavior depends on status codes.

The regression coverage added for this fix uses a local HTTP fixture that returns `HTTP/1.1 509 Bandwidth Limit Exceeded` for the file download URL. The test asserts that the worker emits `RunnerMessage::OutOfBandwidth`, pauses the download, and makes only one file request despite retries being configured.

A session-level regression test also sends `RunnerMessage::OutOfBandwidth` through `TransferSession` and asserts every tracked download is paused. That protects the shared GUI/CLI lifecycle rule without requiring a full UI test.

## Related Issues

- GitHub issue #20: MEGA bandwidth-limit responses should not retry and should pause downloads.
- PR #30: `https://github.com/chanderlud/giga-grabber/pull/30`
- `docs/solutions/best-practices/session-centered-transfer-core-2026-04-18.md` - related shared session-core pattern; this fix applies that pattern to a new worker event.
