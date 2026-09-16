//! The native shell's collaboration transport.
//!
//! The browser cannot use `opendoc-service`'s Rust client — it is built on
//! `tokio` and `tungstenite`, and that crate must stay out of the WebAssembly
//! dependency graph (ADR 0015) — so the page owns the socket there and
//! `crates/opendoc-wasm/src/collab.rs` owns the frames. Here the opposite is
//! true and this is the better half of the trade: the shell has a real
//! runtime, the service crate ships a tested client of its own protocol, and
//! keeping the socket in Rust means the session token never enters the webview
//! at all. `docs/adr/0018` records the decision and what the two paths share.
//!
//! What the page sees is the same either way: a `CollabStatus` to render. It
//! arrives as a return value from `collab_connect` and then as
//! `opendoc://collab-status` events.
//!
//! # Shape
//!
//! One OS thread per session, running a current-thread runtime and owning the
//! [`DocumentSession`]. It reaches the document through the same
//! `Mutex<OpenDocApp>` the `dispatch` command uses, and holds that lock only
//! across synchronous calls — never across an `await`, so a slow socket cannot
//! block a keystroke.
//!
//! Commands from the UI arrive on a channel. The thread ends when the channel
//! closes, when a `Disconnect` arrives, or when reconnecting is pointless.

use opendoc_app::{
    OpenDocApp, OpenDocPresencePeer, OpenDocServiceRole, OpenDocServiceSession, Operation,
};
use opendoc_service::client::{DocumentSession, ServiceClient};
use opendoc_service::{GrantView, PeerView, Role, ServerMessage, ServiceError};
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// Outbound sweep interval. Matches the browser path's pump for the same
/// reason: a local gesture becomes an operation inside `dispatch`, and nothing
/// in the app notifies a transport when one did.
const PUMP: Duration = Duration::from_millis(250);

/// Backoff for a dropped socket, then the last value repeats.
const RETRY_MS: [u64; 4] = [500, 1000, 2000, 4000];

/// How many attempts before the session is declared over. Bounded for the same
/// reason as in the browser: "reconnecting…" against a service that will never
/// answer is the dishonest state.
const MAX_ATTEMPTS: u32 = 8;

/// How many `Submit` frames one sweep may put on the wire. The browser driver's
/// `MAX_SUBMIT_FRAMES_PER_TICK`, for the same reason.
const MAX_SUBMIT_FRAMES_PER_SWEEP: usize = 8;

/// Sweeps to wait for an acknowledgement before sending the batch again.
///
/// The sweep runs every [`PUMP`], so this is ten seconds. Resending is safe:
/// the service acknowledges a byte-identical replay of work it already holds
/// without committing anything (ADR 0015, "History immutability"). Before this
/// there was no timeout on either path, so an `Accepted` lost on a socket that
/// stayed open stranded that batch and everything after it.
const ACK_TIMEOUT_SWEEPS: u64 = 40;

/// Refusals this session recovers from before giving up. The browser driver's
/// `MAX_REFUSAL_RECOVERIES`, for the same reason: a refusal usually means this
/// replica and the service disagree about what the service holds, which a
/// fresh welcome settles — and a refusal a welcome cannot fix must end the
/// session rather than loop.
const MAX_REFUSAL_RECOVERIES: u32 = 3;

