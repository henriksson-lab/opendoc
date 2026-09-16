//! The wire protocol: what a client may say, and what the server says back.
//!
//! This is not a command schema. ADR 0004 forbids the service owning one, and
//! it does not: there is no way to ask the service to *do* anything to a
//! document except hand it typed [`Operation`]s, which the service stores and
//! relays but never authors. Everything else on this wire is session state —
//! who you are, what you may do, who else is here.
//!
//! Operations travel as their own serde form, so the type on the wire is the
//! type `opendoc-merge` merges. Documents travel as base64 of canonical CBOR,
//! because the client is expected to compare those bytes.

use crate::permission::{GrantAuditEvent, Role};
use opendoc_core::Document;
use opendoc_merge::{ActorId, Operation};
use serde::{Deserialize, Serialize};

/// Bumped when a message's meaning changes. A client that does not recognise
/// the server's version should refuse to connect rather than guess.
pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientMessage {
    /// Hand the server operations to make durable and relay.
    Submit {
        /// Echoed back on the acknowledgement so a client can match a reply
        /// to a batch it is still holding.
        batch_id: String,
        operations: Vec<Operation>,
    },
    /// Update this connection's presence. Role is *not* settable here.
    Presence {
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        cursor_anchor: Option<String>,
        /// The fixed endpoint of this user's active textual selection. The
        /// cursor is its focus endpoint. Both are opaque to the service.
        #[serde(default)]
        selection_anchor: Option<String>,
    },
    Ping,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ServerMessage {
    /// Sent once, first, on a successful connection. Carries everything a
    /// client needs to reach the server's current document by itself: the
    /// merge base and the whole operation log.
    Welcome {
        protocol_version: u32,
        document_uuid: String,
        subject: String,
        actor: ActorId,
        role: Role,
        commit_seq: u64,
        /// base64 of canonical CBOR of the merge base document.
        base_document: String,
        operations: Vec<Operation>,
        peers: Vec<PeerView>,
        /// The largest batch this service will accept in one `Submit`.
        ///
        /// The cap is the server's, so the server is the only place it is
        /// written down; a client that restated it would be a second
        /// definition of somebody else's limit. A client chunks its outbox to
        /// this number, which is why it has to arrive before the client can
        /// submit anything — and the welcome is the only frame that is
        /// guaranteed to. See `document::DEFAULT_MAX_OPERATIONS_PER_SUBMIT`.
        max_operations_per_submit: usize,
    },
    /// The batch is durable. Sent only to the submitter.
    Accepted {
        batch_id: String,
        commit_seq: u64,
        operation_ids: Vec<WireOperationId>,
    },
    /// The batch was not stored. Nothing about the document changed.
    Rejected {
        batch_id: String,
        code: String,
        message: String,
    },
    /// A commit, relayed to every connection on the document including the
    /// submitter's. Sending it to the author too is deliberate: it makes every
    /// client's input the same ordered stream, and `merge_operations`
    /// deduplicates by operation id, so applying one's own work twice is a
    /// no-op rather than a special case.
    Committed {
        commit_seq: u64,
        subject: String,
        actor: ActorId,
        operations: Vec<Operation>,
    },
    /// The full peer list, resent whenever any of it changes.
    Presence {
        peers: Vec<PeerView>,
    },
    /// The server is ending this connection and why.
    Closed {
        code: String,
        message: String,
    },
    Pong,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WireOperationId {
    pub actor: String,
    pub seq: u64,
}

impl From<&opendoc_merge::OperationId> for WireOperationId {
    fn from(id: &opendoc_merge::OperationId) -> Self {
        Self {
            actor: id.actor.0.clone(),
            seq: id.seq,
        }
    }
}

/// One peer, as the server sees it.
///
/// `subject`, `actor` and `role` are server state. `display_name` and
/// `cursor_anchor` and `selection_anchor` are the only fields a client contributes, and a client can
/// only contribute them for itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PeerView {
    pub subject: String,
    pub actor: ActorId,
    pub display_name: String,
    pub role: Role,
    pub cursor_anchor: Option<String>,
    #[serde(default)]
    pub selection_anchor: Option<String>,
    pub last_seen_ms: u64,
    /// How many live connections this subject holds. One person in two tabs
    /// is one peer, not two.
    pub connections: usize,
}

pub fn encode_document(document: &Document) -> Result<String, crate::error::ServiceError> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    let bytes = opendoc_format::encode_canonical_cbor(document)
        .map_err(|error| crate::error::ServiceError::Internal(error.to_string()))?;
    Ok(STANDARD.encode(bytes))
}

pub fn decode_document(encoded: &str) -> Result<Document, crate::error::ServiceError> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| crate::error::ServiceError::BadRequest(error.to_string()))?;
    opendoc_format::decode_cbor(&bytes)
        .map_err(|error| crate::error::ServiceError::BadRequest(error.to_string()))
}

// ---- HTTP bodies -------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenSessionRequest {
    pub subject: String,
    pub api_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenSessionResponse {
    pub token: String,
    pub subject: String,
    pub actor: ActorId,
    pub expires_at_ms: u64,
    pub protocol_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentView {
    pub document_uuid: String,
    pub role: Role,
    pub commit_seq: u64,
    pub head: Option<String>,
    pub peers: Vec<PeerView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrantView {
    pub subject: String,
    pub role: Role,
}

/// An ACL event emitted by the service, not a claim supplied by the browser.
pub type GrantAuditView = GrantAuditEvent;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SetGrantRequest {
    pub subject: String,
    /// `None` revokes.
    #[serde(default)]
    pub role: Option<Role>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateDocumentRequest {
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}
