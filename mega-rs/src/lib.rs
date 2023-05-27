//! This is an API client library for interacting with MEGA's API using Rust.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use aes::Aes128;
use base64::prelude::{BASE64_STANDARD_NO_PAD, BASE64_URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, TimeZone, Utc};
use cipher::{BlockDecryptMut, BlockEncrypt, BlockEncryptMut, KeyInit, KeyIvInit, StreamCipher};
use cipher::generic_array::GenericArray;
use cipher::StreamCipherSeek;
use futures::{AsyncSeek, AsyncSeekExt, stream, StreamExt};
use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::sleep;
use url::Url;

use crate::commands::{Request, Response, UploadAttributes};
pub use crate::commands::NodeKind;
pub use crate::error::{Error, ErrorCode, Result};
use crate::http::{ClientState, HttpClient, UserSession};
use crate::metadata::MetaData;
use crate::utils::FileAttributes;
pub use crate::utils::StorageQuotas;

mod commands;
mod error;
mod http;
mod utils;
mod metadata;

pub const MIN_SECTION_SIZE: usize = 1024 * 1024;
// 1 MB
pub const MAX_SECTION_SIZE: usize = 1024 * 1024 * 128;
// 128 MB
pub(crate) const DEFAULT_API_ORIGIN: &str = "https://g.api.mega.co.nz/";

/// A builder to initialize a [`Client`] instance.
pub struct ClientBuilder {
    /// The API's origin.
    origin: Url,
    /// The number of allowed retries.
    max_retries: usize,
    /// The minimum amount of time between retries.
    min_retry_delay: Duration,
    /// The maximum amount of time between retries.
    max_retry_delay: Duration,
    /// The timeout duration to use for each request.
    timeout: Option<Duration>,
    /// Whether to use HTTPS for file downloads and uploads, instead of plain HTTP.
    ///
    /// Using plain HTTP for file transfers is fine because the file contents are already encrypted,
    /// making protocol-level encryption a bit redundant and potentially slowing down the transfer.
    https: bool,
}

impl ClientBuilder {
    /// Creates a default [`ClientBuilder`].
    pub fn new() -> Self {
        Self {
            origin: Url::parse(DEFAULT_API_ORIGIN).unwrap(),
            max_retries: 10,
            min_retry_delay: Duration::from_millis(10),
            max_retry_delay: Duration::from_secs(5),
            timeout: Some(Duration::from_secs(10)),
            https: false,
        }
    }

    /// Sets the API's origin.
    pub fn origin(mut self, origin: impl Into<Url>) -> Self {
        self.origin = origin.into();
        self
    }

    /// Sets the maximum amount of retries.
    pub fn max_retries(mut self, amount: usize) -> Self {
        self.max_retries = amount;
        self
    }

    /// Sets the minimum delay duration between retries.
    pub fn min_retry_delay(mut self, delay: Duration) -> Self {
        self.min_retry_delay = delay;
        self
    }

    /// Sets the maximum delay duration between retries.
    pub fn max_retry_delay(mut self, delay: Duration) -> Self {
        self.max_retry_delay = delay;
        self
    }

    /// Sets the timeout duration to use for each request.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Sets whether to use HTTPS for file uploads and downloads, instead of plain HTTP.
    pub fn https(mut self, value: bool) -> Self {
        self.https = value;
        self
    }

    /// Builds a [`Client`] instance with the current settings and the specified HTTP client.
    pub fn build<T: HttpClient + 'static>(self, client: T) -> Result<Client> {
        let state = ClientState {
            origin: self.origin,
            max_retries: self.max_retries,
            min_retry_delay: self.min_retry_delay,
            max_retry_delay: self.max_retry_delay,
            timeout: self.timeout,
            https: self.https,
            id_counter: AtomicU64::new(0),
            session: None,
        };

        Ok(Client {
            state,
            client: Box::new(client),
        })
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// The MEGA API Client itself.
pub struct Client {
    /// The client's state.
    pub(crate) state: ClientState,
    /// The HTTP client.
    pub(crate) client: Box<dyn HttpClient>,
}

impl Client {
    /// Creates a builder to initialize a [`Client`] instance.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Sends a request to the MEGA API.
    pub(crate) async fn send_requests(&self, requests: &[Request]) -> Result<Vec<Response>> {
        self.client.send_requests(&self.state, requests, &[]).await
    }

