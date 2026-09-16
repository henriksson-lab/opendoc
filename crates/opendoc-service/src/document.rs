//! One document, one thread, one order.
//!
//! # Why a thread and not a lock
//!
//! Commit serialization is the property that two clients cannot interleave
//! into an invalid head. A mutex around the commit *expresses* that property
//! and relies on everyone taking it; a single owning thread *is* that
//! property. `DocumentLog` is owned by this thread and reachable from nowhere
//! else, so there is no second path to the head to audit. It also fits the
//! storage layer as it exists: `ObjectStore` is synchronous by design
//! (ADR 0008), so the commit is a blocking call that has no business running
//! on an async executor's worker.
//!
//! Connections are async tasks; they send a request and await a oneshot reply.
//! The queue in front of this thread is the serialization order, and it is the
//! only one.
//!
//! # What this thread enforces
//!
//! Everything a client could otherwise lie about:
//!
//! * **Authorship.** Every operation's actor must be the actor the identity
//!   service bound to the authenticated subject. A client cannot submit as
//!   somebody else, so `OperationId` — which is what last-writer-wins and the
//!   whole causal order are computed from — is server-attested.
//! * **Permission, per submit, from storage.** Not from a role captured when
//!   the socket opened, so a revoked editor's next keystroke is refused.
//! * **Sequence density.** `VectorClock::observed` reads `seq >= n` as "every
//!   one of that actor's operations up to n". That is only true if per-actor
//!   sequences are dense, so the server requires each actor's next operation
//!   to be exactly its last plus one, and gaps are refused rather than stored.
//! * **Causal honesty.** An operation may not claim to have observed an
//!   operation the log does not contain, and may not claim a Lamport timestamp
//!   higher than one past the highest it observed. Without the second check a
//!   client could set `lamport: u64::MAX` and win every last-writer-wins
//!   contest on the document, for ever.
//! * **History immutability.** An operation id already in the log may be
//!   resubmitted only with byte-identical payload (a retry); a different
//!   payload under the same id is refused.
//!
//! # When the thread stops
//!
//! When every clone of its [`DocumentHandle`] is gone. The inbox closes, the
//! loop ends, and the thread joins. Nothing else stops it, and in particular
//! no timer does: a thread stopped while a handle still existed could be
//! replaced by a second thread for the same branch head, which is the one
//! state this design exists to make unreachable. The registry in
//! [`crate::service`] therefore reclaims a document only when
//! [`DocumentHandle::is_solely_held`] says its own clone is the last one, and
//! waits — with a deadline — for the thread to leave its loop before it
//! releases the lock. A thread that has not stopped keeps its document's uuid
//! reserved, so there is no window in which a document has a running thread
//! and no registry entry.

use crate::clock::Clock;
use crate::error::{ServiceError, ServiceResult};
use crate::log::DocumentLog;
use crate::permission::{Action, PermissionService, Role};
use crate::protocol::{
    encode_document, PeerView, ServerMessage, WireOperationId, PROTOCOL_VERSION,
};
use opendoc_core::Document;
use opendoc_merge::{ActorId, Operation};
use opendoc_store::{ObjectStore, Repository};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// The largest batch one submit may carry, by default. A bound the server
/// owns, so a client cannot make the merge arbitrarily expensive with one
/// message.
///
/// This is a *default*, not the number a client must know: the effective cap
/// travels to every client in the welcome
/// ([`ServerMessage::Welcome::max_operations_per_submit`]), so a deployment
/// that changes it changes what its clients chunk to. Restating 512 in a
/// client would be a second definition of the server's own limit, and a client
/// that guessed low would submit needlessly small batches while one that
/// guessed high would be refused for ever (P1-8).
pub const DEFAULT_MAX_OPERATIONS_PER_SUBMIT: usize = 512;

/// The longest cursor anchor a client may contribute.
///
/// An anchor is an opaque string the service relays and never resolves
/// (ADR 0015), which is exactly why it needs a length of its own: it is cloned
/// into a `Presence` frame for every connection on the document, on every
/// presence change, into outbound queues this service does not bound. Without
/// a cap a read-only Viewer can amplify one megabyte into one per peer per
/// keystroke. `normalize_display_name` has always capped at 64; this is the
/// same rule for the field beside it, sized for a `block:inline:offset`
/// triple with room to spare.
pub const MAX_CURSOR_ANCHOR_CHARS: usize = 256;

pub type ConnectionId = u64;

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_connection_id() -> ConnectionId {
    NEXT_CONNECTION_ID.fetch_add(1, Ordering::SeqCst)
}

/// What a submit did. `operations` is empty when every operation in the batch
/// was a byte-identical replay of something already durable — the batch is
/// acknowledged, because it *is* durable, but nothing was written and nothing
/// is fanned out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmitReceipt {
    pub commit_seq: u64,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentStatus {
    pub document_uuid: String,
    pub commit_seq: u64,
    pub head: Option<String>,
    pub peers: Vec<PeerView>,
}

