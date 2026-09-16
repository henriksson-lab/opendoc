//! HTTP and WebSocket transport.
//!
//! This layer has no policy of its own. It resolves a bearer token to a
//! subject, hands the subject to [`OpenDocService`], and turns the answer into
//! bytes. Every decision — may this caller read, may it write, is this
//! operation well-formed — is made behind [`crate::service`], so reading this
//! file tells you the shape of the API and nothing about what it permits.

use crate::document::{next_connection_id, ConnectionId};
use crate::error::{ServiceError, ServiceResult};
use crate::identity::AuthenticatedSubject;
use crate::origin::OriginPolicy;
use crate::permission::{Action, Role};
use crate::protocol::{
    ClientMessage, CreateDocumentRequest, DocumentView, ErrorBody, GrantAuditView, GrantView,
    OpenSessionRequest, OpenSessionResponse, ServerMessage, SetGrantRequest, PROTOCOL_VERSION,
};
use crate::service::OpenDocService;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_MAX_AGE, ORIGIN, VARY,
};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use opendoc_store::ObjectStore;
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = ErrorBody {
            code: self.code().to_string(),
            message: self.message().to_string(),
        };
        (status, Json(body)).into_response()
    }
}

pub fn router<S>(service: Arc<OpenDocService<S>>) -> Router
where
    S: ObjectStore + Send + Sync + 'static,
{
    Router::new()
        .route("/v1/health", get(health))
        .route(
            "/v1/sessions",
            post(open_session::<S>).delete(close_session::<S>),
        )
        .route("/v1/documents", post(create_document::<S>))
        .route("/v1/documents/{document_uuid}", get(describe_document::<S>))
        .route(
            "/v1/documents/{document_uuid}/grants",
            get(list_grants::<S>).put(set_grant::<S>),
        )
        .route(
            "/v1/documents/{document_uuid}/grants/audit",
            get(list_grant_audit::<S>),
        )
        .route(
            "/v1/documents/{document_uuid}/socket",
            get(document_socket::<S>),
        )
        // Wraps every route, including the socket upgrade. A page cannot
        // reach any of them without being on the allowlist, and one that is
        // not on it is refused here rather than at the document thread.
        .layer(from_fn_with_state(
            service.origins().clone(),
            enforce_origin,
        ))
        .with_state(service)
}

/// The only place this service has an opinion about browsers.
///
/// Three cases, and the order is the argument:
///
/// * **No `Origin`.** Not a page (a browser always sends one cross-origin, and
///   always on a WebSocket handshake). Passed through untouched — this is
///   every existing client of this crate.
/// * **An allowed `Origin`.** Answered, with the CORS headers that let the
///   browser hand the response to the page. A preflight is answered here and
///   never routed: `OPTIONS` is not a method any handler below declares, so
///   routing it would produce a 405 the browser reads as a refusal.
/// * **Any other `Origin`.** Refused with 403 and no CORS headers. For a
///   `fetch` the browser would have refused it anyway; for a WebSocket
///   handshake it would *not* — that is the cross-site hijacking case, and
///   this is where it stops.
async fn enforce_origin(
    State(origins): State<OriginPolicy>,
    request: Request,
    next: Next,
) -> Response {
    let origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let Some(origin) = origin else {
        return next.run(request).await;
    };
    if !origins.allows(&origin) {
        return ServiceError::Forbidden(format!("origin {origin} is not allowed")).into_response();
    }
    let preflight = request.method() == Method::OPTIONS;
    let mut response = if preflight {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&origin) {
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    // The answer depends on the request's origin, so a cache that ignored it
    // would serve one page's answer to another.
    headers.insert(VARY, HeaderValue::from_static("origin"));
    if preflight {
        headers.insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
        );
        headers.insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("authorization, content-type"),
        );
        headers.insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    }
    response
}

/// Binds, serves, and hands back the address it actually got.
///
/// Port 0 is the normal argument in a test: the operating system picks a free
/// port, so two test runs never collide and nothing has to guess.
pub async fn serve<S>(
    service: Arc<OpenDocService<S>>,
    address: SocketAddr,
) -> ServiceResult<RunningServer>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let listener = TcpListener::bind(address)
        .await
        .map_err(|error| ServiceError::Internal(format!("bind {address}: {error}")))?;
    let local_address = listener
        .local_addr()
        .map_err(|error| ServiceError::Internal(error.to_string()))?;
    let (shutdown, shutdown_signal) = tokio::sync::oneshot::channel::<()>();
    let router = router(service);
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_signal.await;
            })
            .await;
    });
    Ok(RunningServer {
        local_address,
        shutdown: Some(shutdown),
        join,
    })
}