    /// Authenticates this session with MEGA.
    pub async fn login(&mut self, email: &str, password: &str, mfa: Option<&str>) -> Result<()> {
        let email = email.to_lowercase();

        let request = Request::PreLogin {
            user: email.clone(),
        };
        let responses = self.send_requests(&[request]).await?;

        let response = match responses.as_slice() {
            [Response::PreLogin(response)] => response,
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        };

        let (login_key, user_handle) = match (response.version, response.salt.as_ref()) {
            (1, _) => {
                let key = utils::prepare_key_v1(password.as_bytes());

                let mut hash = GenericArray::from([0u8; 16]);
                for (i, x) in email.bytes().enumerate() {
                    hash[i % 16] ^= x;
                }

                let aes = Aes128::new(key.as_slice().into());
                for _ in 0..16384 {
                    aes.encrypt_block(&mut hash);
                }

                let mut user_handle = [0u8; 8];
                user_handle[..4].copy_from_slice(&hash[0..4]);
                user_handle[4..].copy_from_slice(&hash[8..12]);

                let user_handle = BASE64_URL_SAFE_NO_PAD.encode(&user_handle);

                (key, user_handle)
            }
            (2, Some(salt)) => {
                // TODO: investigate if we really need to re-encode using standard base64 alphabet (for the `pbkdf2` crate).
                let salt = BASE64_URL_SAFE_NO_PAD.decode(salt)?;
                let salt = BASE64_STANDARD_NO_PAD.encode(salt);

                let key = utils::prepare_key_v2(password.as_bytes(), salt.as_str())?;

                let (key, user_handle) = key.split_at(16);

                let key = <[u8; 16]>::try_from(key).unwrap();
                let user_handle = BASE64_URL_SAFE_NO_PAD.encode(user_handle);

                (key, user_handle)
            }
            (2, None) => {
                // missing salt
                todo!()
            }
            (version, _) => {
                return Err(Error::UnknownUserLoginVersion(version));
            }
        };

        let request = Request::Login {
            user: email.clone(),
            hash: user_handle.clone(),
            si: None,
            mfa: mfa.map(|it| it.to_string()),
            session_key: None,
        };
        let responses = self.send_requests(&[request]).await?;

        let response = match responses.as_slice() {
            [Response::Login(response)] => response,
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        };

        let mut key = BASE64_URL_SAFE_NO_PAD.decode(&response.k)?;
        utils::decrypt_ebc_in_place(&login_key, &mut key);

        let t = BASE64_URL_SAFE_NO_PAD.decode(&response.csid)?;
        let (m, _) = utils::get_mpi(&t);

        let mut privk = BASE64_URL_SAFE_NO_PAD.decode(&response.privk)?;
        utils::decrypt_ebc_in_place(&key, &mut privk);

        let (p, q, d) = utils::get_rsa_key(&privk);
        let r = utils::decrypt_rsa(m, p, q, d);

        let sid = BASE64_URL_SAFE_NO_PAD.encode(&r.to_bytes_be()[..43]);

        self.state.session = Some(UserSession {
            sid,
            key: key[..16].try_into().unwrap(),
        });

        Ok(())
    }

    /// Logs out of the current session with MEGA.
    pub async fn logout(&mut self) -> Result<()> {
        let request = Request::Logout {};
        let responses = self.send_requests(&[request]).await?;

        match responses.as_slice() {
            [Response::Error(ErrorCode::OK)] => {
                self.state.session = None;
                Ok(())
            }
            [Response::Error(code)] => Err(Error::from(*code)),
            _ => Err(Error::InvalidResponseType),
        }
    }

    /// Fetches all nodes from the user's own MEGA account.
    pub async fn fetch_own_nodes(&self) -> Result<Nodes> {
        let request = Request::FetchNodes { c: 1, r: None };
        let responses = self.send_requests(&[request]).await?;

        let files = match responses.as_slice() {
            [Response::FetchNodes(files)] => files,
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        };

        let session = self.state.session.as_ref().unwrap();

        let mut nodes = HashMap::<String, Node>::default();

        for file in &files.nodes {
            let (thumbnail_handle, preview_image_handle) =
                if let Some(file_attr) = file.file_attr.as_ref() {
                    let mut thumbnail_handle = None;
                    let mut preview_image_handle = None;

                    let iterator = file_attr
                        .split('/')
                        .filter_map(|it| it.split_once(':')?.1.split_once('*'));

                    for (key, val) in iterator {
                        match key {
                            "0" => {
                                thumbnail_handle = Some(val.to_string());
                            }
                            "1" => {
                                preview_image_handle = Some(val.to_string());
                            }
                            _ => continue,
                        }
                    }

                    (thumbnail_handle, preview_image_handle)
                } else {
                    (None, None)
                };

            match file.kind {
                NodeKind::File | NodeKind::Folder => {
                    let (file_user, file_key) = file.key.as_ref().unwrap().split_once(':').unwrap();

                    if file.user == file_user {
                        // self-owned file or folder

                        let mut file_key = BASE64_URL_SAFE_NO_PAD.decode(file_key)?;
                        utils::decrypt_ebc_in_place(&session.key, &mut file_key);

                        let attrs = {
                            let mut file_key = file_key.clone();
                            utils::unmerge_key_mac(&mut file_key);

                            let mut buffer = BASE64_URL_SAFE_NO_PAD.decode(&file.attr)?;
                            FileAttributes::decrypt_and_unpack(
                                &file_key[..16],
                                buffer.as_mut_slice(),
                            )?
                        };

                        let node = Node {
                            name: attrs.name,
                            hash: file.hash.clone(),
                            size: file.sz.unwrap_or(0),
                            kind: file.kind,
                            parent: (!file.parent.is_empty()).then(|| file.parent.clone()),
                            children: nodes
                                .values()
                                .filter_map(|it| {
                                    let parent = it.parent.as_ref()?;
                                    (parent == &file.hash).then(|| file.hash.clone())
                                })
                                .collect(),
                            key: file_key,
                            created_at: Some(Utc.timestamp_opt(file.ts as i64, 0).unwrap()),
                            download_id: None,
                            thumbnail_handle,
                            preview_image_handle,
                        };

                        if let Some(parent) = nodes.get_mut(&file.parent) {
                            parent.children.push(node.hash.clone());
                        }

                        nodes.insert(node.hash.clone(), node);
                    }
                }
                NodeKind::Root => {
                    let node = Node {
                        name: String::from("Root"),
                        hash: file.hash.clone(),
                        size: file.sz.unwrap_or(0),
                        kind: NodeKind::Root,
                        parent: None,
                        children: nodes
                            .values()
                            .filter_map(|it| {
                                let parent = it.parent.as_ref()?;
                                (parent == &file.hash).then(|| file.hash.clone())
                            })
                            .collect(),
                        key: <_>::default(),
                        created_at: Some(Utc.timestamp_opt(file.ts as i64, 0).unwrap()),
                        download_id: None,
                        thumbnail_handle,
                        preview_image_handle,
                    };
                    nodes.insert(node.hash.clone(), node);
                }
                NodeKind::Inbox => {
                    let node = Node {
                        name: String::from("Inbox"),
                        hash: file.hash.clone(),
                        size: file.sz.unwrap_or(0),
                        kind: NodeKind::Inbox,
                        parent: None,
                        children: nodes
                            .values()
                            .filter_map(|it| {
                                let parent = it.parent.as_ref()?;
                                (parent == &file.hash).then(|| file.hash.clone())
                            })
                            .collect(),
                        key: <_>::default(),
                        created_at: Some(Utc.timestamp_opt(file.ts as i64, 0).unwrap()),
                        download_id: None,
                        thumbnail_handle,
                        preview_image_handle,
                    };
                    nodes.insert(node.hash.clone(), node);
                }
                NodeKind::Trash => {
                    let node = Node {
                        name: String::from("Trash"),
                        hash: file.hash.clone(),
                        size: file.sz.unwrap_or(0),
                        kind: NodeKind::Trash,
                        parent: None,
                        children: nodes
                            .values()
                            .filter_map(|it| {
                                let parent = it.parent.as_ref()?;
                                (parent == &file.hash).then(|| file.hash.clone())
                            })
                            .collect(),
                        key: <_>::default(),
                        created_at: Some(Utc.timestamp_opt(file.ts as i64, 0).unwrap()),
                        download_id: None,
                        thumbnail_handle,
                        preview_image_handle,
                    };
                    nodes.insert(node.hash.clone(), node);
                }
                NodeKind::Unknown => continue,
            }
        }

        Ok(Nodes::new(nodes))
    }