enum DocumentRequest {
    Join {
        connection: ConnectionId,
        subject: String,
        actor: ActorId,
        display_name: String,
        outbox: mpsc::UnboundedSender<ServerMessage>,
        reply: oneshot::Sender<ServiceResult<ServerMessage>>,
    },
    Leave {
        connection: ConnectionId,
    },
    Submit {
        connection: ConnectionId,
        batch_id: String,
        operations: Vec<Operation>,
        reply: oneshot::Sender<ServiceResult<SubmitReceipt>>,
    },
    Presence {
        connection: ConnectionId,
        display_name: Option<String>,
        cursor_anchor: Option<String>,
        selection_anchor: Option<String>,
        reply: oneshot::Sender<ServiceResult<()>>,
    },
    Status {
        reply: oneshot::Sender<ServiceResult<DocumentStatus>>,
    },
    /// A grant changed. Re-read every present subject's role; disconnect the
    /// ones that no longer have read.
    GrantsChanged,
    /// Materialised document, for tests and for the read-only HTTP view.
    Snapshot {
        reply: oneshot::Sender<ServiceResult<Document>>,
    },
}

/// The async-side handle to one document's thread.
///
/// Every clone shares one inner value, and that is load-bearing rather than an
/// allocation saved: it is what lets the registry ask "is anybody still using
/// this document?" and get an answer it can act on. A thread may only be
/// stopped when the answer is no, because the whole point of one owning thread
/// is that a second one for the same branch head cannot exist.
#[derive(Clone)]
pub struct DocumentHandle {
    inner: Arc<HandleInner>,
}

struct HandleInner {
    document_uuid: String,
    requests: mpsc::UnboundedSender<DocumentRequest>,
}

impl DocumentHandle {
    pub fn document_uuid(&self) -> &str {
        &self.inner.document_uuid
    }

    /// True when this is the only clone of the handle left in the process.
    ///
    /// The registry asks this while holding its own lock, which is what turns
    /// a count that would otherwise be stale the instant it is read into one
    /// that can be acted on: the *only* way to obtain a clone of a document
    /// handle is [`crate::OpenDocService::document`], which needs that same
    /// lock, so while it is held the count can fall but never rise. A `true`
    /// answer therefore means no clone exists and none can appear before the
    /// lock is released.
    pub fn is_solely_held(&self) -> bool {
        Arc::strong_count(&self.inner) == 1
    }

    /// True when both handles address the same running thread.
    ///
    /// Not a convenience: it is how a caller — and a test — can tell "this
    /// document is still being served by the thread it was already being
    /// served by" from "a second thread was started for it", which is the
    /// distinction reclamation must never blur.
    pub fn is_same_thread(&self, other: &DocumentHandle) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub async fn join(
        &self,
        connection: ConnectionId,
        subject: &str,
        actor: &ActorId,
        display_name: &str,
        outbox: mpsc::UnboundedSender<ServerMessage>,
    ) -> ServiceResult<ServerMessage> {
        let (reply, response) = oneshot::channel();
        self.send(DocumentRequest::Join {
            connection,
            subject: subject.to_string(),
            actor: actor.clone(),
            display_name: display_name.to_string(),
            outbox,
            reply,
        })?;
        Self::await_reply(response).await?
    }

    pub fn leave(&self, connection: ConnectionId) {
        // A connection closing after the document thread is gone is normal
        // during shutdown and is not worth an error path.
        let _ = self
            .inner
            .requests
            .send(DocumentRequest::Leave { connection });
    }

    pub async fn submit(
        &self,
        connection: ConnectionId,
        batch_id: &str,
        operations: Vec<Operation>,
    ) -> ServiceResult<SubmitReceipt> {
        let (reply, response) = oneshot::channel();
        self.send(DocumentRequest::Submit {
            connection,
            batch_id: batch_id.to_string(),
            operations,
            reply,
        })?;
        Self::await_reply(response).await?
    }

    pub async fn presence(
        &self,
        connection: ConnectionId,
        display_name: Option<String>,
        cursor_anchor: Option<String>,
        selection_anchor: Option<String>,
    ) -> ServiceResult<()> {
        let (reply, response) = oneshot::channel();
        self.send(DocumentRequest::Presence {
            connection,
            display_name,
            cursor_anchor,
            selection_anchor,
            reply,
        })?;
        Self::await_reply(response).await?
    }

    pub async fn status(&self) -> ServiceResult<DocumentStatus> {
        let (reply, response) = oneshot::channel();
        self.send(DocumentRequest::Status { reply })?;
        Self::await_reply(response).await?
    }

    pub async fn snapshot(&self) -> ServiceResult<Document> {
        let (reply, response) = oneshot::channel();
        self.send(DocumentRequest::Snapshot { reply })?;
        Self::await_reply(response).await?
    }

    pub fn grants_changed(&self) {
        let _ = self.inner.requests.send(DocumentRequest::GrantsChanged);
    }

    fn send(&self, request: DocumentRequest) -> ServiceResult<()> {
        self.inner.requests.send(request).map_err(|_| {
            ServiceError::Internal(format!(
                "document {} is not running",
                self.inner.document_uuid
            ))
        })
    }

