use crate::WorkerHandle;
use crate::config::Config;
use crate::worker::{Download, DownloadDriver, RunnerMessage, spawn_workers};
use anyhow::Context;
use log::error;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc::Sender as TokioSender;
use tokio_util::sync::CancellationToken;

struct WorkerState {
    handles: Vec<WorkerHandle>,
    cancel: CancellationToken,
}

impl WorkerState {
    async fn join_handles(self) {
        for handle in self.handles {
            match handle.await {
                Err(error) => error!("worker panicked: {error:?}"),
                Ok(Err(error)) => error!("worker failed: {error:?}"),
                Ok(Ok(())) => ()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum SessionEvent {
    TransferActive(Download),
    TransferTerminal(String),
    Error(String),
    Drained,
}

pub(crate) struct TransferSession<D: DownloadDriver> {
    client: D,
    config: Arc<Config>,
    runner_sender: Option<TokioSender<RunnerMessage>>,
    download_sender: kanal::Sender<Download>,
    download_receiver: kanal::AsyncReceiver<Download>,
    worker: Option<WorkerState>,
    transfers: HashMap<String, Download>,
    active_handles: HashSet<String>,
}

impl<D: DownloadDriver> TransferSession<D> {
    pub(crate) fn new(client: D, config: Config) -> Self {
        let (download_sender, download_receiver) = kanal::unbounded();

        Self {
            client,
            config: Arc::new(config),
            runner_sender: None,
            download_sender,
            download_receiver: download_receiver.to_async(),
            worker: None,
            transfers: HashMap::new(),
            active_handles: HashSet::new(),
        }
    }

    pub(crate) fn set_runner_sender(&mut self, runner_sender: TokioSender<RunnerMessage>) {
        self.runner_sender = Some(runner_sender);
    }

    pub(crate) fn contains_transfer(&self, handle: &str) -> bool {
        self.transfers.contains_key(handle)
    }

    pub(crate) fn handles(&self) -> HashSet<String> {
        self.transfers.keys().cloned().collect()
    }

    pub(crate) fn has_live_transfers(&self) -> bool {
        !self.transfers.is_empty()
    }

    pub(crate) fn is_running(&self) -> bool {
        self.worker.is_some()
    }

    pub(crate) fn active_count(&self) -> usize {
        self.active_handles.len()
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.transfers
            .len()
            .saturating_sub(self.active_handles.len())
    }

    pub(crate) fn add_downloads(
        &mut self,
        downloads: impl IntoIterator<Item = Download>,
    ) -> anyhow::Result<usize> {
        let mut added = 0usize;

        for download in downloads {
            let handle = download.node.handle.clone();
            if self.transfers.contains_key(&handle) {
                continue;
            }

            self.download_sender
                .send(download.clone())
                .with_context(|| format!("queue {}", download.node.name))?;
            self.transfers.insert(handle, download);
            added += 1;
        }

        if added > 0 && self.worker.is_none() {
            self.start_workers()?;
        }

        Ok(added)
    }

    pub(crate) fn handle_runner_message(&mut self, message: RunnerMessage) -> Vec<SessionEvent> {
        let mut events = Vec::new();

        match message {
            RunnerMessage::Active(download) => {
                let handle = download.node.handle.clone();
                self.active_handles.insert(handle.clone());
                self.transfers
                    .entry(handle)
                    .or_insert_with(|| download.clone());
                events.push(SessionEvent::TransferActive(download));
            }
            RunnerMessage::Inactive(handle) => {
                self.active_handles.remove(&handle);
                if self.transfers.remove(&handle).is_some() {
                    events.push(SessionEvent::TransferTerminal(handle));
                }
                if self.transfers.is_empty() {
                    events.push(SessionEvent::Drained);
                }
            }
            RunnerMessage::Error(error) => events.push(SessionEvent::Error(error)),
            RunnerMessage::Finished => {
                if self.transfers.is_empty() {
                    events.push(SessionEvent::Drained);
                }
            }
        }

        events
    }

    pub(crate) fn abort_background(&mut self) -> Vec<String> {
        let handles = self.transfers.keys().cloned().collect();

        for download in self.transfers.values() {
            download.cancel();
        }

        self.drain_download_queue();
        self.transfers.clear();
        self.active_handles.clear();
        self.finish_background();

        handles
    }

    pub(crate) async fn finish(&mut self) {
        if let Some(state) = self.worker.take() {
            state.cancel.cancel();
            state.join_handles().await;
        }
    }

    pub(crate) fn finish_background(&mut self) {
        if let Some(state) = self.worker.take() {
            state.cancel.cancel();
            tokio::spawn(state.join_handles());
        }
    }

    fn start_workers(&mut self) -> anyhow::Result<()> {
        let runner_sender = self
            .runner_sender
            .clone()
            .context("runner sender not available")?;
        let cancel = CancellationToken::new();

        self.worker = Some(WorkerState {
            handles: spawn_workers(
                self.client.clone(),
                self.config.clone(),
                self.download_receiver.clone(),
                self.download_sender.clone_async(),
                runner_sender,
                cancel.clone(),
                self.config.max_workers_bounded(),
            ),
            cancel,
        });

        Ok(())
    }

    fn drain_download_queue(&mut self) {
        while let Ok(Some(download)) = self.download_receiver.try_recv() {
            download.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionEvent, TransferSession};
    use crate::config::Config;
    use crate::worker::fake::{DriverAction, FakeDriver};
    use crate::worker::tests::{
        make_download, next_message, wait_for_driver_calls, wait_for_paused,
    };
    use std::collections::VecDeque;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::sync::mpsc::channel;
    use tokio::time::{sleep, timeout};

    fn make_driver(actions: Vec<DriverAction>) -> FakeDriver {
        FakeDriver::new(VecDeque::from(actions))
    }

    #[tokio::test]
    async fn test_session_append_keeps_single_active_session_until_all_transfers_finish() {
        let temp = TempDir::new().expect("temp dir");
        let first = make_download("first.bin", 1024, temp.path().to_path_buf());
        let second = make_download("second.bin", 1024, temp.path().to_path_buf());
        let driver = make_driver(vec![DriverAction::Hang, DriverAction::Complete]);
        let (runner_sender, mut runner_receiver) = channel(64);

        let mut session = TransferSession::new(driver.clone(), Config::default());
        session.set_runner_sender(runner_sender);
        assert_eq!(
            session
                .add_downloads(vec![first.clone()])
                .expect("queue first"),
            1
        );
        assert!(session.is_running());
        assert!(session.contains_transfer(&first.node.handle));

        let events = session.handle_runner_message(next_message(&mut runner_receiver).await);
        assert!(matches!(
            events.as_slice(),
            [SessionEvent::TransferActive(_)]
        ));
        assert_eq!(session.active_count(), 1);
        assert_eq!(session.pending_count(), 0);

        assert_eq!(
            session
                .add_downloads(vec![second.clone()])
                .expect("queue second"),
            1
        );
        assert!(session.contains_transfer(&second.node.handle));
        assert_eq!(session.pending_count(), 1);

        first.stop.cancel();
        wait_for_driver_calls(&driver, 2).await;

        let mut saw_drained = false;
        timeout(Duration::from_secs(3), async {
            while !saw_drained {
                for event in session.handle_runner_message(next_message(&mut runner_receiver).await)
                {
                    if matches!(event, SessionEvent::Drained) {
                        saw_drained = true;
                    }
                }
            }
        })
        .await
        .expect("session drain timeout");

        assert!(!session.has_live_transfers());
        session.finish().await;
    }

    #[tokio::test]
    async fn test_session_stays_alive_while_download_is_paused() {
        let temp = TempDir::new().expect("temp dir");
        let download = make_download("paused.bin", 1024, temp.path().to_path_buf());
        let driver = make_driver(vec![DriverAction::Pause, DriverAction::Complete]);
        let (runner_sender, mut runner_receiver) = channel(64);

        let mut session = TransferSession::new(driver, Config::default());
        session.set_runner_sender(runner_sender);
        session
            .add_downloads(vec![download.clone()])
            .expect("queue download");

        let events = session.handle_runner_message(next_message(&mut runner_receiver).await);
        assert!(matches!(
            events.as_slice(),
            [SessionEvent::TransferActive(_)]
        ));
        wait_for_paused(&download).await;
        sleep(Duration::from_millis(100)).await;

        assert!(session.has_live_transfers());
        assert_eq!(session.active_count(), 1);

        download.resume();

        let mut saw_drained = false;
        timeout(Duration::from_secs(3), async {
            while !saw_drained {
                for event in session.handle_runner_message(next_message(&mut runner_receiver).await)
                {
                    if matches!(event, SessionEvent::Drained) {
                        saw_drained = true;
                    }
                }
            }
        })
        .await
        .expect("session drain timeout");

        assert!(!session.has_live_transfers());
        session.finish().await;
    }

    #[tokio::test]
    async fn test_session_config_is_snapshotted_at_creation() {
        let temp = TempDir::new().expect("temp dir");
        let download = make_download("snapshot.bin", 1024, temp.path().to_path_buf());
        let driver = make_driver(vec![DriverAction::Complete]);
        let (runner_sender, mut runner_receiver) = channel(64);
        let config = Config {
            max_workers: 2,
            ..Default::default()
        };

        let mut session = TransferSession::new(driver, config.clone());
        session.set_runner_sender(runner_sender);

        let mut mutated = config;
        mutated.max_workers = 9;

        session
            .add_downloads(vec![download])
            .expect("queue download");

        let mut saw_drained = false;
        timeout(Duration::from_secs(3), async {
            while !saw_drained {
                for event in session.handle_runner_message(next_message(&mut runner_receiver).await)
                {
                    if matches!(event, SessionEvent::Drained) {
                        saw_drained = true;
                    }
                }
            }
        })
        .await
        .expect("session drain timeout");

        assert!(session.is_running());
        session.finish().await;
        assert_eq!(mutated.max_workers, 9);
    }
}