    /// Fetches all nodes from a public MEGA link.
    pub async fn fetch_public_nodes(&self, url: &str) -> Result<Nodes> {
        // supported URL formats:
        // - https://mega.nz/file/{node_id}#{node_key}
        // - https://mega.nz/folder/{node_id}#{node_key}

        let shared_url = Url::parse(url)?;
        let (node_kind, node_id) = {
            let segments: Vec<&str> = shared_url.path().split('/').skip(1).collect();
            match segments.as_slice() {
                ["file", file_id] => (NodeKind::File, file_id.to_string()),
                ["folder", folder_id] => (NodeKind::Folder, folder_id.to_string()),
                _ => {
                    // TODO: replace with its own error enum variant.
                    return Err(Error::Other("invalid URL format".into()));
                }
            }
        };

        let node_key = {
            let fragment = shared_url
                .fragment()
                .ok_or_else(|| Error::Other("invalid URL format".into()))?;
            let key = fragment.split_once('/').map_or(fragment, |it| it.0);
            BASE64_URL_SAFE_NO_PAD.decode(key)?
        };

        let mut nodes = HashMap::<String, Node>::default();

        match node_kind {
            NodeKind::File => {
                let request = Request::Download {
                    g: 1,
                    ssl: 0,
                    p: Some(node_id.clone()),
                    n: None,
                };
                let responses = self.send_requests(&[request]).await?;

                let file = match responses.as_slice() {
                    [Response::Download(file)] => file,
                    [Response::Error(code)] => {
                        return Err(Error::from(*code));
                    }
                    _ => {
                        return Err(Error::InvalidResponseType);
                    }
                };

                let attrs = {
                    let mut node_key = node_key.clone();
                    utils::unmerge_key_mac(&mut node_key);

                    let mut buffer = BASE64_URL_SAFE_NO_PAD.decode(&file.attr)?;
                    FileAttributes::decrypt_and_unpack(&node_key[..16], buffer.as_mut_slice())?
                };

                let node = Node {
                    name: attrs.name,
                    hash: node_id.clone(),
                    size: file.size,
                    kind: NodeKind::File,
                    parent: None,
                    children: Vec::default(),
                    key: node_key,
                    created_at: None,
                    download_id: Some(node_id),
                    thumbnail_handle: None,
                    preview_image_handle: None,
                };

                nodes.insert(node.hash.clone(), node);

                Ok(Nodes::new(nodes))
            }
            NodeKind::Folder => {
                let request = Request::FetchNodes { c: 1, r: Some(1) };
                let responses = self
                    .client
                    .send_requests(&self.state, &[request], &[("n", node_id.as_str())])
                    .await?;

                let files = match responses.as_slice() {
                    [Response::FetchNodes(files)] => files,
                    [Response::Error(code)] => {
                        return Err(Error::from(*code));
                    }
                    _ => {
                        return Err(Error::InvalidResponseType);
                    }
                };

                for file in &files.nodes {
                    match file.kind {
                        NodeKind::File | NodeKind::Folder => {
                            let (_, file_key) = file.key.as_ref().unwrap().split_once(':').unwrap();

                            let mut file_key = BASE64_URL_SAFE_NO_PAD.decode(file_key)?;
                            utils::decrypt_ebc_in_place(&node_key, &mut file_key);

                            let attrs = {
                                let mut file_key = file_key.clone();
                                utils::unmerge_key_mac(&mut file_key);

                                let mut buffer = BASE64_URL_SAFE_NO_PAD.decode(&file.attr)?;
                                FileAttributes::decrypt_and_unpack(
                                    &file_key[..16],
                                    buffer.as_mut_slice(),
                                )?
                            };

                            let (thumbnail_handle, preview_image_handle) =
                                if let Some(file_attr) = file.file_attr.as_ref() {
                                    let mut thumbnail_handle = None;
                                    let mut preview_image_handle = None;

                                    let iterator = file_attr
                                        .split('/')
                                        .filter_map(|it| it.split_once(':')?.1.split_once('*'));

                                    for (key, val) in iterator {
                                        match key {
                                            "0" => {
                                                thumbnail_handle = Some(val.to_string());
                                            }
                                            "1" => {
                                                preview_image_handle = Some(val.to_string());
                                            }
                                            _ => continue,
                                        }
                                    }

                                    (thumbnail_handle, preview_image_handle)
                                } else {
                                    (None, None)
                                };

                            let node = Node {
                                name: attrs.name,
                                hash: file.hash.clone(),
                                size: file.sz.unwrap_or(0),
                                kind: file.kind,
                                parent: (!file.parent.is_empty()).then(|| file.parent.clone()),
                                children: nodes
                                    .values()
                                    .filter_map(|it| {
                                        let parent = it.parent.as_ref()?;
                                        (parent == &file.hash).then(|| file.hash.clone())
                                    })
                                    .collect(),
                                key: file_key,
                                created_at: Some(Utc.timestamp_opt(file.ts as i64, 0).unwrap()),
                                download_id: Some(node_id.clone()),
                                thumbnail_handle,
                                preview_image_handle,
                            };

                            if let Some(parent) = nodes.get_mut(&file.parent) {
                                parent.children.push(node.hash.clone());
                            }

                            nodes.insert(node.hash.clone(), node);
                        }
                        _ => unreachable!(),
                    }
                }

                Ok(Nodes::new(nodes))
            }
            _ => unreachable!(),
        }
    }