    async fn await_reply<T>(response: oneshot::Receiver<T>) -> ServiceResult<T> {
        response
            .await
            .map_err(|_| ServiceError::Internal("document thread dropped the reply".to_string()))
    }
}

#[derive(Clone, Debug)]
struct Connection {
    subject: String,
    actor: ActorId,
    display_name: String,
    cursor_anchor: Option<String>,
    selection_anchor: Option<String>,
    last_seen_ms: u64,
    outbox: mpsc::UnboundedSender<ServerMessage>,
}

struct DocumentActor<S: ObjectStore> {
    log: DocumentLog<S>,
    permissions: Arc<PermissionService<S>>,
    clock: Clock,
    connections: BTreeMap<ConnectionId, Connection>,
    /// The largest batch this document accepts in one submit, and the number
    /// every welcome hands its client so the client can chunk to it.
    max_operations_per_submit: usize,
    /// Per actor, the running maximum Lamport timestamp over its first `n`
    /// logged operations, indexed by `seq - 1`.
    ///
    /// Sequences are dense — the server refuses a gap on intake and
    /// `DocumentLog::load` refuses a stored log that has one — so the index is
    /// exact, and both questions the causal-honesty check asks ("is this
    /// operation in the log" and "what is the highest timestamp this clock
    /// could have observed") are array lookups rather than scans.
    actor_lamports: BTreeMap<ActorId, Vec<u64>>,
}

/// One document's thread, as the registry that owns it sees it.
///
/// The handle is what callers use; the join handle is what makes stopping the
/// thread an operation with an end rather than a hope. Reclaiming a document
/// means dropping the last handle and then *waiting* for the thread to leave
/// its loop, and a caller that cannot wait cannot know the thread is gone.
pub struct RunningDocument {
    pub handle: DocumentHandle,
    pub thread: std::thread::JoinHandle<()>,
}

/// Spawns the owning thread for one document and returns it.
pub fn spawn_document<S>(
    repository: Repository<S>,
    permissions: Arc<PermissionService<S>>,
    document_uuid: &str,
    clock: Clock,
    max_operations_per_submit: usize,
) -> ServiceResult<RunningDocument>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let log = DocumentLog::load(repository, document_uuid)?;
    spawn_with_log(log, permissions, clock, max_operations_per_submit)
}

/// Starts a document's owning thread.
///
/// **Fallible, and that is the point.** This used to end in
/// `.expect("spawning a document thread")`, and every caller reached it while
/// holding the service's document-registry mutex — so a process out of threads
/// or file descriptors did not merely fail to open one document, it panicked
/// inside the lock and poisoned the registry, after which *every* document
/// open on that process answered 500 for ever. An operating system that cannot
/// give this process another thread is a condition to report, not to die of.
pub fn spawn_with_log<S>(
    log: DocumentLog<S>,
    permissions: Arc<PermissionService<S>>,
    clock: Clock,
    max_operations_per_submit: usize,
) -> ServiceResult<RunningDocument>
where
    S: ObjectStore + Send + Sync + 'static,
{
    let document_uuid = log.document_uuid().to_string();
    let (requests, mut inbox) = mpsc::unbounded_channel();
    let thread_name = format!("opendoc-document-{document_uuid}");
    let thread = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let mut actor = DocumentActor::new(log, permissions, clock, max_operations_per_submit);
            // `blocking_recv` is correct here and only here: this is a plain
            // OS thread, never an executor worker.
            while let Some(request) = inbox.blocking_recv() {
                actor.handle(request);
            }
        })
        .map_err(|error| {
            ServiceError::Internal(format!(
                "could not start the owning thread for document {document_uuid}: {error}"
            ))
        })?;
    Ok(RunningDocument {
        handle: DocumentHandle {
            inner: Arc::new(HandleInner {
                document_uuid,
                requests,
            }),
        },
        thread,
    })
}

impl<S: ObjectStore> DocumentActor<S> {
    fn new(
        log: DocumentLog<S>,
        permissions: Arc<PermissionService<S>>,
        clock: Clock,
        max_operations_per_submit: usize,
    ) -> Self {
        let mut actor_lamports: BTreeMap<ActorId, Vec<u64>> = BTreeMap::new();
        for operation in log.operations() {
            record_lamport(&mut actor_lamports, operation);
        }
        Self {
            log,
            permissions,
            clock,
            connections: BTreeMap::new(),
            max_operations_per_submit: max_operations_per_submit.max(1),
            actor_lamports,
        }
    }

