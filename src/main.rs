use crate::app::build_app;
use crate::cli::run_cli;
use crate::config::Config;
use crate::mega_client::{MegaClient, Node, NodeKind};
use log::{LevelFilter, error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::fs::{create_dir_all, rename};
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, Notify, RwLock, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep};
use tokio::{select, spawn};
use tokio_util::sync::CancellationToken;

mod app;
mod cli;
mod config;
mod helpers;
mod loading_wheel;
mod mega_client;
mod resources;
mod screens;
mod styles;

type WorkerHandle = JoinHandle<anyhow::Result<()>>;

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize, Eq)]
enum ProxyMode {
    /// Use a random proxy from the list
    Random,
    /// Use a single proxy
    Single,
    /// No proxy
    None,
}

impl ProxyMode {
    pub const ALL: [Self; 3] = [Self::None, Self::Single, Self::Random];
}

impl Display for ProxyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::None => "No Proxy",
                Self::Single => "Single Proxy",
                Self::Random => "Proxy List",
            }
        )
    }
}

#[derive(Debug, Clone)]
struct MegaFile {
    node: Node,
    file_path: PathBuf,
    children: Vec<Self>,
}

impl MegaFile {
    fn new(node: Node, file_path: PathBuf) -> Self {
        Self {
            node,
            file_path,
            children: Vec::new(),
        }
    }

    fn add_children(mut self, children: Vec<Self>) -> Self {
        self.children.extend(children);
        self
    }

    fn iter(&self) -> FileIter<'_> {
        FileIter { stack: vec![self] }
    }
}

struct FileIter<'a> {
    stack: Vec<&'a MegaFile>,
}

impl<'a> Iterator for FileIter<'a> {
    type Item = &'a MegaFile;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        self.stack.extend(node.children.iter().rev());
        Some(node)
    }
}

#[derive(Debug, Clone)]
struct Download {
    node: Node,
    file_path: PathBuf,
    downloaded: Arc<AtomicUsize>,
    start: Arc<RwLock<Option<Instant>>>,
    stop: CancellationToken,
    pause: Arc<Notify>,
    paused: Arc<AtomicBool>,
    retries: Arc<AtomicU32>,
    last_tried_at: Arc<Mutex<Option<Instant>>>,
}

impl Download {
    fn new(file: &MegaFile) -> Self {
        Self {
            node: file.node.clone(),
            file_path: file.file_path.clone(),
            downloaded: Default::default(),
            start: Default::default(),
            stop: Default::default(),
            pause: Default::default(),
            paused: Default::default(),
            retries: Default::default(),
            last_tried_at: Default::default(),
        }
    }
}

impl Download {
    async fn start(&self) {
        self.start.write().await.replace(Instant::now());
    }

    async fn set_retried(&self) {
        self.retries.fetch_add(1, Ordering::Relaxed);
        self.last_tried_at.lock().await.replace(Instant::now());
    }

    fn progress(&self) -> f32 {
        if self.node.size == 0 {
            return 0.0;
        }
        (self.downloaded.load(Ordering::Relaxed) as f32 / self.node.size as f32).clamp(0.0, 1.0)
    }

