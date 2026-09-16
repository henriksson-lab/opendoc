//! The durable operation log, and the commit that makes an operation durable.
//!
//! # Shape
//!
//! One document is one branch in an `opendoc-store` [`Repository`]: a chain of
//! manifests, each naming the operation segment that produced it and the
//! snapshot of the document at that point. The genesis manifest names no
//! segment; its snapshot is the merge base. Every later manifest's parent is
//! the manifest it was committed against.
//!
//! Nothing here is a new storage format. The segment chain is exactly the one
//! `opendoc-app` writes when it saves locally; the difference is only what
//! rides inside an operation entry (`opendoc_merge::Operation`, the typed
//! operation, rather than the app's envelope DTO — see "What remains" in
//! `docs/adr/0015`).
//!
//! # Why the whole log, and why re-merge from genesis
//!
//! ADR 0007 made the merged document a pure function of (merge base,
//! operation *set*), and paid for that with a stated limit: the per-character
//! identities are reconstructed on every merge and do not survive it, so an
//! operation can only be placed by identity while the base it was written
//! against is still the base. Materialising each commit against the *previous
//! commit's output* would therefore re-anchor every operation that arrives
//! late, positionally and clamped — the exact corruption ADR 0007 exists to
//! remove, reintroduced by the server. So the server keeps the genesis base
//! and the entire operation log, and re-merges the set.
//!
//! The cost is real and is stated rather than hidden: a commit is O(log) and a
//! document's history is O(commits²) of merge work over its lifetime. See
//! "Known limitations" in `docs/adr/0015`.
//!
//! # Head safety
//!
//! ADR 0008's rule — a durable head must never name non-durable objects — is
//! the ordering this module obeys: snapshot object, then segment object, then
//! manifest object, and only then the compare-and-swap that moves the branch
//! head. `Repository::commit_manifest` performs that last pair; the objects
//! the manifest names are written before it is called. A crash at any point
//! leaves objects nothing points at, which is garbage, not corruption.

use crate::error::{ServiceError, ServiceResult};
use opendoc_core::{digest_bytes, Document, HashRef};
use opendoc_format::{
    decode_cbor, decode_record, encode_canonical_cbor, encode_record, ManifestRecord,
    OperationSegmentRecord, SnapshotRecord,
};
use opendoc_merge::{merge_operations, Operation};
use opendoc_store::{ObjectStore, Repository};

/// The branch every service document lives on. The service does not expose
/// branching; divergence is resolved by merging operations, not by forking
/// heads.
pub const SERVICE_BRANCH: &str = "main";

/// The `source_format` of a snapshot the service writes.
///
/// Deliberately not `opendoc.app-document.v0`: that format's payload is
/// `opendoc_app::AppDocument`, a projection DTO the app owns. The service
/// stores `opendoc_core::Document`, the canonical model, and says so.
pub const SERVICE_DOCUMENT_FORMAT: &str = "opendoc.service-document.v0";

/// How far back the loader will walk a manifest chain before refusing.
/// A cycle in `parent` is not reachable through this module's own writes, but
/// the loader reads bytes it did not necessarily write.
pub const MANIFEST_CHAIN_LIMIT: usize = 100_000;

/// One durable commit: what the head moved to and what it contained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commit {
    /// Monotone per document, starting at 0 for genesis. Not persisted as a
    /// field: it is the length of the manifest chain, so it is recomputed
    /// identically on every load.
    pub commit_seq: u64,
    pub manifest: HashRef,
    pub parent: Option<HashRef>,
    pub operations: Vec<Operation>,
}

/// The whole durable state of one document, in memory, with the storage it
/// came from.
///
/// This type is deliberately *not* `Sync`-shared: exactly one thread owns it
/// (see [`crate::document`]), which is what makes "two clients cannot
/// interleave into an invalid head" structural rather than a rule someone has
/// to follow.
pub struct DocumentLog<S: ObjectStore> {
    repository: Repository<S>,
    document_uuid: String,
    /// The merge base: the document as of genesis, before any operation.
    base: Document,
    /// Every operation ever accepted, in commit order.
    operations: Vec<Operation>,
    /// The current materialised document: `merge_operations(base, log)`.
    document: Document,
    head: Option<HashRef>,
    previous_segment: Option<HashRef>,
    commit_seq: u64,
}