    fn handle(&mut self, request: DocumentRequest) {
        match request {
            DocumentRequest::Join {
                connection,
                subject,
                actor,
                display_name,
                outbox,
                reply,
            } => {
                let result = self.join(connection, subject, actor, display_name, outbox);
                let joined = result.is_ok();
                let _ = reply.send(result);
                if joined {
                    self.broadcast_presence();
                }
            }
            DocumentRequest::Leave { connection } => {
                if self.connections.remove(&connection).is_some() {
                    self.broadcast_presence();
                }
            }
            DocumentRequest::Submit {
                connection,
                batch_id,
                operations,
                reply,
            } => {
                let result = self.submit(connection, operations);
                match &result {
                    Ok(receipt) => {
                        let (subject, actor) = self
                            .connections
                            .get(&connection)
                            .map(|held| (held.subject.clone(), held.actor.clone()))
                            .unwrap_or_else(|| (String::new(), ActorId(String::new())));
                        self.send_to(
                            connection,
                            ServerMessage::Accepted {
                                batch_id: batch_id.clone(),
                                commit_seq: receipt.commit_seq,
                                operation_ids: receipt
                                    .operations
                                    .iter()
                                    .map(|operation| WireOperationId::from(&operation.id))
                                    .collect(),
                            },
                        );
                        // A pure replay wrote nothing, so there is nothing to
                        // relay. Fanning out an empty commit would advance
                        // every replica's sequence past a commit that does
                        // not exist.
                        if !receipt.operations.is_empty() {
                            self.broadcast(ServerMessage::Committed {
                                commit_seq: receipt.commit_seq,
                                subject,
                                actor,
                                operations: receipt.operations.clone(),
                            });
                        }
                    }
                    Err(error) => self.send_to(
                        connection,
                        ServerMessage::Rejected {
                            batch_id: batch_id.clone(),
                            code: error.code().to_string(),
                            message: error.message().to_string(),
                        },
                    ),
                }
                let _ = reply.send(result);
            }
            DocumentRequest::Presence {
                connection,
                display_name,
                cursor_anchor,
                selection_anchor,
                reply,
            } => {
                let result =
                    self.update_presence(connection, display_name, cursor_anchor, selection_anchor);
                // Only a real change is fanned out. A presence frame is cloned
                // once per connection into queues this service does not bound,
                // so re-announcing an unchanged name and anchor — which the
                // browser's 250 ms pump would do on every tick if it were not
                // also suppressing it — would be pure amplification.
                let changed = matches!(result, Ok(true));
                let _ = reply.send(result.map(|_| ()));
                if changed {
                    self.broadcast_presence();
                }
            }
            DocumentRequest::Status { reply } => {
                let status = DocumentStatus {
                    document_uuid: self.log.document_uuid().to_string(),
                    commit_seq: self.log.commit_seq(),
                    head: self.log.head().map(ToString::to_string),
                    peers: self.peers(),
                };
                let _ = reply.send(Ok(status));
            }
            DocumentRequest::Snapshot { reply } => {
                let _ = reply.send(Ok(self.log.document().clone()));
            }
            DocumentRequest::GrantsChanged => self.enforce_grants(),
        }
    }

    fn join(
        &mut self,
        connection: ConnectionId,
        subject: String,
        actor: ActorId,
        display_name: String,
        outbox: mpsc::UnboundedSender<ServerMessage>,
    ) -> ServiceResult<ServerMessage> {
        let role = self
            .permissions
            .authorize(self.log.document_uuid(), &subject, Action::Read)?;
        let display_name = normalize_display_name(display_name, &subject);
        self.connections.insert(
            connection,
            Connection {
                subject: subject.clone(),
                actor: actor.clone(),
                display_name,
                cursor_anchor: None,
                selection_anchor: None,
                last_seen_ms: self.clock.now_ms(),
                outbox,
            },
        );
        Ok(ServerMessage::Welcome {
            protocol_version: PROTOCOL_VERSION,
            document_uuid: self.log.document_uuid().to_string(),
            subject,
            actor,
            role,
            commit_seq: self.log.commit_seq(),
            base_document: encode_document(self.log.base())?,
            operations: self.log.operations().to_vec(),
            peers: self.peers(),
            max_operations_per_submit: self.max_operations_per_submit,
        })
    }

    fn submit(
        &mut self,
        connection: ConnectionId,
        operations: Vec<Operation>,
    ) -> ServiceResult<SubmitReceipt> {
        let held = self.connections.get(&connection).cloned().ok_or_else(|| {
            ServiceError::Unauthenticated("connection is not joined to this document".to_string())
        })?;

        // Re-read the grant every time. A role captured at connect would let a
        // revoked editor keep writing until it reconnected.
        //
        // *Which* permission depends on what the batch contains. A batch made
        // only of annotation operations needs `Action::Comment`; anything that
        // reaches the document body needs `Action::Write`. Before this,
        // `Action::Write` was required unconditionally, which made
        // `Action::Comment` dead code and left a `Commenter` unable to submit
        // so much as a comment — functionally a Viewer with a different word
        // on the pill.
        self.permissions.authorize(
            self.log.document_uuid(),
            &held.subject,
            required_action(&operations),
        )?;

        let accepted = self.validate_batch(&held, operations)?;
        if accepted.is_empty() {
            // Every operation was a byte-identical replay of something already
            // durable. The honest acknowledgement is "yes, that is stored",
            // with no commit, because there was nothing left to store.
            return Ok(SubmitReceipt {
                commit_seq: self.log.commit_seq(),
                operations: Vec::new(),
            });
        }

        let commit = self.log.append(accepted)?;
        for operation in &commit.operations {
            record_lamport(&mut self.actor_lamports, operation);
        }
        if let Some(held) = self.connections.get_mut(&connection) {
            held.last_seen_ms = self.clock.now_ms();
        }
        Ok(SubmitReceipt {
            commit_seq: commit.commit_seq,
            operations: commit.operations,
        })
    }