    /// Returns the status of the current storage quotas.
    pub async fn get_storage_quotas(&self) -> Result<StorageQuotas> {
        let responses = self
            .send_requests(&[Request::Quota { xfer: 1, strg: 1 }])
            .await?;

        let [Response::Quota(quota)] = responses.as_slice() else {
            return Err(Error::InvalidResponseType);
        };

        Ok(StorageQuotas {
            memory_used: quota.cstrg,
            memory_total: quota.mstrg,
        })
    }

    /// Downloads a file, identified by its hash, into the given writer.
    pub async fn download_node<W: AsyncWrite>(&self, node: &Node, writer: W, threads: usize, metadata_path: &PathBuf) -> Result<()>
        where
            W: AsyncWrite + AsyncSeek + Unpin,
    {
        let responses = if let Some(download_id) = node.download_id() {
            let request = if node.hash.as_str() == download_id {
                Request::Download {
                    g: 1,
                    ssl: if self.state.https { 2 } else { 0 },
                    n: None,
                    p: Some(node.hash.clone()),
                }
            } else {
                Request::Download {
                    g: 1,
                    ssl: if self.state.https { 2 } else { 0 },
                    n: Some(node.hash.clone()),
                    p: None,
                }
            };

            self.client
                .send_requests(&self.state, &[request], &[("n", download_id)])
                .await?
        } else {
            let request = Request::Download {
                g: 1,
                ssl: if self.state.https { 2 } else { 0 },
                p: None,
                n: Some(node.hash.clone()),
            };

            self.send_requests(&[request]).await?
        };

        let response = match responses.as_slice() {
            [Response::Download(response)] => response,
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        };

        let mut file_key = node.key.clone();
        utils::unmerge_key_mac(&mut file_key);

        let mut section_size = response.size as usize / threads;

        let mut file_iv = [0u8; 16];

        file_iv[..8].copy_from_slice(&node.key[16..24]);
        let ctr = ctr::Ctr128BE::<Aes128>::new(file_key[..16].into(), (&file_iv).into());

        file_iv[8..].copy_from_slice(&node.key[16..24]);

        if section_size < MIN_SECTION_SIZE {
            section_size = MIN_SECTION_SIZE;
        }

        if section_size > MAX_SECTION_SIZE {
            section_size = MAX_SECTION_SIZE;
        }

        let mut sections = generate_sections(response.size as usize, section_size);
        let metadata = MetaData::new(&sections, metadata_path).await?;

        if metadata_path.exists() {
            let completed_sections = metadata.incomplete_sections();
            sections = sections.iter().filter(|(start, _end)| completed_sections.contains(start)).cloned().collect();
        }

        let urls = generate_section_urls(&response.download_url, &sections);
        let shared_writer = Arc::new(Mutex::new(writer));
        let shared_metadata = Arc::new(Mutex::new(metadata));

        let bodies = stream::iter(urls)
            .map(|(start, url)| {
                let ctr = ctr.clone();

                async move {
                    let mut retries = 0;

                    loop {
                        match self.client.get(url.clone()).await {
                            Ok(mut reader) => {
                                let mut buffer = Vec::with_capacity(section_size);
                                retries = 0;

                                let result = loop {
                                    match reader.read_to_end(&mut buffer).await {
                                        Ok(_) => {
                                            if buffer.len() == 0 {
                                                break Err(Error::InvalidResponseFormat)
                                            } else {
                                                break Ok(buffer)
                                            }

                                        }
                                        Err(e) => {
                                            if retries < self.state.max_retries {
                                                retries += 1;
                                                sleep(self.state.max_retry_delay).await;
                                            } else {
                                                break Err(Error::IoError(e))
                                            }
                                        }
                                    }
                                };

                                match result {
                                    Ok(mut buffer) => {
                                        let mut updated_ctr = ctr.clone();
                                        updated_ctr.seek(start as u64);
                                        updated_ctr.apply_keystream(&mut buffer);

                                        return Ok::<_, Error>((start, buffer))
                                    }
                                    Err(_e) => {
                                        if retries < self.state.max_retries {
                                            retries += 1;
                                            sleep(self.state.max_retry_delay).await;
                                        } else {
                                            return Err(Error::MaxRetriesReached)
                                        }
                                    }
                                }
                            }
                            Err(_e) => {
                                if retries < self.state.max_retries {
                                    retries += 1;
                                    sleep(self.state.max_retry_delay).await;
                                } else {
                                    return Err(Error::MaxRetriesReached)
                                }
                            }
                        }
                    }
                }
            })
            .buffer_unordered(threads);

        bodies.for_each(|buffer| async {
            let (start, data) = buffer.unwrap();
            let mut writer = shared_writer.lock().await;

            writer.flush().await.unwrap();
            let _ = writer.seek(SeekFrom::Start(start as u64)).await.unwrap();
            let _ = writer.write_all(&data).await.unwrap();

            let mut metadata = shared_metadata.lock().await;
            metadata.complete(start).await.unwrap();
        }).await;

        Ok(())
    }