pub struct RunningServer {
    local_address: SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    join: tokio::task::JoinHandle<()>,
}

impl RunningServer {
    pub fn local_address(&self) -> SocketAddr {
        self.local_address
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.local_address)
    }

    pub fn websocket_url(&self, document_uuid: &str, token: &str) -> String {
        format!(
            "ws://{}/v1/documents/{document_uuid}/socket?token={token}",
            self.local_address
        )
    }

    pub async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = self.join.await;
    }
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "protocol_version": PROTOCOL_VERSION }))
}

async fn open_session<S>(
    State(service): State<Arc<OpenDocService<S>>>,
    Json(request): Json<OpenSessionRequest>,
) -> Result<Json<OpenSessionResponse>, ServiceError>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let issued = service
        .identity()
        .open_session(&request.subject, &request.api_key)?;
    Ok(Json(OpenSessionResponse {
        token: issued.token,
        subject: issued.subject,
        actor: issued.actor,
        expires_at_ms: issued.expires_at_ms,
        protocol_version: PROTOCOL_VERSION,
    }))
}

async fn close_session<S>(
    State(service): State<Arc<OpenDocService<S>>>,
    headers: HeaderMap,
) -> Result<StatusCode, ServiceError>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let token = bearer_token(&headers)?;
    service.identity().close_session(&token)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_document<S>(
    State(service): State<Arc<OpenDocService<S>>>,
    headers: HeaderMap,
    Json(request): Json<CreateDocumentRequest>,
) -> Result<Json<serde_json::Value>, ServiceError>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let caller = authenticate(&service, &headers)?;
    let document_uuid = service.create_document(&caller.subject, &request.title)?;
    Ok(Json(json!({ "document_uuid": document_uuid })))
}

async fn describe_document<S>(
    State(service): State<Arc<OpenDocService<S>>>,
    Path(document_uuid): Path<String>,
    headers: HeaderMap,
) -> Result<Json<DocumentView>, ServiceError>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let caller = authenticate(&service, &headers)?;
    let (handle, role) = service.open_document(&document_uuid, &caller.subject, Action::Read)?;
    let status = handle.status().await?;
    Ok(Json(DocumentView {
        document_uuid: status.document_uuid,
        role,
        commit_seq: status.commit_seq,
        head: status.head,
        peers: status.peers,
    }))
}

async fn list_grants<S>(
    State(service): State<Arc<OpenDocService<S>>>,
    Path(document_uuid): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<GrantView>>, ServiceError>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let caller = authenticate(&service, &headers)?;
    service
        .permissions()
        .authorize(&document_uuid, &caller.subject, Action::Share)?;
    let grants = service
        .permissions()
        .grant_list(&document_uuid)?
        .into_iter()
        .map(|(subject, role)| GrantView { subject, role })
        .collect();
    Ok(Json(grants))
}

async fn set_grant<S>(
    State(service): State<Arc<OpenDocService<S>>>,
    Path(document_uuid): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SetGrantRequest>,
) -> Result<StatusCode, ServiceError>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let caller = authenticate(&service, &headers)?;
    service.set_grant(
        &document_uuid,
        &caller.subject,
        &request.subject,
        request.role,
    )?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_grant_audit<S>(
    State(service): State<Arc<OpenDocService<S>>>,
    Path(document_uuid): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<GrantAuditView>>, ServiceError>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let caller = authenticate(&service, &headers)?;
    service
        .permissions()
        .authorize(&document_uuid, &caller.subject, Action::Share)?;
    Ok(Json(service.permissions().grant_audit(&document_uuid)?))
}

#[derive(Debug, Deserialize)]
struct SocketQuery {
    /// A WebSocket handshake from a browser cannot carry an `Authorization`
    /// header, so the token rides in the query string. That puts it in access
    /// logs and `Referer`, which is why sessions are short-lived and
    /// individually revocable rather than long-lived keys.
    token: String,
    #[serde(default)]
    display_name: Option<String>,
}

