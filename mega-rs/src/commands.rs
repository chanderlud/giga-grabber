use std::collections::HashMap;

use json::Value;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::error::{Error, ErrorCode};

/// Represents the kind of a given node.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
pub enum NodeKind {
    /// A regular file.
    File = 0,
    /// A regular folder.
    Folder = 1,
    /// The Cloud Drive root node.
    Root = 2,
    /// The Inbox root node.
    Inbox = 3,
    /// The Rubbish Bin root node.
    Trash = 4,
    /// Unknown node kind (used as a catch-all for unidentified nodes).
    #[serde(other)]
    Unknown = u8::MAX,
}

impl NodeKind {
    /// Returns whether the node is a regular file.
    pub fn is_file(self) -> bool {
        matches!(self, NodeKind::File)
    }

    /// Returns whether the node is a regular folder.
    pub fn is_folder(self) -> bool {
        matches!(self, NodeKind::Folder)
    }

    /// Returns whether the node is specifically the Cloud Drive root.
    pub fn is_root(self) -> bool {
        matches!(self, NodeKind::Root)
    }

    /// Returns whether the node is specifically the Rubbish Bin root.
    pub fn is_rubbish_bin(self) -> bool {
        matches!(self, NodeKind::Trash)
    }

    /// Returns whether the node is specifically the Inbox root.
    pub fn is_inbox(self) -> bool {
        matches!(self, NodeKind::Inbox)
    }
}

/// Represents a request message to MEGA's API.
///
/// Keep in mind that these message definitions have been somewhat reverse-engineered from MEGA's C++ SDK, and are, therefore, not complete.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "a")]
pub enum Request {
    /// Message for initiating a login ceremony.
    #[serde(rename = "us0")]
    PreLogin {
        /// The user's email address.
        #[serde(rename = "user")]
        user: String,
    },
    /// Message for completing a login ceremony.
    #[serde(rename = "us")]
    Login {
        /// The user's email address.
        #[serde(rename = "user")]
        user: String,
        /// The user's handle.
        #[serde(rename = "uh")]
        hash: String,
        /// The session key to use.
        #[serde(rename = "sek", skip_serializing_if = "Option::is_none")]
        session_key: Option<String>,
        /// TODO
        #[serde(rename = "si", skip_serializing_if = "Option::is_none")]
        si: Option<String>,
        /// The multi-factor token to use.
        #[serde(rename = "mfa", skip_serializing_if = "Option::is_none")]
        mfa: Option<String>,
    },
    /// Message for terminating the current session.
    #[serde(rename = "sml")]
    Logout {},
    /// Message for getting information about the current user.
    #[serde(rename = "ug")]
    UserInfo {},
    /// Message for getting the current storage quotas.
    #[serde(rename = "uq")]
    Quota {
        // `xfer` should be 1.
        #[serde(rename = "xfer")]
        xfer: i32,
        // Without `strg` set to 1, only reports total capacity for account.
        #[serde(rename = "strg")]
        strg: i32,
    },
    /// Message for fetching all available nodes for the current user.
    #[serde(rename = "f")]
    FetchNodes {
        /// `c` should be 1.
        #[serde(rename = "c")]
        c: i32,
        /// TODO
        #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
        r: Option<i32>,
    },
    /// Message for initiating the download of a node.
    #[serde(rename = "g")]
    Download {
        /// TODO
        #[serde(rename = "g")]
        g: i32,
        /// Whether to use HTTPS (by setting it to 2, rarely needed because everything is encrypted already).
        #[serde(rename = "ssl")]
        ssl: i32,
        /// TODO
        #[serde(rename = "p", skip_serializing_if = "Option::is_none")]
        p: Option<String>,
        /// The hash of the node to download.
        #[serde(rename = "n", skip_serializing_if = "Option::is_none")]
        n: Option<String>,
    },
    /// Message for initiating the upload for a new node.
    #[serde(rename = "u")]
    Upload {
        /// The size of the new node.
        #[serde(rename = "s")]
        s: u64,
        /// Whether to use HTTPS (by setting it to 2, rarely needed because everything is encrypted already).
        #[serde(rename = "ssl")]
        ssl: i32,
    },
    /// Message for completing the upload for a new node.
    #[serde(rename = "p")]
    UploadComplete {
        /// The hash of the target parent node.
        #[serde(rename = "t")]
        t: String,
        /// The attributes for the new node.
        #[serde(rename = "n")]
        n: [UploadAttributes; 1],
        /// The idempotence token (needed for request retries).
        #[serde(rename = "i", skip_serializing_if = "String::is_empty")]
        i: String,
    },
    /// Message for changing a node's attributes.
    #[serde(rename = "a")]
    SetFileAttributes {
        /// The new attributes to use for the node.
        #[serde(rename = "attr")]
        attr: String,
        /// The new key to use for the node.
        #[serde(rename = "key", skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        /// The hash of the involved node.
        #[serde(rename = "n")]
        n: String,
        /// The idempotence token (needed for request retries).
        #[serde(rename = "i")]
        i: String,
    },
    /// Message for moving a node to a different location.
    #[serde(rename = "m")]
    Move {
        /// The hash of the node to move.
        #[serde(rename = "n")]
        n: String,
        /// The hash of the target parent node.
        #[serde(rename = "t")]
        t: String,
        /// The idempotence token (needed for request retries).
        #[serde(rename = "i")]
        i: String,
    },
    /// Message for deleting a node.
    #[serde(rename = "d")]
    Delete {
        /// The hash of the node to delete.
        #[serde(rename = "n")]
        n: String,
        /// The idempotence token (needed for request retries).
        #[serde(rename = "i")]
        i: String,
    },
    /// Message for uploading file attributes (also used for downloading file attributes).
    #[serde(rename = "ufa")]
    UploadFileAttributes {
        /// The hash (or handle) of the involved MEGA node.
        h: Option<String>,
        /// The file attribute handler.
        fah: Option<String>,
        /// The size of the file to upload.
        s: Option<u64>,
        /// Whether to use HTTPS (by setting it to 2, rarely needed because everything is encrypted already).
        ssl: i32,
        /// TODO
        r: Option<i32>,
    },
    /// Message for completing the upload of file attributes.
    #[serde(rename = "pfa")]
    PutFileAttributes {
        /// The hash (or handle) of the involved MEGA node.
        n: String,
        /// The file attributes' encoded string.
        fa: String,
    },
}