    /// Turns a submitted batch into the operations that will be logged, or an
    /// error that leaves the log untouched.
    ///
    /// Returns an empty vector when every operation was an exact replay.
    fn validate_batch(
        &self,
        held: &Connection,
        operations: Vec<Operation>,
    ) -> ServiceResult<Vec<Operation>> {
        if operations.is_empty() {
            return Err(ServiceError::BadRequest(
                "submit carried no operations".to_string(),
            ));
        }
        if operations.len() > self.max_operations_per_submit {
            return Err(ServiceError::BadRequest(format!(
                "submit carried {} operations, over the limit of {}",
                operations.len(),
                self.max_operations_per_submit
            )));
        }

        // The batch extends exactly one actor's sequence — every operation in
        // it must be the session's own — so one running prefix-max vector is
        // the whole of the in-batch state.
        let logged_seq = self.logged_seq(&held.actor);
        let mut pending: Vec<u64> = Vec::new();
        let mut accepted = Vec::new();
        let mut seen_in_batch = std::collections::BTreeSet::new();

        for operation in operations {
            if operation.id.actor != held.actor {
                return Err(ServiceError::Forbidden(format!(
                    "operation claims actor {} but the session is bound to actor {}",
                    operation.id.actor.0, held.actor.0
                )));
            }
            if operation.id.seq == 0 {
                return Err(ServiceError::BadRequest(
                    "operation sequence zero is reserved for 'never'".to_string(),
                ));
            }
            // Whose name the payload puts into the document. The actor check
            // above binds *who submitted*; this binds *who it says wrote the
            // words*, which is a different string and, for a comment or a
            // suggestion, the one a reader sees. `Comment::author` and
            // `Suggestion::author` are free text inside the operation and they
            // land in signed source state, so without this a subject can sign
            // in as itself and attribute a comment to somebody else —
            // permanently, with a signature over it.
            //
            // Bound, not rewritten. Rewriting the field would leave the
            // operation this server logs different from the one the client
            // holds under the same id, and neither side would ever be told;
            // refusing leaves exactly one payload for that id and the message
            // says what the author should have been.
            for author in classify(&operation).authors {
                if author != held.subject {
                    return Err(ServiceError::Forbidden(format!(
                        "operation {}#{} attributes content to {:?}, but this session is authenticated as {:?}",
                        operation.id.actor.0, operation.id.seq, author, held.subject
                    )));
                }
            }
            // The UI only exposes a comment's edit/delete controls to its
            // author, but that is a convenience, not an authorization
            // boundary.  These operations name existing durable content and
            // carry no author field of their own, so bind them here before
            // they can enter the signed operation log.  Resolution remains a
            // normal review action; this deliberately covers only rewriting,
            // deleting, or restoring somebody else's words (and rewriting an
            // existing proposal).
            ensure_authored_annotation_mutation(self.log.document(), &held.subject, &operation)?;
            if !seen_in_batch.insert(operation.id.clone()) {
                return Err(ServiceError::BadRequest(format!(
                    "operation {}#{} appears twice in one submit",
                    operation.id.actor.0, operation.id.seq
                )));
            }

            // An exact replay of something already durable is a retry, not a
            // rewrite: drop it and carry on. A different payload under a
            // logged id is an attempt to rewrite history.
            if let Some(logged) = self.log.logged(&operation) {
                if logged == &operation {
                    continue;
                }
                return Err(ServiceError::Conflict(format!(
                    "operation {}#{} is already logged with a different payload",
                    operation.id.actor.0, operation.id.seq
                )));
            }

            let next_seq = logged_seq + pending.len() as u64 + 1;
            if operation.id.seq != next_seq {
                return Err(ServiceError::Conflict(format!(
                    "operation {}#{} is out of sequence; the next sequence for this actor is {next_seq}",
                    operation.id.actor.0, operation.id.seq
                )));
            }

            if let Some(context) = &operation.context {
                let mut highest_observed_lamport = 0u64;
                for (actor, seq) in &context.observed.0 {
                    if *seq == 0 {
                        continue;
                    }
                    // The clock must name only operations the log actually
                    // has. Anything else is a claim about a causal past the
                    // server cannot see, which would reorder honest work.
                    let Some(lamport) =
                        self.observed_prefix_max(&held.actor, &pending, actor, *seq)
                    else {
                        return Err(ServiceError::BadRequest(format!(
                            "operation {}#{} claims to have observed {}#{}, which is not in the log",
                            operation.id.actor.0, operation.id.seq, actor.0, seq
                        )));
                    };
                    highest_observed_lamport = highest_observed_lamport.max(lamport);
                }
                if context.lamport > highest_observed_lamport.saturating_add(1) {
                    return Err(ServiceError::BadRequest(format!(
                        "operation {}#{} claims Lamport timestamp {} but observed nothing above {}",
                        operation.id.actor.0,
                        operation.id.seq,
                        context.lamport,
                        highest_observed_lamport
                    )));
                }
            }

            let running = pending
                .last()
                .copied()
                .or_else(|| {
                    self.actor_lamports
                        .get(&held.actor)
                        .and_then(|held| held.last().copied())
                })
                .unwrap_or(0)
                .max(operation.lamport());
            pending.push(running);
            accepted.push(operation);
        }
        Ok(accepted)
    }

