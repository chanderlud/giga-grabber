use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use async_trait::async_trait;
use futures::io::AsyncRead;
use url::Url;
use dyn_clone::DynClone;

use crate::commands::{Request, Response};
use crate::error::Error;

#[cfg(feature = "reqwest")]
mod reqwest;

/// Stores the data representing a user's session.
#[derive(Debug, Clone)]
pub struct UserSession {
    /// The user's session id.
    pub(crate) sid: String,
    /// The user's master key.
    pub(crate) key: [u8; 16],
}

/// Stores the data representing the client's state.
#[derive(Debug, Clone)]
pub struct ClientState {
    /// The API's origin.
    pub(crate) origin: Url,
    /// The number of allowed retries.
    pub(crate) max_retries: usize,
    /// The minimum amount of time between retries.
    pub(crate) min_retry_delay: Duration,
    /// The maximum amount of time between retries.
    pub(crate) max_retry_delay: Duration,
    /// The timeout duration to use for each request.
    pub(crate) timeout: Option<Duration>,
    /// Whether to use HTTPS for file downloads and uploads, instead of plain HTTP.
    ///
    /// Using plain HTTP for file transfers is fine because the file contents are already encrypted,
    /// making protocol-level encryption a bit redundant and potentially slowing down the transfer.
    pub(crate) https: bool,
    /// The request counter, for idempotency.
    pub(crate) id_counter: Arc<AtomicU64>,
    /// The user's session.
    pub(crate) session: Option<UserSession>,
}

#[async_trait]
pub trait HttpClient: DynClone {
    /// Sends the given requests to MEGA's API and parses the responses accordingly.
    async fn send_requests(
        &self,
        state: &ClientState,
        requests: &[Request],
        query_params: &[(&str, &str)],
    ) -> Result<Vec<Response>, Error>;

    /// Initiates a simple GET request, returning the response body as a reader.
    async fn get(&self, url: Url) -> Result<Pin<Box<dyn AsyncRead + Send>>, Error>;

    /// Initiates a simple POST request, with body and optional `content-length`, returning the response body as a reader.
    async fn post(
        &self,
        url: Url,
        body: Pin<Box<dyn AsyncRead + Send + Sync>>,
        content_length: Option<u64>,
    ) -> Result<Pin<Box<dyn AsyncRead>>, Error>;
}