    fn speed(&self) -> f32 {
        if self.paused.load(Ordering::Relaxed) {
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

    fn cancel(&self) {
        self.stop.cancel();
    }

    fn pause(&self) {
        self.pause.notify_one();
    }

    fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
        // the worker will be sitting on notified
        self.pause.notify_one();
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
enum RunnerMessage {
    /// notifies UI that this download has become active
    Active(Download),
    /// notifies the UI that this download if finished
    Inactive(String),
    /// notifies the UI when non-critical errors bubble up
    Error(String),
    /// may be emitted during shutdown
    Finished,
}

enum RetryDecision {
    Wait,
    TryNow,
    GiveUp,
}

/// main entry point which runs the Iced UI
fn main() -> iced::Result {
    simple_logging::log_to_file("giga-grabber.log", LevelFilter::Warn).unwrap();
    log_panics::init();

    let mut args = env::args();
    let _exe = args.next(); // skip program name

    if let Some(url) = args.next() {
        // CLI mode: run downloads inside a Tokio runtime
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

        rt.block_on(async move {
            if let Err(err) = run_cli(url).await {
                error!("CLI error: {err:?}");
            }
        });

        Ok(())
    } else {
        // No CLI args → launch GUI
        build_app().run()
    }
}

/// load the nodes of a mega folder producing an array of MegaFile
/// each MegaFile is prepared to become a Download
async fn get_files(
    mega: MegaClient,
    url: String,
    index: usize,
) -> Result<(Vec<MegaFile>, usize), usize> {
    let nodes = mega.fetch_public_nodes(&url).await.map_err(|error| {
        error!("Error fetching files: {error:?}");
        index
    })?; // get all nodes

    // build a file structure for each root node using an optimized index
    let files = build_file_tree(&nodes);

    Ok((files, index))
}

/// Build a tree of `MegaFile`s from a flat `HashMap` of nodes in O(n)
fn build_file_tree(nodes: &HashMap<String, Node>) -> Vec<MegaFile> {
    // Map: parent_handle -> children
    // Keys are &str pointing into the Node's `parent` String.
    let mut children_by_parent: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut roots: Vec<&Node> = Vec::new();

    for node in nodes.values() {
        if let Some(parent) = node.parent.as_deref() {
            children_by_parent.entry(parent).or_default().push(node);
        } else {
            // Parent is None → this is a root node
            roots.push(node);
        }
    }

    roots
        .into_iter()
        .map(|root_node| parse_files(root_node, PathBuf::new(), &children_by_parent))
        .collect()
}

/// Recursive function that builds the file structure using a precomputed
/// parent -> children index.
///
/// `path` here is the *parent directory path* of `node`.
fn parse_files<'a>(
    node: &'a Node,
    path: PathBuf,
    children_by_parent: &HashMap<&'a str, Vec<&'a Node>>,
) -> MegaFile {
    // Directory path including this node's name
    let mut current_path = path.clone();
    current_path.push(&node.name);

    // Build children MegaFiles (if any)
    let children: Vec<MegaFile> = children_by_parent
        .get(node.handle.as_str())
        .into_iter()
        .flat_map(|children_vec| children_vec.iter())
        .map(|child_node| {
            if child_node.kind == NodeKind::Folder {
                parse_files(child_node, current_path.clone(), children_by_parent)
            } else {
                MegaFile::new((*child_node).clone(), current_path.clone())
            }
        })
        .collect();

    MegaFile::new(node.clone(), path).add_children(children)
}

/// spawns worker tasks
fn spawn_workers(
    client: MegaClient,
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
async fn worker(
    client: MegaClient,
    config: Arc<Config>,
    receiver: kanal::AsyncReceiver<Download>,
    download_sender: kanal::AsyncSender<Download>,
    message_sender: Sender<RunnerMessage>,
    cancellation_token: CancellationToken,
    concurrency_sem: Arc<Semaphore>,
) -> anyhow::Result<()> {
    loop {
        select! {
            _ = cancellation_token.cancelled() => break,
            Ok(download) = receiver.recv() => {
                if download.stop.is_cancelled() {
                    continue;
                }

                let since_last_retry = download.last_tried_at.lock().await.as_ref().map(|i| i.elapsed());
                if let Some(elapsed) = since_last_retry {
                    let retries = download.retries.load(Ordering::Relaxed);
                    match retry_decision(elapsed, retries, &config) {
                        RetryDecision::Wait => {
                             // avoid hammering workers with the same task
                            if download_sender.len() < config.max_workers {
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
                if full_path.exists() {
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
                                if let Err(error) = rename(partial_path, full_path).await {
                                    error!("Error renaming file: {error:?}");
                                }
                            }
                            // the download has been paused
                            Ok(false) => {
                                // wait for download to unpause
                                // respect cancellation & stops
                                select! {
                                    _ = cancellation_token.cancelled() => break,
                                    _ = download.stop.cancelled() => continue,
                                    _ = download.pause.notified() => ()
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

fn retry_decision(
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
