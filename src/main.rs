use crate::app::{App, settings};
use crate::mega_client::{MegaClient, Node, NodeKind};
use iced::Application;
use log::error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::fs::{create_dir_all, remove_file, rename};
use tokio::sync::{Notify, RwLock};
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio::{io, select, spawn};
use tokio_util::sync::CancellationToken;

mod app;
mod config;
mod loading_wheel;
mod mega_client;
mod modal;
mod slider;
mod styles;

type WorkerHandle = JoinHandle<io::Result<()>>;

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize, Eq)]
enum ProxyMode {
    // Use a random proxy from the list
    Random,

    // Use a single proxy
    Single,

    // No proxy
    None,
}

impl ProxyMode {
    pub const ALL: [Self; 3] = [Self::None, Self::Single, Self::Random];
}

// implement display for proxy mode dropdown
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
}

impl From<MegaFile> for Download {
    fn from(value: MegaFile) -> Self {
        Self {
            node: value.node,
            file_path: value.file_path,
            downloaded: Default::default(),
            start: Default::default(),
            stop: Default::default(),
            pause: Default::default(),
            paused: Default::default(),
        }
    }
}

impl Download {
    async fn start(&self) {
        *self.start.write().await = Some(Instant::now());
    }

    fn progress(&self) -> f32 {
        self.downloaded.load(Ordering::Relaxed) as f32 / self.node.size as f32
    }

    fn speed(&self) -> f32 {
        if self.paused.load(Ordering::Relaxed) {
            return 0_f32;
        }

        if let Some(start) = self.start.blocking_read().as_ref() {
            let elapsed = start.elapsed().as_secs_f32(); // elapsed time in seconds
            (self.downloaded.load(Ordering::Relaxed) as f32 / elapsed) / 1048576_f32
        // convert to MB/s
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
    Finished(String),
    /// notifies the UI when non-critical errors bubble up
    Error(String),
}

/// main entry point which runs the Iced UI
fn main() -> iced::Result {
    App::run(settings())
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

    // build a file structure for each root node
    let files = nodes
        .values()
        .filter(|node| node.parent.is_none())
        .map(|root_node| parse_files(&nodes, root_node, PathBuf::new()))
        .collect();

    Ok((files, index))
}

/// recursive function that builds the file structure
fn parse_files(nodes: &HashMap<String, Node>, node: &Node, path: PathBuf) -> MegaFile {
    let mut current_path = path.clone(); // clone path so it can be used in the closure
    current_path.push(&node.name); // add current node to path

    let children = nodes
        .values()
        .filter(move |n| n.parent.as_deref() == Some(&node.handle)) // get list of children (by parent handle)
        .map(|child_node| {
            if child_node.kind == NodeKind::Folder {
                // recurse if folder
                parse_files(nodes, child_node, current_path.clone())
            } else {
                // create file if file
                MegaFile::new(child_node.clone(), current_path.clone())
            }
        })
        .collect();

    // create a MegaFile for the current node with its children
    MegaFile::new(node.clone(), path).add_children(children)
}

/// spawns worker tasks
fn spawn_workers(
    client: MegaClient,
    receiver: kanal::AsyncReceiver<Download>,
    download_sender: kanal::AsyncSender<Download>,
    message_sender: Sender<RunnerMessage>,
    cancellation_token: CancellationToken,
    workers: usize,
) -> Vec<WorkerHandle> {
    (0..workers)
        .map(|_| {
            spawn(worker(
                client.clone(),
                receiver.clone(),
                download_sender.clone(),
                message_sender.clone(),
                cancellation_token.clone(),
            ))
        })
        .collect()
}

// TODO update the downloaded field of Download
// TODO use notifications from pause inside download method
// TODO set paused flag from inside download method
/// downloads one file at a time from the channel
/// may be canceled at any time by the token
async fn worker(
    client: MegaClient,
    receiver: kanal::AsyncReceiver<Download>,
    download_sender: kanal::AsyncSender<Download>,
    message_sender: Sender<RunnerMessage>,
    cancellation_token: CancellationToken,
) -> io::Result<()> {
    loop {
        select! {
            _ = cancellation_token.cancelled() => break,
            Ok(download) = receiver.recv() => {
                let file_path = Path::new("downloads").join(&download.file_path); // create file path for the node
                create_dir_all(&file_path).await?; // create folders

                let partial_path = file_path.join(download.node.name.to_owned() + ".partial"); // full file path to partial file
                let metadata_path = file_path.join(download.node.name.to_owned() + ".metadata"); // full file path to metadata file
                let full_path = file_path.join(&download.node.name); // full file path

                download.start().await;
                message_sender.send(RunnerMessage::Active(download.clone())).await.map_err(io::Error::other)?;

                select! {
                    _ = cancellation_token.cancelled() => break,
                    _ = download.stop.cancelled() => (),
                    result = client.download_file(&download.node, &partial_path) => {
                        if let Err(error) = result {
                            error!("Error downloading file: {}", error);
                            message_sender.send(RunnerMessage::Error(error.to_string())).await.map_err(io::Error::other)?;
                            download_sender.send(download).await.map_err(io::Error::other)?;
                        } else {
                            rename(partial_path, full_path).await?; // rename the file to its original name
                            remove_file(metadata_path).await?; // remove the metadata file
                            message_sender.send(RunnerMessage::Finished(download.node.handle.clone())).await.map_err(io::Error::other)?;
                        }
                    }
                }
            }
            else => break,
        }
    }

    Ok(())
}
