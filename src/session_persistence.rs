use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SESSION_PATH: &str = "download-session.json";
const FORMAT_VERSION: u32 = 1;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    UnsupportedVersion(u32),
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "IO error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported download-session version: {version}")
            }
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DownloadSessionRecord {
    pub(crate) source_url: String,
    pub(crate) node_handle: String,
    pub(crate) destination_dir: PathBuf,
    pub(crate) expected_size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    records: Vec<DownloadSessionRecord>,
}

pub(crate) fn load() -> Result<Vec<DownloadSessionRecord>> {
    load_from(Path::new(SESSION_PATH))
}

pub(crate) fn save(records: &[DownloadSessionRecord]) -> Result<()> {
    save_to(Path::new(SESSION_PATH), records)
}

pub(crate) fn remove() -> Result<()> {
    remove_from(Path::new(SESSION_PATH))
}

fn load_from(path: &Path) -> Result<Vec<DownloadSessionRecord>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };

    let parsed = serde_json::from_reader::<_, SessionFile>(file).and_then(|file| {
        if file.version != FORMAT_VERSION {
            return Err(serde_json::Error::io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported version {}", file.version),
            )));
        }
        Ok(file.records)
    });

    match parsed {
        Ok(records) => Ok(records),
        Err(error) => {
            let backup = backup_path(path);
            fs::copy(path, &backup)?;
            if let Some(version) = extract_version(path) {
                return Err(Error::UnsupportedVersion(version));
            }
            Err(error.into())
        }
    }
}

fn extract_version(path: &Path) -> Option<u32> {
    let file = File::open(path).ok()?;
    let value: serde_json::Value = serde_json::from_reader(file).ok()?;
    let version = value.get("version")?.as_u64()?;
    u32::try_from(version)
        .ok()
        .filter(|version| *version != FORMAT_VERSION)
}

fn save_to(path: &Path, records: &[DownloadSessionRecord]) -> Result<()> {
    let contents = serde_json::to_vec_pretty(&SessionFile {
        version: FORMAT_VERSION,
        records: records.to_vec(),
    })?;
    let temporary_path = temporary_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        file.write_all(&contents)?;
        #[cfg(unix)]
        if let Ok(metadata) = fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.sync_all()?;
        replace_file(&temporary_path, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(temporary_path: &Path, path: &Path) -> io::Result<()> {
    fs::rename(temporary_path, path)
}

#[cfg(windows)]
fn replace_file(temporary_path: &Path, path: &Path) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    const REPLACEFILE_WRITE_THROUGH: u32 = 0x00000001;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x00000001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x00000008;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    let target_exists = path_exists(path)?;
    let temporary_path: Vec<u16> = temporary_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let replaced = if target_exists {
        unsafe {
            ReplaceFileW(
                path.as_ptr(),
                temporary_path.as_ptr(),
                ptr::null(),
                REPLACEFILE_WRITE_THROUGH,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        }
    } else {
        unsafe {
            MoveFileExW(
                temporary_path.as_ptr(),
                path.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
    };

    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn path_exists(path: &Path) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn remove_from(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        ".{}.tmp.{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("session"),
        std::process::id(),
        timestamp_nanos()
    ))
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.backup.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("session"),
        timestamp_nanos()
    ))
}

fn timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn record() -> DownloadSessionRecord {
        DownloadSessionRecord {
            source_url: "https://mega.nz/file/example#key".to_string(),
            node_handle: "node-handle".to_string(),
            destination_dir: PathBuf::from("downloads/2026"),
            expected_size: 42,
        }
    }

    #[test]
    fn save_and_load_round_trip_only_safe_fields() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join(SESSION_PATH);
        let records = vec![record()];

        save_to(&path, &records).expect("save session");

        assert_eq!(load_from(&path).expect("load session"), records);
        let json = std::fs::read_to_string(path).expect("read session");
        assert!(json.contains("\"version\": 1"));
        assert!(!json.contains("progress"));
        assert!(!json.contains("crypto"));
    }

    #[test]
    fn saving_replaces_an_existing_session_file() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join(SESSION_PATH);
        let mut replacement = record();
        replacement.node_handle = "replacement-node-handle".to_string();
        replacement.destination_dir = PathBuf::from("downloads/restored");

        save_to(&path, &[record()]).expect("save initial session");
        save_to(&path, &[replacement.clone()]).expect("replace existing session");

        assert_eq!(
            load_from(&path).expect("load replacement session"),
            vec![replacement]
        );
        assert_eq!(
            std::fs::read_dir(temp.path())
                .expect("read session directory")
                .count(),
            1,
            "successful replacement must not leave temporary files"
        );
    }

    #[test]
    fn missing_session_loads_empty_and_remove_is_idempotent() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join(SESSION_PATH);

        assert!(load_from(&path).expect("load missing session").is_empty());
        remove_from(&path).expect("remove missing session");
    }

    #[test]
    fn corrupt_session_is_backed_up_before_error() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join(SESSION_PATH);
        std::fs::write(&path, b"not json").expect("write corrupt session");

        assert!(matches!(load_from(&path), Err(Error::Json(_))));
        let backups: Vec<_> = std::fs::read_dir(temp.path())
            .expect("read temp dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(".backup."))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std::fs::read(backups[0].path()).expect("read backup"),
            b"not json"
        );
    }

    #[test]
    fn unknown_version_is_backed_up_before_error() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join(SESSION_PATH);
        std::fs::write(&path, br#"{"version":99,"records":[]}"#).expect("write unknown version");

        assert!(matches!(
            load_from(&path),
            Err(Error::UnsupportedVersion(99))
        ));
        assert!(
            std::fs::read_dir(temp.path())
                .expect("read temp dir")
                .filter_map(|entry| entry.ok())
                .any(|entry| entry.file_name().to_string_lossy().contains(".backup."))
        );
    }
}
