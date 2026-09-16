//! The service object: identity, grants and the set of running documents.
//!
//! Transport-free on purpose. Everything the HTTP/WebSocket layer does, it
//! does by calling this, which is what makes the service testable without a
//! socket and what keeps the socket layer from growing policy of its own.

use crate::clock::Clock;
use crate::document::{
    spawn_document, spawn_with_log, DocumentHandle, RunningDocument,
    DEFAULT_MAX_OPERATIONS_PER_SUBMIT,
};
use crate::error::{ServiceError, ServiceResult};
use crate::identity::IdentityService;
use crate::log::DocumentLog;
use crate::origin::OriginPolicy;
use crate::permission::{Action, PermissionService, Role};
use crate::store::SharedStore;
use opendoc_core::{Block, Document};
use opendoc_format::{decode_cbor, encode_canonical_cbor};
use opendoc_store::{ObjectStore, Repository};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// How many documents one process will own threads for at once.
///
/// Every open document is one OS thread, so without a bound the number of
/// threads and file descriptors this process holds is whatever its callers ask
/// for. The bound turns exhaustion — which used to arrive as a panic *inside*
/// the registry mutex, poisoning it for the life of the process — into a
/// refusal the caller can read.
///
/// It is a bound on *concurrently open* documents, not on the documents this
/// process will ever serve: an idle document's thread is reclaimed (see
/// [`DEFAULT_DOCUMENT_IDLE_TIMEOUT_MS`]), so reaching the limit is a refusal
/// that lifts on its own rather than one that lasts until a restart.
pub const DEFAULT_MAX_OPEN_DOCUMENTS: usize = 1024;

/// How long a document nobody holds a handle to keeps its thread.
///
/// Reclamation is *not* on a timer alone, and this is only half the condition.
/// A document is reclaimed when no clone of its handle exists anywhere in the
/// process **and** nothing has asked for it in this long. The first half is
/// the safety property — a thread stopped while a handle lived could be
/// replaced by a second thread for the same branch head — and the second half
/// is the only thing this constant decides: how long a document that nothing
/// is using stays warm, so that a client reconnecting to the document it just
/// closed does not pay for a reload of the whole log.
///
/// Fifteen minutes because the cost it trades against is one
/// `DocumentLog::load` — every manifest and segment in the document's history
/// — and the thing it holds is one idle OS thread plus that document's log in
/// memory.
pub const DEFAULT_DOCUMENT_IDLE_TIMEOUT_MS: u64 = 15 * 60 * 1000;

/// How many documents one subject may create.
///
/// `POST /v1/documents` is authenticated and, until this existed, authorized
/// by nothing: any subject the operator provisioned could create documents
/// without limit, and each one took a thread for the life of the process. This
/// is the authorization that endpoint was missing, expressed as the only thing
/// this service knows about a subject that could bound it.
pub const DEFAULT_MAX_DOCUMENTS_PER_SUBJECT: usize = 256;

/// What one subject has created, durable beside the grant table.
///
/// Durable rather than in-process because a quota a restart forgets is not a
/// quota. It lives in the service's own namespace, like `service/permissions/`
/// — deployment state, never document source, never a manifest (ADR 0004).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct SubjectQuotaRecord {
    subject: String,
    created_documents: usize,
}

/// How long reclamation waits for a document thread to leave its loop.
///
/// It is a wait, not a join, and that is the point. Dropping the last handle
/// closes the thread's inbox, so in the ordinary case the thread is gone
/// within microseconds and this is never approached. A plain `JoinHandle::join`
/// would be simpler and would be *wrong in the one case that matters*: if the
/// registry were ever mistaken about a handle being the last one, the join
/// would block for ever holding the registry lock, and a service that has
/// silently stopped answering is the least debuggable failure there is. With a
/// deadline, the same mistake becomes a document this service refuses to
/// reopen, with a message that says exactly what happened.
const THREAD_EXIT_TIMEOUT_MS: u64 = 5_000;