    /// How many operations this actor already has durably logged.
    fn logged_seq(&self, actor: &ActorId) -> u64 {
        self.actor_lamports
            .get(actor)
            .map(|lamports| lamports.len() as u64)
            .unwrap_or(0)
    }

    /// The highest Lamport timestamp among an actor's first `seq` operations,
    /// or `None` when it does not have that many — counting the operations
    /// earlier in the batch being validated.
    fn observed_prefix_max(
        &self,
        batch_actor: &ActorId,
        pending: &[u64],
        actor: &ActorId,
        seq: u64,
    ) -> Option<u64> {
        let logged = self
            .actor_lamports
            .get(actor)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let in_batch: &[u64] = if actor == batch_actor { pending } else { &[] };
        let index = usize::try_from(seq).ok()?.checked_sub(1)?;
        match logged.get(index) {
            Some(lamport) => Some(*lamport),
            None => in_batch.get(index - logged.len()).copied(),
        }
    }

    /// Records this connection's presence, answering whether anything about it
    /// actually changed.
    fn update_presence(
        &mut self,
        connection: ConnectionId,
        display_name: Option<String>,
        cursor_anchor: Option<String>,
        selection_anchor: Option<String>,
    ) -> ServiceResult<bool> {
        let subject = self
            .connections
            .get(&connection)
            .map(|held| held.subject.clone())
            .ok_or_else(|| {
                ServiceError::Unauthenticated(
                    "connection is not joined to this document".to_string(),
                )
            })?;
        self.permissions
            .authorize(self.log.document_uuid(), &subject, Action::Present)?;
        let now = self.clock.now_ms();
        let Some(held) = self.connections.get_mut(&connection) else {
            return Err(ServiceError::Internal(
                "connection vanished during presence update".to_string(),
            ));
        };
        let mut changed = false;
        if let Some(display_name) = display_name {
            let display_name = normalize_display_name(display_name, &held.subject);
            changed |= held.display_name != display_name;
            held.display_name = display_name;
        }
        let cursor_anchor = normalize_cursor_anchor(cursor_anchor);
        changed |= held.cursor_anchor != cursor_anchor;
        held.cursor_anchor = cursor_anchor;
        let selection_anchor = normalize_cursor_anchor(selection_anchor);
        changed |= held.selection_anchor != selection_anchor;
        held.selection_anchor = selection_anchor;
        held.last_seen_ms = now;
        Ok(changed)
    }

    /// Re-reads every present subject's grant and closes the connections that
    /// have lost read access.
    ///
    /// Revocation that only takes effect on reconnect is not revocation: a
    /// socket already open would keep receiving every commit.
    fn enforce_grants(&mut self) {
        let document_uuid = self.log.document_uuid().to_string();
        let doomed: Vec<ConnectionId> = self
            .connections
            .iter()
            .filter(|(_, held)| {
                self.permissions
                    .authorize(&document_uuid, &held.subject, Action::Read)
                    .is_err()
            })
            .map(|(connection, _)| *connection)
            .collect();
        for connection in &doomed {
            self.send_to(
                *connection,
                ServerMessage::Closed {
                    code: "forbidden".to_string(),
                    message: "read access to this document was revoked".to_string(),
                },
            );
            self.connections.remove(connection);
        }
        self.broadcast_presence();
    }

    /// One entry per subject, not per connection: two tabs are one person.
    fn peers(&self) -> Vec<PeerView> {
        let document_uuid = self.log.document_uuid().to_string();
        let mut by_subject: BTreeMap<String, PeerView> = BTreeMap::new();
        for held in self.connections.values() {
            let role = match self.permissions.role_for(&document_uuid, &held.subject) {
                Ok(Some(role)) => role,
                // A subject with no readable grant should already have been
                // disconnected; showing it as a viewer is the conservative
                // reading and never widens anything.
                _ => Role::Viewer,
            };
            let entry = by_subject
                .entry(held.subject.clone())
                .or_insert_with(|| PeerView {
                    subject: held.subject.clone(),
                    actor: held.actor.clone(),
                    display_name: held.display_name.clone(),
                    role,
                    cursor_anchor: held.cursor_anchor.clone(),
                    selection_anchor: held.selection_anchor.clone(),
                    last_seen_ms: held.last_seen_ms,
                    connections: 0,
                });
            entry.connections += 1;
            if held.last_seen_ms >= entry.last_seen_ms {
                entry.last_seen_ms = held.last_seen_ms;
                entry.display_name = held.display_name.clone();
                entry.cursor_anchor = held.cursor_anchor.clone();
                entry.selection_anchor = held.selection_anchor.clone();
            }
        }
        by_subject.into_values().collect()
    }

    fn broadcast_presence(&mut self) {
        let peers = self.peers();
        self.broadcast(ServerMessage::Presence { peers });
    }

