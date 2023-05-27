use std::fs::rename;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use futures::{stream, StreamExt};
use futures::AsyncWriteExt;
use indicatif::{ProgressBar, ProgressStyle};
use mega::{Client, Node, Nodes};
use reqwest::{Client as HttpClient, Proxy, Url};
use structopt::StructOpt;
use tokio::fs::{create_dir_all, File, OpenOptions, remove_file};
use tokio::spawn;
use tokio::sync::{mpsc, Mutex};
use tokio_util::compat::TokioAsyncWriteCompatExt;

#[derive(Debug, StructOpt)]
struct Options {
    #[structopt(short, long, default_value = "4", help = "Threads per file")]
    threads: usize,

    #[structopt(short, long, default_value = "10", help = "Number of concurrent file downloads")]
    concurrent_files: usize,

    #[structopt(short, long, help = "The MEGA public folder URL")]
    url: String,

    #[structopt(long, default_value = "none", help = "The proxy mode [random, single, none]")]
    proxy_mode: ProxyMode,

    #[structopt(long, help = "Proxy URL [socks5://user:pass@1.1.1.1:8080]")]
    proxy_url: Option<String>,

    #[structopt(long, help = "Proxy file [one proxy per line, URL format]")]
    proxy_file: Option<String>,
}

#[derive(Debug, PartialEq)]
enum ProxyMode {
    /// Use a random proxy from the list
    Random,

    /// Use a single proxy
    Single,

    /// No proxy
    None,
}

impl FromStr for ProxyMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "random" => Ok(ProxyMode::Random),
            "single" => Ok(ProxyMode::Single),
            "none" => Ok(ProxyMode::None),
            _ => Err(format!("Invalid proxy mode: {}", s)),
        }
    }
}


#[tokio::main]
async fn main() {
    let options = Options::from_args();

    if options.proxy_mode == ProxyMode::Single && options.proxy_url.is_none() {
        panic!("Proxy URL is required for single proxy mode");
    }

    let proxy_list: Vec<String> = match options.proxy_mode {
        ProxyMode::Random => {
            if options.proxy_file.is_none() {
                panic!("Proxy file is required for random proxy mode");
            }

            let proxy_file = options.proxy_file.as_ref().unwrap();
            let proxy_str = std::fs::read_to_string(proxy_file).unwrap();
            proxy_str.split_whitespace().map(|proxy| proxy.to_string()).collect()
        }
        _ => Vec::new(), // empty vector for single and none modes
    };

    let http_client = HttpClient::builder()
        .proxy(Proxy::custom(move |_| {
            match options.proxy_mode {
                ProxyMode::Random => {
                    let i = fastrand::usize(..proxy_list.len());
                    let proxy_url = &proxy_list[i];
                    Url::parse(proxy_url).unwrap().into()
                }
                ProxyMode::Single => {
                    Url::parse(options.proxy_url.as_ref().unwrap()).unwrap().into()
                }
                ProxyMode::None => None::<Url>,
            }
        }))
        .build().unwrap();

    let mut mega = Client::builder()
        .https(false)
        .timeout(Duration::from_secs(10))
        .max_retry_delay(Duration::from_secs(10))
        .max_retries(20)
        .build(http_client).unwrap();

    run(&mut mega, &options.url, options.concurrent_files, options.threads).await.unwrap();
}

async fn run(mega: &mut Client, public_url: &str, concurrent_files: usize, threads: usize) -> mega::Result<()> {
    let nodes = mega.fetch_public_nodes(public_url).await?;

    let all_files: Vec<(Node, PathBuf)> = nodes
        .roots()
        .flat_map(|root| get_files_recursively(&nodes, root, PathBuf::new()))
        .collect();

    download(mega, &all_files, concurrent_files, threads).await
}

async fn download(mega: &mut Client, files: &Vec<(Node, PathBuf)>, concurrent_files: usize, threads: usize) -> mega::Result<()> {
    let total_files = files.len();
    let progress = Arc::new(
        Mutex::new(
            ProgressBar::new(total_files as u64)
        )
    );

    {
        let progress = progress.lock().await;

        progress.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})").unwrap()
            .progress_chars("#>-"));
    }

    let (tx, mut rx) = mpsc::channel::<()>(16);

    let progress_task = {
        let progress = progress.clone();

        spawn(async move {
            while let Some(_) = rx.recv().await {
                progress.lock().await.inc(1);
            }
        })
    };

    let update_progress_task = {
        let progress = progress.clone();

        spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                progress.lock().await.tick();
            }
        })
    };

    let bodies = stream::iter(files)
        .map(|(node, path)| {
            let mega = &mega;
            let tx = &tx;

            async move {
                let file_path = Path::new("downloads").join(path); // create file path for the node
                create_dir_all(&file_path).await.unwrap(); // create folders
                let partial_path = file_path.join(node.name().to_owned() + ".partial"); // full file path to partial file
                let metadata_path = file_path.join(node.name().to_owned() + ".metadata");
                let full_path = file_path.join(node.name()); // full file path

                if full_path.exists() {
                    tx.send(()).await.unwrap();
                    return Ok::<_, mega::Error>(());
                } else if partial_path.exists() {
                    let file = OpenOptions::new().write(true).open(&partial_path).await?; // open file
                    let mut compat_file = file.compat_write(); // futures compatible file

                    // download the missing sections of the node and write its contents to the file
                    mega.download_node(node, &mut compat_file, threads, &metadata_path).await?;
                    compat_file.flush().await?; // flush the file to ensure all data is written

                    rename(partial_path, full_path).unwrap(); // rename the file to its original name
                    remove_file(metadata_path).await.unwrap();
                    tx.send(()).await.unwrap();
                    return Ok::<_, mega::Error>(());
                }

                let file = File::create(&partial_path).await?; // create file
                let mut compat_file = file.compat_write(); // futures compatible file

                // download the node and write its contents to the file
                mega.download_node(node, &mut compat_file, threads, &metadata_path).await?;
                compat_file.flush().await?; // flush the file to ensure all data is written

                rename(partial_path, full_path).unwrap(); // rename the file to its original name
                remove_file(metadata_path).await.unwrap();
                tx.send(()).await.unwrap();
                Ok::<_, mega::Error>(())
            }
        })
        .buffer_unordered(concurrent_files);

    bodies.collect::<Vec<_>>().await;

    drop(tx);

    progress_task.await.unwrap(); // Wait for the progress task to finish.
    update_progress_task.abort(); // Abort the update progress task, as it runs in an infinite loop.

    Ok(())
}

fn get_files_recursively(nodes: &Nodes, node: &Node, path: PathBuf) -> Vec<(Node, PathBuf)> {
    let mut current_path = path.clone();
    current_path.push(node.name());

    node.children()
        .iter()
        .filter_map(|hash| nodes.get_node_by_hash(hash))
        .flat_map(|child_node| {
            if child_node.kind().is_folder() {
                get_files_recursively(nodes, &child_node, current_path.clone())
            } else {
                vec![(child_node.clone(), current_path.clone())]
            }
        })
        .collect()
}
