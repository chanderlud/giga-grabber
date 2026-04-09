use super::fake::{DriverAction, FakeDriver};
use super::{Download, PauseState, RunnerMessage, spawn_workers};
use crate::MegaFile;
use crate::config::Config;
use crate::mega_client::Node;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc::{Receiver, channel};
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

fn make_download(name: &str, size: u64, dir: PathBuf) -> Download {
    let node = Node::test_file(format!("handle-{name}"), name.to_string(), size);
    let file = MegaFile::new(node, dir);
    Download::new(&file)
}

fn make_driver(actions: Vec<DriverAction>) -> FakeDriver {
    FakeDriver::new(VecDeque::from(actions))
}

async fn next_message(receiver: &mut Receiver<RunnerMessage>) -> RunnerMessage {
    timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("message timeout")
        .expect("message channel closed")
}

async fn wait_for_paused(download: &Download) {
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
        if matches!(next_message(receiver).await, RunnerMessage::Inactive(_)) {
            inactive += 1;
        }
    }
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
        message_sender,
        cancel.clone(),
        1,
    );

    download_sender
        .send(download.clone())
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active(_)
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive(_)
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
        message_sender,
        cancel.clone(),
        1,
    );

    download_sender
        .send(download)
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive(_)
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
        message_sender,
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
        message_sender,
        cancel.clone(),
        1,
    );

    download_sender
        .send(download.clone())
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active(_)
    ));
    download.stop.cancel();
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive(_)
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
        message_sender,
        cancel.clone(),
        1,
    );

    download_sender
        .send(download.clone())
        .await
        .expect("enqueue download");
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active(_)
    ));

    wait_for_paused(&download).await;
    assert!(download.is_paused());

    download.resume();

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active(_)
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive(_)
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
        message_sender,
        cancel.clone(),
        1,
    );

    download_sender
        .send(download)
        .await
        .expect("enqueue download");

    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active(_)
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active(_)
    ));
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Inactive(_)
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
        message_sender,
        cancel.clone(),
        1,
    );

    download_sender
        .send(download.clone())
        .await
        .expect("enqueue download");
    assert!(matches!(
        next_message(&mut message_receiver).await,
        RunnerMessage::Active(_)
    ));
    wait_for_paused(&download).await;
    download.stop.cancel();

    let maybe_msg = timeout(Duration::from_millis(300), message_receiver.recv()).await;
    assert!(maybe_msg.is_err(), "unexpected message after pause-cancel");

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

    let mut config = Config::default();
    config.max_retries = 3;
    config.min_retry_delay = Duration::ZERO;
    config.max_retry_delay = Duration::from_millis(1);

    let workers = spawn_workers(
        driver.clone(),
        Arc::new(config),
        download_receiver,
        download_sender.clone(),
        message_sender,
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

    let mut config = Config::default();
    config.max_retries = 2;
    config.min_retry_delay = Duration::ZERO;
    config.max_retry_delay = Duration::from_millis(1);

    let workers = spawn_workers(
        driver.clone(),
        Arc::new(config),
        download_receiver,
        download_sender.clone(),
        message_sender,
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
            RunnerMessage::Error(error) => {
                if error.contains("Max retries reached") {
                    saw_max_retries_error = true;
                }
            }
            RunnerMessage::Inactive(_) => saw_inactive = true,
            RunnerMessage::Active(_) => (),
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
        message_sender,
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

    let mut config = Config::default();
    config.concurrency_budget = 10;
    let workers = spawn_workers(
        driver,
        Arc::new(config),
        download_receiver,
        download_sender.clone(),
        message_sender,
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
            RunnerMessage::Active(_) => {
                active_count += 1;
                max_active = max_active.max(active_count);
            }
            RunnerMessage::Inactive(_) => {
                active_count = active_count.saturating_sub(1);
                inactive_count += 1;
            }
            RunnerMessage::Error(_) => (),
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
        message_sender,
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
        message_sender,
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
