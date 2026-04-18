use crate::worker::{Download, PauseState};
use aes::Aes128;
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cbc::Decryptor;
use cipher::KeyInit;
use cipher::StreamCipherSeek;
use cipher::{BlockDecrypt, BlockDecryptMut, KeyIvInit, StreamCipher};
use ctr::Ctr128BE;
use futures::StreamExt;
use log::error;
use reqwest::header;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::SeekFrom;
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio::{fs, select};
use url::Url;

/// MEGA API origin
const DEFAULT_API_ORIGIN: &str = "https://g.api.mega.co.nz/";
/// safety margin for resuming partial downloads
const RESUME_REWIND: u64 = 1024 * 1024;

/// File node vs folder node
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeKind {
    File,
    Folder,
}

/// A single node in a public tree
#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) name: String,
    pub(crate) handle: String,
    pub(crate) parent: Option<String>,
    pub(crate) kind: NodeKind,
    pub(crate) size: u64,
    aes_key: [u8; 16],
    aes_iv: Option<[u8; 8]>,
    pub(crate) root_handle: String,
}

#[cfg(test)]
impl Node {
    pub(crate) fn test_file(handle: impl Into<String>, name: impl Into<String>, size: u64) -> Self {
        let handle = handle.into();
        Self {
            name: name.into(),
            handle: handle.clone(),
            parent: Some("root".to_string()),
            kind: NodeKind::File,
            size,
            aes_key: [0; 16],
            aes_iv: Some([0; 8]),
            root_handle: handle,
        }
    }

    pub(crate) fn test_folder(handle: impl Into<String>, name: impl Into<String>) -> Self {
        let handle = handle.into();
        Self {
            name: name.into(),
            handle: handle.clone(),
            parent: None,
            kind: NodeKind::Folder,
            size: 0,
            aes_key: [0; 16],
            aes_iv: None,
            root_handle: handle,
        }
    }
}

/// What kind of public link this is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublicLinkKind {
    File,
    Folder,
}

/// Struct used internally for URL parsing
struct ParsedPublicLink {
    kind: PublicLinkKind,
    node_id: String,
    node_key: Vec<u8>,
}

/// Minimal MEGA client, using just `reqwest`
#[derive(Clone)]
pub(crate) struct MegaClient {
    http: reqwest::Client,
    origin: Url,
    id_counter: Arc<AtomicU64>,
}

