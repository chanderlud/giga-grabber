use serde_repr::{Deserialize_repr, Serialize_repr};
use thiserror::Error;

/// The `Result` type for this library.
pub type Result<T> = std::result::Result<T, Error>;

/// The main error type for this library.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid server response type.
    #[error("invalid server response type")]
    InvalidResponseType,
    /// Invalid response format.
    #[error("invalid response format")]
    InvalidResponseFormat,
    /// Unknown user login version.
    #[error("unknown user login version: {0}")]
    UnknownUserLoginVersion(i32),
    #[error("runner error: {:?}", .0)]
    RunnerError(Vec<Error>),
    #[error("runner paused")]
    RunnerPaused,
    #[error("no proxies available")]
    NoProxies,
    /// Failed MAC verification.
    #[error("failed MAC verification")]
    MacMismatch,
    /// Failed to find node.
    #[error("failed to find node")]
    NodeNotFound,
    /// Failed to find node attribute.
    #[error("failed to find node attribute")]
    NodeAttributeNotFound,
    /// Could not get a meaningful response after maximum retries.
    #[error("could not get a meaningful response after maximum retries")]
    MaxRetriesReached,
    /// Reqwest error.
    #[cfg(feature = "reqwest")]
    #[error("reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),
    /// URL parse error.
    #[error("URL parse error: {0}")]
    UrlError(#[from] url::ParseError),
    /// JSON error.
    #[error("JSON error: {0}")]
    JsonError(#[from] json::Error),
    /// Base64 encode error.
    #[error("base64 encode error: {0}")]
    Base64EncodeError(#[from] base64::EncodeSliceError),
    /// Base64 decode error.
    #[error("base64 encode error: {0}")]
    Base64DecodeError(#[from] base64::DecodeError),
    /// PBKDF2 error.
    #[error("pbkdf2 error: {0}")]
    Pbkdf2Error(#[from] pbkdf2::password_hash::Error),
    /// MEGA error (error codes from API).
    #[error("MEGA error: {0}")]
    MegaError(#[from] ErrorCode),
    /// I/O error.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Bincode error: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("Out of bandwidth error")]
    OutOfBandwidth,
    #[error("Join error")]
    JoinError(#[from] tokio::task::JoinError),
    /// Other errors.
    #[error("unknown error: {0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

/// Error code originating from MEGA's API.
#[repr(i8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error, Serialize_repr, Deserialize_repr)]
pub enum ErrorCode {
    /// Success
    #[error("no error")]
    OK = 0,
    /// Internal Error
    #[error("internal error")]
    EINTERNAL = -1,
    /// Invalid arguments
    #[error("invalid argument")]
    EARGS = -2,
    /// Request failed (but may be retried)
    #[error("request failed, retrying")]
    EAGAIN = -3,
    /// Rate-limited
    #[error("rate limit exceeded")]
    ERATELIMIT = -4,
    /// Upload failed
    #[error("failed permanently")]
    EFAILED = -5,
    /// Too many IPs are trying to access this resource
    #[error("too many concurrent connections or transfers")]
    ETOOMANY = -6,
    /// The file packet is out of range
    #[error("out of range")]
    ERANGE = -7,
    /// The upload target url has expired
    #[error("expired")]
    EEXPIRED = -8,
    /// Object not found
    #[error("not found")]
    ENOENT = -9,
    /// Attempted circular link
    #[error("circular linkage detected")]
    ECIRCULAR = -10,
    /// Access violation (like writing to a read-only share)
    #[error("access denied")]
    EACCESS = -11,
    /// Tried to create an object that already exists
    #[error("already exists")]
    EEXIST = -12,
    /// Tried to access an incomplete resource
    #[error("incomplete")]
    EINCOMPLETE = -13,
    /// A decryption operation failed
    #[error("invalid key / decryption error")]
    EKEY = -14,
    /// Invalid or expired user session
    #[error("bad session ID")]
    ESID = -15,
    /// User blocked
    #[error("blocked")]
    EBLOCKED = -16,
    /// Request over quota
    #[error("over quota")]
    EOVERQUOTA = -17,
    /// Resource temporarily unavailable
    #[error("temporarily not available")]
    ETEMPUNAVAIL = -18,
    /// Too many connections to this resource
    #[error("connection overflow")]
    ETOOMANYCONNECTIONS = -19,
    /// Write failed
    #[error("write error")]
    EWRITE = -20,
    /// Read failed
    #[error("read error")]
    EREAD = -21,
    /// Invalid App key
    #[error("invalid application key")]
    EAPPKEY = -22,
    /// SSL verification failed
    #[error("SSL verification failed")]
    ESSL = -23,
    /// Not enough quota
    #[error("not enough quota")]
    EGOINGOVERQUOTA = -24,
    /// Need multifactor authentication
    #[error("multi-factor authentication required")]
    EMFAREQUIRED = -26,
    /// Access denied for sub-users (buisness accounts only)
    #[error("access denied for users")]
    EMASTERONLY = -27,
    /// Business account expired
    #[error("business account has expired")]
    EBUSINESSPASTDUE = -28,
    /// Over Disk Quota Paywall
    #[error("storage quota exceeded")]
    EPAYWALL = -29,
    /// Unknown error
    #[serde(other)]
    #[error("unknown error")]
    UNKNOWN = 1,
}