    /// Uploads a file within a parent folder.
    pub async fn upload_node<R: AsyncRead>(
        &self,
        parent: &Node,
        name: &str,
        size: u64,
        reader: R,
    ) -> Result<()> {
        let request = Request::Upload {
            s: size,
            ssl: if self.state.https { 2 } else { 0 },
        };
        let responses = self.send_requests(&[request]).await?;

        let response = match responses.as_slice() {
            [Response::Upload(response)] => response,
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        };

        let (file_key, file_iv_seed): ([u8; 16], [u8; 8]) = rand::random();

        let mut file_iv = [0u8; 16];
        file_iv[..8].copy_from_slice(&file_iv_seed);

        let mut ctr = ctr::Ctr128BE::<Aes128>::new((&file_key).into(), (&file_iv).into());
        file_iv[8..].copy_from_slice(&file_iv_seed);

        let (pipe_reader, mut pipe_writer) = sluice::pipe::pipe();

        let fut_1 = async move {
            let mut chunk_size: u64 = 131_072; // 2^17
            let mut cur_mac = [0u8; 16];

            let mut final_mac_data = [0u8; 16];
            let mut final_mac =
                cbc::Encryptor::<Aes128>::new((&file_key).into(), (&final_mac_data).into());

            let mut buffer = Vec::with_capacity(chunk_size as usize);

            let reader = reader.take(size);

            futures::pin_mut!(reader);
            loop {
                buffer.clear();

                let bytes_read = (&mut reader)
                    .take(chunk_size)
                    .read_to_end(&mut buffer)
                    .await?;

                if bytes_read == 0 {
                    break;
                }

                let (chunks, leftover) = buffer.split_at(buffer.len() - buffer.len() % 16);

                let mut mac = cbc::Encryptor::<Aes128>::new((&file_key).into(), (&file_iv).into());

                for chunk in chunks.chunks_exact(16) {
                    mac.encrypt_block_b2b_mut(chunk.into(), (&mut cur_mac).into());
                }

                if !leftover.is_empty() {
                    let mut padded_chunk = [0u8; 16];
                    padded_chunk[..leftover.len()].copy_from_slice(leftover);
                    mac.encrypt_block_b2b_mut((&padded_chunk).into(), (&mut cur_mac).into());
                }

                final_mac.encrypt_block_b2b_mut((&cur_mac).into(), (&mut final_mac_data).into());

                ctr.apply_keystream(&mut buffer);
                pipe_writer.write_all(&buffer).await?;

                if chunk_size < 1_048_576 {
                    chunk_size += 131_072;
                }
            }

            Ok(final_mac_data)
        };

        let url = Url::parse(format!("{0}/{1}", response.upload_url, 0).as_str())?;
        let fut_2 = async move {
            let mut reader = self
                .client
                .post(url, Box::pin(pipe_reader), Some(size))
                .await?;

            let mut buffer = Vec::default();
            reader.read_to_end(&mut buffer).await?;

            Ok::<_, Error>(String::from_utf8_lossy(&buffer).into_owned())
        };

        let (mut final_mac_data, completion_handle) = futures::try_join!(fut_1, fut_2)?;

        for i in 0..4 {
            final_mac_data[i] = final_mac_data[i] ^ final_mac_data[i + 4];
            final_mac_data[i + 4] = final_mac_data[i + 8] ^ final_mac_data[i + 12];
        }

        let file_attr = FileAttributes {
            name: name.to_string(),
            c: None,
        };

        let file_attr_buffer = {
            let buffer = file_attr.pack_and_encrypt(&file_key)?;
            BASE64_URL_SAFE_NO_PAD.encode(&buffer)
        };

        let mut key = [0u8; 32];
        key[..16].copy_from_slice(&file_key);
        key[16..24].copy_from_slice(&file_iv[..8]);
        key[24..].copy_from_slice(&final_mac_data[..8]);
        utils::merge_key_mac(&mut key);

        let session = self.state.session.as_ref().unwrap();
        utils::encrypt_ebc_in_place(&session.key, &mut key);

        let key_b64 = BASE64_URL_SAFE_NO_PAD.encode(&key);

        let attrs = UploadAttributes {
            kind: NodeKind::File,
            key: key_b64,
            attr: file_attr_buffer,
            completion_handle,
            file_attr: None,
        };

        let idempotence_id = utils::random_string(10);

        let request = Request::UploadComplete {
            t: parent.hash.clone(),
            n: [attrs],
            i: idempotence_id,
        };

        let responses = self.send_requests(&[request]).await?;

        match responses.as_slice() {
            [Response::UploadComplete(_)] => {}
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        };

        Ok(())
    }