// ---- What the page renders ----------------------------------------------
//
// The same field names `collab::CollabStatus` serializes in `opendoc-wasm`, so
// `collab.ts` has one type for both runtimes. Pinned from both sides:
// `the_status_serializes_with_the_fields_the_frontend_reads` there and
// `the_status_carries_the_fields_the_frontend_reads` here.

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Notice {
    pub kind: String,
    pub message: String,
    pub resumable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CollabStatus {
    pub phase: &'static str,
    pub document_uuid: String,
    pub display_name: String,
    pub commit_seq: u64,
    pub acknowledged_seq: u64,
    pub pending_operations: usize,
    pub can_submit: bool,
    /// Always false on this path, and present so `collab.ts` has one type for
    /// both runtimes. The browser's driver holds no socket, so it asks its
    /// caller for a fresh one through this field; here the socket is in this
    /// process and a resynchronisation is just this connection ending with a
    /// resumable notice, which the loop in [`NativeCollab::run`] then acts on.
    pub reconnect_requested: bool,
    /// Always false on this path: the shell pushes a status *after* it has
    /// already applied the change, and the page re-reads the document on every
    /// status it receives.
    pub document_changed: bool,
    /// The caret, rebased across the remote edit just applied. `None` when the
    /// page has not reported one, or when nothing moved it. The browser path
    /// carries the same field; both are read once, so a later status cannot
    /// drag the caret backwards.
    pub selection: Option<opendoc_app::EditorSelection>,
    pub notice: Option<Notice>,
    pub session: Option<OpenDocServiceSession>,
}

impl CollabStatus {
    pub fn idle() -> Self {
        Self {
            phase: "idle",
            document_uuid: String::new(),
            display_name: String::new(),
            commit_seq: 0,
            acknowledged_seq: 0,
            pending_operations: 0,
            can_submit: false,
            reconnect_requested: false,
            document_changed: false,
            selection: None,
            notice: None,
            session: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectOptions {
    pub service_url: String,
    pub subject: String,
    pub api_key: String,
    #[serde(default)]
    pub document_uuid: Option<String>,
    pub display_name: String,
    /// Used only when a document is being created.
    #[serde(default)]
    pub title: Option<String>,
}

// ---- The session thread --------------------------------------------------

enum Command {
    Cursor(Option<String>),
    SelectionAnchor(Option<String>),
    /// The page's caret, so a remote commit can rebase it. Parsed on the page
    /// side; `None` clears it.
    Selection(Option<opendoc_app::EditorSelection>),
    Disconnect,
}

/// Everything the shell keeps about collaboration between commands.
pub struct NativeCollab {
    app: Arc<Mutex<OpenDocApp>>,
    emit: Arc<dyn Fn(CollabStatus) + Send + Sync>,
    status: Mutex<CollabStatus>,
    commands: Mutex<Option<mpsc::UnboundedSender<Command>>>,
    /// The authenticated HTTP client stays in the native process.  The page
    /// may ask the shell to use it for the service's ACL endpoints, but never
    /// receives its bearer token (ADR 0018).
    sharing: Mutex<Option<SharingSession>>,
}

#[derive(Clone)]
struct SharingSession {
    client: ServiceClient,
    document_uuid: String,
    /// The human/copyable service address the user supplied, never a bearer
    /// link. `ServiceClient` deliberately stores only its resolved socket.
    service_url: String,
}

impl NativeCollab {
    pub fn new(app: Arc<Mutex<OpenDocApp>>, emit: Arc<dyn Fn(CollabStatus) + Send + Sync>) -> Self {
        Self {
            app,
            emit,
            status: Mutex::new(CollabStatus::idle()),
            commands: Mutex::new(None),
            sharing: Mutex::new(None),
        }
    }

    pub fn status(&self) -> CollabStatus {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|_| CollabStatus::idle())
    }

    /// Starts a session. Returns immediately with a `connecting` status; every
    /// later status arrives as an event.
    pub fn connect(self: &Arc<Self>, options: ConnectOptions) -> Result<CollabStatus, String> {
        let address = resolve_address(&options.service_url)?;
        // Keep the display/share address on the same input-validation path as
        // the transport address.  `resolve_address` intentionally reduces an
        // address to `host:port`; retaining the original URI here used to let
        // a query or user-info string which transport ignored reappear in a
        // supposedly credential-free share link.
        let service_url = canonical_service_url(&options.service_url)?;
        self.stop();
        let (sender, receiver) = mpsc::unbounded_channel();
        *self.commands.lock().map_err(poisoned)? = Some(sender);

        let connecting = CollabStatus {
            phase: "connecting",
            display_name: options.display_name.clone(),
            document_uuid: options.document_uuid.clone().unwrap_or_default(),
            ..CollabStatus::idle()
        };
        self.publish(connecting.clone());

        let session = Arc::clone(self);
        std::thread::Builder::new()
            .name("opendoc-collab".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        session.fail("runtime", &format!("no async runtime: {error}"), false);
                        return;
                    }
                };
                runtime.block_on(session.run(address, service_url, options, receiver));
            })
            .map_err(|error| format!("could not start the collaboration thread: {error}"))?;
        Ok(connecting)
    }

    /// Ends the session. The operation log stays; the service's answers do not.
    pub fn disconnect(&self) -> CollabStatus {
        self.stop();
        if let Ok(mut sharing) = self.sharing.lock() {
            *sharing = None;
        }
        if let Ok(mut app) = self.app.lock() {
            if app.service_session().is_some() {
                app.leave_collaboration_session();
            }
        }
        let idle = CollabStatus::idle();
        self.publish(idle.clone());
        idle
    }

    pub fn set_selection(&self, selection: Option<opendoc_app::EditorSelection>) {
        let sender = self.commands.lock().ok().and_then(|slot| slot.clone());
        if let Some(sender) = sender {
            let _ = sender.send(Command::Selection(selection));
        }
    }

    pub fn set_cursor(&self, anchor: Option<String>) {
        let sender = self.commands.lock().ok().and_then(|slot| slot.clone());
        if let Some(sender) = sender {
            let _ = sender.send(Command::Cursor(anchor));
        }
    }

    pub fn set_selection_anchor(&self, anchor: Option<String>) {
        let sender = self.commands.lock().ok().and_then(|slot| slot.clone());
        if let Some(sender) = sender {
            let _ = sender.send(Command::SelectionAnchor(anchor));
        }
    }

    /// Lists the server-owned grant table for the live document. The webview
    /// cannot name another document or supply a bearer token; the service
    /// still performs the owner check and returns its own refusal.
    pub async fn list_grants(&self) -> Result<Vec<GrantView>, String> {
        let sharing = self.live_sharing_session()?;
        sharing
            .client
            .list_grants(&sharing.document_uuid)
            .await
            .map_err(|error| describe(&error))
    }

    /// Lists bounded server-owned ACL history for the live document. As with
    /// grants, neither a document id nor a bearer token crosses the webview
    /// boundary and the service still performs the owner check.
    pub async fn list_grant_audit(&self) -> Result<Vec<opendoc_service::GrantAuditView>, String> {
        let sharing = self.live_sharing_session()?;
        sharing
            .client
            .list_grant_audit(&sharing.document_uuid)
            .await
            .map_err(|error| describe(&error))
    }

    /// Requests one server-authoritative ACL mutation. `None` is revoke; role
    /// parsing is deliberately closed so the bridge cannot smuggle a new
    /// product-level link or invitation policy into the service protocol.
    pub async fn set_grant(&self, subject: String, role: Option<String>) -> Result<(), String> {
        let subject = subject.trim();
        if subject.is_empty() {
            return Err("a subject is required".to_string());
        }
        let role = role
            .as_deref()
            .map(Role::parse)
            .transpose()
            .map_err(|error| describe(&error))?;
        let sharing = self.live_sharing_session()?;
        sharing
            .client
            .set_grant(&sharing.document_uuid, subject, role)
            .await
            .map_err(|error| describe(&error))
    }

    /// Returns the credential-free document address for the connected service.
    pub fn share_link(&self) -> Result<String, String> {
        let sharing = self.live_sharing_session()?;
        Ok(format!(
            "{}/v1/documents/{}",
            sharing.service_url.trim_end_matches('/'),
            sharing.document_uuid
        ))
    }

    fn live_sharing_session(&self) -> Result<SharingSession, String> {
        let status = self.status();
        if status.phase != "live" || status.session.is_none() {
            return Err(
                "connect to a live collaboration service before managing access".to_string(),
            );
        }
        self.sharing
            .lock()
            .map_err(poisoned)?
            .clone()
            .filter(|sharing| sharing.document_uuid == status.document_uuid)
            .ok_or_else(|| {
                "the native collaboration session has no authenticated access context".to_string()
            })
    }

    fn stop(&self) {
        if let Ok(mut slot) = self.commands.lock() {
            if let Some(sender) = slot.take() {
                let _ = sender.send(Command::Disconnect);
            }
        }
    }

    fn publish(&self, status: CollabStatus) {
        if let Ok(mut held) = self.status.lock() {
            *held = status.clone();
        }
        (self.emit)(status);
    }

    fn fail(&self, kind: &str, message: &str, resumable: bool) {
        let mut status = self.status();
        status.phase = if resumable { "reconnecting" } else { "closed" };
        status.can_submit = false;
        status.notice = Some(Notice {
            kind: kind.to_string(),
            message: message.to_string(),
            resumable,
        });
        self.publish(status);
    }

    /// The whole session: sign in, find the document, then connect and
    /// reconnect until told to stop or until reconnecting is pointless.
    async fn run(
        self: Arc<Self>,
        address: SocketAddr,
        service_url: String,
        options: ConnectOptions,
        mut commands: mpsc::UnboundedReceiver<Command>,
    ) {
        // A failure here is reported as *over*, not as reconnecting, whatever
        // kind of failure it is. Nothing retries the sign-in — the reconnect
        // loop below starts after it — so "reconnecting…" would describe a
        // thread that has already exited.
        let client =
            match ServiceClient::open_session(address, &options.subject, &options.api_key).await {
                Ok(client) => client,
                Err(error) => {
                    self.fail("connect-failed", &describe(&error), false);
                    return;
                }
            };
        let document_uuid = match options
            .document_uuid
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(uuid) => uuid.to_string(),
            None => {
                let title = options.title.as_deref().unwrap_or("Shared document");
                match client.create_document(title).await {
                    Ok(uuid) => uuid,
                    Err(error) => {
                        self.fail("create-failed", &describe(&error), false);
                        return;
                    }
                }
            }
        };

        // The token remains in `ServiceClient` behind this mutex, reachable
        // only by the narrow ACL methods above. Clearing it at the start of a
        // replacement connection prevents an old document's client from
        // becoming a capability for the new one.
        if let Ok(mut sharing) = self.sharing.lock() {
            *sharing = Some(SharingSession {
                client: client.clone(),
                document_uuid: document_uuid.clone(),
                service_url,
            });
        }

        let mut book = Book {
            document_uuid,
            display_name: options.display_name.clone(),
            ..Book::default()
        };
        let mut attempt = 0u32;
        loop {
            match client
                .connect(&book.document_uuid, &book.display_name)
                .await
            {
                Ok(session) => {
                    attempt = 0;
                    match self
                        .serve_connection(session, &mut book, &mut commands)
                        .await
                    {
                        Outcome::Stopped => return,
                        Outcome::Fatal(notice) => {
                            self.fail(&notice.kind, &notice.message, false);
                            return;
                        }
                        Outcome::Dropped(reason) => {
                            book.on_disconnect();
                            self.fail("socket-closed", &reason, true);
                        }
                        Outcome::Resync(notice) => {
                            book.on_disconnect();
                            self.fail(&notice.kind, &notice.message, true);
                        }
                    }
                }
                Err(error) => {
                    if !resumable(&error) {
                        self.fail("connect-refused", &describe(&error), false);
                        return;
                    }
                    self.fail("socket-closed", &describe(&error), true);
                }
            }
            if attempt >= MAX_ATTEMPTS {
                let unsent = book.pending;
                self.fail(
                    "reconnect-exhausted",
                    &format!(
                        "Could not reach the service after {MAX_ATTEMPTS} attempts, so this session is over.{}",
                        if unsent > 0 {
                            format!(" {unsent} local change(s) were never sent; save the document to keep them.")
                        } else {
                            String::new()
                        }
                    ),
                    false,
                );
                return;
            }
            let delay = RETRY_MS[(attempt as usize).min(RETRY_MS.len() - 1)];
            attempt += 1;
            // A disconnect while waiting must not be ignored for four seconds.
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                command = commands.recv() => match command {
                    Some(Command::Disconnect) | None => return,
                    Some(Command::Cursor(anchor)) => book.cursor = anchor,
                    Some(Command::SelectionAnchor(anchor)) => book.selection_anchor = anchor,
                    Some(Command::Selection(selection)) => book.selection = selection,
                },
            }
        }
    }

    /// One connection, from its welcome to its end.
    async fn serve_connection(
        &self,
        mut session: DocumentSession,
        book: &mut Book,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) -> Outcome {
        if let Err(notice) = self.adopt_welcome(&session, book) {
            return Outcome::Fatal(notice);
        }
        self.publish(self.live_status(&mut *book));

        let mut ticker = tokio::time::interval(PUMP);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                event = session.next_event() => match event {
                    Ok(message) => {
                        match self.apply(message, book) {
                            Applied::Continue => {}
                            Applied::Ended(notice) => {
                                return if notice.resumable {
                                    Outcome::Resync(notice)
                                } else {
                                    Outcome::Fatal(notice)
                                };
                            }
                        }
                        self.publish(self.live_status(&mut *book));
                    }
                    Err(error) => return Outcome::Dropped(describe(&error)),
                },
                command = commands.recv() => match command {
                    Some(Command::Disconnect) | None => return Outcome::Stopped,
                    Some(Command::Cursor(anchor)) => book.cursor = anchor,
                    Some(Command::SelectionAnchor(anchor)) => book.selection_anchor = anchor,
                    Some(Command::Selection(selection)) => book.selection = selection,
                },
                _ = ticker.tick() => {
                    if let Err(error) = self.sweep(&mut session, book).await {
                        return Outcome::Dropped(describe(&error));
                    }
                    self.publish(self.live_status(&mut *book));
                }
            }
        }
    }

    /// The welcome, which is also how a reconnect resynchronises. Same four
    /// steps, same order, and for the same reasons as the browser driver —
    /// see `opendoc-wasm`'s `CollabDriver::welcome`.
    fn adopt_welcome(&self, session: &DocumentSession, book: &mut Book) -> Result<(), Notice> {
        let mut app = self.app.lock().map_err(|_| Notice {
            kind: "poisoned".to_string(),
            message: "the document state is poisoned".to_string(),
            resumable: false,
        })?;
        let rejoining = book.joined;
        let held: Vec<Operation> = if rejoining {
            app.local_operations_after(0)
        } else {
            Vec::new()
        };
        let service_session = OpenDocServiceSession::new(
            session.subject(),
            session.actor().0.clone(),
            session.document_uuid(),
            role_of(session.role()),
        )
        .with_peers(session.peers().iter().map(presence_of).collect());
        app.join_collaboration_session(
            service_session,
            session.base().clone(),
            session.operations().to_vec(),
        )
        .map_err(|error| Notice {
            kind: "join-failed".to_string(),
            message: format!("this replica could not adopt the service's document: {error}"),
            resumable: false,
        })?;

        let acknowledged = highest_local_seq(&app);
        app.acknowledge_service_operations(acknowledged);
        book.acknowledged = acknowledged;
        book.submitted_through = acknowledged;
        book.commit_seq = session.commit_seq();
        book.joined = true;
        book.blocked = None;
        book.announced = false;
        book.sent_cursor = None;
        book.sent_selection_anchor = None;
        book.awaiting_ack_since = None;
        book.document_uuid = session.document_uuid().to_string();
        // The cap is the service's, and the welcome is where it arrives.
        book.submit_limit = session.max_operations_per_submit().max(1);

        let unsent: Vec<Operation> = held
            .into_iter()
            .filter(|operation| operation.id.seq > acknowledged)
            .collect();
        book.notice = if unsent.is_empty() {
            rejoining.then(|| Notice {
                kind: "resynchronised".to_string(),
                message: "Reconnected and resynchronised from the service's log.".to_string(),
                resumable: true,
            })
        } else {
            let count = unsent.len();
            match app.apply_remote_operations(unsent, book.selection.clone()) {
                Ok(intake) => Some(Notice {
                    kind: "resynchronised".to_string(),
                    message: format!(
                        "Reconnected and resynchronised from the service's log; {count} change(s) made while disconnected are being resubmitted."
                    ),
                    resumable: {
                        if intake.selection.is_some() {
                            book.selection.clone_from(&intake.selection);
                            book.rebased_selection = intake.selection;
                        }
                        true
                    },
                }),
                Err(error) => {
                    // Work this replica cannot put on the wire ends the
                    // session. The notice always said `resumable: false`, but
                    // the phase stayed "live" and only `can_submit` went
                    // false, so the pill read "Live" over a session that could
                    // never send anything again. The document is still here;
                    // the session is what is over.
                    let notice = Notice {
                        kind: "unsendable-work".to_string(),
                        message: format!(
                            "Reconnected, but {count} change(s) made while disconnected could not be replayed onto the service's log, so this session is over and those changes are only in this copy: {error}"
                        ),
                        resumable: false,
                    };
                    book.blocked = Some(notice.clone());
                    book.pending = app.local_operations_after(book.acknowledged).len();
                    return Err(notice);
                }
            }
        };
        book.pending = app.local_operations_after(book.acknowledged).len();
        Ok(())
    }

    fn apply(&self, message: ServerMessage, book: &mut Book) -> Applied {
        let Ok(mut app) = self.app.lock() else {
            return Applied::Ended(Notice {
                kind: "poisoned".to_string(),
                message: "the document state is poisoned".to_string(),
                resumable: false,
            });
        };
        match message {
            ServerMessage::Accepted {
                commit_seq,
                operation_ids,
                ..
            } => {
                if let Some(seq) = operation_ids
                    .iter()
                    .filter(|id| id.actor == app.actor_id())
                    .map(|id| id.seq)
                    .max()
                {
                    app.acknowledge_service_operations(seq);
                    book.acknowledged = book.acknowledged.max(seq);
                }
                if book.acknowledged >= book.submitted_through {
                    book.awaiting_ack_since = None;
                }
                // A batch the service took is the only evidence that this
                // replica and the service agree about what the service holds.
                book.refusal_recoveries = 0;
                book.commit_seq = book.commit_seq.max(commit_seq);
            }
            ServerMessage::Rejected {
                batch_id,
                code,
                message,
            } => {
                // Every refusal this service gives is about the batch, not
                // the moment, so submitting stops at once — resubmitting would
                // only bury the reason. But *why* the batch is wrong is almost
                // always that this replica and the service disagree about what
                // the service already holds, and a fresh welcome settles that
                // from the service's own log. So a refusal resynchronises
                // rather than wedging the session, bounded so a refusal a
                // welcome cannot fix ends it instead of looping. Same rule,
                // same numbers, as the browser driver.
                //
                // Only a refusal that arrives while this session is still
                // submitting counts: a chunked outbox can have several batches
                // in flight, and once the first is refused the rest are the
                // same failure arriving again.
                if book.blocked.is_none() {
                    book.refusal_recoveries += 1;
                }
                let exhausted = book.refusal_recoveries >= MAX_REFUSAL_RECOVERIES;
                let tail = if exhausted {
                    format!(
                        "This session is over after {MAX_REFUSAL_RECOVERIES} refusals; the document is still here, and reconnecting is how to find out whether the service will take it."
                    )
                } else {
                    format!(
                        "Resynchronising from the service's log and trying again (attempt {} of {MAX_REFUSAL_RECOVERIES}).",
                        book.refusal_recoveries
                    )
                };
                let notice = Notice {
                    kind: format!("refused-{code}"),
                    message: format!(
                        "The service refused this replica's changes ({code}): {message}. {tail} (batch {batch_id})"
                    ),
                    resumable: !exhausted,
                };
                book.blocked = Some(notice.clone());
                book.notice = Some(notice.clone());
                book.pending = app.local_operations_after(book.acknowledged).len();
                return Applied::Ended(notice);
            }
            ServerMessage::Committed {
                commit_seq,
                operations,
                ..
            } => {
                match app.apply_remote_operations(operations, book.selection.clone()) {
                    Ok(intake) => {
                        if intake.selection.is_some() {
                            book.selection.clone_from(&intake.selection);
                            book.rebased_selection = intake.selection;
                        }
                    }
                    Err(error) => {
                        // `apply_remote_operations` is all or nothing, so the
                        // whole commit is lost. This arm used to record a
                        // notice, carry on submitting, **and advance
                        // `commit_seq` for a commit it had not applied** —
                        // which made the divergence permanently invisible, on
                        // this path worse than on the browser's (P1-9).
                        //
                        // The sequence stays where it is, because this replica
                        // really is behind it, and the connection ends with a
                        // resumable notice: the welcome that follows carries
                        // the base and the whole log, so the lost commit comes
                        // back. That is the only way this client has of
                        // re-requesting one.
                        let notice = Notice {
                            kind: "commit-not-applied".to_string(),
                            message: format!(
                                "A commit from the service could not be applied, so this replica is behind it and has stopped sending: {error}. Resynchronising from the service's log."
                            ),
                            resumable: true,
                        };
                        book.blocked = Some(notice.clone());
                        book.notice = Some(notice.clone());
                        book.pending = app.local_operations_after(book.acknowledged).len();
                        return Applied::Ended(notice);
                    }
                }
                book.commit_seq = book.commit_seq.max(commit_seq);
            }
            ServerMessage::Presence { peers } => {
                app.apply_service_presence(peers.iter().map(presence_of).collect());
            }
            ServerMessage::Closed { code, message } => {
                let resumable = !matches!(code.as_str(), "forbidden" | "unauthenticated");
                return Applied::Ended(Notice {
                    kind: format!("closed-{code}"),
                    message: format!("The service closed this session ({code}): {message}"),
                    resumable,
                });
            }
            ServerMessage::Welcome { .. } | ServerMessage::Pong => {}
        }
        book.pending = app.local_operations_after(book.acknowledged).len();
        Applied::Continue
    }

    /// Puts the caret and any new batch on the wire. The refusal checks are
    /// the browser driver's, for the same reasons.
    async fn sweep(
        &self,
        session: &mut DocumentSession,
        book: &mut Book,
    ) -> Result<(), ServiceError> {
        book.sweep = book.sweep.wrapping_add(1);
        expire_unacknowledged_submit(book);
        let mut presence = None;
        if !book.announced
            || book.sent_cursor != book.cursor
            || book.sent_selection_anchor != book.selection_anchor
        {
            presence = Some((
                book.display_name.clone(),
                book.cursor.clone(),
                book.selection_anchor.clone(),
            ));
            book.sent_cursor = book.cursor.clone();
            book.sent_selection_anchor = book.selection_anchor.clone();
            book.announced = true;
        }
        let batch = {
            let Ok(app) = self.app.lock() else {
                return Ok(());
            };
            book.pending = app.local_operations_after(book.acknowledged).len();
            if book.blocked.is_some() {
                None
            } else {
                // Whichever is further along: what this connection has sent,
                // or what the service has said is durable.
                let from = book.submitted_through.max(book.acknowledged);
                let highest = highest_local_seq(&app);
                if highest < from {
                    // Fewer of this replica's own operations than the service
                    // has made durable: the next id it mints is one the log
                    // already holds. Undo used to cause this by rewinding the
                    // counter; ADR 0017 made an undo new inverse operations
                    // instead, so this is a cross-check against silent
                    // divergence rather than an expected state.
                    let notice = Notice {
                        kind: "history-rewritten".to_string(),
                        message: format!(
                            "This replica holds its own operations only up to {highest}, but the service has already made {from} durable. The service refuses to rewrite history, so nothing further will be sent; reconnect to resynchronise from its log."
                        ),
                        resumable: true,
                    };
                    book.blocked = Some(notice.clone());
                    book.notice = Some(notice);
                    None
                } else if !app.local_operations_are_dense_after(from) {
                    let notice = Notice {
                        kind: "sequence-gap".to_string(),
                        message: format!(
                            "This replica's operations after {from} are not a dense sequence, which the service refuses rather than stores. Nothing further will be sent on this session."
                        ),
                        resumable: true,
                    };
                    book.blocked = Some(notice.clone());
                    book.notice = Some(notice);
                    None
                } else if !write_allowed(&app) {
                    let role = app
                        .service_session()
                        .map(|session| session.role.as_str())
                        .unwrap_or("none");
                    book.notice = Some(Notice {
                        kind: "not-an-editor".to_string(),
                        message: format!(
                            "The service granted this subject the {role} role on this document, which may not write, so local changes are not being sent."
                        ),
                        resumable: true,
                    });
                    None
                } else {
                    let batch = app.local_operations_after(from);
                    if batch.is_empty() {
                        None
                    } else {
                        Some(batch)
                    }
                }
            }
        };

        if let Some((display_name, cursor, selection_anchor)) = presence {
            session
                .announce_presence(
                    Some(&display_name),
                    cursor.as_deref(),
                    selection_anchor.as_deref(),
                )
                .await?;
        }
        if let Some(batch) = batch {
            // Chunked to the service's own cap. One sweep can find far more
            // than the service accepts — a replayed disconnection, a large
            // paste, an import — and sending that as one message was refused,
            // which blocked the session, which nothing cleared (P1-8).
            //
            // The watermark moves *after* each submit is awaited, not before.
            // The browser path's binding had the opposite order and dropped
            // frames it believed it had sent; that asymmetry was undocumented
            // and is now gone from both.
            let limit = book.submit_limit.max(1);
            for chunk in batch.chunks(limit).take(MAX_SUBMIT_FRAMES_PER_SWEEP) {
                book.next_batch += 1;
                let highest = chunk
                    .iter()
                    .map(|operation| operation.id.seq)
                    .max()
                    .unwrap_or(book.submitted_through);
                let batch_id = format!("{}-{}", book.document_uuid, book.next_batch);
                session.submit(&batch_id, chunk.to_vec()).await?;
                book.submitted_through = highest;
            }
            if book.awaiting_ack_since.is_none() {
                book.awaiting_ack_since = Some(book.sweep);
            }
        }
        Ok(())
    }

    /// Takes the rebased caret, so it reaches the page exactly once — the
    /// same read-once rule the browser path uses, and the reason this needs
    /// `&mut Book`.
    fn live_status(&self, book: &mut Book) -> CollabStatus {
        let session = self
            .app
            .lock()
            .ok()
            .and_then(|app| app.service_session().cloned());
        let can_submit = book.blocked.is_none()
            && session
                .as_ref()
                .is_some_and(|session| session.role.allows_action("write"));
        CollabStatus {
            phase: "live",
            document_uuid: book.document_uuid.clone(),
            display_name: book.display_name.clone(),
            commit_seq: book.commit_seq,
            acknowledged_seq: book.acknowledged,
            pending_operations: book.pending,
            can_submit,
            reconnect_requested: false,
            document_changed: false,
            selection: book.rebased_selection.take(),
            notice: book.notice.clone(),
            session,
        }
    }
}