impl MegaClient {
    pub(crate) fn new(http: reqwest::Client) -> Result<Self> {
        let origin = Url::parse(DEFAULT_API_ORIGIN)?;
        Ok(Self {
            http,
            origin,
            id_counter: Default::default(),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_origin(http: reqwest::Client, origin: Url) -> Self {
        Self {
            http,
            origin,
            id_counter: Default::default(),
        }
    }

    fn next_request_id(&self) -> u64 {
        self.id_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Fetch all nodes from a MEGA public link (file or folder)
    ///
    /// Supported formats:
    /// - https://mega.nz/file/{node_id}#{node_key}
    /// - https://mega.nz/folder/{node_id}#{node_key}
    pub(crate) async fn fetch_public_nodes(&self, url: &str) -> Result<HashMap<String, Node>> {
        let parsed = parse_public_link(url)?;

        match parsed.kind {
            PublicLinkKind::File => self.fetch_public_file(parsed).await,
            PublicLinkKind::Folder => self.fetch_public_folder(parsed).await,
        }
    }

    /// Download a single node to `dest_path`
    pub(crate) async fn download_file(
        &self,
        download: &Download,
        dest_path: &Path,
    ) -> Result<bool> {
        let (download_url, remote_size) = self.get_download_url(&download.node).await?;

        // figure out resume offset & open file accordingly
        let (mut file, resume_from) = if dest_path.exists() {
            let meta = fs::metadata(dest_path)
                .await
                .with_context(|| format!("stat {:?}", dest_path))?;
            let local_len = meta.len();

            if local_len == 0 {
                // file exists but is empty: just start from 0
                (
                    fs::File::create(dest_path)
                        .await
                        .with_context(|| format!("creating {:?}", dest_path))?,
                    0,
                )
            } else if local_len >= remote_size {
                // already complete or bigger than the remote size; assume done
                return Ok(true);
            } else {
                // resume with some rewind.
                let resume_from = local_len.saturating_sub(RESUME_REWIND);

                let mut f = OpenOptions::new()
                    .write(true)
                    .open(dest_path)
                    .await
                    .with_context(|| format!("opening for resume {:?}", dest_path))?;

                // overwrite from resume_from onward (not append)
                f.seek(SeekFrom::Start(resume_from))
                    .await
                    .with_context(|| format!("seeking {:?}", dest_path))?;

                (f, resume_from)
            }
        } else {
            (
                fs::File::create(dest_path)
                    .await
                    .with_context(|| format!("creating {:?}", dest_path))?,
                0,
            )
        };

        // fetch the download response, handling pausing
        let mut req = self.http.get(&download_url);
        if resume_from > 0 {
            let range_header = format!("bytes={}-", resume_from);
            req = req.header(header::RANGE, range_header);
        }

        let mut pause_receiver = download.pause_receiver();

        let resp = select! {
            _ = pause_loop(&mut pause_receiver) => {
                // Pause may have been resumed concurrently; only persist Paused
                // when a pause intent is still current.
                download.mark_paused_if_requested();
                return Ok(false);
            }
            result = req.send() => {
                result
                    .context("MEGA file download request failed")?
                    .error_for_status()
                    .context("MEGA file download HTTP error")?
            }
        };

        let mut stream = resp.bytes_stream();

        // Build AES-CTR cipher
        let mut iv_block = [0u8; 16];
        if let Some(iv8) = download.node.aes_iv {
            iv_block[..8].copy_from_slice(&iv8);
        }
        let mut ctr = Ctr128BE::<Aes128>::new((&download.node.aes_key).into(), (&iv_block).into());
        ctr.seek(resume_from);

        loop {
            select! {
                _ = pause_loop(&mut pause_receiver) => {
                    // Pause may have been resumed concurrently; only persist Paused
                    // when a pause intent is still current.
                    download.mark_paused_if_requested();
                    return Ok(false);
                }
                chunk_option = stream.next() => {
                    if let Some(chunk) = chunk_option {
                        let mut buf = chunk?.to_vec();
                        ctr.apply_keystream(&mut buf);
                        file.write_all(&buf).await?;
                        download.downloaded.fetch_add(buf.len(), Relaxed);
                    } else {
                        break;
                    }
                }
            }
        }

        file.flush().await?;
        Ok(true)
    }

    /// Call the MEGA `g` (download) command and return the URL
    async fn get_download_url(&self, node: &Node) -> Result<(String, u64)> {
        let is_standalone_file = node.parent.is_none();

        let url = {
            let mut url = self.origin.join("cs")?;
            let mut qp = url.query_pairs_mut();
            qp.append_pair("id", self.next_request_id().to_string().as_str());
            if !is_standalone_file {
                qp.append_pair("n", &node.root_handle);
            }
            drop(qp);
            url
        };

        let request = if is_standalone_file {
            ApiRequest::Download {
                g: 1,
                ssl: 2,
                p: Some(node.handle.clone()),
                n: None,
            }
        } else {
            ApiRequest::Download {
                g: 1,
                ssl: 2,
                p: None,
                n: Some(node.handle.clone()),
            }
        };

        let body = vec![request];

        let resp_bytes = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .context("MEGA download cs request failed")?
            .error_for_status()
            .context("MEGA download cs HTTP error")?
            .bytes()
            .await
            .context("reading MEGA download response body")?;

        let values: Vec<serde_json::Value> =
            serde_json::from_slice(&resp_bytes).context("parsing MEGA download JSON")?;

        let value = values
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty MEGA download response"))?;

        if let Some(num) = value.as_i64() {
            bail!("MEGA download API error code {}", num);
        }

        let resp: DownloadResponse = serde_json::from_value(value)?;
        Ok((resp.download_url, resp.size))
    }

    async fn fetch_public_file(&self, parsed: ParsedPublicLink) -> Result<HashMap<String, Node>> {
        if parsed.node_key.len() != 32 {
            bail!(
                "unexpected file key size {}, expected 32 bytes",
                parsed.node_key.len()
            );
        }

        // For a pure file link, we call `g` once to get attrs + size.
        let url = {
            let mut url = self.origin.join("cs")?;
            let mut qp = url.query_pairs_mut();
            qp.append_pair("id", self.next_request_id().to_string().as_str());
            drop(qp);
            url
        };

        let request = ApiRequest::Download {
            g: 1,
            ssl: 2,
            p: Some(parsed.node_id.clone()),
            n: None,
        };

        let body = vec![request];

        let resp_bytes = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .context("MEGA file cs request failed")?
            .error_for_status()
            .context("MEGA file cs HTTP error")?
            .bytes()
            .await
            .context("reading MEGA file response body")?;

        let values: Vec<serde_json::Value> =
            serde_json::from_slice(&resp_bytes).context("parsing MEGA file JSON")?;

        let value = values
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty MEGA file response"))?;

        if let Some(num) = value.as_i64() {
            bail!("MEGA file API error code {}", num);
        }

        let file: DownloadResponse = serde_json::from_value(value)?;

        let mut key = parsed.node_key.clone();
        unmerge_key_mac(&mut key);

        let (aes_key_bytes, rest) = key.split_at(16);
        let (aes_iv_bytes, _mac_bytes) = rest.split_at(8);

        let mut aes_key = [0u8; 16];
        aes_key.copy_from_slice(aes_key_bytes);

        let mut aes_iv = [0u8; 8];
        aes_iv.copy_from_slice(aes_iv_bytes);

        let name = decrypt_attrs(&aes_key, &file.attr)?;

        let node = Node {
            name,
            handle: parsed.node_id.clone(),
            parent: None,
            kind: NodeKind::File,
            size: file.size,
            aes_key,
            aes_iv: Some(aes_iv),
            root_handle: parsed.node_id,
        };

        let mut map = HashMap::new();
        map.insert(node.handle.clone(), node);

        Ok(map)
    }

    async fn fetch_public_folder(&self, parsed: ParsedPublicLink) -> Result<HashMap<String, Node>> {
        if parsed.node_key.len() != 16 {
            bail!(
                "unexpected folder key size {}, expected 16 bytes",
                parsed.node_key.len()
            );
        }

        let url = {
            let mut url = self.origin.join("cs")?;
            let mut qp = url.query_pairs_mut();
            qp.append_pair("id", self.next_request_id().to_string().as_str());
            qp.append_pair("n", parsed.node_id.as_str());
            drop(qp);
            url
        };

        let request = ApiRequest::FetchNodes { c: 1, r: Some(1) };
        let body = vec![request];

        let resp_bytes = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .context("MEGA folder cs request failed")?
            .error_for_status()
            .context("MEGA folder cs HTTP error")?
            .bytes()
            .await
            .context("reading MEGA folder response body")?;

        let values: Vec<serde_json::Value> =
            serde_json::from_slice(&resp_bytes).context("parsing MEGA folder JSON")?;

        let value = values
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty MEGA folder response"))?;

        if let Some(num) = value.as_i64() {
            bail!("MEGA folder API error code {}", num);
        }

        let resp: FetchNodesResponse = serde_json::from_value(value)?;

        let mut nodes_map: HashMap<String, Node> = HashMap::new();

        let root_key: [u8; 16] = parsed
            .node_key
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("unexpected folder key size after validation"))?;

        let mut share_keys: HashMap<String, [u8; 16]> = HashMap::new();
        share_keys.insert(parsed.node_id.clone(), root_key);

        for entry in &resp.ok {
            let mut decoded = match URL_SAFE_NO_PAD.decode(&entry.key) {
                Ok(d) => d,
                Err(_) => continue,
            };
            if decoded.len() != 16 {
                continue;
            }
            decrypt_ebc_in_place(&root_key, &mut decoded);
            let mut share_key = [0u8; 16];
            share_key.copy_from_slice(&decoded);
            share_keys.insert(entry.handle.clone(), share_key);
        }

        for file in &resp.nodes {
            let Some(sk) = file.sharing_key.as_deref() else {
                continue;
            };
            let mut decoded = match URL_SAFE_NO_PAD.decode(sk) {
                Ok(d) => d,
                Err(_) => continue,
            };
            if decoded.len() != 16 {
                continue;
            }
            decrypt_ebc_in_place(&root_key, &mut decoded);
            let mut share_key = [0u8; 16];
            share_key.copy_from_slice(&decoded);
            share_keys.insert(file.handle.clone(), share_key);
        }

        // Precompute a deterministic, root-first ordered list of share-key handles once,
        // so the per-node helper doesn't re-sort on every call.
        let mut share_key_handles: Vec<&str> = share_keys.keys().map(String::as_str).collect();
        share_key_handles.sort_unstable();
        if let Some(root_idx) = share_key_handles.iter().position(|h| *h == parsed.node_id) {
            share_key_handles.swap(0, root_idx);
        }

        for file in resp.nodes {
            let kind = match file.kind {
                0 => NodeKind::File,
                1 => NodeKind::Folder,
                2..=4 => continue,
                _ => continue, // skip unknown types
            };

            // Skip nodes without keys.
            let Some(file_key_str) = file.key.as_deref() else {
                continue;
            };

            let mut node_material: Option<([u8; 16], Option<[u8; 8]>, String)> = None;
            let mut last_attr_error = None;
            let has_candidate = decrypt_node_key_candidates(
                file_key_str,
                &share_keys,
                &share_key_handles,
                |mut file_key_bytes| {
                    let (aes_key, aes_iv) = if kind == NodeKind::File {
                        if file_key_bytes.len() != 32 {
                            return ControlFlow::Continue(());
                        }
                        // 32 bytes: [16 key][8 iv][8 mac]
                        unmerge_key_mac(&mut file_key_bytes);

                        let (key_part, rest) = file_key_bytes.split_at(16);
                        let (iv_part, _mac_part) = rest.split_at(8);

                        let mut aes_key = [0u8; 16];
                        aes_key.copy_from_slice(key_part);

                        let mut aes_iv = [0u8; 8];
                        aes_iv.copy_from_slice(iv_part);

                        (aes_key, Some(aes_iv))
                    } else {
                        if file_key_bytes.len() != 16 {
                            return ControlFlow::Continue(());
                        }
                        // 16 bytes: just AES key, no IV.
                        let mut aes_key = [0u8; 16];
                        aes_key.copy_from_slice(&file_key_bytes[..16]);
                        (aes_key, None)
                    };

                    match decrypt_attrs(&aes_key, &file.attr) {
                        Ok(name) => {
                            node_material = Some((aes_key, aes_iv, name));
                            ControlFlow::Break(())
                        }
                        Err(error) => {
                            last_attr_error = Some(error);
                            ControlFlow::Continue(())
                        }
                    }
                },
            );

            if !has_candidate {
                continue;
            }

            let Some((aes_key, aes_iv, name)) = node_material else {
                if let Some(error) = last_attr_error {
                    error!(
                        "failed to decrypt attributes for {}: {:?}",
                        file.handle, error
                    );
                }
                continue;
            };

            let node = Node {
                name,
                handle: file.handle.clone(),
                parent: file.parent,
                kind,
                size: file.size.unwrap_or(0),
                aes_key,
                aes_iv,
                root_handle: parsed.node_id.clone(),
            };

            nodes_map.insert(node.handle.clone(), node);
        }

        let handles: HashSet<String> = nodes_map.keys().cloned().collect();
        for node in nodes_map.values_mut() {
            if let Some(ref p) = node.parent
                && !handles.contains(p)
            {
                node.parent = None;
            }
        }

        Ok(nodes_map)
    }
}

/// Internal request enum for MEGA `cs` calls
#[derive(Debug, Serialize)]
#[serde(tag = "a")]
enum ApiRequest {
    /// Fetch nodes: {"a":"f","c":1,"r":1}
    #[serde(rename = "f")]
    FetchNodes {
        #[serde(rename = "c")]
        c: i32,
        #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
        r: Option<i32>,
    },

    /// Download: {"a":"g","g":1,"ssl":2,"n":...} / {"a":"g","g":1,"ssl":2,"p":...}
    #[serde(rename = "g")]
    Download {
        #[serde(rename = "g")]
        g: i32,
        #[serde(rename = "ssl")]
        ssl: i32,
        #[serde(rename = "p", skip_serializing_if = "Option::is_none")]
        p: Option<String>,
        #[serde(rename = "n", skip_serializing_if = "Option::is_none")]
        n: Option<String>,
    },
}

/// Minimal subset of MEGA's node attributes
#[derive(Debug, Deserialize)]
struct NodeAttributes {
    #[serde(rename = "n")]
    name: String,
}

/// Single node entry from FetchNodes
#[derive(Debug, Deserialize)]
struct FileNode {
    #[serde(rename = "t")]
    kind: u8,
    #[serde(rename = "a")]
    attr: String,
    #[serde(rename = "h")]
    handle: String,
    #[serde(rename = "p", default)]
    parent: Option<String>,
    #[serde(rename = "k")]
    key: Option<String>,
    #[serde(rename = "s")]
    size: Option<u64>,
    #[serde(rename = "sk")]
    sharing_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SharedKey {
    #[serde(rename = "h")]
    handle: String,
    #[serde(rename = "k")]
    key: String,
}

/// Response for FetchNodes
#[derive(Debug, Deserialize)]
struct FetchNodesResponse {
    #[serde(rename = "f")]
    nodes: Vec<FileNode>,
    #[serde(default, rename = "ok")]
    ok: Vec<SharedKey>,
}

/// Response for Download
#[derive(Debug, Deserialize)]
struct DownloadResponse {
    #[serde(rename = "g")]
    download_url: String,
    #[serde(rename = "s")]
    size: u64,
    #[serde(rename = "at")]
    attr: String,
}

/// Parse public MEGA link: file/folder, node id, raw key bytes
fn parse_public_link(url: &str) -> Result<ParsedPublicLink> {
    const PREFIX: &str = "https://mega.nz/";
    if !url.starts_with(PREFIX) {
        bail!("unsupported MEGA URL: {}", url);
    }
    let payload = &url[PREFIX.len()..];

    let (kind, rest) = match payload.split_once('/') {
        Some(("file", rest)) => (PublicLinkKind::File, rest),
        Some(("folder", rest)) => (PublicLinkKind::Folder, rest),
        _ => bail!("invalid MEGA public URL format"),
    };

    let (node_id, key_part) = rest
        .split_once('#')
        .ok_or_else(|| anyhow::anyhow!("missing #key in MEGA URL"))?;

    // For folder links that include a path, the part after '/' is path, which we ignore.
    let key_str = key_part.split_once('/').map(|(k, _)| k).unwrap_or(key_part);

    let node_key = URL_SAFE_NO_PAD
        .decode(key_str)
        .with_context(|| "invalid base64 key in MEGA URL")?;

    Ok(ParsedPublicLink {
        kind,
        node_id: node_id.to_string(),
        node_key,
    })
}

/// AES-ECB decrypt `data` in-place using `key`
fn decrypt_ebc_in_place(key: &[u8], data: &mut [u8]) {
    let aes = Aes128::new(key.into());
    for block in data.chunks_mut(16) {
        aes.decrypt_block(block.into());
    }
}

fn decrypt_node_key_candidates<F>(
    key_field: &str,
    share_keys: &HashMap<String, [u8; 16]>,
    share_key_handles: &[&str],
    mut on_candidate: F,
) -> bool
where
    F: FnMut(Vec<u8>) -> ControlFlow<()>,
{
    let entries: Vec<(&str, &str)> = key_field
        .split('/')
        .filter_map(|entry| entry.split_once(':'))
        .collect();

    let mut seen = HashSet::new();
    let mut emitted_any = false;

    // Pass 1: exact handle -> share key match.
    for (handle, b64) in &entries {
        let Some(share_key) = share_keys.get(*handle) else {
            continue;
        };
        let mut decoded = match URL_SAFE_NO_PAD.decode(*b64) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if decoded.len() != 16 && decoded.len() != 32 {
            continue;
        }
        decrypt_ebc_in_place(share_key, &mut decoded);
        if seen.insert(decoded.clone()) {
            emitted_any = true;
            if on_candidate(decoded).is_break() {
                return true;
            }
        }
    }

    // Pass 2: use the precomputed deterministic handle order (root-first, then sorted).
    for (_, b64) in &entries {
        let decoded = match URL_SAFE_NO_PAD.decode(b64) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if decoded.len() != 16 && decoded.len() != 32 {
            continue;
        }

        for handle in share_key_handles {
            let Some(share_key) = share_keys.get(*handle) else {
                continue;
            };
            let mut candidate = decoded.clone();
            decrypt_ebc_in_place(share_key, &mut candidate);
            if seen.insert(candidate.clone()) {
                emitted_any = true;
                if on_candidate(candidate).is_break() {
                    return true;
                }
            }
        }
    }

    emitted_any
}

/// XOR first 16 bytes with second 16 bytes (undo merged key+MAC)
fn unmerge_key_mac(key: &mut [u8]) {
    let (fst, snd) = key.split_at_mut(16);
    for (a, b) in fst.iter_mut().zip(snd) {
        *a ^= *b;
    }
}

/// Decrypt MEGA node attributes and return the node name
fn decrypt_attrs(aes_key: &[u8; 16], attr_b64: &str) -> Result<String> {
    let mut buf = URL_SAFE_NO_PAD
        .decode(attr_b64)
        .context("invalid base64 attrs")?;

    let mut cbc = Decryptor::<Aes128>::new(aes_key.into(), &Default::default());
    for chunk in buf.chunks_exact_mut(16) {
        cbc.decrypt_block_mut(chunk.into());
    }

    if &buf[..4] != b"MEGA" {
        bail!("invalid MEGA attribute header");
    }

    let len = buf.iter().take_while(|b| **b != 0).count();
    let json_bytes = &buf[4..len];

    let attrs: NodeAttributes =
        serde_json::from_slice(json_bytes).context("parsing node attributes JSON")?;

    Ok(attrs.name)
}

async fn pause_loop(pause_receiver: &mut watch::Receiver<PauseState>) {
    loop {
        let state = *pause_receiver.borrow();
        if matches!(state, PauseState::PauseRequested | PauseState::Paused) {
            break;
        }
        if pause_receiver.changed().await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncrypt;
    use cbc::Encryptor;
    use cipher::{BlockEncryptMut, KeyIvInit};
    use serde_json::json;
    use std::collections::BTreeMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::{Duration, timeout};

    fn ecb_encrypt_in_place(key: &[u8; 16], data: &mut [u8]) {
        let aes = Aes128::new(key.into());
        for block in data.chunks_mut(16) {
            aes.encrypt_block(block.into());
        }
    }

    /// Build a root-first, deterministically ordered list of share-key handles for tests.
    fn ordered_share_key_handles<'a>(
        share_keys: &'a HashMap<String, [u8; 16]>,
        root_handle: &str,
    ) -> Vec<&'a str> {
        let mut handles: Vec<&str> = share_keys.keys().map(String::as_str).collect();
        handles.sort_unstable();
        if let Some(i) = handles.iter().position(|h| *h == root_handle) {
            handles.swap(0, i);
        }
        handles
    }

    fn encrypt_attrs(aes_key: &[u8; 16], name: &str) -> String {
        let payload = format!(r#"{{"n":"{name}"}}"#);
        let mut plain = b"MEGA".to_vec();
        plain.extend_from_slice(payload.as_bytes());
        let pad = (16 - (plain.len() % 16)) % 16;
        plain.resize(plain.len() + pad, 0);

        let mut cbc = Encryptor::<Aes128>::new(aes_key.into(), &Default::default());
        for chunk in plain.chunks_exact_mut(16) {
            cbc.encrypt_block_mut(chunk.into());
        }
        URL_SAFE_NO_PAD.encode(plain)
    }

    fn encrypted_key_b64(share_key: &[u8; 16], plain: &[u8]) -> String {
        let mut encrypted = plain.to_vec();
        ecb_encrypt_in_place(share_key, &mut encrypted);
        URL_SAFE_NO_PAD.encode(encrypted)
    }

    fn build_file_key_material(aes_key: [u8; 16], aes_iv: [u8; 8], mac: [u8; 8]) -> Vec<u8> {
        let mut second = [0u8; 16];
        second[..8].copy_from_slice(&aes_iv);
        second[8..].copy_from_slice(&mac);

        let mut merged_first = [0u8; 16];
        for (idx, byte) in merged_first.iter_mut().enumerate() {
            *byte = aes_key[idx] ^ second[idx];
        }

        let mut material = Vec::with_capacity(32);
        material.extend_from_slice(&merged_first);
        material.extend_from_slice(&second);
        material
    }

    fn make_key_field(handle: &str, share_key: &[u8; 16], plain: &[u8]) -> String {
        format!("{handle}:{}", encrypted_key_b64(share_key, plain))
    }

    async fn spawn_single_cs_fixture(cs_response: String) -> (Url, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let addr = listener.local_addr().expect("fixture local addr");
        let base_url = Url::parse(&format!("http://{addr}/")).expect("fixture base URL");

        let task = tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };

            let mut request = Vec::new();
            let mut buf = [0u8; 2048];
            let header_end = loop {
                let read = match socket.read(&mut buf).await {
                    Ok(0) => return,
                    Ok(n) => n,
                    Err(_) => return,
                };
                request.extend_from_slice(&buf[..read]);
                if let Some(idx) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break idx + 4;
                }
            };

            let request_head = String::from_utf8_lossy(&request[..header_end]);
            let mut content_length = 0usize;
            for line in request_head.lines() {
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse::<usize>().unwrap_or(0);
                }
            }

            let body_read = request.len().saturating_sub(header_end);
            if content_length > body_read {
                let mut remaining = content_length - body_read;
                while remaining > 0 {
                    let read = match socket.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    remaining = remaining.saturating_sub(read);
                }
            }

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                cs_response.len(),
                cs_response
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });

