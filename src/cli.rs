use crate::app::mega_builder;
use crate::config::Config;
use crate::mega_client::NodeKind;
use crate::{Download, RunnerMessage, get_files, spawn_workers};
use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use log::error;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc::channel;
use tokio_util::sync::CancellationToken;

/// Run a simple CLI download given a MEGA URL.
/// This uses the same worker pipeline as the GUI and shows a progress bar.
pub(crate) async fn run_cli(url: String) -> Result<()> {
    let config = Config::load().expect("config error");
    let client = mega_builder(&config)?;

    let (files, _) = get_files(client.clone(), url.clone(), 0)
        .await
        .map_err(|_| anyhow::anyhow!("Failed to fetch files for URL: {url}"))?;

    // flatten all files
    let mut downloads: Vec<Download> = Vec::new();
    let mut total_bytes: u64 = 0;

    for root in &files {
        for mf in root.iter() {
            if mf.node.kind == NodeKind::Folder {
                continue;
            }
            total_bytes += mf.node.size;
            downloads.push(Download::new(mf));
        }
    }

    if downloads.is_empty() {
        println!("No downloadable files found for URL: {url}");
        return Ok(());
    }

    let total_files = downloads.len();

    // progress bar
    let pb = if total_bytes > 0 {
        ProgressBar::new(total_bytes)
    } else {
        // degenerate case: no sizes reported, just a spinner
        ProgressBar::new(0)
    };

    if total_bytes > 0 {
        pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise} / {eta_precise}] {bar:40} {bytes}/{total_bytes} {bytes_per_sec}",
            )?
                .progress_chars("=>-"),
        );
    } else {
        pb.set_style(ProgressStyle::with_template(
            "{spinner} Downloading files...",
        )?);
    }

    pb.println(format!(
        "Starting download of {total_files} file(s) from {url}"
    ));

    // channel for downloads and UI messages
    let (download_sender, download_receiver) = kanal::unbounded_async();
    let (message_sender, mut message_receiver) = channel::<RunnerMessage>(100);

    let cancellation_token = CancellationToken::new();
    let config = Arc::new(config);

    // queue all downloads
    for d in &downloads {
        download_sender.send(d.clone()).await?;
    }

    // spawn workers using your existing helper
    let workers = spawn_workers(
        client.clone(),
        config.clone(),
        download_receiver,
        download_sender.clone(),
        message_sender.clone(),
        cancellation_token.clone(),
        config.max_workers,
    );

    // progress updater task: sum all `downloaded` counters
    let downloads_for_progress = downloads.clone();
    let pb_for_progress = pb.clone();
    let total_bytes_for_progress = total_bytes;

    let progress_task = tokio::spawn(async move {
        // avoid division by zero weirdness
        if total_bytes_for_progress == 0 {
            return;
        }

        let mut ticker = tokio::time::interval(Duration::from_millis(200));
        loop {
            ticker.tick().await;
            let downloaded: u64 = downloads_for_progress
                .iter()
                .map(|d| d.downloaded.load(Ordering::Relaxed) as u64)
                .sum();
            pb_for_progress.set_position(downloaded.min(total_bytes_for_progress));
            if downloaded >= total_bytes_for_progress {
                break;
            }
        }
    });

    // consume RunnerMessage to know when all files are done & log errors
    let mut finished_files = 0usize;

    while let Some(msg) = message_receiver.recv().await {
        match msg {
            RunnerMessage::Active(download) => {
                pb.println(format!("→ {}", download.node.name));
            }
            RunnerMessage::Inactive(_handle) => {
                finished_files += 1;
                if finished_files == total_files {
                    break;
                }
            }
            RunnerMessage::Error(err) => {
                pb.println(format!("Error: {err}"));
            }
            RunnerMessage::Finished => {
                break;
            }
        }
    }

    // stop workers once everything is done
    cancellation_token.cancel();

    for handle in workers {
        if let Err(join_err) = handle.await {
            error!("Worker join error: {join_err:?}");
        }
    }

    let _ = progress_task.await;

    if total_bytes > 0 {
        pb.finish_with_message("Download complete");
    } else {
        pb.finish_with_message("Download(s) complete");
    }

    Ok(())
}