/// Per-session bookkeeping: what the service is known to hold, and what this
/// connection has said.
#[derive(Default)]
struct Book {
    document_uuid: String,
    display_name: String,
    commit_seq: u64,
    acknowledged: u64,
    submitted_through: u64,
    next_batch: u64,
    pending: usize,
    cursor: Option<String>,
    sent_cursor: Option<String>,
    selection_anchor: Option<String>,
    sent_selection_anchor: Option<String>,
    /// The page's caret as last reported, and the rebased answer waiting to be
    /// handed back to it exactly once.
    selection: Option<opendoc_app::EditorSelection>,
    rebased_selection: Option<opendoc_app::EditorSelection>,
    announced: bool,
    joined: bool,
    blocked: Option<Notice>,
    notice: Option<Notice>,
    /// The service's own submit cap, as its welcome stated it. Not restated
    /// here: the outbox chunks to whatever number arrived.
    submit_limit: usize,
    /// Sweeps since this session began; the pump is this book's clock.
    sweep: u64,
    /// The sweep at which this connection began waiting for an
    /// acknowledgement it has not had.
    awaiting_ack_since: Option<u64>,
    /// Refusals since the last accepted batch. Survives a reconnect, which is
    /// what bounds the reject-resynchronise loop.
    refusal_recoveries: u32,
}