/// Represents a response message from MEGA's API.
///
/// Keep in mind that these message definitions have been somewhat reverse-engineered from MEGA's C++ SDK, and are, therefore, not complete.
pub enum Response {
    /// An error response.
    Error(ErrorCode),
    /// Response for the `Request::PreLogin` message.
    PreLogin(PreLoginResponse),
    /// Response for the `Request::Login` message.
    Login(LoginResponse),
    /// Response for the `Request::Logout` message.
    Logout(LogoutResponse),
    /// Response for the `Request::UserInfo` message.
    UserInfo(UserInfoResponse),
    /// Response for the `Request::Quota` message.
    Quota(QuotaResponse),
    /// Response for the `Request::FetchNodes` message.
    FetchNodes(FetchNodesResponse),
    /// Response for the `Request::Download` message.
    Download(DownloadResponse),
    /// Response for the `Request::Upload` message.
    Upload(UploadResponse),
    /// Response for the `Request::UploadComplete` message.
    UploadComplete(UploadCompleteResponse),
    /// Response for the `Request::SetFileAttributes` message.
    SetFileAttributes(SetFileAttributesResponse),
    /// Response for the `Request::Move` message.
    Move(MoveResponse),
    /// Response for the `Request::Delete` message.
    Delete(DeleteResponse),
    /// Response for the `Request::UploadFileAttributes` message.
    UploadFileAttributes(UploadFileAttributesResponse),
    /// Response for the `Request::PutFileAttributes` message.
    PutFileAttributes(PutFileAttributesResponse),
}

/// Response for the `Request::PreLogin` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreLoginResponse {
    /// Version of the login ceremony to use.
    #[serde(rename = "v")]
    pub version: i32,
    /// The salt for the user's password-derived key.
    #[serde(rename = "s")]
    pub salt: Option<String>,
}

/// Response for the `Request::Login` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginResponse {
    #[serde(rename = "ach")]
    pub ach: i32,
    #[serde(rename = "csid")]
    pub csid: String,
    #[serde(rename = "k")]
    pub k: String,
    #[serde(rename = "privk")]
    pub privk: String,
    #[serde(rename = "u")]
    pub u: String,
}

/// Response for the `Request::Logout` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogoutResponse {}

/// Response for the `Request::UserInfo` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserInfoResponse {
    #[serde(rename = "u")]
    pub u: String,
    #[serde(rename = "s")]
    pub s: i32,
    #[serde(rename = "email")]
    pub email: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "k")]
    pub key: String,
    #[serde(rename = "c")]
    pub c: i32,
    #[serde(rename = "pubk")]
    pub pubk: String,
    #[serde(rename = "privk")]
    pub privk: String,
    #[serde(rename = "terms")]
    pub terms: Option<String>,
    #[serde(rename = "ts")]
    pub ts: String,
}

/// Response for the `Request::Quota` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaResponse {
    /// Total capacity, in bytes.
    #[serde(rename = "mstrg")]
    pub mstrg: u64,
    /// Used capacity, in bytes.
    #[serde(rename = "cstrg")]
    pub cstrg: u64,
    /// Per folder usage, in bytes ?
    #[serde(rename = "cstrgn")]
    pub cstrgn: HashMap<String, Vec<u64>>,
}

/// Represents a node's metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileMetadata {
    #[serde(rename = "h")]
    pub hash: String,
    #[serde(rename = "k")]
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileS {
    #[serde(rename = "h")]
    pub hash: String,
    #[serde(rename = "u")]
    pub user: String,
}

/// Represents a node's owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileUser {
    #[serde(rename = "u")]
    pub user: String,
    #[serde(rename = "c")]
    pub c: i32,
    #[serde(rename = "m")]
    pub email: String,
}