        (base_url, task)
    }

    #[test]
    fn decrypt_node_key_candidates_exact_handle_success() {
        let root = [0x11; 16];
        let exact = [0x33; 16];
        let expected = vec![0xAB; 16];
        let key_field = format!("exact:{}", encrypted_key_b64(&exact, &expected));

        let mut share_keys = HashMap::new();
        share_keys.insert("root".to_string(), root);
        share_keys.insert("exact".to_string(), exact);

        let share_key_handles = ordered_share_key_handles(&share_keys, "root");

        let mut seen = Vec::new();
        let has_candidate =
            decrypt_node_key_candidates(&key_field, &share_keys, &share_key_handles, |candidate| {
                seen.push(candidate);
                ControlFlow::Break(())
            });

        assert!(has_candidate);
        assert_eq!(seen, vec![expected]);
    }

    #[test]
    fn decrypt_node_key_candidates_fallback_after_exact_failure() {
        let root = [0x44; 16];
        let exact = [0x55; 16];
        let exact_plain = vec![0x10; 16];
        let fallback_plain = vec![0x20; 16];
        let key_field = format!(
            "exact:{}/missing:{}",
            encrypted_key_b64(&exact, &exact_plain),
            encrypted_key_b64(&root, &fallback_plain)
        );

        let mut share_keys = HashMap::new();
        share_keys.insert("root".to_string(), root);
        share_keys.insert("exact".to_string(), exact);

        let share_key_handles = ordered_share_key_handles(&share_keys, "root");

        let mut attempts = Vec::new();
        let has_candidate =
            decrypt_node_key_candidates(&key_field, &share_keys, &share_key_handles, |candidate| {
                let should_break = candidate == fallback_plain;
                attempts.push(candidate);
                if should_break {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            });

        assert!(has_candidate);
        let exact_idx = attempts
            .iter()
            .position(|candidate| *candidate == exact_plain)
            .expect("expected exact-handle candidate attempt");
        let fallback_idx = attempts
            .iter()
            .position(|candidate| *candidate == fallback_plain)
            .expect("expected fallback candidate attempt");
        assert!(
            exact_idx < fallback_idx,
            "expected exact-handle candidate attempt before fallback candidate attempt"
        );
    }

    #[test]
    fn decrypt_node_key_candidates_root_first_deterministic_fallback_order() {
        let root = [0x01; 16];
        let alpha = [0x02; 16];
        let zeta = [0x03; 16];
        let ciphertext = vec![0x7F; 16];
        let key_field = format!("missing:{}", URL_SAFE_NO_PAD.encode(&ciphertext));

        let mut share_keys = HashMap::new();
        share_keys.insert("zeta".to_string(), zeta);
        share_keys.insert("root".to_string(), root);
        share_keys.insert("alpha".to_string(), alpha);

        let mut expected = Vec::new();
        for key in [root, alpha, zeta] {
            let mut candidate = ciphertext.clone();
            decrypt_ebc_in_place(&key, &mut candidate);
            expected.push(candidate);
        }

        let share_key_handles = ordered_share_key_handles(&share_keys, "root");

        let mut observed = Vec::new();
        let has_candidate =
            decrypt_node_key_candidates(&key_field, &share_keys, &share_key_handles, |candidate| {
                observed.push(candidate);
                ControlFlow::Continue(())
            });

        assert!(has_candidate);
        assert_eq!(observed, expected);
    }

    #[test]
    fn decrypt_node_key_candidates_deduplicates_candidates() {
        let root = [0x0A; 16];
        let plain = vec![0xCD; 16];
        let encoded = encrypted_key_b64(&root, &plain);
        let key_field = format!("root:{encoded}/root:{encoded}");

        let mut share_keys = HashMap::new();
        share_keys.insert("root".to_string(), root);

        let share_key_handles = ordered_share_key_handles(&share_keys, "root");

        let mut observed = Vec::new();
        let has_candidate =
            decrypt_node_key_candidates(&key_field, &share_keys, &share_key_handles, |candidate| {
                observed.push(candidate);
                ControlFlow::Continue(())
            });

        assert!(has_candidate);
        assert_eq!(observed, vec![plain]);
    }

    #[tokio::test]
    async fn fetch_public_folder_includes_and_skips_nodes_by_candidate_attr_results() {
        let root_handle = "root123";
        let root_key = [0xA1; 16];
        let root_key_b64 = URL_SAFE_NO_PAD.encode(root_key);

        let exact_file_aes = [0x31; 16];
        let exact_file_key = build_file_key_material(exact_file_aes, [0x01; 8], [0x02; 8]);
        let exact_node = json!({
            "t": 0,
            "a": encrypt_attrs(&exact_file_aes, "exact.bin"),
            "h": "exact-node",
            "p": root_handle,
            "k": make_key_field("exact-node", &root_key, &exact_file_key),
            "s": 7u64
        });

        let fallback_file_aes = [0x42; 16];
        let fallback_file_key = build_file_key_material(fallback_file_aes, [0x03; 8], [0x04; 8]);
        let fallback_node = json!({
            "t": 0,
            "a": encrypt_attrs(&fallback_file_aes, "fallback.bin"),
            "h": "fallback-node",
            "p": root_handle,
            "k": make_key_field("missing-handle", &root_key, &fallback_file_key),
            "s": 9u64
        });

        let bad_attr_file_key = build_file_key_material([0x77; 16], [0x05; 8], [0x06; 8]);
        let bad_attr = URL_SAFE_NO_PAD.encode([0u8; 16]);
        let skipped_node = json!({
            "t": 0,
            "a": bad_attr,
            "h": "skipped-node",
            "p": root_handle,
            "k": make_key_field("skipped-node", &root_key, &bad_attr_file_key),
            "s": 11u64
        });

        let cs_payload = json!([{
            "f": vec![exact_node, fallback_node, skipped_node],
            "ok": Vec::<BTreeMap<String, String>>::new()
        }])
        .to_string();

        let (origin, fixture_task) = spawn_single_cs_fixture(cs_payload).await;
        let client = MegaClient::with_origin(reqwest::Client::new(), origin);
        let parsed = ParsedPublicLink {
            kind: PublicLinkKind::Folder,
            node_id: root_handle.to_string(),
            node_key: URL_SAFE_NO_PAD
                .decode(root_key_b64)
                .expect("decode root key"),
        };

        let nodes = client
            .fetch_public_folder(parsed)
            .await
            .expect("fetch public folder");

        assert!(nodes.contains_key("exact-node"));
        assert!(nodes.contains_key("fallback-node"));
        assert!(!nodes.contains_key("skipped-node"));
        assert_eq!(nodes["exact-node"].name, "exact.bin");
        assert_eq!(nodes["fallback-node"].name, "fallback.bin");

        timeout(Duration::from_secs(2), fixture_task)
            .await
            .expect("fixture join timeout")
            .expect("fixture join panic");
    }
}