impl Book {
    /// A new socket has been told nothing, so nothing has been submitted on it
    /// and no presence announced on it.
    fn on_disconnect(&mut self) {
        self.submitted_through = self.acknowledged;
        self.sent_cursor = None;
        self.sent_selection_anchor = None;
        self.announced = false;
        self.awaiting_ack_since = None;
    }
}

enum Outcome {
    /// The UI asked to stop.
    Stopped,
    /// The socket went away; another attempt may help.
    Dropped(String),
    /// This connection cannot go on, and a fresh welcome is the fix: the
    /// service refused a batch, or a commit could not be applied. The notice
    /// is the service's own words, kept rather than replaced by
    /// "socket-closed", because *why* is the part the user needs.
    Resync(Notice),
    /// Over, and reconnecting would not change it.
    Fatal(Notice),
}

enum Applied {
    Continue,
    Ended(Notice),
}

fn role_of(role: Role) -> OpenDocServiceRole {
    OpenDocServiceRole::parse(role.as_str()).unwrap_or(OpenDocServiceRole::Viewer)
}

fn presence_of(peer: &PeerView) -> OpenDocPresencePeer {
    OpenDocPresencePeer {
        subject: peer.subject.clone(),
        actor: peer.actor.0.clone(),
        display_name: peer.display_name.clone(),
        role: role_of(peer.role),
        cursor_anchor: peer.cursor_anchor.clone(),
        selection_anchor: peer.selection_anchor.clone(),
        last_seen_ms: peer.last_seen_ms,
        connections: peer.connections as u32,
    }
}

