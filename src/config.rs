use crate::ProxyMode;
use iced::Theme;
use log::error;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::fs::File;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

type Result<T> = std::result::Result<T, Error>;

pub(crate) const MIN_MAX_WORKERS: usize = 1;
pub(crate) const MAX_MAX_WORKERS: usize = 10;
pub(crate) const MIN_CONCURRENCY: usize = 1;
pub(crate) const MAX_CONCURRENCY: usize = 100;

#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidConfig(String),
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            Self::Io(error) => write!(f, "IO error: {}", error),
            Self::Json(error) => write!(f, "JSON error: {}", error),
            Self::InvalidConfig(message) => write!(f, "Invalid config: {}", message),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct Config {
    pub(crate) theme: String,
    pub(crate) max_workers: usize,
    pub(crate) concurrency_budget: usize,
    pub(crate) max_retries: u32,
    pub(crate) timeout: Duration,
    pub(crate) max_retry_delay: Duration,
    pub(crate) min_retry_delay: Duration,
    pub(crate) proxy_mode: ProxyMode,
    pub(crate) proxies: Vec<Url>,
}

// default options
impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "System".to_string(),
            max_workers: 10,
            concurrency_budget: 10,
            max_retries: 3,
            timeout: Duration::from_secs(20),
            max_retry_delay: Duration::from_secs(30),
            min_retry_delay: Duration::from_secs(10),
            proxy_mode: ProxyMode::None,
            proxies: Vec::new(),
        }
    }
}

impl Config {
    pub(crate) fn new() -> (Self, Option<String>) {
        match Config::load() {
            Ok(config) => (config, None),
            Err(Error::Io(ref io_error)) if io_error.kind() == ErrorKind::NotFound => {
                // expected on first launch: create & persist defaults
                (save_default(), None)
            }
            Err(load_error) => {
                error!("Failed to load config: {load_error}");

                let config_path = Path::new("config.json");
                if config_path.exists() {
                    let candidate = config_backup_path();
                    if let Err(error) = std::fs::copy(config_path, &candidate) {
                        error!("Failed to copy config file: {error}");
                    }
                }

                (
                    save_default(),
                    Some("Failed to load config from disk, applying default options".to_string()),
                )
            }
        }
    }

    /// load config from file if possible
    fn load() -> Result<Self> {
        let path = Path::new("config.json");
        let file = File::open(path)?;
        let mut config: Config = serde_json::from_reader(file)?;
        config.normalize();
        config.validate().map_err(Error::InvalidConfig)?;
        Ok(config)
    }

    /// save config to file
    pub(crate) fn save(&self) -> Result<()> {
        let path = Path::new("config.json");
        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    pub(crate) fn weighted_concurrency_budget(&self) -> usize {
        self.concurrency_budget
            .clamp(MIN_CONCURRENCY, MAX_CONCURRENCY)
    }

    pub(crate) fn max_workers_bounded(&self) -> usize {
        self.max_workers.clamp(MIN_MAX_WORKERS, MAX_MAX_WORKERS)
    }

    pub(crate) fn normalize(&mut self) {
        self.max_workers = self.max_workers_bounded();
        self.concurrency_budget = self.weighted_concurrency_budget();
    }

    pub(crate) fn validate(&self) -> std::result::Result<(), String> {
        if self.proxy_mode != ProxyMode::None && self.proxies.is_empty() {
            return Err("proxy_mode is enabled but no proxies are configured".to_string());
        }

        Ok(())
    }

    pub(crate) fn download_weight(&self, size_bytes: u64) -> usize {
        const MB: u64 = 1024 * 1024;

        let raw_weight = if size_bytes < 5 * MB {
            1
        } else if size_bytes < 20 * MB {
            2
        } else if size_bytes < 100 * MB {
            5
        } else {
            10
        };

        // never allow weight > budget
        let budget = self.weighted_concurrency_budget().max(1);
        raw_weight.min(budget)
    }

    pub(crate) fn get_theme(&self) -> Option<Theme> {
        Theme::ALL
            .iter()
            .find(|t| t.to_string() == self.theme)
            .cloned()
    }
}

fn config_backup_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    PathBuf::from(format!("config.json.backup.{}", timestamp))
}

fn save_default() -> Config {
    let mut config = Config::default();
    config.normalize();
    if let Err(save_error) = config.save() {
        error!("Failed to save default config: {save_error}",);
    }
    config
}
