use std::fmt::Display;
use std::fs::File;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app::Theme;
use crate::ProxyMode;

type Result<T> = std::result::Result<T, Error>;

// error type
#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    Json(serde_json::Error),
}

// implement from for serde json error
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

// implement from for io error
impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

// implement display for error
impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            Self::Io(error) => write!(f, "IO error: {}", error),
            Self::Json(error) => write!(f, "JSON error: {}", error),
        }
    }
}

// configuration options
#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct Config {
    pub(crate) theme: Theme,
    pub(crate) max_threads: usize,
    pub(crate) max_threads_per_file: usize,
    pub(crate) max_concurrent_files: usize,
    pub(crate) max_retries: usize,
    // ms
    pub(crate) timeout: u64,
    // ms
    pub(crate) max_retry_delay: u64,
    // ms
    pub(crate) min_retry_delay: u64,
    pub(crate) proxy_mode: ProxyMode,
    pub(crate) proxies: Vec<String>,
}

// default options
impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            max_threads: 10,
            max_threads_per_file: 10,
            max_concurrent_files: 10,
            max_retries: 3,
            timeout: 10000,
            max_retry_delay: 10000,
            min_retry_delay: 1000,
            proxy_mode: ProxyMode::None,
            proxies: Vec::new(),
        }
    }
}

impl Config {
    // load config from file, create config file w/ defaults if needed
    pub(crate) fn load() -> Result<Self> {
        let path = Path::new("config.json");

        if path.exists() {
            let file = File::open(path)?;
            let config = serde_json::from_reader(file)?;
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    // save config to file
    pub(crate) fn save(&self) -> Result<()> {
        let path = Path::new("config.json");
        let file = File::create(path).unwrap();
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
}