    /// Downloads the node's attribute payload into the given writer, if it exists.
    pub(crate) async fn download_attribute<W: AsyncWrite>(
        &self,
        kind: AttributeKind,
        attr_handle: &str,
        node: &Node,
        writer: W,
    ) -> Result<()> {
        let request = Request::UploadFileAttributes {
            h: None,
            fah: Some(attr_handle.to_string()),
            s: None,
            ssl: if self.state.https { 2 } else { 0 },
            r: Some(1),
        };
        let responses = self.send_requests(&[request]).await?;

        let [Response::UploadFileAttributes(response)] = responses.as_slice() else {
            return Err(Error::InvalidResponseType);
        };

        let attr_handle = BASE64_URL_SAFE_NO_PAD.decode(attr_handle)?;

        let mut reader = {
            let url = format!("{0}/{1}", response.p, kind as u8);
            let url = Url::parse(url.as_str())?;
            let len = attr_handle.len();
            let body = futures::io::Cursor::new(attr_handle);
            self.client
                .post(url, Box::pin(body), Some(len as _))
                .await?
        };

        let _id = {
            let mut hash = [0u8; 8];
            reader.read_exact(&mut hash).await?;
            hash
        };

        let len = {
            let mut len_bytes = [0u8; 4];
            reader.read_exact(&mut len_bytes).await?;
            u32::from_le_bytes(len_bytes)
        };

        let file_key = {
            let mut file_key = node.key.clone();
            utils::unmerge_key_mac(&mut file_key);
            file_key
        };

        let mut cbc = cbc::Decryptor::<Aes128>::new(file_key[..16].into(), (&[0u8; 16]).into());

        futures::pin_mut!(writer);
        let mut reader = reader.take(len.into());

        let mut block = Vec::default();
        loop {
            block.clear();
            let bytes_read = (&mut reader).take(16).read_to_end(&mut block).await?;

            if bytes_read == 0 {
                break;
            }

            if bytes_read < 16 {
                let padding = std::iter::repeat(0).take(16 - bytes_read);
                block.extend(padding);
            }

            cbc.decrypt_block_mut(block.as_mut_slice().into());
            writer.write_all(&block[..bytes_read]).await?;
        }

        Ok(())
    }

    /// Downloads the node's thumbnail image into the given writer, if it exists.
    pub async fn download_thumbnail<W: AsyncWrite>(&self, node: &Node, writer: W) -> Result<()> {
        let Some(attr_handle) = node.thumbnail_handle.as_deref() else {
            return Err(Error::NodeAttributeNotFound);
        };

        self.download_attribute(AttributeKind::Thumbnail, attr_handle, node, writer)
            .await
    }

    /// Downloads the node's preview image into the given writer, if it exists.
    pub async fn download_preview_image<W: AsyncWrite>(
        &self,
        node: &Node,
        writer: W,
    ) -> Result<()> {
        let Some(preview_image_handle) = node.preview_image_handle.as_deref() else {
            return Err(Error::NodeAttributeNotFound);
        };

        self.download_attribute(
            AttributeKind::PreviewImage,
            preview_image_handle,
            node,
            writer,
        )
            .await
    }

    /// Uploads an attribute's payload for an existing node from a given reader.
    pub(crate) async fn upload_attribute<R: AsyncRead>(
        &self,
        kind: AttributeKind,
        node: &Node,
        size: u64,
        reader: R,
    ) -> Result<()> {
        let request = Request::UploadFileAttributes {
            h: Some(node.hash.clone()),
            fah: None,
            s: Some(size),
            ssl: if self.state.https { 2 } else { 0 },
            r: None,
        };
        let responses = self.send_requests(&[request]).await?;

        let [Response::UploadFileAttributes(response)] = responses.as_slice() else {
            return Err(Error::InvalidResponseType);
        };

        let file_key = {
            let mut file_key = node.key.clone();
            utils::unmerge_key_mac(&mut file_key);
            file_key
        };

        let mut cbc = cbc::Encryptor::<Aes128>::new(file_key[..16].into(), (&[0u8; 16]).into());

        let (pipe_reader, mut pipe_writer) = sluice::pipe::pipe();

        let fut_1 = async move {
            let reader = reader.take(size);
            futures::pin_mut!(reader);

            let mut block = Vec::default();
            loop {
                block.clear();
                let bytes_read = (&mut reader).take(16).read_to_end(&mut block).await?;

                if bytes_read == 0 {
                    break;
                }

                if bytes_read < 16 {
                    let padding = std::iter::repeat(0).take(16 - bytes_read);
                    block.extend(padding);
                }

                cbc.encrypt_block_mut(block.as_mut_slice().into());
                pipe_writer.write_all(&block[..bytes_read]).await?;
            }

            Ok(())
        };

        let url = Url::parse(format!("{0}/{1}", response.p, kind as u8).as_str())?;
        let fut_2 = async move {
            let mut reader = self
                .client
                .post(url, Box::pin(pipe_reader), Some(size))
                .await?;

            let mut buffer = Vec::default();
            reader.read_to_end(&mut buffer).await?;

            Ok::<_, Error>(BASE64_URL_SAFE_NO_PAD.encode(&buffer))
        };

        let (_, fah) = futures::try_join!(fut_1, fut_2)?;

        let request = Request::PutFileAttributes {
            n: node.hash.clone(),
            fa: format!("{0}*{fah}", kind as u8),
        };
        let responses = self.send_requests(&[request]).await?;

        match responses.as_slice() {
            [Response::PutFileAttributes(_)] => {}
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        }

        Ok(())
    }

