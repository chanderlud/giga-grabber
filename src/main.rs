#[cfg(feature = "gui")]
use crate::app::build_app;
use crate::cli::{CliArgs, run_cli};
use crate::mega_client::{MegaClient, Node, NodeKind};
use clap::Parser;
use log::{LevelFilter, error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(feature = "gui")]
use std::env;
use std::fmt::Display;
use std::path::PathBuf;
use tokio::task::JoinHandle;

#[cfg(feature = "gui")]
mod app;
mod cli;
#[cfg(feature = "gui")]
mod components;
mod config;
mod helpers;
mod mega_client;
#[cfg(feature = "gui")]
mod screens;
mod session;
#[cfg(feature = "gui")]
mod styles;
mod worker;

type WorkerHandle = JoinHandle<anyhow::Result<()>>;
pub(crate) use session::*;
pub(crate) use worker::*;

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize, Eq, clap::ValueEnum)]
enum ProxyMode {
    /// No proxy
    None,
    /// Use a single proxy
    Single,
    /// Use a random proxy from the list
    Random,
}

impl ProxyMode {
    #[cfg(feature = "gui")]
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

/// main entry point which runs the Iced UI
#[cfg(feature = "gui")]
fn main() -> iced::Result {
    simple_logging::log_to_file("giga-grabber.log", LevelFilter::Warn).unwrap();
    log_panics::init();

    let mut args = env::args();
    let _exe = args.next(); // skip program name
    let has_cli_args = args.len() > 0;

    if has_cli_args {
        // CLI mode: parse args with clap, then run downloads inside a Tokio runtime
        let cli = CliArgs::parse();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

        rt.block_on(async move {
            if let Err(err) = run_cli(cli).await {
                error!("CLI error: {err:?}");
            }
        });

        Ok(())
    } else {
        // No CLI args → launch GUI
        build_app().run()
    }
}

#[cfg(not(feature = "gui"))]
fn main() {
    simple_logging::log_to_file("giga-grabber.log", LevelFilter::Warn).unwrap();
    log_panics::init();

    // CLI-only build always runs in CLI mode
    let cli = CliArgs::parse();
    let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    rt.block_on(async move {
        if let Err(err) = run_cli(cli).await {
            error!("CLI error: {err:?}");
        }
    });
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
