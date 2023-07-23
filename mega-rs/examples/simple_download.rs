//!
//! Example program that simply downloads a file from MEGA
//! with progress reporting.
//!

use std::env;
use std::sync::Arc;
use std::time::Duration;

use tokio::fs::File;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use async_read_progress::AsyncReadProgressExt;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};

async fn run(mega: &mut mega::Client, distant_file_path: &str) -> mega::Result<()> {
    let nodes = mega.fetch_own_nodes().await?;

    let node = nodes
        .get_node_by_path(distant_file_path)
        .expect("could not find node by path");

    let (reader, writer) = sluice::pipe::pipe();

    let bar = ProgressBar::new(node.size());
    bar.set_style(progress_bar_style());
    bar.set_message("downloading file...");

    let file = File::create(node.name()).await?;

    let bar = Arc::new(bar);

    let reader = {
        let bar = bar.clone();
        reader.report_progress(Duration::from_secs(1), move |bytes_read| {
            bar.set_position(bytes_read as u64);
        })
    };

    let handle =
        tokio::spawn(async move { futures::io::copy(reader, &mut file.compat_write()).await });
    mega.download_node(node, writer).await?;
    handle.await.unwrap()?;

    bar.finish_with_message(format!("{} downloaded !", node.name()));

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let email = env::var("MEGA_EMAIL").expect("missing MEGA_EMAIL environment variable");
    let password = env::var("MEGA_PASSWORD").expect("missing MEGA_PASSWORD environment variable");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [distant_file_path] = args.as_slice() else {
        panic!("expected 1 command-line argument: {{distant_file_path}}");
    };

    let http_client = reqwest::Client::new();
    let mut mega = mega::Client::builder().build(http_client).unwrap();

    mega.login(&email, &password, None).await.unwrap();

    let result = run(&mut mega, distant_file_path).await;
    mega.logout().await.unwrap();

    result.unwrap();
}

pub fn progress_bar_style() -> ProgressStyle {
    let template = format!(
        "{}{{bar:30.magenta.bold/magenta/bold}}{} {{percent}} % (ETA {{eta}}): {{msg}}",
        style("▐").bold().magenta(),
        style("▌").bold().magenta(),
    );

    ProgressStyle::default_bar()
        .progress_chars("▨▨╌")
        .template(template.as_str())
        .unwrap()
}