/// Rolls an unacknowledged submit back so the next sweep sends it again.
///
/// The browser driver's `expire_unacknowledged_submit`, for the same reason and
/// with the same safety argument: an operation id already in the service's log
/// may be resubmitted with a byte-identical payload, and the service
/// acknowledges that without committing anything, so the worst case of a
/// timeout that fired early is one redundant frame.
fn expire_unacknowledged_submit(book: &mut Book) {
    let Some(since) = book.awaiting_ack_since else {
        return;
    };
    if book.sweep.saturating_sub(since) < ACK_TIMEOUT_SWEEPS {
        return;
    }
    book.awaiting_ack_since = None;
    if book.submitted_through <= book.acknowledged {
        return;
    }
    let unacknowledged = book.submitted_through;
    book.submitted_through = book.acknowledged;
    book.notice = Some(Notice {
        kind: "acknowledgement-timed-out".to_string(),
        message: format!(
            "The service did not acknowledge changes up to {unacknowledged}; it has confirmed {} as durable. Sending them again — the service treats an exact resend of work it already holds as a retry.",
            book.acknowledged
        ),
        resumable: true,
    });
}

fn write_allowed(app: &OpenDocApp) -> bool {
    app.service_session()
        .is_some_and(|session| session.role.allows_action("write"))
}

