use crate::config::Config;
use crate::mega_client::MegaClient;
use crate::{MegaFile, WorkerHandle};
use log::error;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::fs::{create_dir_all, rename, try_exists};
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, RwLock, Semaphore, watch};
use tokio::time::{Instant, sleep};
use tokio::{select, spawn};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PauseState {
    Running,
    PauseRequested,
    Paused,
}

#[derive(Debug, Clone)]
pub(crate) struct Download {
    pub(crate) node: crate::mega_client::Node,
    pub(crate) file_path: PathBuf,
    pub(crate) downloaded: Arc<AtomicUsize>,
    start: Arc<RwLock<Option<Instant>>>,
    pub(crate) stop: CancellationToken,
    pause_state: Arc<watch::Sender<PauseState>>,
    retries: Arc<AtomicU32>,
    last_tried_at: Arc<Mutex<Option<Instant>>>,
}

impl Download {
    pub(crate) fn new(file: &MegaFile) -> Self {
        let (pause_state, _) = watch::channel(PauseState::Running);
        Self {
            node: file.node.clone(),
            file_path: file.file_path.clone(),
            downloaded: Default::default(),
            start: Default::default(),
            stop: Default::default(),
            pause_state: Arc::new(pause_state),
            retries: Default::default(),
            last_tried_at: Default::default(),
        }
    }
}

impl Download {
    pub(crate) async fn start(&self) {
        self.start.write().await.replace(Instant::now());
    }

    pub(crate) async fn set_retried(&self) {
        self.retries.fetch_add(1, Ordering::Relaxed);
        self.last_tried_at.lock().await.replace(Instant::now());
    }

    #[cfg(feature = "gui")]
    pub(crate) fn progress(&self) -> f32 {
        if self.node.size == 0 {
            return 0.0;
        }
        (self.downloaded.load(Ordering::Relaxed) as f32 / self.node.size as f32).clamp(0.0, 1.0)
    }

    #[cfg(feature = "gui")]
    pub(crate) fn speed(&self) -> f32 {
        if self.is_paused() {
            return 0_f32;
        }

        if let Some(start) = self.start.blocking_read().as_ref() {
            let elapsed = start.elapsed().as_secs_f32();
            if elapsed <= 0.0 {
                return 0_f32;
            }
            (self.downloaded.load(Ordering::Relaxed) as f32 / elapsed) / 1_048_576.0
        } else {
            0_f32
        }
    }

    pub(crate) fn cancel(&self) {
        self.stop.cancel();
    }

    pub(crate) fn pause(&self) {
        let _ = self.pause_state.send_if_modified(|state| {
            if *state == PauseState::Running {
                *state = PauseState::PauseRequested;
                return true;
            }
            false
        });
    }

    pub(crate) fn resume(&self) {
        let _ = self.pause_state.send_replace(PauseState::Running);
    }

    pub(crate) fn is_paused(&self) -> bool {
        matches!(
            self.pause_state(),
            PauseState::PauseRequested | PauseState::Paused
        )
    }

    pub(crate) fn pause_receiver(&self) -> watch::Receiver<PauseState> {
        self.pause_state.subscribe()
    }

    pub(crate) fn pause_state(&self) -> PauseState {
        *self.pause_state.borrow()
    }