    fn broadcast(&mut self, message: ServerMessage) {
        let mut closed = Vec::new();
        for (connection, held) in &self.connections {
            if held.outbox.send(message.clone()).is_err() {
                closed.push(*connection);
            }
        }
        for connection in closed {
            self.connections.remove(&connection);
        }
    }

    fn send_to(&mut self, connection: ConnectionId, message: ServerMessage) {
        let dead = match self.connections.get(&connection) {
            Some(held) => held.outbox.send(message).is_err(),
            None => false,
        };
        if dead {
            self.connections.remove(&connection);
        }
    }
}

/// Refuse a mutation of an existing authored review item by a different
/// authenticated subject.
///
/// Missing targets intentionally remain the merge layer's concern: a target
/// may be created earlier in this same submitted batch, and a stale target
/// must retain the established warning/convergence behavior rather than turn
/// into an authorization oracle.
fn ensure_authored_annotation_mutation(
    document: &opendoc_core::Document,
    subject: &str,
    operation: &Operation,
) -> ServiceResult<()> {
    use opendoc_merge::OperationKind as Kind;

    let author = match &operation.kind {
        Kind::UpdateSuggestionInsertContent { suggestion_id, .. } => document
            .suggestions
            .iter()
            .find(|suggestion| suggestion.id == *suggestion_id)
            .map(|suggestion| {
                (
                    "suggestion",
                    suggestion_id.as_str(),
                    suggestion.author.as_str(),
                )
            }),
        Kind::UpdateCommentBody {
            thread_id,
            comment_id,
            ..
        }
        | Kind::DeleteComment {
            thread_id,
            comment_id,
        }
        | Kind::RestoreComment {
            thread_id,
            comment_id,
        } => document
            .comments
            .iter()
            .find(|thread| thread.id == *thread_id)
            .and_then(|thread| {
                thread
                    .comments
                    .iter()
                    .find(|comment| comment.id == *comment_id)
            })
            .map(|comment| ("comment", comment_id.as_str(), comment.author.as_str())),
        _ => None,
    };

    if let Some((kind, id, author)) = author {
        if author != subject {
            return Err(ServiceError::Forbidden(format!(
                "{kind} {id} was authored by {author:?}, but this session is authenticated as {subject:?}"
            )));
        }
    }
    Ok(())
}

/// What one operation is, for the two questions this service asks of a batch:
/// which permission it needs, and whose name it may put into signed state.
///
/// One classifier rather than two, so the exhaustive match below is written
/// once and a new [`OperationKind`] has to answer both questions.
struct OperationClass<'a> {
    /// True when the operation touches only `document.comments` or
    /// `document.suggestions` and never the block tree.
    annotation: bool,
    /// Every author field this payload carries, borrowed from it.
    ///
    /// `Comment::author` and `Suggestion::author` are free text a client
    /// chooses, and they land inside signed source state — so the service
    /// checks them against the authenticated subject rather than relaying
    /// whatever arrived.
    authors: Vec<&'a str>,
}