/// Not `.last()`: the journal's order is arrival order, and after a reconnect
/// replay this actor's own operations arrive after the service's log.
fn highest_local_seq(app: &OpenDocApp) -> u64 {
    app.local_operations_after(0)
        .iter()
        .map(|operation| operation.id.seq)
        .max()
        .unwrap_or(0)
}

/// Whether another attempt could plausibly succeed. A credential the service
/// refused and a grant it revoked will be refused identically next time.
fn resumable(error: &ServiceError) -> bool {
    !matches!(
        error,
        ServiceError::Unauthenticated(_) | ServiceError::Forbidden(_) | ServiceError::NotFound(_)
    )
}

fn describe(error: &ServiceError) -> String {
    format!("{}: {}", error.code(), error.message())
}

fn poisoned<T>(_: T) -> String {
    "the collaboration state is poisoned".to_string()
}

/// `host:port`, with or without a scheme. The service speaks `http://` and
/// `ws://` only (ADR 0015: no TLS), so a `https://` address would be a
/// promise this client cannot keep and is refused rather than downgraded.
fn resolve_address(value: &str) -> Result<SocketAddr, String> {
    let service_url = canonical_service_url(value)?;
    let authority = service_url
        .strip_prefix("http://")
        .expect("canonical service URL always has its scheme");
    authority
        .to_socket_addrs()
        .map_err(|error| format!("{authority} is not a reachable address: {error}"))?
        .next()
        .ok_or_else(|| format!("{authority} resolved to no address"))
}

/// The only native service URI shape this transport can honour: a plain
/// HTTP origin. The service client itself accepts a socket address, not a URL,
/// so accepting a path/query/user-info and silently discarding it for the
/// connection would make the separately rendered share link dishonest (and
/// could leak a pasted secret). Browser clients may use a reverse-proxy path;
/// this native transport deliberately does not implement one.
fn canonical_service_url(value: &str) -> Result<String, String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("a service address is required".to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return Err(format!(
            "this service speaks http/ws only, so {rest} cannot be reached over TLS; put a terminating proxy in front of it and give this shell the proxy's plain address"
        ));
    }
    let authority = trimmed.strip_prefix("http://").unwrap_or(trimmed);
    if authority.is_empty() {
        return Err("a service address is required".to_string());
    }
    if authority.contains(['/', '@', '?', '#']) {
        return Err(
            "the native service address must be a plain HTTP origin without a path, credentials, query, or fragment"
                .to_string(),
        );
    }
    Ok(format!("http://{authority}"))
}

#[cfg(test)]
mod tests;
