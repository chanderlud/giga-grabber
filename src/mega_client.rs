use crate::config::Config;
use aes::Aes128;
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cbc::Decryptor;
use cipher::KeyInit;
use cipher::{BlockDecrypt, BlockDecryptMut, KeyIvInit, StreamCipher};
use ctr::Ctr128BE;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use url::Url;
use futures::StreamExt;

/// MEGA API origin.
const DEFAULT_API_ORIGIN: &str = "https://g.api.mega.co.nz/";

/// File node vs folder node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeKind {
    File,
    Folder,
}

/// A single node in a public tree.
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

/// What kind of public link this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublicLinkKind {
    File,
    Folder,
}

/// Struct used internally for URL parsing.
struct ParsedPublicLink {
    kind: PublicLinkKind,
    node_id: String,
    node_key: Vec<u8>,
}

/// Minimal MEGA client, using just `reqwest`.
#[derive(Clone)]
pub(crate) struct MegaClient {
    http: reqwest::Client,
    config: Config, // TODO use config for retries and timeouts inside download method
    origin: Url,
    id_counter: Arc<AtomicU64>,
}

impl MegaClient {
    pub(crate) fn new(http: reqwest::Client, config: Config) -> Result<Self> {
        let origin = Url::parse(DEFAULT_API_ORIGIN)?;
        Ok(Self {
            http,
            config,
            origin,
            id_counter: Default::default(),
        })
    }

    fn next_request_id(&self) -> u64 {
        self.id_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Fetch all nodes from a MEGA public link (file or folder).
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

    // TODO use chunking & save metadata
    /// Download a single node to `dest_path`.
    pub(crate) async fn download_file(&self, node: &Node, dest_path: &Path) -> Result<()> {
        let (download_url, _size) = self.get_download_url(&node.root_handle, node).await?;

        let resp = self
            .http
            .get(&download_url)
            .send()
            .await
            .context("MEGA file download request failed")?
            .error_for_status()
            .context("MEGA file download HTTP error")?;

        let mut stream = resp.bytes_stream();

        let mut file = fs::File::create(dest_path)
            .await
            .with_context(|| format!("creating {:?}", dest_path))?;

        // Build AES-CTR cipher
        let mut iv_block = [0u8; 16];
        if let Some(iv8) = node.aes_iv {
            iv_block[..8].copy_from_slice(&iv8);
        }
        let mut ctr = Ctr128BE::<Aes128>::new((&node.aes_key).into(), (&iv_block).into());

        while let Some(chunk) = stream.next().await {
            let mut buf = chunk.context("error reading download stream")?.to_vec();
            ctr.apply_keystream(&mut buf);
            file.write_all(&buf).await?;
        }

        file.flush().await?;
        Ok(())
    }

    /// Call the MEGA `g` (download) command and return the URL.
    async fn get_download_url(&self, root_handle: &str, node: &Node) -> Result<(String, u64)> {
        let url = {
            let mut url = self.origin.join("cs")?;
            let mut qp = url.query_pairs_mut();
            qp.append_pair("id", self.next_request_id().to_string().as_str());
            qp.append_pair("n", root_handle);
            drop(qp);
            url
        };

        let request = ApiRequest::Download {
            g: 1,
            ssl: 2,
            p: None,
            n: Some(node.handle.clone()),
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

        // Root folder key used to decrypt child keys.
        let root_key = parsed.node_key;

        for file in resp.nodes {
            let kind = match file.kind {
                0 => NodeKind::File,
                1 => NodeKind::Folder,
                _ => continue, // skip unknown types
            };

            // Skip nodes without keys.
            let Some(file_key_str) = file.key.as_deref() else {
                continue;
            };

            // Keys are like "userhandle:base64key[/userhandle:base64key...]"
            let mut file_key_bytes_opt = None;
            for entry in file_key_str.split('/') {
                let (_, base64_part) = match entry.split_once(':') {
                    Some(parts) => parts,
                    None => continue,
                };

                if base64_part.len() >= 44 {
                    // RSA-based key; ignoring for this barebones client.
                    continue;
                }

                let mut decoded = match URL_SAFE_NO_PAD.decode(base64_part) {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                // File -> 32 bytes, folder -> 16 bytes
                if (kind == NodeKind::File && decoded.len() != 32)
                    || (kind == NodeKind::Folder && decoded.len() != 16)
                {
                    continue;
                }

                // Decrypt with root folder key using AES-ECB.
                decrypt_ebc_in_place(&root_key, &mut decoded);
                file_key_bytes_opt = Some(decoded);
                break;
            }

            let mut file_key_bytes = match file_key_bytes_opt {
                Some(k) => k,
                None => continue,
            };

            let (aes_key, aes_iv) = if kind == NodeKind::File {
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
                // 16 bytes: just AES key, no IV.
                let mut aes_key = [0u8; 16];
                aes_key.copy_from_slice(&file_key_bytes[..16]);
                (aes_key, None)
            };

            let name = decrypt_attrs(&aes_key, &file.attr)?;

            let node = Node {
                name,
                handle: file.handle.clone(),
                parent: Some(file.parent),
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
                && !handles.contains(p) {
                    node.parent = None;
                }
        }

        Ok(nodes_map)
    }
}

/// Internal request enum for MEGA `cs` calls.
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

/// Minimal subset of MEGA's node attributes.
#[derive(Debug, Deserialize)]
struct NodeAttributes {
    #[serde(rename = "n")]
    name: String,
}

/// Single node entry from FetchNodes.
#[derive(Debug, Deserialize)]
struct FileNode {
    #[serde(rename = "t")]
    kind: u8,
    #[serde(rename = "a")]
    attr: String,
    #[serde(rename = "h")]
    handle: String,
    #[serde(rename = "p")]
    parent: String,
    #[serde(rename = "k")]
    key: Option<String>,
    #[serde(rename = "s")]
    size: Option<u64>,
}

/// Response for FetchNodes.
#[derive(Debug, Deserialize)]
struct FetchNodesResponse {
    #[serde(rename = "f")]
    nodes: Vec<FileNode>,
}

/// Response for Download.
#[derive(Debug, Deserialize)]
struct DownloadResponse {
    #[serde(rename = "g")]
    download_url: String,
    #[serde(rename = "s")]
    size: u64,
    #[serde(rename = "at")]
    attr: String,
}

/// Parse public MEGA link: file/folder, node id, raw key bytes.
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

/// AES-ECB decrypt `data` in-place using `key`.
fn decrypt_ebc_in_place(key: &[u8], data: &mut [u8]) {
    let aes = Aes128::new(key.into());
    for block in data.chunks_mut(16) {
        aes.decrypt_block(block.into());
    }
}

/// XOR first 16 bytes with second 16 bytes (undo merged key+MAC).
fn unmerge_key_mac(key: &mut [u8]) {
    let (fst, snd) = key.split_at_mut(16);
    for (a, b) in fst.iter_mut().zip(snd) {
        *a ^= *b;
    }
}

/// Decrypt MEGA node attributes and return the node name.
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