impl<S: ObjectStore> DocumentLog<S> {
    /// Writes the genesis manifest for a document that does not exist yet.
    ///
    /// The compare-and-swap expects *no* current head, so two services racing
    /// to create the same document produce one winner and one
    /// [`ServiceError::Conflict`], never two genesis chains.
    pub fn create(repository: Repository<S>, base: Document) -> ServiceResult<Self> {
        let document_uuid = base.uuid.as_str().to_string();
        if repository
            .store()
            .read_head(&document_uuid, SERVICE_BRANCH)?
            .is_some()
        {
            return Err(ServiceError::Conflict(format!(
                "document {document_uuid} already exists"
            )));
        }
        let snapshot_hash = write_snapshot(&repository, &base)?;
        let manifest = ManifestRecord {
            document_uuid: document_uuid.clone(),
            branch: SERVICE_BRANCH.to_string(),
            parent: None,
            snapshot: snapshot_hash,
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 0,
        };
        let Some(head) = repository.commit_manifest(&manifest, None)? else {
            return Err(ServiceError::Conflict(format!(
                "document {document_uuid} was created concurrently"
            )));
        };
        Ok(Self {
            repository,
            document_uuid,
            document: base.clone(),
            base,
            operations: Vec::new(),
            head: Some(head),
            previous_segment: None,
            commit_seq: 0,
        })
    }

    /// Rebuilds a document from durable storage alone.
    ///
    /// This is the restart path and the only one: nothing in the service
    /// carries state across a process boundary except the object store.
    pub fn load(repository: Repository<S>, document_uuid: &str) -> ServiceResult<Self> {
        let document_uuid = document_uuid.trim().to_string();
        let Some(head) = repository
            .store()
            .read_head(&document_uuid, SERVICE_BRANCH)?
        else {
            return Err(ServiceError::NotFound(format!(
                "document {document_uuid} has no branch head"
            )));
        };

        // Walk parents to the root, newest first, then reverse: the chain is
        // singly linked toward the past.
        let mut chain: Vec<ManifestRecord> = Vec::new();
        let mut cursor = Some(head.clone());
        while let Some(hash) = cursor {
            if chain.len() >= MANIFEST_CHAIN_LIMIT {
                return Err(ServiceError::Storage(format!(
                    "document {document_uuid} manifest chain exceeds {MANIFEST_CHAIN_LIMIT}"
                )));
            }
            let manifest = repository.read_manifest(&hash)?.ok_or_else(|| {
                ServiceError::Storage(format!("manifest {hash} named by the chain is missing"))
            })?;
            if manifest.document_uuid != document_uuid {
                return Err(ServiceError::Storage(format!(
                    "manifest {hash} names document {} on document {document_uuid}'s chain",
                    manifest.document_uuid
                )));
            }
            cursor = manifest.parent.clone();
            chain.push(manifest);
        }
        chain.reverse();

        let genesis = chain.first().ok_or_else(|| {
            ServiceError::Storage(format!(
                "document {document_uuid} has an empty manifest chain"
            ))
        })?;
        let base = read_snapshot(&repository, &genesis.snapshot)?;

        let mut operations: Vec<Operation> = Vec::new();
        let mut previous_segment = None;
        for manifest in &chain {
            for segment_hash in &manifest.operation_segments {
                let segment = read_segment(&repository, segment_hash)?;
                operations.extend(segment.operations);
                previous_segment = Some(segment_hash.clone());
            }
        }

        verify_sequences_are_dense(&document_uuid, &operations)?;
        let document = materialise(&base, std::slice::from_ref(&operations))?;
        let commit_seq = (chain.len() - 1) as u64;
        Ok(Self {
            repository,
            document_uuid,
            base,
            operations,
            document,
            head: Some(head),
            previous_segment,
            commit_seq,
        })
    }

    /// Opens a document, creating it from `base` only if it does not exist.
    pub fn open_or_create(repository: Repository<S>, base: Document) -> ServiceResult<Self> {
        let document_uuid = base.uuid.as_str().to_string();
        if repository
            .store()
            .read_head(&document_uuid, SERVICE_BRANCH)?
            .is_some()
        {
            Self::load(repository, &document_uuid)
        } else {
            Self::create(repository, base)
        }
    }