    /// Uploads a thumbnail image for an existing node from a given reader.
    pub async fn upload_thumbnail<R: AsyncRead>(
        &self,
        node: &Node,
        size: u64,
        reader: R,
    ) -> Result<()> {
        self.upload_attribute(AttributeKind::Thumbnail, node, size, reader)
            .await
    }

    /// Uploads a preview image for an existing node from a given reader.
    pub async fn upload_preview_image<R: AsyncRead>(
        &self,
        node: &Node,
        size: u64,
        reader: R,
    ) -> Result<()> {
        self.upload_attribute(AttributeKind::PreviewImage, node, size, reader)
            .await
    }

    /// Creates a new directory.
    pub async fn create_dir(&self, parent: &Node, name: &str) -> Result<()> {
        let (file_key, file_iv_seed): ([u8; 16], [u8; 8]) = rand::random();

        let mut file_iv = [0u8; 16];
        file_iv[..8].copy_from_slice(&file_iv_seed);

        let file_attr = FileAttributes {
            name: name.to_string(),
            c: None,
        };

        let file_attr_buffer = {
            let buffer = file_attr.pack_and_encrypt(&file_key)?;
            BASE64_URL_SAFE_NO_PAD.encode(&buffer)
        };

        let mut key = [0u8; 24];
        key[..16].copy_from_slice(&file_key);
        key[16..].copy_from_slice(&file_iv[..8]);
        utils::merge_key_mac(&mut key);

        let session = self.state.session.as_ref().unwrap();
        utils::encrypt_ebc_in_place(&session.key, &mut key);

        let key_b64 = BASE64_URL_SAFE_NO_PAD.encode(&key);

        let attrs = UploadAttributes {
            kind: NodeKind::Folder,
            key: key_b64,
            attr: file_attr_buffer,
            completion_handle: String::from("xxxxxxxx"),
            file_attr: None,
        };

        let idempotence_id = utils::random_string(10);

        let request = Request::UploadComplete {
            t: parent.hash.clone(),
            n: [attrs],
            i: idempotence_id,
        };

        let responses = self.send_requests(&[request]).await?;

        match responses.as_slice() {
            [Response::UploadComplete(_)] => {}
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        };

        Ok(())
    }

    /// Renames a node.
    pub async fn rename_node(&self, node: &Node, name: &str) -> Result<()> {
        let file_key = {
            let mut file_key = node.key.clone();
            utils::unmerge_key_mac(&mut file_key);
            file_key
        };

        let file_attr = FileAttributes {
            name: name.to_string(),
            c: None,
        };

        let file_attr_buffer = {
            let buffer = file_attr.pack_and_encrypt(&file_key[..16])?;
            BASE64_URL_SAFE_NO_PAD.encode(&buffer)
        };

        let idempotence_id = utils::random_string(10);

        let request = Request::SetFileAttributes {
            n: node.hash.clone(),
            key: None,
            attr: file_attr_buffer,
            i: idempotence_id,
        };

        let responses = self.send_requests(&[request]).await?;

        match responses.as_slice() {
            [Response::Error(ErrorCode::OK)] => {}
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        }

        Ok(())
    }

    /// Moves a node to a different folder.
    pub async fn move_node(&self, node: &Node, parent: &Node) -> Result<()> {
        let idempotence_id = utils::random_string(10);

        let request = Request::Move {
            n: node.hash.clone(),
            t: parent.hash.clone(),
            i: idempotence_id,
        };

        let responses = self.send_requests(&[request]).await?;

        match responses.as_slice() {
            [Response::Error(ErrorCode::OK)] => {}
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        }

        Ok(())
    }

    /// Deletes a node.
    pub async fn delete_node(&self, node: &Node) -> Result<()> {
        let idempotence_id = utils::random_string(10);

        let request = Request::Delete {
            n: node.hash.clone(),
            i: idempotence_id,
        };

        let responses = self.send_requests(&[request]).await?;

        match responses.as_slice() {
            [Response::Error(ErrorCode::OK)] => {}
            [Response::Error(code)] => {
                return Err(Error::from(*code));
            }
            _ => {
                return Err(Error::InvalidResponseType);
            }
        }

        Ok(())
    }
}

/// Represents a node stored in MEGA (either a file or a folder).
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// The name of the node.
    pub(crate) name: String,
    /// The hash (or handle) of the node.
    pub(crate) hash: String,
    /// The size (in bytes) of the node.
    pub(crate) size: u64,
    /// The kind of the node.
    pub(crate) kind: NodeKind,
    /// The hash (or handle) of the node's parent.
    pub(crate) parent: Option<String>,
    /// The hashes (or handles) of the node's children.
    pub(crate) children: Vec<String>,
    /// The de-obfuscated file key of the node.
    pub(crate) key: Vec<u8>,
    /// The creation date of the node.
    pub(crate) created_at: Option<DateTime<Utc>>,
    /// The ID of the public link this node is from.
    pub(crate) download_id: Option<String>,
    pub(crate) thumbnail_handle: Option<String>,
    pub(crate) preview_image_handle: Option<String>,
}

