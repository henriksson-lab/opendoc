//! The client side of the protocol.
//!
//! A service whose only client is a test harness is a service whose protocol
//! has never been implemented twice, so this ships as part of the crate: one
//! reader and one writer of the wire format, side by side, and a replica that
//! builds operations the way `opendoc-app` builds them — through
//! [`CausalContext::observing`] over what it has applied.
//!
//! [`DocumentSession`] keeps the merge base and the operation set the server
//! sent it, applies every `Committed` message into that set, and materialises
//! with the same [`merge_operations`] the server uses. That is the whole
//! convergence argument: both sides hold the same base and the same set, and
//! ADR 0007 makes the merged bytes a function of exactly those two things.

use crate::error::{ServiceError, ServiceResult};
use crate::protocol::{
    decode_document, ClientMessage, CreateDocumentRequest, DocumentView, GrantAuditView, GrantView,
    OpenSessionRequest, OpenSessionResponse, PeerView, ServerMessage, SetGrantRequest,
    PROTOCOL_VERSION,
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use hyper::body::Bytes;
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use opendoc_core::Document;
use opendoc_merge::{
    merge_operations, ActorId, CausalContext, Operation, OperationId, OperationKind,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

/// An authenticated client of one service.
#[derive(Clone, Debug)]
pub struct ServiceClient {
    address: SocketAddr,
    token: String,
    subject: String,
    actor: ActorId,
}

impl ServiceClient {
    /// Exchanges an API key for a session.
    pub async fn open_session(
        address: SocketAddr,
        subject: &str,
        api_key: &str,
    ) -> ServiceResult<Self> {
        let response: OpenSessionResponse = post_json(
            address,
            "/v1/sessions",
            None,
            &OpenSessionRequest {
                subject: subject.to_string(),
                api_key: api_key.to_string(),
            },
        )
        .await?;
        Ok(Self {
            address,
            token: response.token,
            subject: response.subject,
            actor: response.actor,
        })
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub async fn create_document(&self, title: &str) -> ServiceResult<String> {
        let value: serde_json::Value = post_json(
            self.address,
            "/v1/documents",
            Some(&self.token),
            &CreateDocumentRequest {
                title: title.to_string(),
            },
        )
        .await?;
        value
            .get("document_uuid")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
            .ok_or_else(|| ServiceError::Internal("create returned no document uuid".to_string()))
    }

    pub async fn describe_document(&self, document_uuid: &str) -> ServiceResult<DocumentView> {
        request_json(
            self.address,
            "GET",
            &format!("/v1/documents/{document_uuid}"),
            Some(&self.token),
            None,
        )
        .await
    }

    pub async fn list_grants(&self, document_uuid: &str) -> ServiceResult<Vec<GrantView>> {
        request_json(
            self.address,
            "GET",
            &format!("/v1/documents/{document_uuid}/grants"),
            Some(&self.token),
            None,
        )
        .await
    }

    pub async fn list_grant_audit(
        &self,
        document_uuid: &str,
    ) -> ServiceResult<Vec<GrantAuditView>> {
        request_json(
            self.address,
            "GET",
            &format!("/v1/documents/{document_uuid}/grants/audit"),
            Some(&self.token),
            None,
        )
        .await
    }

    pub async fn set_grant(
        &self,
        document_uuid: &str,
        subject: &str,
        role: Option<crate::permission::Role>,
    ) -> ServiceResult<()> {
        let body = serde_json::to_vec(&SetGrantRequest {
            subject: subject.to_string(),
            role,
        })
        .map_err(|error| ServiceError::Internal(error.to_string()))?;
        let (status, bytes) = request(
            self.address,
            "PUT",
            &format!("/v1/documents/{document_uuid}/grants"),
            Some(&self.token),
            Some(body),
        )
        .await?;
        if status.is_success() {
            Ok(())
        } else {
            Err(error_from_body(status, &bytes))
        }
    }

    /// Opens the WebSocket and consumes the `Welcome`.
    pub async fn connect(
        &self,
        document_uuid: &str,
        display_name: &str,
    ) -> ServiceResult<DocumentSession> {
        let url = format!(
            "ws://{}/v1/documents/{document_uuid}/socket?token={}&display_name={}",
            self.address,
            urlencode(&self.token),
            urlencode(display_name)
        );
        let (mut socket, _response) = match connect_async(url).await {
            Ok(pair) => pair,
            // The server refuses an unauthorised upgrade with an HTTP status,
            // before the socket exists. Reporting that as an internal error
            // would lose the one thing the caller needs to know — whether it
            // was refused, or whether the connection broke.
            Err(WsError::Http(response)) => {
                let status = response.status();
                let body = response.into_body().unwrap_or_default();
                return Err(upgrade_refused(status.as_u16(), &body));
            }
            Err(error) => {
                return Err(ServiceError::Internal(format!(
                    "websocket connect: {error}"
                )))
            }
        };
        let first = next_server_message(&mut socket).await?;
        match first {
            ServerMessage::Welcome {
                protocol_version,
                document_uuid,
                subject,
                actor,
                role,
                commit_seq,
                base_document,
                operations,
                peers,
                max_operations_per_submit,
            } => {
                // The browser's driver has always refused a welcome from a
                // protocol it does not speak; this client used to discard the
                // field with a `..`, so the one path that ships the server's
                // own types beside it was the one that guessed. A version that
                // disagrees means the frames below parse and mean something
                // else, which is the failure that is silent if it is not
                // checked here.
                if protocol_version != PROTOCOL_VERSION {
                    return Err(ServiceError::Conflict(format!(
                        "the service speaks protocol version {protocol_version} and this client speaks {PROTOCOL_VERSION}"
                    )));
                }
                let base = decode_document(&base_document)?;
                Ok(DocumentSession {
                    socket,
                    document_uuid,
                    subject,
                    actor,
                    role,
                    commit_seq,
                    base,
                    operations,
                    peers,
                    max_operations_per_submit,
                })
            }
            ServerMessage::Closed { code, message } => Err(match code.as_str() {
                "forbidden" => ServiceError::Forbidden(message),
                "unauthenticated" => ServiceError::Unauthenticated(message),
                _ => ServiceError::Internal(message),
            }),
            other => Err(ServiceError::Internal(format!(
                "expected a welcome, got {other:?}"
            ))),
        }
    }
}

type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A live connection plus the replica it maintains.
pub struct DocumentSession {
    socket: ClientSocket,
    document_uuid: String,
    subject: String,
    actor: ActorId,
    role: crate::permission::Role,
    commit_seq: u64,
    base: Document,
    operations: Vec<Operation>,
    peers: Vec<PeerView>,
    max_operations_per_submit: usize,
}

impl DocumentSession {
    pub fn document_uuid(&self) -> &str {
        &self.document_uuid
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn role(&self) -> crate::permission::Role {
        self.role
    }

    pub fn commit_seq(&self) -> u64 {
        self.commit_seq
    }

    pub fn peers(&self) -> &[PeerView] {
        &self.peers
    }

    /// The largest batch this service accepts in one submit, as its welcome
    /// said. A transport chunks to this rather than to a number of its own.
    pub fn max_operations_per_submit(&self) -> usize {
        self.max_operations_per_submit
    }

    pub fn base(&self) -> &Document {
        &self.base
    }

    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    /// The replica's current document: the merge base folded with every
    /// operation this client has seen.
    pub fn document(&self) -> ServiceResult<Document> {
        merge_operations(&self.base, std::slice::from_ref(&self.operations))
            .map(|result| result.document)
            .map_err(|error| ServiceError::Internal(format!("client merge failed: {error:?}")))
    }

    /// The canonical CBOR of [`Self::document`] — the bytes two replicas must
    /// agree on.
    pub fn document_bytes(&self) -> ServiceResult<Vec<u8>> {
        opendoc_format::encode_canonical_cbor(&self.document()?)
            .map_err(|error| ServiceError::Internal(error.to_string()))
    }

    /// Builds an operation in this replica's current causal context.
    ///
    /// The sequence number is one past the highest this replica has issued —
    /// which the server independently requires — and the context observes
    /// exactly what the replica has applied, which is what makes the merge's
    /// happened-before relation true rather than assumed.
    pub fn author(&self, kind: OperationKind) -> Operation {
        let next_seq = self
            .operations
            .iter()
            .filter(|operation| operation.id.actor == self.actor)
            .map(|operation| operation.id.seq)
            .max()
            .unwrap_or(0)
            + 1;
        Operation::in_context(
            OperationId {
                actor: self.actor.clone(),
                seq: next_seq,
            },
            kind,
            CausalContext::observing(self.operations.iter()),
        )
    }

    /// Authors `count` operations in one causal context, numbered
    /// consecutively — the shape a burst of typing produces.
    pub fn author_batch(&self, kinds: Vec<OperationKind>) -> Vec<Operation> {
        let mut applied = self.operations.clone();
        let mut batch = Vec::new();
        let next_seq = applied
            .iter()
            .filter(|operation| operation.id.actor == self.actor)
            .map(|operation| operation.id.seq)
            .max()
            .unwrap_or(0);
        for (index, kind) in kinds.into_iter().enumerate() {
            let next_seq = next_seq + index as u64 + 1;
            let operation = Operation::in_context(
                OperationId {
                    actor: self.actor.clone(),
                    seq: next_seq,
                },
                kind,
                CausalContext::observing(applied.iter()),
            );
            applied.push(operation.clone());
            batch.push(operation);
        }
        batch
    }

    pub async fn submit(
        &mut self,
        batch_id: &str,
        operations: Vec<Operation>,
    ) -> ServiceResult<()> {
        self.send(&ClientMessage::Submit {
            batch_id: batch_id.to_string(),
            operations,
        })
        .await
    }

    pub async fn announce_presence(
        &mut self,
        display_name: Option<&str>,
        cursor_anchor: Option<&str>,
        selection_anchor: Option<&str>,
    ) -> ServiceResult<()> {
        self.send(&ClientMessage::Presence {
            display_name: display_name.map(ToString::to_string),
            cursor_anchor: cursor_anchor.map(ToString::to_string),
            selection_anchor: selection_anchor.map(ToString::to_string),
        })
        .await
    }

    /// Reads the next server message and folds it into the replica.
    pub async fn next_event(&mut self) -> ServiceResult<ServerMessage> {
        let message = next_server_message(&mut self.socket).await?;
        match &message {
            ServerMessage::Committed {
                commit_seq,
                operations,
                ..
            } => {
                self.commit_seq = *commit_seq;
                for operation in operations {
                    if !self
                        .operations
                        .iter()
                        .any(|existing| existing.id == operation.id)
                    {
                        self.operations.push(operation.clone());
                    }
                }
            }
            ServerMessage::Presence { peers } => self.peers = peers.clone(),
            _ => {}
        }
        Ok(message)
    }

    /// Reads events until the replica has seen `commit_seq`.
    pub async fn wait_for_commit(&mut self, commit_seq: u64) -> ServiceResult<()> {
        while self.commit_seq < commit_seq {
            let message = self.next_event().await?;
            if let ServerMessage::Closed { code, message } = message {
                return Err(ServiceError::Internal(format!("{code}: {message}")));
            }
        }
        Ok(())
    }

    pub async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }

    async fn send(&mut self, message: &ClientMessage) -> ServiceResult<()> {
        let text = serde_json::to_string(message)
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        self.socket
            .send(WsMessage::Text(text.into()))
            .await
            .map_err(|error| ServiceError::Internal(format!("websocket send: {error}")))
    }
}

async fn next_server_message(socket: &mut ClientSocket) -> ServiceResult<ServerMessage> {
    loop {
        let Some(frame) = socket.next().await else {
            return Err(ServiceError::Internal(
                "websocket closed before a message arrived".to_string(),
            ));
        };
        let frame =
            frame.map_err(|error| ServiceError::Internal(format!("websocket read: {error}")))?;
        match frame {
            WsMessage::Text(text) => {
                return serde_json::from_str(&text).map_err(|error| {
                    ServiceError::Internal(format!("server message did not parse: {error}"))
                })
            }
            WsMessage::Binary(bytes) => {
                return serde_json::from_slice(&bytes).map_err(|error| {
                    ServiceError::Internal(format!("server message did not parse: {error}"))
                })
            }
            WsMessage::Close(_) => {
                return Err(ServiceError::Internal(
                    "server closed the websocket".to_string(),
                ))
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_) => continue,
        }
    }
}

// ---- a very small HTTP/1.1 client over hyper ---------------------------

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Vec<u8>>,
) -> ServiceResult<(StatusCode, Bytes)> {
    let stream = TcpStream::connect(address)
        .await
        .map_err(|error| ServiceError::Internal(format!("connect {address}: {error}")))?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|error| ServiceError::Internal(format!("http handshake: {error}")))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(hyper::header::HOST, address.to_string());
    if let Some(token) = token {
        builder = builder.header(hyper::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header(hyper::header::CONTENT_TYPE, "application/json");
    }
    let request = builder
        .body(http_body_util::Full::new(Bytes::from(
            body.unwrap_or_default(),
        )))
        .map_err(|error| ServiceError::Internal(error.to_string()))?;

    let response = sender
        .send_request(request)
        .await
        .map_err(|error| ServiceError::Internal(format!("http request: {error}")))?;
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|error| ServiceError::Internal(format!("http body: {error}")))?
        .to_bytes();
    Ok((status, bytes))
}

async fn request_json<T: DeserializeOwned>(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Vec<u8>>,
) -> ServiceResult<T> {
    let (status, bytes) = request(address, method, path, token, body).await?;
    if !status.is_success() {
        return Err(error_from_body(status, &bytes));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| ServiceError::Internal(format!("response did not parse: {error}")))
}

async fn post_json<B: Serialize, T: DeserializeOwned>(
    address: SocketAddr,
    path: &str,
    token: Option<&str>,
    body: &B,
) -> ServiceResult<T> {
    let encoded =
        serde_json::to_vec(body).map_err(|error| ServiceError::Internal(error.to_string()))?;
    request_json(address, "POST", path, token, Some(encoded)).await
}

fn error_from_body(status: StatusCode, bytes: &[u8]) -> ServiceError {
    let parsed: Option<crate::protocol::ErrorBody> = serde_json::from_slice(bytes).ok();
    let message = parsed
        .as_ref()
        .map(|body| body.message.clone())
        .unwrap_or_else(|| String::from_utf8_lossy(bytes).to_string());
    match status.as_u16() {
        401 => ServiceError::Unauthenticated(message),
        403 => ServiceError::Forbidden(message),
        404 => ServiceError::NotFound(message),
        400 => ServiceError::BadRequest(message),
        409 => ServiceError::Conflict(message),
        _ => ServiceError::Internal(message),
    }
}

/// Turns a refused WebSocket upgrade into the error its status names, marked
/// so a caller can tell it apart from a socket that opened and was then closed.
fn upgrade_refused(status: u16, body: &[u8]) -> ServiceError {
    let parsed: Option<crate::protocol::ErrorBody> = serde_json::from_slice(body).ok();
    let detail = parsed
        .map(|body| body.message)
        .unwrap_or_else(|| String::from_utf8_lossy(body).to_string());
    let message = format!("websocket upgrade refused (HTTP {status}): {detail}");
    match status {
        401 => ServiceError::Unauthenticated(message),
        403 => ServiceError::Forbidden(message),
        404 => ServiceError::NotFound(message),
        400 => ServiceError::BadRequest(message),
        409 => ServiceError::Conflict(message),
        _ => ServiceError::Internal(message),
    }
}

fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