    pub(crate) fn mark_paused_if_requested(&self) -> bool {
        let _ = self.pause_state.send_if_modified(|state| {
            if *state == PauseState::PauseRequested {
                *state = PauseState::Paused;
                return true;
            }
            false
        });

        matches!(
            self.pause_state(),
            PauseState::PauseRequested | PauseState::Paused
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) enum RunnerMessage {
    /// notifies UI that this download has become active
    Active(Download),
    /// notifies the UI that this download is finished
    Inactive(String),
    /// notifies the UI when non-critical errors bubble up
    Error(String),
    /// may be emitted during shutdown
    Finished,
}

pub(crate) enum RetryDecision {
    Wait,
    TryNow,
    GiveUp,
}

pub(crate) trait DownloadDriver: Send + Sync + Clone + 'static {
    fn download_file(
        &self,
        download: &Download,
        dest_path: &Path,
    ) -> impl Future<Output = anyhow::Result<bool>> + Send;
}

impl DownloadDriver for MegaClient {
    fn download_file(
        &self,
        download: &Download,
        dest_path: &Path,
    ) -> impl Future<Output = anyhow::Result<bool>> + Send {
        MegaClient::download_file(self, download, dest_path)
    }
}

/// spawns worker tasks
pub(crate) fn spawn_workers<D: DownloadDriver>(
    client: D,
    config: Arc<Config>,
    receiver: kanal::AsyncReceiver<Download>,
    download_sender: kanal::AsyncSender<Download>,
    message_sender: Sender<RunnerMessage>,
    cancellation_token: CancellationToken,
    workers: usize,
) -> Vec<WorkerHandle> {
    let budget = config.weighted_concurrency_budget();
    let concurrency_sem = Arc::new(Semaphore::new(budget));

    (0..workers)
        .map(|_| {
            spawn(worker(
                client.clone(),
                config.clone(),
                receiver.clone(),
                download_sender.clone(),
                message_sender.clone(),
                cancellation_token.clone(),
                concurrency_sem.clone(),
            ))
        })
        .collect()
}

/// downloads one file at a time from the channel
/// may be canceled at any time by the token
pub(crate) async fn worker<D: DownloadDriver>(
    client: D,
    config: Arc<Config>,
    receiver: kanal::AsyncReceiver<Download>,
    download_sender: kanal::AsyncSender<Download>,
    message_sender: Sender<RunnerMessage>,
    cancellation_token: CancellationToken,
    concurrency_sem: Arc<Semaphore>,
) -> anyhow::Result<()> {
    'worker: loop {
        select! {
            _ = cancellation_token.cancelled() => break,
            Ok(download) = receiver.recv() => {
                if download.stop.is_cancelled() {
                    // If this task had already become active in a prior attempt, emit
                    // Inactive once so UI/CLI state does not keep a stale active entry.
                    if download.start.read().await.is_some() {
                        message_sender
                            .send(RunnerMessage::Inactive(download.node.handle.clone()))
                            .await?;
                    }
                    continue;
                }

                let since_last_retry = download.last_tried_at.lock().await.as_ref().map(|i| i.elapsed());
                if let Some(elapsed) = since_last_retry {
                    let retries = download.retries.load(Ordering::Relaxed);
                    match retry_decision(elapsed, retries, &config) {
                        RetryDecision::Wait => {
                             // avoid hammering workers with the same task
                            if download_sender.len() < config.max_workers_bounded() {
                                sleep(Duration::from_millis(10)).await;
                            }
                            // requeue download
                            download_sender.send(download).await?;
                            continue;
                        }
                        RetryDecision::TryNow => {
                            // proceed
                        }
                        RetryDecision::GiveUp => {
                            // report the error to the UI
                            message_sender.send(RunnerMessage::Error(
                                format!("Max retries reached for {}", download.node.name)
                            )).await?;
                            // report as Inactive to help CLI track completion
                            message_sender.send(RunnerMessage::Inactive(download.node.handle)).await?;
                            continue;
                        }
                    }
                }

                // create file path for the node
                let file_path = Path::new("downloads").join(&download.file_path);
                // create folders if needed
                create_dir_all(&file_path).await?;

                // full file path to partial file
                let partial_path = file_path.join(format!("{}.partial", download.node.name));
                // full path to final destination
                let full_path = file_path.join(&download.node.name);
                // this download is already complete
                if try_exists(&full_path).await? {
                    // report as Inactive to help CLI track completion
                    message_sender.send(RunnerMessage::Inactive(download.node.handle)).await?;
                    continue;
                }

                // figure out size & weight before downloading.
                let size_bytes = download.node.size;
                let weight = config.download_weight(size_bytes);

                // acquire weighted permits, but be cancel-aware.
                let _permits = {
                    let sem = concurrency_sem.clone();
                    select! {
                        _ = cancellation_token.cancelled() => {
                            break;
                        }
                        _ = download.stop.cancelled() => {
                            continue;
                        }
                        res = sem.acquire_many_owned(weight as u32) => {
                            res?
                        }
                    }
                };

                // mark the download as started
                download.start().await;
                // alert the UI of the change
                message_sender.send(RunnerMessage::Active(download.clone())).await?;

                select! {
                    // abort entire worker when canceled
                    _ = cancellation_token.cancelled() => break,
                    // abort download when individually stopped
                    _ = download.stop.cancelled() => (),
                    result = client.download_file(&download, &partial_path) => {
                        match result {
                            // the partial file now contains the full contents of the download
                            Ok(true) => {
                                if let Err(error) = rename(&partial_path, full_path).await {
                                    error!("Error renaming file: {error:?}");
                                    // treat rename failures as retryable errors so we do not
                                    // mark the download complete before the final file exists.
                                    download.set_retried().await;
                                    message_sender
                                        .send(RunnerMessage::Error(format!("Error renaming file {partial_path:?}: {error:?}")))
                                        .await?;
                                    // requeue download
                                    download_sender.send(download).await?;
                                    continue;
                                }
                            }
                            // the download has been paused
                            Ok(false) => {
                                // wait for download to unpause
                                // respect cancellation & stops
                                let mut pause_receiver = download.pause_receiver();
                                loop {
                                    if *pause_receiver.borrow() == PauseState::Running {
                                        break;
                                    }

                                    select! {
                                        _ = cancellation_token.cancelled() => break 'worker,
                                        _ = download.stop.cancelled() => {
                                            message_sender
                                                .send(RunnerMessage::Inactive(download.node.handle.clone()))
                                                .await?;
                                            continue 'worker;
                                        }
                                        result = pause_receiver.changed() => {
                                            if result.is_err() {
                                                break;
                                            }
                                        }
                                    }
                                }
                                // requeue download
                                download_sender.send(download).await?;
                                continue;
                            }
                            Err(error) => {
                                error!("Error downloading file: {error:?}");
                                // keep track of per-download retries
                                download.set_retried().await;
                                // report error to UI
                                message_sender.send(RunnerMessage::Error(error.to_string())).await?;
                                // requeue download
                                download_sender.send(download).await?;
                                continue;
                            }
                        }
                    }
                }

                // in every case, we want the UI to mark this download inactive
                message_sender.send(RunnerMessage::Inactive(download.node.handle)).await?
            }
            else => break,
        }
    }