    pub fn document_uuid(&self) -> &str {
        &self.document_uuid
    }

    pub fn base(&self) -> &Document {
        &self.base
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    pub fn head(&self) -> Option<&HashRef> {
        self.head.as_ref()
    }

    pub fn commit_seq(&self) -> u64 {
        self.commit_seq
    }

    /// True when an operation with this id is already in the log.
    pub fn contains(&self, operation: &Operation) -> bool {
        self.operations
            .iter()
            .any(|existing| existing.id == operation.id)
    }

    /// The operation already logged under this id, if any.
    pub fn logged(&self, operation: &Operation) -> Option<&Operation> {
        self.operations
            .iter()
            .find(|existing| existing.id == operation.id)
    }

    /// Appends operations and makes them durable before returning.
    ///
    /// The caller gets a [`Commit`] only after the branch head names a
    /// manifest that names a segment that contains these operations. There is
    /// no "accepted, will be written" state to reason about.
    ///
    /// A failed compare-and-swap means something other than this log moved the
    /// head. The log refuses rather than retrying: it cannot know whether the
    /// other writer's operations belong in its base, and a silent retry would
    /// be exactly the last-writer-wins ADR 0004 forbids.
    pub fn append(&mut self, operations: Vec<Operation>) -> ServiceResult<Commit> {
        if operations.is_empty() {
            return Err(ServiceError::BadRequest(
                "commit contains no operations".to_string(),
            ));
        }

        // Materialise first. An operation set that cannot be merged is not a
        // set that should reach storage.
        let mut next_operations = self.operations.clone();
        next_operations.extend(operations.iter().cloned());
        let next_document = materialise(&self.base, std::slice::from_ref(&next_operations))?;

        // Objects before the head that names them (ADR 0008).
        let snapshot_hash = write_snapshot(&self.repository, &next_document)?;
        let segment_hash = write_segment(
            &self.repository,
            &self.document_uuid,
            self.previous_segment.clone(),
            self.head.as_ref().map(ToString::to_string),
            &operations,
        )?;
        let manifest = ManifestRecord {
            document_uuid: self.document_uuid.clone(),
            branch: SERVICE_BRANCH.to_string(),
            parent: self.head.clone(),
            snapshot: snapshot_hash,
            operation_segments: vec![segment_hash.clone()],
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 0,
        };
        let Some(manifest_hash) = self
            .repository
            .commit_manifest(&manifest, self.head.as_ref())?
        else {
            return Err(ServiceError::Conflict(format!(
                "branch head for document {} moved under the commit",
                self.document_uuid
            )));
        };

        // Durable. Only now does in-memory state advance.
        self.operations = next_operations;
        self.document = next_document;
        self.previous_segment = Some(segment_hash);
        self.head = Some(manifest_hash.clone());
        self.commit_seq += 1;
        Ok(Commit {
            commit_seq: self.commit_seq,
            manifest: manifest_hash,
            parent: manifest.parent,
            operations,
        })
    }
}

/// Refuses a log whose per-actor sequence numbers are not 1, 2, 3, ...
///
/// `VectorClock::observed` reads "seq >= n" as "every one of that actor's
/// operations up to n", which is only true of a dense sequence. The service
/// enforces density on intake; this is the matching check on the way back in
/// from storage, and it is what lets the causal-honesty check upstream be an
/// array index rather than a scan.
fn verify_sequences_are_dense(document_uuid: &str, operations: &[Operation]) -> ServiceResult<()> {
    let mut highest: std::collections::BTreeMap<&opendoc_merge::ActorId, u64> =
        std::collections::BTreeMap::new();
    for operation in operations {
        let entry = highest.entry(&operation.id.actor).or_insert(0);
        if operation.id.seq != *entry + 1 {
            return Err(ServiceError::Storage(format!(
                "document {document_uuid} log has actor {} at sequence {} after {}",
                operation.id.actor.0, operation.id.seq, *entry
            )));
        }
        *entry = operation.id.seq;
    }
    Ok(())
}

/// The document the operation set folds the base into.
///
/// Takes the streams shape `merge_operations` takes so the caller can hand it
/// the log it already owns; the merge is a pure function of the operation
/// *set*, so how it is partitioned into streams changes nothing (ADR 0007).
fn materialise(base: &Document, streams: &[Vec<Operation>]) -> ServiceResult<Document> {
    merge_operations(base, streams)
        .map(|result| result.document)
        .map_err(|error| ServiceError::BadRequest(format!("operations did not merge: {error:?}")))
}

fn write_snapshot<S: ObjectStore>(
    repository: &Repository<S>,
    document: &Document,
) -> ServiceResult<HashRef> {
    let source = encode_canonical_cbor(document)
        .map_err(|error| ServiceError::Storage(error.to_string()))?;
    let record = SnapshotRecord::new(
        document.uuid.as_str().to_string(),
        SERVICE_DOCUMENT_FORMAT,
        source,
    );
    record
        .validate()
        .map_err(|error| ServiceError::Storage(error.to_string()))?;
    put_record(repository, &encode_record(&record))
}

fn read_snapshot<S: ObjectStore>(
    repository: &Repository<S>,
    hash: &HashRef,
) -> ServiceResult<Document> {
    let bytes = repository
        .store()
        .get(hash)?
        .ok_or_else(|| ServiceError::Storage(format!("snapshot {hash} is missing")))?;
    let record: SnapshotRecord<Vec<u8>> =
        decode_record(&bytes).map_err(|error| ServiceError::Storage(error.to_string()))?;
    record
        .validate()
        .map_err(|error| ServiceError::Storage(error.to_string()))?;
    if record.source_format != SERVICE_DOCUMENT_FORMAT {
        return Err(ServiceError::Storage(format!(
            "snapshot {hash} has source format {}, expected {SERVICE_DOCUMENT_FORMAT}",
            record.source_format
        )));
    }
    decode_cbor(&record.source).map_err(|error| ServiceError::Storage(error.to_string()))
}

fn write_segment<S: ObjectStore>(
    repository: &Repository<S>,
    document_uuid: &str,
    previous_segment: Option<HashRef>,
    base_manifest: Option<String>,
    operations: &[Operation],
) -> ServiceResult<HashRef> {
    let encoded = operations
        .iter()
        .map(|operation| {
            encode_canonical_cbor(operation)
                .map_err(|error| ServiceError::Storage(error.to_string()))
        })
        .collect::<ServiceResult<Vec<_>>>()?;
    let record = OperationSegmentRecord::new(
        document_uuid.to_string(),
        SERVICE_BRANCH,
        previous_segment,
        base_manifest,
        encoded,
    );
    record
        .validate()
        .map_err(|error| ServiceError::Storage(error.to_string()))?;
    put_record(repository, &encode_record(&record))
}

fn read_segment<S: ObjectStore>(
    repository: &Repository<S>,
    hash: &HashRef,
) -> ServiceResult<OperationSegmentRecord<Operation>> {
    let bytes = repository
        .store()
        .get(hash)?
        .ok_or_else(|| ServiceError::Storage(format!("operation segment {hash} is missing")))?;
    let record: OperationSegmentRecord<Vec<u8>> =
        decode_record(&bytes).map_err(|error| ServiceError::Storage(error.to_string()))?;
    record
        .validate()
        .map_err(|error| ServiceError::Storage(error.to_string()))?;
    let operations = record
        .operations
        .iter()
        .map(|entry| decode_cbor(entry).map_err(|error| ServiceError::Storage(error.to_string())))
        .collect::<ServiceResult<Vec<Operation>>>()?;
    Ok(OperationSegmentRecord::new(
        record.document_uuid,
        record.branch,
        record.previous_segment,
        record.base_manifest,
        operations,
    ))
}

fn put_record<S: ObjectStore>(repository: &Repository<S>, bytes: &[u8]) -> ServiceResult<HashRef> {
    let hash = digest_bytes("sha256", bytes)
        .map_err(|error| ServiceError::Storage(format!("{error:?}")))?;
    repository.store().put_if_absent(&hash, bytes)?;
    Ok(hash)
}
