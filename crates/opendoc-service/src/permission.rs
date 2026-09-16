//! What the caller is allowed to do — decided from the server's own durable
//! state, never from anything the caller sent.
//!
//! This is the piece `opendoc-api`'s `authorize_runtime_command` cannot be.
//! That function takes the grants as an argument, so the client asserts its
//! own permissions; here the grants are read from storage the client cannot
//! write, keyed by the subject the session table resolved.
//!
//! Grants are deployment state, not document state (ADR 0004): they live in
//! the service's own `service/permissions/` namespace, are not part of the
//! document's signed source, and never reach a manifest.

use crate::error::{ServiceError, ServiceResult};
use opendoc_format::{decode_cbor, encode_canonical_cbor};
use opendoc_store::ObjectStore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// A role is a totally ordered bundle of actions. Ordering is what makes
/// "at least commenter" expressible without enumerating actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Viewer,
    Commenter,
    Editor,
    Owner,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Commenter => "commenter",
            Self::Editor => "editor",
            Self::Owner => "owner",
        }
    }

    pub fn parse(value: &str) -> ServiceResult<Self> {
        match value.trim() {
            "viewer" => Ok(Self::Viewer),
            "commenter" => Ok(Self::Commenter),
            "editor" => Ok(Self::Editor),
            "owner" => Ok(Self::Owner),
            other => Err(ServiceError::BadRequest(format!("unknown role {other}"))),
        }
    }

    pub fn allows(self, action: Action) -> bool {
        self >= action.minimum_role()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    /// Open the document and receive its operation stream.
    Read,
    /// Announce presence and a cursor.
    Present,
    /// Submit comment and suggestion operations.
    Comment,
    /// Submit any document operation.
    Write,
    /// Change other subjects' grants.
    Share,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Present => "present",
            Self::Comment => "comment",
            Self::Write => "write",
            Self::Share => "share",
        }
    }

    pub fn minimum_role(self) -> Role {
        match self {
            Self::Read | Self::Present => Role::Viewer,
            Self::Comment => Role::Commenter,
            Self::Write => Role::Editor,
            Self::Share => Role::Owner,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct DocumentGrantsRecord {
    document_uuid: String,
    /// subject -> role. A `BTreeMap` so the canonical CBOR of equal grant sets
    /// is equal bytes, which is what makes an unchanged write a no-op.
    entries: BTreeMap<String, Role>,
    /// The recent, durable history of ACL mutations.  This deliberately lives
    /// in the same canonical record as the grant table: a successful access
    /// change cannot be persisted without its audit event (or vice versa).
    #[serde(default)]
    audit: Vec<GrantAuditEvent>,
}

/// One server-authenticated ACL mutation.  It has no wall-clock field because
/// the sequence is the durable order, even with a manual or unavailable clock.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrantAuditEvent {
    pub sequence: u64,
    pub actor_subject: String,
    pub target_subject: String,
    pub previous_role: Option<Role>,
    pub role: Option<Role>,
}

const MAX_AUDIT_EVENTS: usize = 256;

/// The server's grant table, durable in the object store.
///
/// Reads go through a write-through cache; writes land in storage *before*
/// the cache, so a failed write never leaves the service believing a grant it
/// did not persist. That direction matters more than it looks: the opposite
/// order would let a crash silently widen access on restart.
pub struct PermissionService<S: ObjectStore> {
    store: S,
    cache: Mutex<BTreeMap<String, DocumentGrantsRecord>>,
    /// Serializes every read-modify-write of a grant table.
    ///
    /// The cache mutex cannot serve: it is taken and released inside the read
    /// and again inside the write, so two concurrent share requests could both
    /// read the same table and both write their own version of it — one grant
    /// silently lost, and, worse, the "never leave a document without an
    /// owner" check below made from a table that was already stale. This is
    /// the compare-and-swap this service does not get from the object store's
    /// named puts.
    writes: Mutex<()>,
}

impl<S: ObjectStore> PermissionService<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            cache: Mutex::new(BTreeMap::new()),
            writes: Mutex::new(()),
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// The role a subject holds on a document, or `None` for no access.
    pub fn role_for(&self, document_uuid: &str, subject: &str) -> ServiceResult<Option<Role>> {
        Ok(self
            .grants(document_uuid)?
            .entries
            .get(subject.trim())
            .copied())
    }

    /// The single authorization entry point.
    ///
    /// Everything that enforces a permission calls this, so there is one place
    /// to read to know what the server checks.
    pub fn authorize(
        &self,
        document_uuid: &str,
        subject: &str,
        action: Action,
    ) -> ServiceResult<Role> {
        match self.role_for(document_uuid, subject)? {
            Some(role) if role.allows(action) => Ok(role),
            Some(role) => Err(ServiceError::Forbidden(format!(
                "subject {subject} holds {} on document {document_uuid}, which does not allow {}",
                role.as_str(),
                action.as_str()
            ))),
            // Deliberately the same shape as the role-too-low error: a caller
            // with no access learns that it has no access, not whether the
            // document exists.
            None => Err(ServiceError::Forbidden(format!(
                "subject {subject} holds no grant on document {document_uuid}"
            ))),
        }
    }

    /// Records the creator as owner. Refuses to re-seed a document that
    /// already has grants, so "create" can never be used to take over.
    pub fn seed_owner(&self, document_uuid: &str, subject: &str) -> ServiceResult<()> {
        let _guard = self.lock_writes()?;
        let mut record = self.grants(document_uuid)?;
        if !record.entries.is_empty() {
            return Err(ServiceError::Conflict(format!(
                "document {document_uuid} already has grants"
            )));
        }
        record
            .entries
            .insert(subject.trim().to_string(), Role::Owner);
        record.audit.push(GrantAuditEvent {
            sequence: 1,
            actor_subject: subject.trim().to_string(),
            target_subject: subject.trim().to_string(),
            previous_role: None,
            role: Some(Role::Owner),
        });
        self.persist(record)
    }

    /// Sets one subject's role.
    ///
    /// `actor_subject` must itself hold [`Action::Share`], and cannot remove
    /// the last owner — a document with no owner can never be shared again.
    pub fn set_role(
        &self,
        document_uuid: &str,
        actor_subject: &str,
        target_subject: &str,
        role: Option<Role>,
    ) -> ServiceResult<()> {
        // Held across the whole read-modify-write, including the
        // authorization: an owner demoting itself concurrently with another
        // owner's change must not be able to interleave into a table with no
        // owner in it.
        let _guard = self.lock_writes()?;
        self.authorize(document_uuid, actor_subject, Action::Share)?;
        let target = target_subject.trim();
        if target.is_empty() {
            return Err(ServiceError::BadRequest(
                "target subject is empty".to_string(),
            ));
        }
        let mut record = self.grants(document_uuid)?;
        let previous_role = record.entries.get(target).copied();
        match role {
            Some(role) => {
                record.entries.insert(target.to_string(), role);
            }
            None => {
                record.entries.remove(target);
            }
        }
        if !record
            .entries
            .values()
            .any(|existing| *existing == Role::Owner)
        {
            return Err(ServiceError::Conflict(format!(
                "document {document_uuid} would be left with no owner"
            )));
        }
        if previous_role != role {
            let sequence = record
                .audit
                .last()
                .map_or(1, |event| event.sequence.saturating_add(1));
            record.audit.push(GrantAuditEvent {
                sequence,
                actor_subject: actor_subject.trim().to_string(),
                target_subject: target.to_string(),
                previous_role,
                role,
            });
            let excess = record.audit.len().saturating_sub(MAX_AUDIT_EVENTS);
            if excess > 0 {
                record.audit.drain(..excess);
            }
        }
        self.persist(record)
    }

    pub fn grant_list(&self, document_uuid: &str) -> ServiceResult<Vec<(String, Role)>> {
        Ok(self
            .grants(document_uuid)?
            .entries
            .into_iter()
            .collect::<Vec<_>>())
    }

    /// Returns newest-first bounded ACL history.  Callers must authorize this
    /// exactly as they authorize the grant list; audit metadata is access
    /// management data, not document content.
    pub fn grant_audit(&self, document_uuid: &str) -> ServiceResult<Vec<GrantAuditEvent>> {
        let mut audit = self.grants(document_uuid)?.audit;
        audit.reverse();
        Ok(audit)
    }

    /// Drops every cached grant table, so the next read comes from storage.
    /// This is what a restart does implicitly and what a test uses to prove
    /// the durable copy, not the cache, is the answer.
    pub fn forget_cache(&self) -> ServiceResult<()> {
        self.lock_cache()?.clear();
        Ok(())
    }

    fn grants(&self, document_uuid: &str) -> ServiceResult<DocumentGrantsRecord> {
        let key = document_uuid.trim().to_string();
        if key.is_empty() {
            return Err(ServiceError::BadRequest(
                "document uuid is empty".to_string(),
            ));
        }
        if let Some(cached) = self.lock_cache()?.get(&key) {
            return Ok(cached.clone());
        }
        let record = match self.store.get_named(&permission_path(&key)?)? {
            Some(bytes) => decode_cbor::<DocumentGrantsRecord>(&bytes)
                .map_err(|error| ServiceError::Storage(error.to_string()))?,
            None => DocumentGrantsRecord {
                document_uuid: key.clone(),
                entries: BTreeMap::new(),
                audit: Vec::new(),
            },
        };
        if record.document_uuid != key {
            return Err(ServiceError::Storage(format!(
                "permission record at {key} names document {}",
                record.document_uuid
            )));
        }
        self.lock_cache()?.insert(key, record.clone());
        Ok(record)
    }

    fn persist(&self, record: DocumentGrantsRecord) -> ServiceResult<()> {
        let bytes = encode_canonical_cbor(&record)
            .map_err(|error| ServiceError::Storage(error.to_string()))?;
        let path = permission_path(&record.document_uuid)?;
        // Durable first, cache second. A failure here leaves the cache
        // holding the pre-write value, which is the value storage still has.
        self.store.put_named(&path, &bytes)?;
        self.lock_cache()?
            .insert(record.document_uuid.clone(), record);
        Ok(())
    }

    fn lock_writes(&self) -> ServiceResult<std::sync::MutexGuard<'_, ()>> {
        self.writes
            .lock()
            .map_err(|_| ServiceError::Internal("grant write lock poisoned".to_string()))
    }

    fn lock_cache(
        &self,
    ) -> ServiceResult<std::sync::MutexGuard<'_, BTreeMap<String, DocumentGrantsRecord>>> {
        self.cache
            .lock()
            .map_err(|_| ServiceError::Internal("permission cache lock poisoned".to_string()))
    }
}

fn permission_path(document_uuid: &str) -> ServiceResult<String> {
    let uuid = document_uuid.trim();
    let safe = !uuid.is_empty()
        && uuid != "."
        && uuid != ".."
        && uuid
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.');
    if !safe {
        return Err(ServiceError::BadRequest(format!(
            "document uuid {uuid} is not a valid storage key segment"
        )));
    }
    Ok(format!("service/permissions/{uuid}.perm"))
}