/// One running document, as its registry owns it.
///
/// The registry's clone of the handle is the one that is never handed out, so
/// [`DocumentHandle::is_solely_held`] on it answers exactly the question
/// reclamation turns on: does anything else in this process still hold this
/// document?
struct OpenDocument {
    handle: DocumentHandle,
    thread: std::thread::JoinHandle<()>,
    /// When the registry last handed this document's handle to anybody.
    ///
    /// Not "when the document was last written to": the registry cannot see
    /// inside the thread, and does not need to. A document being written to
    /// has a live connection holding a handle, which
    /// [`DocumentHandle::is_solely_held`] already reports.
    last_used_ms: u64,
}

impl OpenDocument {
    /// Whether this document's thread may be stopped.
    ///
    /// Both halves are required. Without the first, a timer could stop a
    /// thread while a connection still held its handle, and the next open
    /// would start a **second** thread for the same branch head — two writers,
    /// which is the one thing the one-thread-per-document design exists to
    /// make impossible. Without the second, closing a tab would throw away the
    /// document's whole in-memory log and the next open would reload it.
    fn is_reclaimable(&self, now_ms: u64, idle_timeout_ms: u64) -> bool {
        self.handle.is_solely_held() && now_ms.saturating_sub(self.last_used_ms) >= idle_timeout_ms
    }

    /// Drops the last handle and hands the thread back, on its way out.
    ///
    /// Dropping the handle closes the thread's inbox; the thread leaves its
    /// loop as soon as it has drained whatever was already queued. Waiting for
    /// that is the caller's job, because it is worth doing for a whole batch
    /// at once rather than one document at a time.
    fn start_stopping(self) -> std::thread::JoinHandle<()> {
        let OpenDocument { handle, thread, .. } = self;
        drop(handle);
        thread
    }
}

/// Which documents this process owns threads for.
///
/// Two maps rather than one because a document has a third state between
/// "running" and "gone": its handle has been dropped and its thread has not
/// been *seen* to exit. A uuid in `retiring` is not open, but it is also not
/// free — starting a thread for it would be the second one — so both maps are
/// counted against the open-document limit and both are consulted before any
/// spawn.
#[derive(Default)]
struct Registry {
    open: BTreeMap<String, OpenDocument>,
    retiring: BTreeMap<String, std::thread::JoinHandle<()>>,
}

impl Registry {
    /// How many document threads this process is holding, running or not yet
    /// seen to have stopped.
    fn len(&self) -> usize {
        self.open.len() + self.retiring.len()
    }

    /// Forgets the retiring documents whose threads have since exited.
    ///
    /// In the ordinary case `retiring` is empty and this is two comparisons.
    fn reap_retired(&mut self) {
        let finished: Vec<String> = self
            .retiring
            .iter()
            .filter(|(_, thread)| thread.is_finished())
            .map(|(document_uuid, _)| document_uuid.clone())
            .collect();
        for document_uuid in finished {
            if let Some(thread) = self.retiring.remove(&document_uuid) {
                // Already out of its loop, so this cannot block.
                let _ = thread.join();
            }
        }
    }