async fn document_socket<S>(
    State(service): State<Arc<OpenDocService<S>>>,
    Path(document_uuid): Path<String>,
    Query(query): Query<SocketQuery>,
    upgrade: WebSocketUpgrade,
) -> Response
where
    S: ObjectStore + Send + Sync + 'static,
{
    // Authenticate and authorize *before* upgrading. A refused caller gets an
    // HTTP status it can read, not a socket that closes for reasons it has to
    // guess at.
    let caller = match service.identity().authenticate(&query.token) {
        Ok(caller) => caller,
        Err(error) => return error.into_response(),
    };
    let (handle, _role) = match service.open_document(&document_uuid, &caller.subject, Action::Read)
    {
        Ok(opened) => opened,
        Err(error) => return error.into_response(),
    };
    let display_name = query.display_name.unwrap_or_else(|| caller.subject.clone());
    upgrade.on_upgrade(move |socket| async move {
        run_socket(socket, handle, caller, display_name).await;
    })
}

async fn run_socket(
    socket: WebSocket,
    handle: crate::document::DocumentHandle,
    caller: AuthenticatedSubject,
    display_name: String,
) {
    let connection: ConnectionId = next_connection_id();
    let (outbox, mut outbox_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let welcome = match handle
        .join(
            connection,
            &caller.subject,
            &caller.actor,
            &display_name,
            outbox,
        )
        .await
    {
        Ok(welcome) => welcome,
        Err(error) => {
            let mut socket = socket;
            let _ = socket
                .send(Message::Text(
                    serde_json::to_string(&ServerMessage::Closed {
                        code: error.code().to_string(),
                        message: error.message().to_string(),
                    })
                    .unwrap_or_default()
                    .into(),
                ))
                .await;
            let _ = socket.close().await;
            return;
        }
    };

    let (mut sink, mut stream) = socket.split();
    if send_json(&mut sink, &welcome).await.is_err() {
        handle.leave(connection);
        return;
    }

    // One task drains the document thread's fanout into the socket. It also
    // owns the close: a `Closed` message is the last thing a connection sees.
    let writer = tokio::spawn(async move {
        while let Some(message) = outbox_rx.recv().await {
            let closing = matches!(message, ServerMessage::Closed { .. });
            if send_json(&mut sink, &message).await.is_err() {
                break;
            }
            if closing {
                break;
            }
        }
        let _ = sink.close().await;
    });

    while let Some(frame) = stream.next().await {
        let Ok(frame) = frame else { break };
        let text = match frame {
            Message::Text(text) => text.to_string(),
            Message::Binary(bytes) => match String::from_utf8(bytes.to_vec()) {
                Ok(text) => text,
                Err(_) => continue,
            },
            Message::Close(_) => break,
            // Ping/Pong are answered by the transport; nothing above cares.
            Message::Ping(_) | Message::Pong(_) => continue,
        };
        let Ok(message) = serde_json::from_str::<ClientMessage>(&text) else {
            // A frame this layer cannot parse never reaches the document
            // thread, and is not a reason to drop a session that may be
            // mid-edit. It is dropped, and nothing else about the connection
            // changes.
            continue;
        };
        match message {
            ClientMessage::Submit {
                batch_id,
                operations,
            } => {
                // The reply the client sees is the `Accepted`/`Rejected`
                // message the document thread fans out; the error here is
                // already reported and only ends the loop if the thread died.
                if let Err(error) = handle.submit(connection, &batch_id, operations).await {
                    if matches!(error, ServiceError::Internal(_)) {
                        break;
                    }
                }
            }
            ClientMessage::Presence {
                display_name,
                cursor_anchor,
                selection_anchor,
            } => {
                if handle
                    .presence(connection, display_name, cursor_anchor, selection_anchor)
                    .await
                    .is_err()
                {
                    break;
                }
            }
            ClientMessage::Ping => {}
        }
    }

    handle.leave(connection);
    writer.abort();
}

async fn send_json<Sink>(sink: &mut Sink, message: &ServerMessage) -> Result<(), ()>
where
    Sink: SinkExt<Message> + Unpin,
{
    let text = serde_json::to_string(message).map_err(|_| ())?;
    sink.send(Message::Text(text.into())).await.map_err(|_| ())
}

fn bearer_token(headers: &HeaderMap) -> ServiceResult<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ServiceError::Unauthenticated("authorization header is missing".to_string())
        })?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .ok_or_else(|| {
            ServiceError::Unauthenticated("authorization header is not a bearer token".to_string())
        })?;
    Ok(token.trim().to_string())
}

fn authenticate<S>(
    service: &OpenDocService<S>,
    headers: &HeaderMap,
) -> ServiceResult<AuthenticatedSubject>
where
    S: ObjectStore + Send + Sync + 'static,
{
    service.identity().authenticate(&bearer_token(headers)?)
}

/// Exposed so a caller can render a role without depending on serde details.
pub fn role_name(role: Role) -> &'static str {
    role.as_str()
}
