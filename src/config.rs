use crate::ProxyMode;
use iced::Theme;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    Json(serde_json::Error),
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
    pub(crate) proxies: Vec<String>,
}

// default options
impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "Dark".to_string(),
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
    /// load config from file, create config file w/ defaults if needed
    pub(crate) fn load() -> Result<Self> {
        let path = Path::new("config.json");

        let mut config_option: Option<Config> = None;
        if path.exists() {
            let file = File::open(path)?;
            if let Ok(config) = serde_json::from_reader(file) {
                config_option = Some(config);
            }
        }

        if let Some(config) = config_option {
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    /// save config to file
    pub(crate) fn save(&self) -> Result<()> {
        let path = Path::new("config.json");
        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    pub(crate) fn weighted_concurrency_budget(&self) -> usize {
        self.concurrency_budget.clamp(1, 100)
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

    pub(crate) fn get_theme(&self) -> Theme {
        Theme::ALL
            .iter()
            .find(|t| t.to_string() == self.theme)
            .cloned()
            .unwrap_or(Theme::Dark)
    }

    pub(crate) fn set_theme(&mut self, theme: Theme) {
        self.theme = theme.to_string();
    }
}