    Ok(())
}

pub(crate) fn retry_decision(
    elapsed_since_last_retry: Duration,
    retries: u32,
    config: &Config,
) -> RetryDecision {
    if retries >= config.max_retries {
        return RetryDecision::GiveUp;
    }

    let exp = retries.min(31);
    let factor = 1u32 << exp;

    let base_delay = match config.min_retry_delay.checked_mul(factor) {
        Some(d) => d,
        None => config.max_retry_delay,
    };

    let required_delay = base_delay
        .max(config.min_retry_delay)
        .min(config.max_retry_delay);

    if elapsed_since_last_retry >= required_delay {
        RetryDecision::TryNow
    } else {
        RetryDecision::Wait
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::{Download, DownloadDriver};
    use anyhow::anyhow;
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;
    use tokio::time::{Duration, sleep};

    #[derive(Debug, Clone)]
    pub(crate) enum DriverAction {
        Complete,
        CompleteWithoutPartial,
        Pause,
        PauseThenQuickResume,
        Fail(String),
        Hang,
    }

    #[derive(Clone)]
    pub(crate) struct FakeDriver {
        actions: Arc<Mutex<VecDeque<DriverAction>>>,
        call_count: Arc<AtomicUsize>,
    }

    impl FakeDriver {
        pub(crate) fn new(actions: VecDeque<DriverAction>) -> Self {
            Self {
                actions: Arc::new(Mutex::new(actions)),
                call_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        pub(crate) fn call_count(&self) -> usize {
            self.call_count.load(Ordering::Relaxed)
        }

        async fn run_action(&self, download: &Download, dest_path: &Path) -> anyhow::Result<bool> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            let action = self
                .actions
                .lock()
                .await
                .pop_front()
                .unwrap_or(DriverAction::Complete);
            match action {
                DriverAction::Complete => {
                    if let Some(parent) = dest_path.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::write(dest_path, b"fake").await?;
                    Ok(true)
                }
                DriverAction::CompleteWithoutPartial => Ok(true),
                DriverAction::Pause => {
                    download.pause();
                    download.mark_paused_if_requested();
                    Ok(false)
                }
                DriverAction::PauseThenQuickResume => {
                    download.pause();
                    download.resume();
                    download.mark_paused_if_requested();
                    Ok(false)
                }
                DriverAction::Fail(message) => Err(anyhow!(message)),
                DriverAction::Hang => loop {
                    if download.stop.is_cancelled() {
                        return Ok(true);
                    }
                    sleep(Duration::from_millis(50)).await;
                },
            }
        }
    }

    impl DownloadDriver for FakeDriver {
        fn download_file(
            &self,
            download: &Download,
            dest_path: &Path,
        ) -> impl std::future::Future<Output = anyhow::Result<bool>> + Send {
            self.run_action(download, dest_path)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