/// Represents a node in MEGA.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileNode {
    #[serde(rename = "t")]
    pub kind: NodeKind,
    #[serde(rename = "a")]
    pub attr: String,
    #[serde(rename = "fa")]
    pub file_attr: Option<String>,
    #[serde(rename = "h")]
    pub hash: String,
    #[serde(rename = "p")]
    pub parent: String,
    #[serde(rename = "ts")]
    pub ts: u64,
    #[serde(rename = "u")]
    pub user: String,
    #[serde(rename = "k")]
    pub key: Option<String>,
    #[serde(rename = "su")]
    pub s_user: Option<String>,
    #[serde(rename = "sk")]
    pub s_key: Option<String>,
    #[serde(rename = "s")]
    pub sz: Option<u64>,
}

/// Response for the `Request::FetchNodes` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FetchNodesResponse {
    /// The available nodes for this user.
    #[serde(rename = "f")]
    pub nodes: Vec<FileNode>,
    /// Additional metadata for the nodes.
    #[serde(rename = "ok")]
    pub ok: Option<Vec<FileMetadata>>,
    /// Additional metadata for the nodes.
    #[serde(rename = "s")]
    pub s: Option<Vec<FileS>>,
    /// Additional data about the nodes' owners.
    #[serde(rename = "u")]
    pub user: Option<Vec<FileUser>>,
    /// TODO
    #[serde(rename = "sn")]
    pub sn: String,
}

/// Response for the `Request::Download` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadResponse {
    /// The URL to download the file from.
    #[serde(rename = "g")]
    pub download_url: String,
    /// The size of the file (in bytes).
    #[serde(rename = "s")]
    pub size: u64,
    /// The attributes for the file.
    #[serde(rename = "at")]
    pub attr: String,
    /// Additional error codes.
    #[serde(rename = "e")]
    pub err: Option<ErrorCode>,
}

/// Response for the `Request::Upload` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadResponse {
    /// The URL to upload the file's data to.
    #[serde(rename = "p")]
    pub upload_url: String,
}

/// Represents the attributes for an uploaded node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadAttributes {
    /// The kind of the uploaded node.
    #[serde(rename = "t")]
    pub kind: NodeKind,
    /// The attributes for the uploaded node.
    #[serde(rename = "a")]
    pub attr: String,
    /// The key data for the uploaded node.
    #[serde(rename = "k")]
    pub key: String,
    /// The completion handle to validate the node's upload.
    #[serde(rename = "h")]
    pub completion_handle: String,
    /// The file attributes' encoded string.
    #[serde(rename = "fa")]
    pub file_attr: Option<String>,
}

/// Response for the `Request::UploadComplete` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadCompleteResponse {
    /// The nodes that got created.
    #[serde(rename = "f")]
    pub f: Vec<FileNode>,
}

/// Response for the `Request::SetFileAttributes` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetFileAttributesResponse {}

/// Response for the `Request::UploadFileAttributes` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadFileAttributesResponse {
    /// The upload URL for the attribute.
    pub p: String,
}

/// Response for the `Request::PutFileAttributes` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PutFileAttributesResponse {
    #[serde(rename = "fa")]
    fa: String,
}

/// Response for the `Request::Move` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveResponse {}

/// Response for the `Request::Delete` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteResponse {}

impl Request {
    pub(crate) fn parse_response_data(&self, value: Value) -> Result<Response, Error> {
        if value.is_number() {
            let code = json::from_value(value)?;
            return Ok(Response::Error(code));
        }

        let response = match self {
            Request::PreLogin { .. } => {
                let response = json::from_value(value)?;
                Response::PreLogin(response)
            }
            Request::Login { .. } => {
                let response = json::from_value(value)?;
                Response::Login(response)
            }
            Request::Logout { .. } => {
                let response = json::from_value(value)?;
                Response::Logout(response)
            }
            Request::UserInfo { .. } => {
                let response = json::from_value(value)?;
                Response::UserInfo(response)
            }
            Request::Quota { .. } => {
                let response = json::from_value(value)?;
                Response::Quota(response)
            }
            Request::FetchNodes { .. } => {
                let response = json::from_value(value)?;
                Response::FetchNodes(response)
            }
            Request::Download { .. } => {
                let response = json::from_value(value)?;
                Response::Download(response)
            }
            Request::Upload { .. } => {
                let response = json::from_value(value)?;
                Response::Upload(response)
            }
            Request::UploadComplete { .. } => {
                let response = json::from_value(value)?;
                Response::UploadComplete(response)
            }
            Request::SetFileAttributes { .. } => {
                let response = json::from_value(value)?;
                Response::SetFileAttributes(response)
            }
            Request::Move { .. } => {
                let response = json::from_value(value)?;
                Response::Move(response)
            }
            Request::Delete { .. } => {
                let response = json::from_value(value)?;
                Response::Delete(response)
            }
            Request::UploadFileAttributes { .. } => {
                let response = json::from_value(value)?;
                Response::UploadFileAttributes(response)
            }
            Request::PutFileAttributes { .. } => {
                let response = json::from_value(value)?;
                Response::PutFileAttributes(PutFileAttributesResponse { fa: response })
            }
        };

        Ok(response)
    }
}