    /// Stops these documents, and reserves the uuid of any whose thread will
    /// not stop. Returns how many threads are gone.
    ///
    /// A batch rather than one document at a time, because the wait is shared:
    /// every handle is dropped first, so the threads stop concurrently and one
    /// deadline covers all of them. Done one at a time, a sweep over a hundred
    /// idle documents would hold the registry lock for a hundred separate
    /// waits.
    fn retire_all(&mut self, document_uuids: &[String], deadline_ms: u64) -> usize {
        let mut stopping: Vec<(String, std::thread::JoinHandle<()>)> = Vec::new();
        for document_uuid in document_uuids {
            if let Some(open) = self.open.remove(document_uuid) {
                stopping.push((document_uuid.clone(), open.start_stopping()));
            }
        }
        let started = std::time::Instant::now();
        let mut stopped = 0;
        while !stopping.is_empty() {
            let mut still_running = Vec::with_capacity(stopping.len());
            for (document_uuid, thread) in stopping {
                if thread.is_finished() {
                    // Reaps it. It has left its loop, so this cannot block.
                    let _ = thread.join();
                    stopped += 1;
                } else {
                    still_running.push((document_uuid, thread));
                }
            }
            stopping = still_running;
            if stopping.is_empty() || started.elapsed().as_millis() as u64 >= deadline_ms {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Whatever is left is still running with nothing able to reach it. Its
        // uuid stays reserved: the alternative is forgetting a live thread and
        // starting a second one for its branch head on the next open.
        for (document_uuid, thread) in stopping {
            self.retiring.insert(document_uuid, thread);
        }
        stopped
    }
}

pub struct OpenDocService<S: ObjectStore> {
    store: SharedStore<S>,
    identity: Arc<IdentityService>,
    permissions: Arc<PermissionService<SharedStore<S>>>,
    documents: Mutex<Registry>,
    /// Serializes the read-modify-write of a subject's quota record. The
    /// registry lock cannot serve: it is taken after the quota is charged.
    quota_writes: Mutex<()>,
    /// When the last reclamation pass ran, so an idle process sweeps once per
    /// idle window rather than once per request.
    last_sweep_ms: AtomicU64,
    clock: Clock,
    origins: OriginPolicy,
    max_operations_per_submit: usize,
    max_open_documents: usize,
    max_documents_per_subject: usize,
    document_idle_timeout_ms: u64,
}

impl<S: ObjectStore + Send + Sync + 'static> OpenDocService<S> {
    pub fn new(store: S, clock: Clock) -> Self {
        let store = SharedStore::new(store);
        Self {
            identity: Arc::new(IdentityService::new(clock.clone())),
            permissions: Arc::new(PermissionService::new(store.clone())),
            store,
            documents: Mutex::new(Registry::default()),
            quota_writes: Mutex::new(()),
            last_sweep_ms: AtomicU64::new(clock.now_ms()),
            clock,
            // Deny by default: a service nobody configured is reachable by
            // programs and by no page at all. See `crate::origin`.
            origins: OriginPolicy::deny_all(),
            max_operations_per_submit: DEFAULT_MAX_OPERATIONS_PER_SUBMIT,
            max_open_documents: DEFAULT_MAX_OPEN_DOCUMENTS,
            max_documents_per_subject: DEFAULT_MAX_DOCUMENTS_PER_SUBJECT,
            document_idle_timeout_ms: DEFAULT_DOCUMENT_IDLE_TIMEOUT_MS,
        }
    }

    /// The largest batch this deployment accepts in one submit.
    ///
    /// Every welcome carries it, so lowering it lowers what clients chunk to
    /// rather than turning their ordinary batches into refusals.
    pub fn with_max_operations_per_submit(mut self, limit: usize) -> Self {
        self.max_operations_per_submit = limit.max(1);
        self
    }

    pub fn with_max_open_documents(mut self, limit: usize) -> Self {
        self.max_open_documents = limit.max(1);
        self
    }

    pub fn with_max_documents_per_subject(mut self, limit: usize) -> Self {
        self.max_documents_per_subject = limit;
        self
    }

    /// How long a document nothing holds keeps its thread. Zero reclaims it at
    /// the next pass, which is what a test that wants no warm window sets.
    pub fn with_document_idle_timeout_ms(mut self, idle_timeout_ms: u64) -> Self {
        self.document_idle_timeout_ms = idle_timeout_ms;
        self
    }

    pub fn document_idle_timeout_ms(&self) -> u64 {
        self.document_idle_timeout_ms
    }

    pub fn max_operations_per_submit(&self) -> usize {
        self.max_operations_per_submit
    }

    /// Which browser origins may reach this service.
    ///
    /// Deployment state, like the subject directory and the grant table — not
    /// document state, and never something a request can influence. It lives
    /// here rather than in the router because the router is rebuilt per
    /// `serve` call and this outlives it.
    pub fn with_allowed_origins(mut self, origins: OriginPolicy) -> Self {
        self.origins = origins;
        self
    }

    pub fn origins(&self) -> &OriginPolicy {
        &self.origins
    }

    pub fn identity(&self) -> &Arc<IdentityService> {
        &self.identity
    }

    pub fn permissions(&self) -> &Arc<PermissionService<SharedStore<S>>> {
        &self.permissions
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// Creates a document and makes the caller its owner.
    ///
    /// The grant is seeded *after* the genesis commit, so a failure to create
    /// the document cannot leave an ownerless grant behind. The other order
    /// would be worse in the way that matters: a grant naming a document that
    /// does not exist is invisible until someone creates that uuid.
    pub fn create_document(&self, subject: &str, title: &str) -> ServiceResult<String> {
        // Refused before anything is written, in both directions: a process
        // already at its open-document limit has no thread to give this one,
        // and a genesis commit written for a document that is then refused
        // would leave an owned, durable document its creator was told did not
        // happen. The check is repeated at the insert below, which is the one
        // that is actually race-free; this one only keeps the common refusal
        // from having side effects.
        {
            let mut documents = self.lock_documents()?;
            self.reclaim_before_admitting(&mut documents);
            self.refuse_when_full(&documents, None)?;
        }
        // Charged before anything is written, for the same reason, and
        // durably, so a restart does not hand everybody a fresh allowance.
        self.charge_document_quota(subject)?;
        let mut document = Document::new(title.trim());
        // One paragraph, for the same reason `OpenDocApp::empty_titled` adds
        // one: an editor cannot resolve a caret against a document with no
        // blocks, so a document created here and joined immediately would be
        // one nobody could type into. `Block::paragraph` is the model's own
        // constructor, so this is the canonical empty document rather than a
        // shape this crate invented.
        document.blocks.push(Block::paragraph(""));
        let document_uuid = document.uuid.as_str().to_string();
        let log = DocumentLog::create(self.repository(), document)?;
        self.permissions.seed_owner(&document_uuid, subject)?;
        // Spawn first, register second, and never panic under the registry
        // lock: a thread this process cannot start is a refusal, not a reason
        // to poison the map every other document open goes through.
        let running = spawn_with_log(
            log,
            Arc::clone(&self.permissions),
            self.clock.clone(),
            self.max_operations_per_submit,
        )?;
        let mut documents = self.lock_documents()?;
        self.reclaim_before_admitting(&mut documents);
        self.refuse_when_full(&documents, Some(&document_uuid))?;
        self.register(&mut documents, document_uuid.clone(), running);
        Ok(document_uuid)
    }

    /// Charges one document against `subject`'s quota, or refuses.
    fn charge_document_quota(&self, subject: &str) -> ServiceResult<()> {
        let subject = subject.trim();
        if subject.is_empty() {
            return Err(ServiceError::BadRequest("subject is empty".to_string()));
        }
        // Read-modify-write under one lock, so two concurrent creates cannot
        // both read the same count and both write count + 1.
        let _guard = self
            .quota_writes
            .lock()
            .map_err(|_| ServiceError::Internal("document quota lock poisoned".to_string()))?;
        let path = quota_path(subject);
        let mut record: SubjectQuotaRecord = match self.store.get_named(&path)? {
            Some(bytes) => {
                decode_cbor(&bytes).map_err(|error| ServiceError::Storage(error.to_string()))?
            }
            None => SubjectQuotaRecord {
                subject: subject.to_string(),
                created_documents: 0,
            },
        };
        if record.created_documents >= self.max_documents_per_subject {
            return Err(ServiceError::Forbidden(format!(
                "subject {subject} has created {} documents, which is this service's limit",
                self.max_documents_per_subject
            )));
        }
        record.created_documents += 1;
        let bytes = encode_canonical_cbor(&record)
            .map_err(|error| ServiceError::Storage(error.to_string()))?;
        self.store.put_named(&path, &bytes)?;
        Ok(())
    }

    /// How many documents a subject has created. For tests and operators.
    pub fn documents_created_by(&self, subject: &str) -> ServiceResult<usize> {
        Ok(match self.store.get_named(&quota_path(subject.trim()))? {
            Some(bytes) => {
                decode_cbor::<SubjectQuotaRecord>(&bytes)
                    .map_err(|error| ServiceError::Storage(error.to_string()))?
                    .created_documents
            }
            None => 0,
        })
    }

    /// Refuses when the registry is full and this document is not already in
    /// it. `None` asks the same question for a document that does not exist
    /// yet, which is what `create_document` needs before it writes anything.
    ///
    /// One implementation of the rule, deliberately: a second copy in
    /// `create_document` would be a second thing to keep true.
    ///
    /// The refusal says *why* nothing could be reclaimed. Every caller runs a
    /// reclamation pass immediately before this, so each remaining entry
    /// failed one of the two halves of [`OpenDocument::is_reclaimable`], and
    /// which half it failed is the difference between "wait for someone to
    /// disconnect" and "wait out the idle window" — two different things for
    /// an operator to do, and neither of them "restart the process", which is
    /// what this used to say.
    fn refuse_when_full(
        &self,
        documents: &Registry,
        document_uuid: Option<&str>,
    ) -> ServiceResult<()> {
        let limit = self.max_open_documents;
        if documents.len() < limit
            || document_uuid.is_some_and(|uuid| documents.open.contains_key(uuid))
        {
            return Ok(());
        }
        let in_use = documents
            .open
            .values()
            .filter(|open| !open.handle.is_solely_held())
            .count();
        let warm = documents.open.len() - in_use;
        let idle_timeout_ms = self.document_idle_timeout_ms;
        // Normally empty, and normally omitted for that reason. When it is
        // not, the other two counts do not add up to the limit, and an
        // operator reading a refusal deserves to be told where the rest went
        // rather than left to wonder.
        let stopping = match documents.retiring.len() {
            0 => String::new(),
            count => format!(", and {count} have been stopped but their threads have not exited"),
        };
        let reason = format!(
            "this service already owns {limit} open documents, which is its limit; \
             {in_use} are held by a live connection and {warm} were last used within \
             this service's {idle_timeout_ms} ms idle window{stopping}, so none of them \
             can be reclaimed yet"
        );
        Err(ServiceError::Internal(match document_uuid {
            Some(uuid) => format!(
                "{reason}; document {uuid} can be opened once one of them is released or falls idle"
            ),
            None => format!(
                "{reason}; no further document can be created until one of them is released or falls idle"
            ),
        }))
    }

    /// Registers a freshly spawned document under `document_uuid`.
    ///
    /// Takes the lock guard rather than the lock, because every caller has
    /// already checked the limit under that same guard and a gap between the
    /// check and the insert is exactly the race the check is there to lose.
    fn register(&self, documents: &mut Registry, document_uuid: String, running: RunningDocument) {
        documents.open.insert(
            document_uuid,
            OpenDocument {
                handle: running.handle,
                thread: running.thread,
                last_used_ms: self.clock.now_ms(),
            },
        );
    }

    /// Stops the thread of every document nothing is using any more.
    ///
    /// Safe by construction rather than by timing: an entry is only stopped
    /// when the registry's own clone of its handle is the last one in the
    /// process, and the thread is joined before the entry is removed, all
    /// under the registry lock. So there is no instant at which a document has
    /// a running thread and no registry entry — which is the state that would
    /// let the next open start a second thread for a branch head that already
    /// has one.
    ///
    /// Returns how many were stopped. Public because an operator process may
    /// want to reclaim on its own schedule rather than only when a document is
    /// opened.
    pub fn reclaim_idle_documents(&self) -> ServiceResult<usize> {
        let mut documents = self.lock_documents()?;
        Ok(self.reclaim_locked(&mut documents))
    }

    fn reclaim_locked(&self, documents: &mut Registry) -> usize {
        let now_ms = self.clock.now_ms();
        self.last_sweep_ms.store(now_ms, Ordering::Relaxed);
        documents.reap_retired();
        let idle_timeout_ms = self.document_idle_timeout_ms;
        let reclaimable: Vec<String> = documents
            .open
            .iter()
            .filter(|(_, open)| open.is_reclaimable(now_ms, idle_timeout_ms))
            .map(|(document_uuid, _)| document_uuid.clone())
            .collect();
        documents.retire_all(&reclaimable, THREAD_EXIT_TIMEOUT_MS)
    }

    /// Sweeps before admitting a document, but not on every single open.
    ///
    /// Two conditions, for two different reasons. Under the limit it is a bounded
    /// housekeeping pass at most once per idle window, so a long-lived process
    /// does not sit on threads for documents nobody has touched since
    /// yesterday. *At* the limit it is not housekeeping at all: it is the
    /// difference between a cap that lifts when a document falls idle and a
    /// cap that, once reached, refuses for the life of the process.
    fn reclaim_before_admitting(&self, documents: &mut Registry) {
        let due = self
            .clock
            .now_ms()
            .saturating_sub(self.last_sweep_ms.load(Ordering::Relaxed))
            >= self.document_idle_timeout_ms;
        if due || documents.len() >= self.max_open_documents {
            self.reclaim_locked(documents);
        }
    }

    /// The running handle for a document, starting its thread on first use.
    ///
    /// This performs no authorization: every caller reaching a document goes
    /// through [`Self::open_document`] or does its own check. Keeping the two
    /// apart means the `GrantsChanged` notification, which has no subject,
    /// does not need a permission it cannot have.
    pub fn document(&self, document_uuid: &str) -> ServiceResult<DocumentHandle> {
        let key = document_uuid.trim().to_string();
        let mut documents = self.lock_documents()?;
        // Taken *before* the sweep below, and deliberately. Handing the handle
        // out is the only evidence of use the registry has for a caller that
        // does not hold one, so it is recorded first; and the clone itself
        // then makes the document ineligible, so the document being asked for
        // can never be the one the sweep stops. Swept first, a document at the
        // edge of its idle window would be stopped and started again two lines
        // later, reloading its whole log for nothing.
        let held = documents.open.get_mut(&key).map(|open| {
            open.last_used_ms = self.clock.now_ms();
            open.handle.clone()
        });
        self.reclaim_before_admitting(&mut documents);
        if let Some(handle) = held {
            return Ok(handle);
        }
        // A uuid whose thread has not been seen to exit is not free. Starting
        // one here is the second thread on that branch head, so this refuses
        // instead — retryably, because the ordinary reason to be here is that
        // the thread is a few microseconds behind the sweep that retired it.
        if documents.retiring.contains_key(&key) {
            documents.reap_retired();
        }
        if documents.retiring.contains_key(&key) {
            return Err(ServiceError::Conflict(format!(
                "document {key} is being stopped and its thread has not exited; this service will not start a second thread for one branch head"
            )));
        }
        self.refuse_when_full(&documents, Some(&key))?;
        let running = spawn_document(
            self.repository(),
            Arc::clone(&self.permissions),
            &key,
            self.clock.clone(),
            self.max_operations_per_submit,
        )?;
        let handle = running.handle.clone();
        self.register(&mut documents, key, running);
        Ok(handle)
    }

    /// Authorizes `subject` for `action` and then hands back the document.
    ///
    /// Order matters: a caller with no grant must not learn whether the
    /// document exists, so the permission check runs before the load.
    pub fn open_document(
        &self,
        document_uuid: &str,
        subject: &str,
        action: Action,
    ) -> ServiceResult<(DocumentHandle, Role)> {
        let role = self.permissions.authorize(document_uuid, subject, action)?;
        Ok((self.document(document_uuid)?, role))
    }

    /// Changes one subject's grant, then makes the change bite immediately.
    ///
    /// A revoked subject's live connections are closed by the document thread,
    /// and its sessions are dropped so a reconnect needs a fresh credential.
    pub fn set_grant(
        &self,
        document_uuid: &str,
        actor_subject: &str,
        target_subject: &str,
        role: Option<Role>,
    ) -> ServiceResult<()> {
        self.permissions
            .set_role(document_uuid, actor_subject, target_subject, role)?;
        if role.is_none() {
            // ADR 0015: "Revoking read also closes connections that are
            // already open, and drops the subject's sessions." The first half
            // is the document thread's `GrantsChanged` below. The second half
            // used to be a claim with no caller — `close_sessions_for_subject`
            // was dead code — so a revoked subject's live bearer tokens kept
            // working until they expired, and it could reconnect to anything
            // else it still had a grant on with a credential that was supposed
            // to be gone.
            //
            // Sessions are per subject, not per document, so this is wider
            // than the grant that changed: the subject has to sign in again
            // everywhere. That is the reading the ADR wrote down, and the safe
            // direction to be wrong in.
            let _ = self.identity.close_sessions_for_subject(target_subject);
        }
        if let Ok(handle) = self.document(document_uuid) {
            handle.grants_changed();
        }
        Ok(())
    }

    pub fn repository(&self) -> Repository<SharedStore<S>> {
        Repository::new(self.store.clone())
    }

    pub fn store(&self) -> &SharedStore<S> {
        &self.store
    }

    /// Stops every document thread this process owns, however idle, and
    /// forgets the cached grants with them.
    ///
    /// This is what a restart looks like from inside one process: the next
    /// [`Self::document`] rebuilds from the object store alone. It stops only
    /// what it can stop *safely* — a document a connection still holds keeps
    /// its thread — and returns how many it stopped, so a caller is told the
    /// difference rather than left to assume.
    ///
    /// Its predecessor cleared the map instead, which was not a restart: it
    /// left every running thread running while removing the entry that proved
    /// it existed, so the next open started a second thread on the same branch
    /// head. It had no callers, which is the only reason that never happened.
    pub fn stop_idle_documents(&self) -> ServiceResult<usize> {
        let stopped = {
            let mut documents = self.lock_documents()?;
            documents.reap_retired();
            let reclaimable: Vec<String> = documents
                .open
                .iter()
                .filter(|(_, open)| open.handle.is_solely_held())
                .map(|(document_uuid, _)| document_uuid.clone())
                .collect();
            documents.retire_all(&reclaimable, THREAD_EXIT_TIMEOUT_MS)
        };
        self.permissions.forget_cache()?;
        Ok(stopped)
    }

    /// How many documents this process currently owns running threads for.
    ///
    /// A document whose thread has been stopped is not counted the moment it
    /// is stopped, but one that was asked to stop and has not been seen to is:
    /// it still holds a thread, and the limit counts threads.
    pub fn open_document_count(&self) -> ServiceResult<usize> {
        Ok(self.lock_documents()?.len())
    }

    fn lock_documents(&self) -> ServiceResult<std::sync::MutexGuard<'_, Registry>> {
        self.documents
            .lock()
            .map_err(|_| ServiceError::Internal("document registry lock poisoned".to_string()))
    }
}

/// The storage key for one subject's quota record.
///
/// Keyed by the SHA-256 of the subject rather than by the subject itself: a
/// subject name is operator-provisioned but it is still a string, and a name
/// containing `..` or `/` must not be able to decide where this service
/// writes. A digest is always a safe key segment, which removes the question
/// rather than answering it with a character allowlist.
fn quota_path(subject: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(subject.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("service/quota/{hex}.quota")
}