impl Node {
    /// Returns the name of the node.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the hash (or handle) of the node.
    pub fn hash(&self) -> &str {
        self.hash.as_str()
    }

    /// Returns the size (in bytes) of the node.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns the kind of the node.
    pub fn kind(&self) -> NodeKind {
        self.kind
    }

    /// Returns the hash (or handle) of the node's parent.
    pub fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
    }

    /// Returns the hashes (or handles) of the node's children.
    pub fn children(&self) -> &[String] {
        self.children.as_slice()
    }

    /// Returns the creation date of the node.
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.created_at.as_ref()
    }

    /// Returns the ID of the public link this node is from.
    pub fn download_id(&self) -> Option<&str> {
        self.download_id.as_deref()
    }

    /// Returns whether this node has a associated thumbnail.
    pub fn has_thumbnail(&self) -> bool {
        self.thumbnail_handle.is_some()
    }

    /// Returns whether this node has an associated preview image.
    pub fn has_preview_image(&self) -> bool {
        self.preview_image_handle.is_some()
    }
}

/// Represents a collection of nodes from MEGA.
pub struct Nodes {
    /// The nodes from MEGA, keyed by their hash (or handle).
    pub(crate) nodes: HashMap<String, Node>,
    /// The hash (or handle) of the root node for the Cloud Drive.
    pub(crate) cloud_drive: Option<String>,
    /// The hash (or handle) of the root node for the Rubbish Bin.
    pub(crate) rubbish_bin: Option<String>,
    /// The hash (or handle) of the root node for the Inbox.
    pub(crate) inbox: Option<String>,
}

impl Nodes {
    pub(crate) fn new(nodes: HashMap<String, Node>) -> Self {
        let cloud_drive = nodes
            .values()
            .find_map(|node| (node.kind == NodeKind::Root).then(|| node.hash.clone()));
        let rubbish_bin = nodes
            .values()
            .find_map(|node| (node.kind == NodeKind::Trash).then(|| node.hash.clone()));
        let inbox = nodes
            .values()
            .find_map(|node| (node.kind == NodeKind::Inbox).then(|| node.hash.clone()));

        Self {
            nodes,
            cloud_drive,
            rubbish_bin,
            inbox,
        }
    }

    /// Returns the number of nodes in this collection.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Creates an iterator over all the root nodes.
    pub fn roots(&self) -> impl Iterator<Item=&Node> {
        self.nodes.values().filter(|node| {
            node.parent.as_ref().map_or(true, |parent| {
                // Root nodes from public links can still have
                // a `parent` handle associated with them, but that
                // parent won't be found in the current collection.
                !self.nodes.contains_key(parent)
            })
        })
    }

    /// Gets a node, identified by its hash (or handle).
    pub fn get_node_by_hash(&self, hash: &str) -> Option<&Node> {
        self.nodes.get(hash)
    }

    /// Gets a node, identified by its path.
    pub fn get_node_by_path(&self, path: &str) -> Option<&Node> {
        let path = path.strip_prefix('/').unwrap_or(path);

        let Some((root, path)) = path.split_once('/') else {
            return self.roots().find(|node| node.name == path);
        };

        let root = self.roots().find(|node| node.name == root)?;
        path.split('/').fold(Some(root), |node, name| {
            node?.children.iter().find_map(|hash| {
                let found = self.get_node_by_hash(hash)?;
                (found.name == name).then_some(found)
            })
        })
    }

    /// Gets the root node for the Cloud Drive.
    pub fn cloud_drive(&self) -> Option<&Node> {
        let hash = self.cloud_drive.as_ref()?;
        self.nodes.get(hash)
    }

    /// Gets the root node for the Inbox.
    pub fn inbox(&self) -> Option<&Node> {
        let hash = self.inbox.as_ref()?;
        self.nodes.get(hash)
    }

    /// Gets the root node for the Rubbish Bin.
    pub fn rubbish_bin(&self) -> Option<&Node> {
        let hash = self.rubbish_bin.as_ref()?;
        self.nodes.get(hash)
    }

    /// Creates a borrowing iterator over the nodes.
    pub fn iter(&self) -> impl Iterator<Item=&Node> {
        self.nodes.values()
    }

    /// Creates a mutably-borrowing iterator over the nodes.
    pub fn iter_mut(&mut self) -> impl Iterator<Item=&mut Node> {
        self.nodes.values_mut()
    }
}

impl IntoIterator for Nodes {
    type Item = Node;
    type IntoIter = std::collections::hash_map::IntoValues<String, Node>;

    fn into_iter(self) -> Self::IntoIter {
        self.nodes.into_values()
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub(crate) enum AttributeKind {
    Thumbnail = 0,
    PreviewImage = 1,
}

fn generate_section_urls(base_url: &str, sections: &Vec<(usize, usize)>) -> Vec<(usize, Url)> {
    let mut urls = Vec::new();

    for (start, end) in sections {
        let url_string = format!("{}/{}-{}", base_url, start, end);
        urls.push((*start, Url::parse(&url_string).unwrap()));
    }

    urls
}

fn generate_sections(file_size: usize, section_size: usize) -> Vec<(usize, usize)> {
    let mut sections = Vec::new();

    for i in (0..file_size).step_by(section_size) {
        let start = i;
        let end = std::cmp::min(start + section_size - 1, file_size - 1);
        sections.push((start, end));
    }

    sections
}