/// Classifies one operation; see [`OperationClass`].
///
/// [`OperationClass::annotation`] is true only for an operation that touches
/// `document.comments` or `document.suggestions` and never the block tree.
/// `AcceptSuggestion` and `RejectSuggestion` are deliberately *not*
/// annotations: accepting a suggestion is how its content enters the document.
///
/// The match is exhaustive on purpose. A new [`OperationKind`] must be
/// classified by whoever adds it, and until it is this file does not compile —
/// which is the opposite of the failure mode a `_ =>` arm would give, where a
/// new body-editing operation would silently become something a Commenter may
/// submit, and a new authored payload would silently become one whose author
/// nothing checks.
///
/// ADR 0015 says this classification belongs next to the operation vocabulary
/// in `opendoc-merge` rather than here, and that is still true — the list
/// below is a second reader of that enum. It lives here because the exhaustive
/// match makes the drift a build failure rather than a permission hole, and
/// moving it is an `opendoc-merge` change this workstream does not own.
fn classify(operation: &Operation) -> OperationClass<'_> {
    use opendoc_merge::OperationKind as Kind;
    let (annotation, authors): (bool, Vec<&str>) = match &operation.kind {
        Kind::AddCommentThread { thread } => (
            true,
            thread
                .comments
                .iter()
                .map(|comment| comment.author.as_str())
                .collect(),
        ),
        Kind::AddCommentReply { comment, .. } => (true, vec![comment.author.as_str()]),
        Kind::AddSuggestion { suggestion } => (true, vec![suggestion.author.as_str()]),
        Kind::DeleteCommentThread { .. }
        | Kind::RestoreCommentThread { .. }
        | Kind::DeleteComment { .. }
        | Kind::RestoreComment { .. }
        | Kind::UpdateCommentBody { .. }
        | Kind::UpdateSuggestionInsertContent { .. }
        | Kind::ResolveCommentThread { .. }
        | Kind::ReopenCommentThread { .. }
        | Kind::SetCommentThreadAction { .. } => (true, Vec::new()),
        Kind::SetCommentThreadReaction { actor, .. } => (true, vec![actor.as_str()]),
        Kind::SetDocumentTitle { .. }
        | Kind::SetDocumentDoi { .. }
        | Kind::SetDocumentLocale { .. }
        | Kind::UpsertBookmark { .. }
        | Kind::InsertBlock { .. }
        | Kind::DeleteBlock { .. }
        | Kind::MoveBlock { .. }
        | Kind::SetBlockTextStyle { .. }
        | Kind::InsertInline { .. }
        | Kind::MoveInlineToBlock { .. }
        | Kind::AddMark { .. }
        | Kind::RemoveMark { .. }
        | Kind::AddMarkRange { .. }
        | Kind::UpsertFootnote { .. }
        | Kind::SetEndnotePlacement { .. }
        | Kind::UpsertBibliographyReference { .. }
        | Kind::DeleteBibliographyReference { .. }
        | Kind::UpsertCitationGroup { .. }
        | Kind::DeleteCitationGroup { .. }
        | Kind::UpdateCitationStyle { .. }
        | Kind::UpdateInlineText { .. }
        | Kind::InsertText { .. }
        | Kind::DeleteText { .. }
        | Kind::UpdateInlineEquationSource { .. }
        | Kind::UpdateMentionLabel { .. }
        | Kind::SelectDropdownOption { .. }
        | Kind::UpdateDateChip { .. }
        | Kind::UpdateLinkHref { .. }
        | Kind::UpdateBlockEquationSource { .. }
        | Kind::UpdateImageAltText { .. }
        | Kind::UpdateImageBlobHash { .. }
        | Kind::UpdateImageLayout { .. }
        | Kind::UpdateHeadingLevel { .. }
        | Kind::UpdateListItem { .. }
        | Kind::SetListStart { .. }
        | Kind::SetListFormat { .. }
        | Kind::SetListBulletMarker { .. }
        | Kind::SetBlockProperty { .. }
        | Kind::ClearBlockProperty { .. }
        | Kind::SetPageSetup { .. }
        | Kind::SetPageFurniture { .. }
        | Kind::ClearPageFurnitureOverride { .. }
        | Kind::AcceptSuggestion { .. }
        | Kind::RejectSuggestion { .. }
        | Kind::DeleteInline { .. }
        | Kind::InsertTableRow { .. }
        | Kind::DeleteTableRow { .. }
        | Kind::InsertTableCell { .. }
        | Kind::DeleteTableCell { .. }
        | Kind::InsertTableColumn { .. }
        | Kind::DeleteTableColumn { .. }
        | Kind::SetTableColumnWidth { .. }
        | Kind::SetTableRowHeight { .. }
        | Kind::SetTableRowHeader { .. }
        | Kind::ReorderTableRows { .. }
        | Kind::SetTableBorder { .. }
        | Kind::SetTableAlignment { .. }
        | Kind::SetTableCellSpan { .. }
        | Kind::SetTableCellProperty { .. }
        | Kind::ClearTableCellProperty { .. } => (false, Vec::new()),
    };
    OperationClass {
        annotation,
        authors,
    }
}

/// The least permission a batch needs.
///
/// [`Action::Comment`] only when *every* operation in the batch is an
/// annotation. Anything else, including a batch that mixes the two, needs
/// [`Action::Write`].
fn required_action(operations: &[Operation]) -> Action {
    if operations.is_empty() {
        // Refused as malformed a moment later; asking for the stronger
        // permission keeps an empty submit reported the way it always was.
        return Action::Write;
    }
    if operations
        .iter()
        .all(|operation| classify(operation).annotation)
    {
        Action::Comment
    } else {
        Action::Write
    }
}

/// Appends one operation's running prefix maximum.
///
/// The `else` branch is unreachable through this service's own writes, because
/// intake refuses a gap and `DocumentLog::load` refuses a stored log with one.
/// It exists so a surprising log cannot silently misalign the index.
fn record_lamport(actor_lamports: &mut BTreeMap<ActorId, Vec<u64>>, operation: &Operation) {
    let entry = actor_lamports
        .entry(operation.id.actor.clone())
        .or_default();
    let running = entry.last().copied().unwrap_or(0).max(operation.lamport());
    let index = (operation.id.seq as usize).saturating_sub(1);
    if index == entry.len() {
        entry.push(running);
    } else {
        if entry.len() < index {
            entry.resize(index, running);
        }
        entry.truncate(index);
        entry.push(running);
    }
}

/// Trims an anchor, drops an empty one, and cuts it to
/// [`MAX_CURSOR_ANCHOR_CHARS`].
///
/// Cut by `char`, not by byte, so the result is still a string: truncating
/// UTF-8 at a byte offset would either panic or produce something no client
/// could read back.
fn normalize_cursor_anchor(cursor_anchor: Option<String>) -> Option<String> {
    cursor_anchor
        .map(|value| value.trim().chars().take(MAX_CURSOR_ANCHOR_CHARS).collect())
        .filter(|value: &String| !value.is_empty())
}

fn normalize_display_name(display_name: String, subject: &str) -> String {
    let trimmed = display_name.trim();
    if trimmed.is_empty() {
        subject.to_string()
    } else {
        trimmed.chars().take(64).collect()
    }
}
