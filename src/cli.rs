use crate::config::Config;
use crate::helpers::mega_builder;
use crate::mega_client::NodeKind;
use crate::{Download, RunnerMessage, get_files, spawn_workers};
use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use log::error;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc::channel;
use tokio_util::sync::CancellationToken;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "giga-grabber",
    version,
    about = "High-performance MEGA downloader"
)]
pub(crate) struct CliArgs {
    /// MEGA URL to download
    #[arg(help = "MEGA URL to download")]
    pub(crate) url: String,

    /// Maximum number of concurrent download workers (1-10, default: 10)
    #[arg(
        long,
        help = "Maximum number of concurrent download workers (1-10, default: 10)"
    )]
    pub(crate) max_workers: Option<usize>,

    /// Concurrency budget for weighted downloads (1-100, default: 10)
    #[arg(
        long,
        help = "Concurrency budget for weighted downloads (1-100, default: 10)"
    )]
    pub(crate) concurrency_budget: Option<usize>,

    /// Maximum number of retry attempts (default: 3)
    #[arg(long, help = "Maximum number of retry attempts (default: 3)")]
    pub(crate) max_retries: Option<u32>,

    /// Request timeout in seconds (default: 20)
    #[arg(long, help = "Request timeout in seconds (default: 20)")]
    pub(crate) timeout: Option<u64>,

    /// Maximum retry delay in seconds (default: 30)
    #[arg(long, help = "Maximum retry delay in seconds (default: 30)")]
    pub(crate) max_retry_delay: Option<u64>,

    /// Minimum retry delay in seconds (default: 10)
    #[arg(long, help = "Minimum retry delay in seconds (default: 10)")]
    pub(crate) min_retry_delay: Option<u64>,

    /// Proxy mode: none, single, or random (default: none)
    #[arg(
        long,
        value_enum,
        help = "Proxy mode: none, single, or random (default: none)"
    )]
    pub(crate) proxy_mode: Option<crate::ProxyMode>,

    /// Proxy URL (can be specified multiple times for random mode)
    #[arg(
        long = "proxies",
        help = "Proxy URL (can be specified multiple times for random mode)"
    )]
    pub(crate) proxies: Vec<String>,
}

impl CliArgs {
    pub(crate) fn timeout_duration(&self) -> Option<Duration> {
        self.timeout.map(Duration::from_secs)
    }

    pub(crate) fn max_retry_delay_duration(&self) -> Option<Duration> {
        self.max_retry_delay.map(Duration::from_secs)
    }

    pub(crate) fn min_retry_delay_duration(&self) -> Option<Duration> {
        self.min_retry_delay.map(Duration::from_secs)
    }

    pub(crate) fn parse_proxies(&self) -> Result<Vec<url::Url>> {
        self.proxies
            .iter()
            .map(|p| url::Url::parse(p).with_context(|| format!("Invalid proxy URL: {p}")))
            .collect()
    }
}

fn merge_config_with_args(args: &CliArgs) -> Result<Config> {
    let mut base_config = Config::default();

    if let Some(max_workers) = args.max_workers {
        base_config.max_workers = max_workers;
    }
    if let Some(concurrency_budget) = args.concurrency_budget {
        base_config.concurrency_budget = concurrency_budget;
    }
    if let Some(max_retries) = args.max_retries {
        base_config.max_retries = max_retries;
    }
    if let Some(timeout) = args.timeout_duration() {
        base_config.timeout = timeout;
    }
    if let Some(max_retry_delay) = args.max_retry_delay_duration() {
        base_config.max_retry_delay = max_retry_delay;
    }
    if let Some(min_retry_delay) = args.min_retry_delay_duration() {
        base_config.min_retry_delay = min_retry_delay;
    }
    if let Some(proxy_mode) = args.proxy_mode {
        base_config.proxy_mode = proxy_mode;
    }
    if !args.proxies.is_empty() {
        base_config.proxies = args.parse_proxies()?;
    }

    base_config.normalize();
    base_config.validate().map_err(|msg| anyhow::anyhow!(msg))?;
    Ok(base_config)
}

/// Run a simple CLI download given a MEGA URL.
/// This uses the same worker pipeline as the GUI and shows a progress bar.
pub(crate) async fn run_cli(args: CliArgs) -> Result<()> {
    let config = merge_config_with_args(&args)?;
    let client = mega_builder(&config)?;
    let url = args.url.clone();

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
        config.max_workers_bounded(),
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
            #[cfg(feature = "gui")]
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
