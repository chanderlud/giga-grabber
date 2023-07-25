use std::fmt::Display;
use std::fs::rename;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use deadqueue::unlimited::Queue;
use futures::AsyncWriteExt;
use futures::FutureExt;
use iced::Application;
use mega::{Client, Node, Nodes};
use serde::{Deserialize, Serialize};
use tokio::fs::{create_dir_all, remove_file, File, OpenOptions};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Instant};
use tokio::{select, spawn};
use tokio_util::compat::TokioAsyncWriteCompatExt;

use crate::app::{settings, App};
use crate::config::Config;

mod app;
mod config;
mod loading_wheel;
mod modal;
mod slider;
mod styles;

type DownloadQueue = Arc<Queue<Download>>;

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

    fn iter(&self) -> FileIter {
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
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}

// trait allows `Download` to be used in mega crate
impl mega::Download for &Download {
    fn get_node(&self) -> &Node {
        &self.node
    }

    fn add_downloaded(&self, val: usize) {
        self.downloaded.fetch_add(val, Ordering::Relaxed);
    }

    fn get_pause(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    fn set_pause(&self, paused: bool) {
        self.pause.store(paused, Ordering::Relaxed);
    }
}

impl From<MegaFile> for Download {
    fn from(value: MegaFile) -> Self {
        Self {
            node: value.node,
            file_path: value.file_path,
            downloaded: Default::default(),
            start: Arc::new(RwLock::new(None)), // value is set when download starts
            stop: Arc::new(Default::default()),
            pause: Arc::new(Default::default()),
            paused: Arc::new(Default::default()),
        }
    }
}

impl Download {
    async fn start(&self) {
        *self.start.write().await = Some(Instant::now());
    }

    fn progress(&self) -> f32 {
        self.downloaded.load(Ordering::Relaxed) as f32 / self.node.size() as f32
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
        self.stop.store(true, Ordering::Relaxed);
    }

    fn pause(&self) {
        self.pause.store(true, Ordering::Relaxed);
    }

    fn resume(&self) {
        self.pause.store(false, Ordering::Relaxed);
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
enum RunnerMessage {
    // message for when download starts
    Start(Download),
    // message for when download stops, string is ID in map
    Stop(String),
}

// run iced GUI
fn main() -> iced::Result {
    App::run(settings())
}

// background thread that downloads files
fn runner(
    config: Config,
    mega: &Client,
    queue: &DownloadQueue,
    sender: &Arc<UnboundedSender<RunnerMessage>>,
    queued: &Arc<AtomicUsize>,
    active_threads: &Arc<AtomicUsize>,
    workers: usize,
) -> Vec<JoinHandle<()>> {
    // create workers
    (0..workers)
        .map(|_i| {
            // clone values for thread
            let mega = mega.clone();
            let queue = queue.clone();
            let sender = sender.clone();
            let active_threads = active_threads.clone();
            let queued = queued.clone();

            spawn(async move {
                while !queue.is_empty() {
                    let download = queue.pop().await; // get next download from queue

                    // calculate number of threads to use for download
                    let download_threads = if download.node.size() < 1048576 {
                        1 // if file is less than 1MB, use 1 thread (1MB is smallest chunk size)
                    } else {
                        // min of 1, max of `max_threads_per_file` or `download.node.size() / 524288`
                        1.max(
                            config
                                .max_threads_per_file
                                .min(download.node.size() as usize / 524288),
                        )
                    };

                    // wait until there are enough threads available
                    while (active_threads.load(Ordering::Relaxed) + download_threads)
                        > config.max_threads
                    {
                        sleep(Duration::from_millis(10)).await;
                    }

                    active_threads.fetch_add(download_threads, Ordering::Relaxed); // add threads to active threads
                    queued.fetch_sub(1, Ordering::Relaxed); // remove from queued stat for GUI

                    // send message to GUI that download has started
                    // `Download` has internal Arcs so it can be cloned for the GUI to access stats
                    sender.send(RunnerMessage::Start(download.clone())).unwrap();

                    if let Err(error) = download_file(&download, &mega, download_threads).await {
                        mega.sender.send(error).unwrap(); // send error to GUI
                    }

                    // send message to GUI that download has finished
                    sender
                        .send(RunnerMessage::Stop(download.node.hash().to_string()))
                        .unwrap();

                    active_threads.fetch_sub(download_threads, Ordering::Relaxed);
                    // release threads
                }
            })
        })
        .collect()
}

// get the files from a mega folder
// `index` is used by the GUI to keep track of the url inputs
async fn get_files(
    mega: Client,
    url: String,
    index: usize,
) -> Result<(Vec<MegaFile>, usize), usize> {
    let nodes = mega.fetch_public_nodes(&url).await.map_err(|_e| index)?; // get all nodes

    // build a file structure for each root node
    let files = nodes
        .roots()
        .map(|root_node| parse_files(&nodes, root_node, PathBuf::new()))
        .collect();

    Ok((files, index))
}

// the recursive function that builds the file structure
fn parse_files(nodes: &Nodes, node: &Node, path: PathBuf) -> MegaFile {
    let mut current_path = path.clone(); // clone path so it can be used in the closure
    current_path.push(node.name()); // add current node to path

    let children = node
        .children() // get list of children handles
        .iter() // iterate over handles
        .filter_map(|hash| nodes.get_node_by_hash(hash)) // get child node from handle
        .map(|child_node| {
            if child_node.kind().is_folder() {
                parse_files(nodes, child_node, current_path.clone()) // recurse if folder
            } else {
                MegaFile::new(child_node.clone(), current_path.clone()) // create file if file
            }
        })
        .collect();

    // create a MegaFile for the current node with its children
    MegaFile::new(node.clone(), path).add_children(children)
}

// main download function
async fn download_file(download: &Download, mega: &Client, threads: usize) -> mega::Result<()> {
    let file_path = Path::new("downloads").join(&download.file_path); // create file path for the node
    create_dir_all(&file_path).await?; // create folders

    let partial_path = file_path.join(download.node.name().to_owned() + ".partial"); // full file path to partial file
    let metadata_path = file_path.join(download.node.name().to_owned() + ".metadata"); // full file path to metadata file
    let full_path = file_path.join(download.node.name()); // full file path

    let file; // not initialized if file is already downloaded

    if full_path.exists() {
        return Ok(()); // file is already fully downloaded
    } else if partial_path.exists() {
        file = OpenOptions::new().write(true).open(&partial_path).await?; // file is already partially downloaded
    } else {
        file = File::create(&partial_path).await?; // file is not downloaded at all
    }

    download.start().await; // set start time
    let mut compat_file = file.compat_write(); // futures compatible file

    // create a future that checks if the download should be canceled
    let cancelable = async {
        while !download.stop.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(10)).await;
        }
    };

    let mut result = Option::<mega::Result<()>>::None; // result of the download

    // download the node and write its contents to the file
    select! {
        value = mega.download_node(download, &mut compat_file, threads, &metadata_path) => {
            result = Some(value);
        },
        _ = cancelable.fuse() => {},
    }

    compat_file.flush().await?; // flush the file to ensure all data is written

    // if the download finished
    if let Some(result) = result {
        result?; // propagate any errors

        rename(partial_path, full_path)?; // rename the file to its original name
        remove_file(metadata_path).await?; // remove the metadata file
    }

    Ok(())
}
