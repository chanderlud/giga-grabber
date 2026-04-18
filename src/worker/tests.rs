use super::fake::{DriverAction, FakeDriver};
use super::{Download, PauseState, RunnerMessage, spawn_workers};
use crate::MegaFile;
use crate::config::Config;
use crate::mega_client::{MegaClient, Node};
use aes::Aes128;
use cipher::{KeyIvInit, StreamCipher};
use ctr::Ctr128BE;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{Receiver, channel};
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;
use url::Url;

pub(crate) fn make_download(name: &str, size: u64, dir: PathBuf) -> Download {
    let node = Node::test_file(format!("handle-{name}"), name.to_string(), size);
    let file = MegaFile::new(node, dir);
    Download::new(&file)
}

fn make_driver(actions: Vec<DriverAction>) -> FakeDriver {
    FakeDriver::new(VecDeque::from(actions))
}

pub(crate) async fn next_message(receiver: &mut Receiver<RunnerMessage>) -> RunnerMessage {
    timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("message timeout")
        .expect("message channel closed")
}

pub(crate) async fn wait_for_paused(download: &Download) {
    timeout(Duration::from_secs(3), async {
        loop {
            if download.pause_state() == PauseState::Paused {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("pause state was never reached");
}

async fn wait_for_inactive(receiver: &mut Receiver<RunnerMessage>, expected: usize) {
    let mut inactive = 0usize;
    while inactive < expected {
        if matches!(next_message(receiver).await, RunnerMessage::Inactive { .. }) {
            inactive += 1;
        }
    }
}

pub(crate) async fn wait_for_driver_calls(driver: &FakeDriver, expected: usize) {
    timeout(Duration::from_secs(3), async {
        loop {
            if driver.call_count() >= expected {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("driver call count timeout");
}

fn encrypt_ctr_zero_key(payload: &[u8]) -> Vec<u8> {
    let mut encrypted = payload.to_vec();
    let key = [0_u8; 16];
    let iv = [0_u8; 16];
    let mut ctr = Ctr128BE::<Aes128>::new((&key).into(), (&iv).into());
    ctr.apply_keystream(&mut encrypted);
    encrypted
}

#[derive(Clone)]
struct FixtureState {
    saw_non_zero_range: Arc<AtomicBool>,
    download_requests: Arc<AtomicUsize>,
    phase: Arc<AtomicUsize>,
}

async fn wait_for_fixture_phase(state: &FixtureState, target: usize) {
    timeout(Duration::from_secs(5), async {
        loop {
            if state.phase.load(Ordering::SeqCst) >= target {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture phase timeout");
}

async fn wait_for_downloaded_at_least(download: &Download, min_bytes: usize) {
    timeout(Duration::from_secs(5), async {
        loop {
            if download.downloaded.load(Ordering::Relaxed) >= min_bytes {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("downloaded bytes threshold timeout");
}

async fn spawn_local_mega_fixture(
    encrypted_payload: Vec<u8>,
) -> (
    Url,
    FixtureState,
    CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local fixture");
    let addr = listener.local_addr().expect("fixture local addr");
    let base_url = Url::parse(&format!("http://{addr}/")).expect("fixture base URL");

    let state = FixtureState {
        saw_non_zero_range: Arc::new(AtomicBool::new(false)),
        download_requests: Arc::new(AtomicUsize::new(0)),
        phase: Arc::new(AtomicUsize::new(0)),
    };
    let state_for_task = state.clone();
    let shutdown = CancellationToken::new();
    let shutdown_for_task = shutdown.clone();

    let server = tokio::spawn(async move {
        loop {
            let accept_result = tokio::select! {
                _ = shutdown_for_task.cancelled() => break,
                result = listener.accept() => result,
            };
            let Ok((mut socket, _)) = accept_result else {
                break;
            };

            let mut request = Vec::new();
            let mut buf = [0_u8; 2048];
            let header_end = loop {
                let read = match socket.read(&mut buf).await {
                    Ok(0) => break None,
                    Ok(n) => n,
                    Err(_) => break None,
                };
                request.extend_from_slice(&buf[..read]);
                if let Some(idx) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(idx + 4);
                }
            };
            let Some(header_end) = header_end else {
                continue;
            };

            let request_head = String::from_utf8_lossy(&request[..header_end]);
            let mut lines = request_head.split("\r\n");
            let request_line = lines.next().unwrap_or_default();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or_default();
            let path = parts.next().unwrap_or_default();

            let mut range_start = 0usize;
            let mut content_length = 0usize;
            for line in lines {
                if line.is_empty() {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("range")
                {
                    let header_val = value.trim();
                    if let Some(raw_start) = header_val
                        .strip_prefix("bytes=")
                        .and_then(|rest| rest.split('-').next())
                    {
                        range_start = raw_start.parse::<usize>().unwrap_or(0);
                    }
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse::<usize>().unwrap_or(0);
                }
            }

            let body_read = request.len().saturating_sub(header_end);
            if content_length > body_read {
                let mut remaining = content_length - body_read;
                while remaining > 0 {
                    let read = match socket.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    remaining = remaining.saturating_sub(read);
                }
            }

            if method == "POST" && path.starts_with("/cs") {
                let cs_body = format!(
                    r#"[{{"g":"http://{addr}/file","s":{},"at":"QQ"}}]"#,
                    encrypted_payload.len()
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    cs_body.len(),
                    cs_body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                continue;
            }

            if method != "GET" || path != "/file" {
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }

            let request_index = state_for_task
                .download_requests
                .fetch_add(1, Ordering::SeqCst);
            if range_start > 0 {
                state_for_task
                    .saw_non_zero_range
                    .store(true, Ordering::SeqCst);
            }

            let start = range_start.min(encrypted_payload.len());
            let payload = &encrypted_payload[start..];
            let status = if start > 0 {
                "206 Partial Content"
            } else {
                "200 OK"
            };
            let mut headers = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
                payload.len()
            );
            if start > 0 {
                let end = encrypted_payload.len().saturating_sub(1);
                headers.push_str(&format!(
                    "Content-Range: bytes {start}-{end}/{}\r\n",
                    encrypted_payload.len()
                ));
            }
            headers.push_str("\r\n");

            if request_index == 0 {
                state_for_task.phase.store(1, Ordering::SeqCst);
                sleep(Duration::from_millis(500)).await;
            }

            let _ = socket.write_all(headers.as_bytes()).await;

            if request_index == 1 {
                state_for_task.phase.store(2, Ordering::SeqCst);

                let first_chunk = payload.len().min(1_200_000);
                let _ = socket.write_all(&payload[..first_chunk]).await;
                let _ = socket.flush().await;
                state_for_task.phase.store(3, Ordering::SeqCst);
                sleep(Duration::from_millis(500)).await;
                let _ = socket.write_all(&payload[first_chunk..]).await;
                let _ = socket.flush().await;
            } else {
                let _ = socket.write_all(payload).await;
                let _ = socket.flush().await;
            }
        }
    });

    (base_url, state, shutdown, server)
}

#[tokio::test]
async fn test_real_mega_client_pause_during_send_and_stream_then_resume_and_complete() {
    let temp = TempDir::new().expect("temp dir");
    let expected_plain: Vec<u8> = (0..1_300_000).map(|i| (i % 251) as u8).collect();
    let encrypted_payload = encrypt_ctr_zero_key(&expected_plain);

    let (base_url, fixture_state, fixture_shutdown, fixture_task) =
        spawn_local_mega_fixture(encrypted_payload).await;

    let download = make_download(
        "real-client-pause.bin",
        expected_plain.len() as u64,
        temp.path().to_path_buf(),
    );
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(64);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());
    let mega = MegaClient::with_origin(reqwest::Client::new(), base_url);

    let workers = spawn_workers(
        mega,
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download.clone())
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));

    wait_for_fixture_phase(&fixture_state, 1).await;
    download.pause();
    wait_for_paused(&download).await;
    download.resume();

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));

    wait_for_fixture_phase(&fixture_state, 3).await;
    // Ensure the partial file is large enough to force a non-zero resume range.
    wait_for_downloaded_at_least(&download, 1_100_000).await;
    download.pause();
    wait_for_paused(&download).await;
    download.resume();

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive { .. }
    ));

    let full_path = download.file_path.join(&download.node.name);
    let contents = tokio::fs::read(&full_path)
        .await
        .expect("read completed file");
    assert_eq!(contents, expected_plain);
    assert!(
        fixture_state.saw_non_zero_range.load(Ordering::SeqCst),
        "expected resumed GET request with non-zero Range offset"
    );

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }

    fixture_shutdown.cancel();
    timeout(Duration::from_secs(2), fixture_task)
        .await
        .expect("fixture task join timeout")
        .expect("fixture task join failure");
}

#[tokio::test]
async fn test_single_download_completes() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("single.bin", 1024, temp.path().to_path_buf());
    let driver = make_driver(vec![DriverAction::Complete]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(32);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());

    let workers = spawn_workers(
        driver.clone(),
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download.clone())
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive { .. }
    ));

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_download_already_complete() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("already.bin", 2048, temp.path().to_path_buf());
    let file_path = download.file_path.join(&download.node.name);
    tokio::fs::create_dir_all(&download.file_path)
        .await
        .expect("create dir");
    tokio::fs::write(&file_path, b"done")
        .await
        .expect("create existing file");

    let driver = make_driver(vec![DriverAction::Complete]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(32);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());
    let workers = spawn_workers(
        driver.clone(),
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download)
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive { .. }
    ));
    assert_eq!(driver.call_count(), 0);

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_cancel_before_active() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("cancel-before.bin", 1024, temp.path().to_path_buf());
    download.stop.cancel();

    let driver = make_driver(vec![DriverAction::Complete]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(32);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());
    let workers = spawn_workers(
        driver.clone(),
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download)
        .await
        .expect("enqueue download");
    sleep(Duration::from_millis(150)).await;
    assert!(message_receiver.try_recv().is_err());
    assert_eq!(driver.call_count(), 0);

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_cancel_while_active() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("cancel-active.bin", 1024, temp.path().to_path_buf());
    let driver = make_driver(vec![DriverAction::Hang]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(32);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());
    let workers = spawn_workers(
        driver,
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download.clone())
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));
    download.stop.cancel();
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive { .. }
    ));

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_pause_and_resume() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("pause-resume.bin", 1024, temp.path().to_path_buf());
    let driver = make_driver(vec![DriverAction::Pause, DriverAction::Complete]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(64);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());
    let workers = spawn_workers(
        driver,
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download.clone())
        .await
        .expect("enqueue download");
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));

    wait_for_paused(&download).await;
    assert!(download.is_paused());

    download.resume();

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive { .. }
    ));

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_quick_pause_then_single_resume_requeues_and_completes() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("quick-pause-resume.bin", 1024, temp.path().to_path_buf());
    let driver = make_driver(vec![
        DriverAction::PauseThenQuickResume,
        DriverAction::Complete,
    ]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(64);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());
    let workers = spawn_workers(
        driver.clone(),
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download)
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive { .. }
    ));
    assert_eq!(driver.call_count(), 2);

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_pause_then_cancel() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("pause-cancel.bin", 1024, temp.path().to_path_buf());
    let driver = make_driver(vec![DriverAction::Pause]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(32);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());
    let workers = spawn_workers(
        driver,
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download.clone())
        .await
        .expect("enqueue download");
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));
    wait_for_paused(&download).await;
    download.stop.cancel();

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive { .. }
    ));

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_cancel_during_pause_requeue_emits_single_inactive_and_clears_active() {
    let temp = TempDir::new().expect("temp dir");
    let requeued = make_download("pause-requeue-cancel.bin", 1024, temp.path().to_path_buf());
    let blocker = make_download("pause-requeue-blocker.bin", 1024, temp.path().to_path_buf());
    let requeued_handle = requeued.node.handle.clone();
    let blocker_handle = blocker.node.handle.clone();
    let driver = make_driver(vec![DriverAction::PauseThenQuickResume, DriverAction::Hang]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(64);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());
    let workers = spawn_workers(
        driver.clone(),
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(requeued.clone())
        .await
        .expect("enqueue requeued download");
    download_sender
        .send(blocker.clone())
        .await
        .expect("enqueue blocker download");

    let mut active_handles = HashSet::new();
    let mut requeued_inactive = 0usize;
    let mut blocker_inactive = 0usize;
    let mut saw_requeued_active = false;
    let mut saw_blocker_active = false;

    while !(saw_requeued_active && saw_blocker_active) {
        match next_message(&mut message_receiver).await {
            RunnerMessage::Active { download, .. } => {
                if download.node.handle == requeued_handle {
                    saw_requeued_active = true;
                }
                if download.node.handle == blocker_handle {
                    saw_blocker_active = true;
                }
                active_handles.insert(download.node.handle);
            }
            RunnerMessage::Inactive { handle, .. } => {
                if handle == requeued_handle {
                    requeued_inactive += 1;
                } else if handle == blocker_handle {
                    blocker_inactive += 1;
                }
                active_handles.remove(&handle);
            }
            RunnerMessage::Error { .. } => (),
            #[cfg(feature = "gui")]
            RunnerMessage::Finished => (),
        }
    }

    wait_for_driver_calls(&driver, 2).await;
    requeued.stop.cancel();
    blocker.stop.cancel();

    timeout(Duration::from_secs(3), async {
        while requeued_inactive < 1 || blocker_inactive < 1 {
            match next_message(&mut message_receiver).await {
                RunnerMessage::Active { download, .. } => {
                    active_handles.insert(download.node.handle);
                }
                RunnerMessage::Inactive { handle, .. } => {
                    if handle == requeued_handle {
                        requeued_inactive += 1;
                    } else if handle == blocker_handle {
                        blocker_inactive += 1;
                    }
                    active_handles.remove(&handle);
                }
                RunnerMessage::Error { .. } => (),
                #[cfg(feature = "gui")]
                RunnerMessage::Finished => (),
            }
        }
    })
    .await
    .expect("expected inactive messages for canceled downloads");

    assert_eq!(requeued_inactive, 1);
    assert!(
        !active_handles.contains(&requeued_handle),
        "requeued task remained active after cancel"
    );
    assert!(
        !active_handles.contains(&blocker_handle),
        "blocker task remained active after cancel"
    );
    assert!(
        active_handles.is_empty(),
        "all active entries should be cleared after cancel handling"
    );

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_retry_on_error() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("retry-success.bin", 1024, temp.path().to_path_buf());
    let driver = make_driver(vec![
        DriverAction::Fail("first".to_string()),
        DriverAction::Fail("second".to_string()),
        DriverAction::Complete,
    ]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(64);
    let cancel = CancellationToken::new();

    let config = Config {
        max_retries: 3,
        min_retry_delay: Duration::ZERO,
        max_retry_delay: Duration::from_millis(1),
        ..Default::default()
    };

    let workers = spawn_workers(
        driver.clone(),
        Arc::new(config),
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download)
        .await
        .expect("enqueue download");
    wait_for_inactive(&mut message_receiver, 1).await;
    assert_eq!(driver.call_count(), 3);

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_cancel_during_retry_requeue_emits_single_inactive_and_clears_active() {
    let temp = TempDir::new().expect("temp dir");
    let requeued = make_download("retry-requeue-cancel.bin", 1024, temp.path().to_path_buf());
    let blocker = make_download("retry-requeue-blocker.bin", 1024, temp.path().to_path_buf());
    let requeued_handle = requeued.node.handle.clone();
    let blocker_handle = blocker.node.handle.clone();
    let driver = make_driver(vec![
        DriverAction::Fail("retry me".to_string()),
        DriverAction::Hang,
    ]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(64);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config {
        min_retry_delay: Duration::ZERO,
        max_retry_delay: Duration::ZERO,
        ..Default::default()
    });
    let workers = spawn_workers(
        driver.clone(),
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(requeued.clone())
        .await
        .expect("enqueue requeued download");
    download_sender
        .send(blocker.clone())
        .await
        .expect("enqueue blocker download");

    let mut active_handles = HashSet::new();
    let mut requeued_inactive = 0usize;
    let mut blocker_inactive = 0usize;
    let mut saw_requeued_active = false;
    let mut saw_blocker_active = false;

    while !(saw_requeued_active && saw_blocker_active) {
        match next_message(&mut message_receiver).await {
            RunnerMessage::Active { download, .. } => {
                if download.node.handle == requeued_handle {
                    saw_requeued_active = true;
                }
                if download.node.handle == blocker_handle {
                    saw_blocker_active = true;
                }
                active_handles.insert(download.node.handle);
            }
            RunnerMessage::Inactive { handle, .. } => {
                if handle == requeued_handle {
                    requeued_inactive += 1;
                } else if handle == blocker_handle {
                    blocker_inactive += 1;
                }
                active_handles.remove(&handle);
            }
            RunnerMessage::Error { .. } => (),
            #[cfg(feature = "gui")]
            RunnerMessage::Finished => (),
        }
    }

    wait_for_driver_calls(&driver, 2).await;
    requeued.stop.cancel();
    blocker.stop.cancel();

    timeout(Duration::from_secs(3), async {
        while requeued_inactive < 1 || blocker_inactive < 1 {
            match next_message(&mut message_receiver).await {
                RunnerMessage::Active { download, .. } => {
                    active_handles.insert(download.node.handle);
                }
                RunnerMessage::Inactive { handle, .. } => {
                    if handle == requeued_handle {
                        requeued_inactive += 1;
                    } else if handle == blocker_handle {
                        blocker_inactive += 1;
                    }
                    active_handles.remove(&handle);
                }
                RunnerMessage::Error { .. } => (),
                #[cfg(feature = "gui")]
                RunnerMessage::Finished => (),
            }
        }
    })
    .await
    .expect("expected inactive messages for canceled downloads");

    assert_eq!(requeued_inactive, 1);
    assert!(
        !active_handles.contains(&requeued_handle),
        "requeued task remained active after cancel"
    );
    assert!(
        !active_handles.contains(&blocker_handle),
        "blocker task remained active after cancel"
    );
    assert!(
        active_handles.is_empty(),
        "all active entries should be cleared after cancel handling"
    );

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_rename_failure_is_reported_and_not_marked_inactive_immediately() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("rename-fail.bin", 1024, temp.path().to_path_buf());
    let driver = make_driver(vec![DriverAction::CompleteWithoutPartial]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(64);
    let cancel = CancellationToken::new();

    let config = Config {
        max_retries: 1,
        min_retry_delay: Duration::ZERO,
        max_retry_delay: Duration::from_millis(1),
        ..Default::default()
    };

    let workers = spawn_workers(
        driver,
        Arc::new(config),
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download)
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active { .. }
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Error { .. }
    ));

    let mut saw_max_retries_error = false;
    let mut saw_inactive = false;
    while !(saw_max_retries_error && saw_inactive) {
        match next_message(&mut message_receiver).await {
            RunnerMessage::Error { error, .. } => {
                if error.contains("Max retries reached") {
                    saw_max_retries_error = true;
                }
            }
            RunnerMessage::Inactive { .. } => saw_inactive = true,
            RunnerMessage::Active { .. } => (),
            #[cfg(feature = "gui")]
            RunnerMessage::Finished => (),
        }
    }

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_max_retries_exceeded() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("retry-fail.bin", 1024, temp.path().to_path_buf());
    let driver = make_driver(vec![
        DriverAction::Fail("first".to_string()),
        DriverAction::Fail("second".to_string()),
        DriverAction::Fail("third".to_string()),
    ]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(64);
    let cancel = CancellationToken::new();

    let config = Config {
        max_retries: 2,
        min_retry_delay: Duration::ZERO,
        max_retry_delay: Duration::from_millis(1),
        ..Default::default()
    };

    let workers = spawn_workers(
        driver.clone(),
        Arc::new(config),
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    download_sender
        .send(download)
        .await
        .expect("enqueue download");

    let mut saw_max_retries_error = false;
    let mut saw_inactive = false;
    while !(saw_max_retries_error && saw_inactive) {
        match next_message(&mut message_receiver).await {
            RunnerMessage::Error { error, .. } => {
                if error.contains("Max retries reached") {
                    saw_max_retries_error = true;
                }
            }
            RunnerMessage::Inactive { .. } => saw_inactive = true,
            RunnerMessage::Active { .. } => (),
            #[cfg(feature = "gui")]
            RunnerMessage::Finished => (),
        }
    }
    assert_eq!(driver.call_count(), 2);

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_global_cancel_stops_all_workers() {
    let temp = TempDir::new().expect("temp dir");
    let driver = make_driver(vec![
        DriverAction::Hang,
        DriverAction::Hang,
        DriverAction::Hang,
    ]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, _message_receiver) = channel(64);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());

    let workers = spawn_workers(
        driver,
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        2,
    );

    for i in 0..3 {
        let d = make_download(
            &format!("global-cancel-{i}.bin"),
            1024,
            temp.path().to_path_buf(),
        );
        download_sender.send(d).await.expect("enqueue download");
    }

    sleep(Duration::from_millis(150)).await;
    cancel.cancel();

    for worker in workers {
        timeout(Duration::from_secs(3), worker)
            .await
            .expect("worker join timeout")
            .expect("worker join panic")
            .expect("worker result");
    }
}

#[tokio::test]
async fn test_concurrency_semaphore_limits() {
    let temp = TempDir::new().expect("temp dir");
    let driver = make_driver(vec![
        DriverAction::Complete,
        DriverAction::Complete,
        DriverAction::Complete,
        DriverAction::Complete,
        DriverAction::Complete,
    ]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(128);
    let cancel = CancellationToken::new();

    let config = Config {
        concurrency_budget: 10,
        ..Default::default()
    };
    let workers = spawn_workers(
        driver,
        Arc::new(config),
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        2,
    );

    for i in 0..5 {
        let d = make_download(
            &format!("weighted-{i}.bin"),
            200 * 1024 * 1024,
            temp.path().to_path_buf(),
        );
        download_sender.send(d).await.expect("enqueue download");
    }

    let mut active_count = 0usize;
    let mut max_active = 0usize;
    let mut inactive_count = 0usize;
    while inactive_count < 5 {
        match next_message(&mut message_receiver).await {
            RunnerMessage::Active { .. } => {
                active_count += 1;
                max_active = max_active.max(active_count);
            }
            RunnerMessage::Inactive { .. } => {
                active_count = active_count.saturating_sub(1);
                inactive_count += 1;
            }
            RunnerMessage::Error { .. } => (),
            #[cfg(feature = "gui")]
            RunnerMessage::Finished => (),
        }
    }
    assert_eq!(max_active, 1);

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_pause_resume_race_no_lost_wakeup() {
    let temp = TempDir::new().expect("temp dir");
    let download = make_download("race.bin", 1024, temp.path().to_path_buf());
    let driver = make_driver(vec![DriverAction::Pause, DriverAction::Complete]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(128);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());

    let workers = spawn_workers(
        driver,
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        1,
    );

    let stop_racer = Arc::new(AtomicBool::new(false));
    let race_download = download.clone();
    let stop_racer_clone = stop_racer.clone();
    let racer = tokio::spawn(async move {
        while !stop_racer_clone.load(Ordering::Relaxed) {
            race_download.pause();
            race_download.resume();
            tokio::task::yield_now().await;
        }
        // Ensure we do not leave the worker parked in Paused.
        race_download.resume();
    });

    download_sender
        .send(download)
        .await
        .expect("enqueue download");
    wait_for_inactive(&mut message_receiver, 1).await;
    stop_racer.store(true, Ordering::Relaxed);
    racer.await.expect("racer join");

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}

#[tokio::test]
async fn test_multiple_workers_drain_queue() {
    let temp = TempDir::new().expect("temp dir");
    let driver = make_driver(vec![
        DriverAction::Complete,
        DriverAction::Complete,
        DriverAction::Complete,
        DriverAction::Complete,
        DriverAction::Complete,
        DriverAction::Complete,
    ]);
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel(128);
    let cancel = CancellationToken::new();
    let config = Arc::new(Config::default());

    let workers = spawn_workers(
        driver,
        config,
        download_receiver,
        download_sender.clone(),
        (message_sender, 0),
        cancel.clone(),
        3,
    );

    for i in 0..6 {
        let d = make_download(&format!("drain-{i}.bin"), 1024, temp.path().to_path_buf());
        download_sender.send(d).await.expect("enqueue download");
    }

    wait_for_inactive(&mut message_receiver, 6).await;

    cancel.cancel();
    for worker in workers {
        worker.await.expect("worker join").expect("worker result");
    }
}
