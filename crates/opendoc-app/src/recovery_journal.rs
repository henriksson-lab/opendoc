//! Crash-recovery journal (FS-7).
//!
//! The operation journal in `journal_service.rs` is durable only when a save
//! writes it into the repository as an operation segment. Between saves it
//! lives in memory, so `kill -9` loses everything since the last commit. This
//! module gives that journal a durable shadow: while — and only while — the
//! open document differs from the repository, a *recovery segment* on disk
//! holds a base snapshot plus every operation envelope committed after it.
//!
//! The segment is written outside the manifest/head commit path. Recovery
//! never writes to the repository at all: it rebuilds in-memory state and
//! leaves the user with an ordinary unsaved document, so the manifest chain
//! cannot be corrupted by a replay and the recovered state is saved, validated
//! and signed by exactly the same code as any other edit.
//!
//! Storage is behind [`RecoveryJournalStore`], a byte-level append-only
//! interface. [`FileRecoveryJournalStore`] implements it on a filesystem for
//! the Tauri shell; a browser build simply installs no store and journalling
//! is inert (see `docs/adr/0005-crash-recovery-journal.md`).

use crate::{
    merge_blob_envelopes, merge_operations, merge_operations_from_envelopes,
    merge_spreadsheet_envelope_streams, now_ms, validate_operation_envelopes, AppApiError,
    AppDocument, AppOperationEnvelope, AppOperationRecord, OpenDocApp,
};
use opendoc_format::{decode_cbor, encode_canonical_cbor};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Marks a file as an OpenDoc recovery segment and pins its frame layout.
pub(crate) const RECOVERY_SEGMENT_MAGIC: &[u8] = b"opendoc-recovery-v0\n";
/// `source_format` of the base snapshot carried by a segment header.
pub(crate) const RECOVERY_SEGMENT_FORMAT: &str = "opendoc.recovery-segment.v0";

/// Header of a recovery segment: everything needed to replay it without the
/// repository being reachable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct RecoverySegmentHeader {
    pub format: String,
    pub session_id: String,
    pub actor: String,
    pub document_uuid: String,
    pub title: String,
    pub started_at_ms: u64,
    pub repository_root: Option<String>,
    pub repository_backend: Option<String>,
    pub repository_namespace: Option<String>,
    pub base_manifest: Option<String>,
    /// Unsaved operations already folded into `base`, kept so the recovery
    /// offer can count and name every change the crash caught, not only the
    /// ones replayed as envelopes.
    pub base_operations: Vec<AppOperationRecord>,
    /// Source state the operations replay onto. Imports and new documents are
    /// not typed operations, so a segment cannot start from "empty".
    pub base: AppDocument,
}

/// A decoded recovery segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecoverySegment {
    pub header: RecoverySegmentHeader,
    pub operations: Vec<AppOperationEnvelope>,
    /// True when the last frame on disk was torn by the crash and dropped.
    pub truncated: bool,
}

/// What an unclean prior session left behind, as offered to the user.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppRecoverySession {
    pub id: String,
    pub document_uuid: String,
    pub title: String,
    pub started_at_ms: u64,
    pub operation_count: usize,
    pub repository_root: Option<String>,
    pub repository_backend: Option<String>,
    pub base_manifest: Option<String>,
    /// Summaries of exactly the operations a replay would apply, so the user
    /// can inspect the offer instead of taking it on trust.
    pub operations: Vec<AppOperationRecord>,
    /// The crash tore the final frame; that one operation is not recoverable.
    pub truncated: bool,
}

/// Append-only byte storage for recovery segments.
///
/// Deliberately free of document types: a browser adapter over IndexedDB has
/// only to move these byte strings around. Methods take `&self` because a
/// segment store owns no state worth mutating — the file (or object) is the
/// state — and because the app holds the store behind an `Arc`.
pub trait RecoveryJournalStore: std::fmt::Debug + Send + Sync {
    /// Create or replace the segment named `session_id` with `bytes`.
    fn write_segment(&self, session_id: &str, bytes: &[u8]) -> Result<(), String>;
    /// Append `bytes` to an existing segment.
    fn append_segment(&self, session_id: &str, bytes: &[u8]) -> Result<(), String>;
    /// Session ids of every segment present.
    fn list_segments(&self) -> Result<Vec<String>, String>;
    /// Whole contents of one segment.
    fn read_segment(&self, session_id: &str) -> Result<Option<Vec<u8>>, String>;
    /// Delete one segment.
    fn remove_segment(&self, session_id: &str) -> Result<(), String>;
}

/// Session ids name files (or object keys), so keep them to the alphabet
/// `StableId` produces and refuse anything a caller could aim at another path.
pub(crate) fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty() || session_id.len() > 128 {
        return Err("recovery session id has an unusable length".to_string());
    }
    if !session_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("recovery session id has unsupported characters".to_string());
    }
    Ok(())
}

// ---- Frame codec -----------------------------------------------------------

/// One length-prefixed canonical-CBOR frame.
fn encode_frame(bytes: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(bytes.len() + 4);
    frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(bytes);
    frame
}

pub(crate) fn encode_segment_header(
    header: &RecoverySegmentHeader,
) -> Result<Vec<u8>, AppApiError> {
    let payload =
        encode_canonical_cbor(header).map_err(|err| AppApiError::Format(err.to_string()))?;
    let mut bytes = RECOVERY_SEGMENT_MAGIC.to_vec();
    bytes.extend_from_slice(&encode_frame(&payload));
    Ok(bytes)
}

pub(crate) fn encode_operation_frame(
    envelope: &AppOperationEnvelope,
) -> Result<Vec<u8>, AppApiError> {
    let payload =
        encode_canonical_cbor(envelope).map_err(|err| AppApiError::Format(err.to_string()))?;
    Ok(encode_frame(&payload))
}

/// Split a segment file into frames, reporting a torn tail rather than failing.
///
/// A crash can land mid-write, so the last frame may be short. Everything
/// before it is still intact and still worth offering to the user.
fn split_frames(bytes: &[u8]) -> Result<(Vec<&[u8]>, bool), AppApiError> {
    if !bytes.starts_with(RECOVERY_SEGMENT_MAGIC) {
        return Err(AppApiError::Format(
            "recovery segment is not an OpenDoc recovery file".to_string(),
        ));
    }
    let mut rest = &bytes[RECOVERY_SEGMENT_MAGIC.len()..];
    let mut frames = Vec::new();
    let mut truncated = false;
    while !rest.is_empty() {
        if rest.len() < 4 {
            truncated = true;
            break;
        }
        let len = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        if rest.len() < 4 + len {
            truncated = true;
            break;
        }
        frames.push(&rest[4..4 + len]);
        rest = &rest[4 + len..];
    }
    Ok((frames, truncated))
}

pub(crate) fn decode_segment(bytes: &[u8]) -> Result<RecoverySegment, AppApiError> {
    let (frames, truncated) = split_frames(bytes)?;
    let mut frames = frames.into_iter();
    let header_bytes = frames
        .next()
        .ok_or_else(|| AppApiError::Format("recovery segment has no header frame".to_string()))?;
    let header: RecoverySegmentHeader = decode_cbor(header_bytes)
        .map_err(|err| AppApiError::Format(format!("recovery segment header: {err}")))?;
    if header.format != RECOVERY_SEGMENT_FORMAT {
        return Err(AppApiError::Format(format!(
            "unsupported recovery segment format {}",
            header.format
        )));
    }
    let mut operations = Vec::new();
    for frame in frames {
        let envelope: AppOperationEnvelope = decode_cbor(frame)
            .map_err(|err| AppApiError::Format(format!("recovery segment operation: {err}")))?;
        operations.push(envelope);
    }
    validate_operation_envelopes(&operations)?;
    Ok(RecoverySegment {
        header,
        operations,
        truncated,
    })
}

// ---- Live segment ----------------------------------------------------------

/// Identity of one journalled operation, as far as the recovery segment cares.
///
/// `(actor, seq)` alone is not enough: an undo restores the sequence number it
/// rolled back and then journals its own record under it, so the pair repeats
/// with a different payload.
type EnvelopeIdentity = (String, u64, String);

fn envelope_identity(envelope: &AppOperationEnvelope) -> EnvelopeIdentity {
    (
        envelope.record.actor.clone(),
        envelope.record.seq,
        envelope.record.kind.clone(),
    )
}

/// The recovery segment this process is currently writing.
#[derive(Clone, Debug)]
pub(crate) struct RecoverySegmentCursor {
    session_id: String,
    document_uuid: String,
    /// Saved-operation count the segment was opened at. The segment covers the
    /// journal from here on: everything before it is in the repository.
    saved_operation_count: usize,
    /// Identities of every operation the segment accounts for, base snapshot
    /// first and appended frames after, so that a journal which stopped being
    /// an extension of the segment (undo, redo, a candidate merge) is noticed
    /// wherever it diverged rather than only at the tail.
    covered: Vec<EnvelopeIdentity>,
    /// Signature count the base snapshot was taken at. Signing is not a typed
    /// operation, so a change here has to re-snapshot rather than append.
    signature_count: usize,
}

/// Everything the app keeps for crash recovery.
#[derive(Clone, Debug, Default)]
pub(crate) struct RecoveryJournal {
    pub store: Option<Arc<dyn RecoveryJournalStore>>,
    pub cursor: Option<RecoverySegmentCursor>,
    pub sessions: Vec<AppRecoverySession>,
}

impl OpenDocApp {
    /// Install a recovery store and report what an unclean prior session left.
    ///
    /// The returned document carries `recovery_sessions`; the frontend decides
    /// what to offer. Nothing is replayed here — startup never mutates the
    /// user's document behind their back.
    pub fn install_recovery_journal(
        &mut self,
        store: Arc<dyn RecoveryJournalStore>,
    ) -> Result<AppDocument, AppApiError> {
        self.recovery.store = Some(store);
        self.recovery.cursor = None;
        self.refresh_recovery_sessions();
        Ok(self.document())
    }

    pub(crate) fn recovery_sessions(&self) -> Vec<AppRecoverySession> {
        self.recovery.sessions.clone()
    }

    /// Re-read the store and project every segment it holds.
    pub(crate) fn refresh_recovery_sessions(&mut self) {
        let Some(store) = self.recovery.store.clone() else {
            self.recovery.sessions.clear();
            return;
        };
        let live = self
            .recovery
            .cursor
            .as_ref()
            .map(|cursor| cursor.session_id.clone());
        let mut sessions = Vec::new();
        let ids = match store.list_segments() {
            Ok(ids) => ids,
            Err(err) => {
                self.push_model_warning(
                    "recovery-journal-unavailable",
                    format!("recovery segments could not be listed: {err}"),
                );
                Vec::new()
            }
        };
        for id in ids {
            if live.as_deref() == Some(id.as_str()) {
                continue;
            }
            let bytes = match store.read_segment(&id) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(err) => {
                    self.push_model_warning(
                        "recovery-journal-unreadable",
                        format!("recovery segment {id} could not be read: {err}"),
                    );
                    continue;
                }
            };
            match decode_segment(&bytes) {
                Ok(segment) => {
                    let operations = segment
                        .header
                        .base_operations
                        .iter()
                        .cloned()
                        .chain(
                            segment
                                .operations
                                .iter()
                                .map(|envelope| envelope.record.clone()),
                        )
                        .collect::<Vec<_>>();
                    sessions.push(AppRecoverySession {
                        id: id.clone(),
                        document_uuid: segment.header.document_uuid.clone(),
                        title: segment.header.title.clone(),
                        started_at_ms: segment.header.started_at_ms,
                        operation_count: operations.len(),
                        repository_root: segment.header.repository_root.clone(),
                        repository_backend: segment.header.repository_backend.clone(),
                        base_manifest: segment.header.base_manifest.clone(),
                        operations,
                        truncated: segment.truncated,
                    })
                }
                Err(err) => self.push_model_warning(
                    "recovery-journal-unreadable",
                    format!("recovery segment {id} could not be decoded: {err}"),
                ),
            }
        }
        sessions.sort_by(|left, right| {
            right
                .started_at_ms
                .cmp(&left.started_at_ms)
                .then_with(|| left.id.cmp(&right.id))
        });
        self.recovery.sessions = sessions;
    }

    /// Bring the on-disk recovery segment back in step with memory.
    ///
    /// Called once, from `dispatch_command`, after any command that succeeded.
    /// Driving it from the dispatcher rather than from the places that mint
    /// envelopes is what makes it total: undo, redo and candidate merges move
    /// the journal without minting anything, and they are covered here for
    /// free.
    pub(crate) fn sync_recovery_journal(&mut self) {
        if self.recovery.store.is_none() {
            return;
        }
        if let Err(err) = self.sync_recovery_journal_inner() {
            self.push_model_warning(
                "recovery-journal-unavailable",
                format!("crash recovery journal is not being written: {err}"),
            );
        }
    }

    fn sync_recovery_journal_inner(&mut self) -> Result<(), AppApiError> {
        let store = match self.recovery.store.clone() {
            Some(store) => store,
            None => return Ok(()),
        };
        // Nothing to recover while the repository already holds everything.
        if !self.is_open || !self.has_unsaved_changes() {
            if let Some(cursor) = self.recovery.cursor.take() {
                store
                    .remove_segment(&cursor.session_id)
                    .map_err(AppApiError::Store)?;
            }
            return Ok(());
        }
        if self.recovery_cursor_is_current() {
            let cursor = self
                .recovery
                .cursor
                .as_ref()
                .expect("checked by recovery_cursor_is_current");
            let next = cursor.saved_operation_count + cursor.covered.len();
            let mut appended = Vec::new();
            let mut frames = Vec::new();
            for envelope in &self.operation_envelopes[next..] {
                frames.extend_from_slice(&encode_operation_frame(envelope)?);
                appended.push(envelope_identity(envelope));
            }
            if frames.is_empty() {
                return Ok(());
            }
            let session_id = cursor.session_id.clone();
            store
                .append_segment(&session_id, &frames)
                .map_err(AppApiError::Store)?;
            if let Some(cursor) = self.recovery.cursor.as_mut() {
                cursor.covered.extend(appended);
            }
            return Ok(());
        }
        self.begin_recovery_segment(&store)
    }

    /// Does the live segment still account for exactly a prefix of the
    /// journal's unsaved operations?
    ///
    /// If not, the base snapshot or a written frame describes state the app no
    /// longer has, and appending would leave a segment that replays to the
    /// wrong document.
    fn recovery_cursor_is_current(&self) -> bool {
        let Some(cursor) = self.recovery.cursor.as_ref() else {
            return false;
        };
        if cursor.document_uuid != self.document.uuid.to_string()
            || cursor.signature_count != self.signature_count()
            || cursor.saved_operation_count != self.saved_operation_count
        {
            return false;
        }
        let end = cursor.saved_operation_count + cursor.covered.len();
        if self.operation_envelopes.len() < end {
            return false;
        }
        self.operation_envelopes[cursor.saved_operation_count..end]
            .iter()
            .map(envelope_identity)
            .eq(cursor.covered.iter().cloned())
    }

    /// Start a fresh segment from the current state and drop the previous one.
    fn begin_recovery_segment(
        &mut self,
        store: &Arc<dyn RecoveryJournalStore>,
    ) -> Result<(), AppApiError> {
        let previous = self.recovery.cursor.take();
        let saved = self
            .saved_operation_count
            .min(self.operation_envelopes.len());
        let session_id = opendoc_core::StableId::new("recovery").to_string();
        validate_session_id(&session_id).map_err(AppApiError::Format)?;
        let header = RecoverySegmentHeader {
            format: RECOVERY_SEGMENT_FORMAT.to_string(),
            session_id: session_id.clone(),
            actor: self.actor_id.clone(),
            document_uuid: self.document.uuid.to_string(),
            title: self.document.title.clone(),
            started_at_ms: now_ms(),
            repository_root: self
                .repository_root
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            repository_backend: self.repository_backend.clone(),
            repository_namespace: self.repository_namespace.clone(),
            base_manifest: self.last_manifest.clone(),
            base_operations: self.operation_journal[saved.min(self.operation_journal.len())..]
                .to_vec(),
            base: self.snapshot_document(),
        };
        let bytes = encode_segment_header(&header)?;
        store
            .write_segment(&session_id, &bytes)
            .map_err(AppApiError::Store)?;
        let covered = self.operation_envelopes[saved..]
            .iter()
            .map(envelope_identity)
            .collect::<Vec<_>>();
        self.recovery.cursor = Some(RecoverySegmentCursor {
            session_id,
            document_uuid: header.document_uuid,
            saved_operation_count: saved,
            covered,
            signature_count: self.signature_count(),
        });
        if let Some(previous) = previous {
            store
                .remove_segment(&previous.session_id)
                .map_err(AppApiError::Store)?;
        }
        Ok(())
    }

    /// Forget one recovery segment without replaying it.
    pub fn discard_recovery_session(
        &mut self,
        session_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let session_id = session_id.as_ref();
        validate_session_id(session_id).map_err(AppApiError::Format)?;
        let store =
            self.recovery.store.clone().ok_or_else(|| {
                AppApiError::Conflict("no recovery journal is installed".to_string())
            })?;
        if !self
            .recovery
            .sessions
            .iter()
            .any(|session| session.id == session_id)
        {
            return Err(AppApiError::NotFound(format!(
                "recovery session {session_id} was not found"
            )));
        }
        store
            .remove_segment(session_id)
            .map_err(AppApiError::Store)?;
        self.refresh_recovery_sessions();
        Ok(self.document())
    }

    /// Replay one recovery segment into the open document.
    ///
    /// Rebuilds state in memory only: the base snapshot is the segment's own,
    /// the operations run through the same typed replay the repository uses,
    /// and the result is an ordinary unsaved document. Nothing is committed,
    /// so the manifest chain is untouched until the user saves.
    pub fn recover_session(
        &mut self,
        session_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let session_id = session_id.as_ref().to_string();
        validate_session_id(&session_id).map_err(AppApiError::Format)?;
        let store =
            self.recovery.store.clone().ok_or_else(|| {
                AppApiError::Conflict("no recovery journal is installed".to_string())
            })?;
        let bytes = store
            .read_segment(&session_id)
            .map_err(AppApiError::Store)?
            .ok_or_else(|| {
                AppApiError::NotFound(format!("recovery session {session_id} was not found"))
            })?;
        let segment = decode_segment(&bytes)?;
        segment.header.base.validate_source()?;

        let base_document = segment.header.base.to_core()?;
        let replayed = merge_operations(
            &base_document,
            &[merge_operations_from_envelopes(&segment.operations)],
        )
        .map_err(|err| AppApiError::Model(err.to_string()))?;
        let (workbook, spreadsheet_warnings) = merge_spreadsheet_envelope_streams(
            segment.header.base.workbook.clone(),
            &[segment.operations.as_slice()],
        )?;
        let blobs = merge_blob_envelopes(
            segment.header.base.blobs.clone(),
            segment.header.base.blobs.clone(),
            segment.header.base.blobs.clone(),
            &[segment.operations.as_slice()],
        )?;

        self.document = replayed.document;
        self.workbook = workbook.evaluated();
        self.blobs = blobs;
        self.blob_bytes.clear();
        self.blob_signatures.clear();
        self.blob_tombstones.clear();
        self.blob_tombstone_records.clear();
        self.signatures.clear();
        self.is_open = true;
        self.defer_spreadsheet_evaluation = false;
        self.invalidate_projection();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.undo_coalesce = None;

        // The segment's operations are exactly the delta the repository has
        // not seen, so they become the whole in-memory journal and none of it
        // counts as saved.
        self.operation_envelopes = segment.operations;
        self.operation_journal = self
            .operation_envelopes
            .iter()
            .map(|envelope| envelope.record.clone())
            .collect();
        self.saved_operation_count = 0;
        self.saved_signature_count = 0;
        self.next_seq = self
            .operation_envelopes
            .iter()
            .filter(|envelope| envelope.record.actor == self.actor_id)
            .map(|envelope| envelope.record.seq)
            .max()
            .map_or(1, |seq| seq + 1);
        self.repository_root = segment.header.repository_root.clone().map(Into::into);
        self.repository_backend = segment.header.repository_backend.clone();
        self.repository_namespace = segment.header.repository_namespace.clone();
        self.last_manifest = segment.header.base_manifest.clone();

        for warning in replayed.warnings {
            self.push_model_warning(&warning.code, warning.message);
        }
        for warning in spreadsheet_warnings {
            self.push_model_warning(&warning.code, warning.message);
        }
        if segment.truncated {
            self.push_model_warning(
                "recovery-journal-truncated",
                "the last operation of the recovered session was cut short by the crash and was not replayed",
            );
        }
        if !self.blobs.is_empty() {
            self.push_model_warning(
                "recovery-journal-blob-bytes",
                "attachment contents added after the last save are not part of a recovery segment; re-attach the files",
            );
        }

        // The replayed session is now live in memory; its segment stops being
        // an offer and the next dispatch re-snapshots under a new session id.
        store
            .remove_segment(&session_id)
            .map_err(AppApiError::Store)?;
        self.recovery.cursor = None;
        self.refresh_recovery_sessions();
        Ok(self.document())
    }
}

// ---- Filesystem store ------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
mod file_store {
    use super::{validate_session_id, RecoveryJournalStore, RECOVERY_SEGMENT_MAGIC};
    use std::io::Write;
    use std::path::{Path, PathBuf};

    /// Recovery segments as `<root>/<session id>.recovery` files.
    ///
    /// Appends are written and flushed but not `fsync`ed: the threat this
    /// journal answers is a process that dies (`kill -9`, a panic, a WebView
    /// crash), and the kernel keeps written bytes across that. Surviving a
    /// power cut would mean an `fsync` per keystroke; see ADR 0005.
    #[derive(Clone, Debug)]
    pub struct FileRecoveryJournalStore {
        root: PathBuf,
    }

    impl FileRecoveryJournalStore {
        pub fn new(root: impl Into<PathBuf>) -> Self {
            Self { root: root.into() }
        }

        fn path(&self, session_id: &str) -> Result<PathBuf, String> {
            validate_session_id(session_id)?;
            Ok(self.root.join(format!("{session_id}.recovery")))
        }

        fn ensure_root(&self) -> Result<(), String> {
            std::fs::create_dir_all(&self.root)
                .map_err(|err| format!("{}: {err}", self.root.display()))
        }

        fn session_id_of(path: &Path) -> Option<String> {
            if path.extension().and_then(|ext| ext.to_str()) != Some("recovery") {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_string();
            validate_session_id(&stem).ok()?;
            Some(stem)
        }
    }

    impl RecoveryJournalStore for FileRecoveryJournalStore {
        fn write_segment(&self, session_id: &str, bytes: &[u8]) -> Result<(), String> {
            let path = self.path(session_id)?;
            self.ensure_root()?;
            std::fs::write(&path, bytes).map_err(|err| format!("{}: {err}", path.display()))
        }

        fn append_segment(&self, session_id: &str, bytes: &[u8]) -> Result<(), String> {
            let path = self.path(session_id)?;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            file.write_all(bytes)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            file.flush()
                .map_err(|err| format!("{}: {err}", path.display()))
        }

        fn list_segments(&self) -> Result<Vec<String>, String> {
            let entries = match std::fs::read_dir(&self.root) {
                Ok(entries) => entries,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(err) => return Err(format!("{}: {err}", self.root.display())),
            };
            let mut ids = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|err| format!("{}: {err}", self.root.display()))?;
                if let Some(id) = Self::session_id_of(&entry.path()) {
                    ids.push(id);
                }
            }
            ids.sort();
            Ok(ids)
        }

        fn read_segment(&self, session_id: &str) -> Result<Option<Vec<u8>>, String> {
            let path = self.path(session_id)?;
            match std::fs::read(&path) {
                Ok(bytes) if bytes.starts_with(RECOVERY_SEGMENT_MAGIC) => Ok(Some(bytes)),
                Ok(_) => Err(format!(
                    "{}: not an OpenDoc recovery segment",
                    path.display()
                )),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(err) => Err(format!("{}: {err}", path.display())),
            }
        }

        fn remove_segment(&self, session_id: &str) -> Result<(), String> {
            let path = self.path(session_id)?;
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(format!("{}: {err}", path.display())),
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use file_store::FileRecoveryJournalStore;

// ---- Key/value volume store ------------------------------------------------

mod volume_store {
    use super::{validate_session_id, RecoveryJournalStore, RECOVERY_SEGMENT_MAGIC};
    use opendoc_store::MirroredVolume;

    /// Recovery segments in a [`MirroredVolume`], which is how the browser
    /// gets the crash protection the Tauri shell already has (ADR 0005 §6,
    /// ADR 0008).
    ///
    /// A segment is **not** one value. `append_segment` is called once per
    /// committed gesture, and a segment's first frame carries a whole document
    /// snapshot, so storing the segment as a single growing value would
    /// rewrite that snapshot into durable storage on every keystroke. Each
    /// frame batch is its own key instead — `recovery/<session>/<index>`,
    /// zero-padded so lexicographic key order is frame order — which makes an
    /// append a single small insert, the same shape the file store's `O_APPEND`
    /// write has.
    #[derive(Clone, Debug)]
    pub struct VolumeRecoveryJournalStore {
        volume: MirroredVolume,
        prefix: String,
    }

    /// Zero-padded to ten digits: 10^10 frames is far past any session, and
    /// fixed width is what makes byte order and frame order the same thing.
    fn frame_name(index: u64) -> String {
        format!("{index:010}")
    }

    impl VolumeRecoveryJournalStore {
        /// `prefix` is the volume subtree the journal owns. Repository roots
        /// live under a different subtree, so no repository path can name a
        /// segment key.
        pub fn new(volume: MirroredVolume, prefix: impl Into<String>) -> Self {
            Self {
                volume,
                prefix: prefix.into().trim_matches('/').to_string(),
            }
        }

        pub fn volume(&self) -> &MirroredVolume {
            &self.volume
        }

        fn session_prefix(&self, session_id: &str) -> Result<String, String> {
            validate_session_id(session_id)?;
            Ok(format!("{}/{session_id}", self.prefix))
        }

        fn frame_keys(&self, session_id: &str) -> Result<Vec<String>, String> {
            Ok(self
                .volume
                .keys_with_prefix(&self.session_prefix(session_id)?))
        }
    }

    impl RecoveryJournalStore for VolumeRecoveryJournalStore {
        fn write_segment(&self, session_id: &str, bytes: &[u8]) -> Result<(), String> {
            let prefix = self.session_prefix(session_id)?;
            // "Create or replace": an id reused after a discard must not
            // inherit the discarded segment's tail.
            self.volume.delete_prefix(&prefix);
            self.volume
                .put(&format!("{prefix}/{}", frame_name(0)), bytes);
            Ok(())
        }

        fn append_segment(&self, session_id: &str, bytes: &[u8]) -> Result<(), String> {
            let prefix = self.session_prefix(session_id)?;
            let keys = self.frame_keys(session_id)?;
            // Matching the file store: appending to a segment that is not
            // there is an error, not a silent create, because a segment
            // without its header frame replays to nothing.
            let last = keys
                .last()
                .ok_or_else(|| format!("recovery segment {session_id} is missing"))?;
            let index: u64 = last
                .rsplit('/')
                .next()
                .and_then(|name| name.parse().ok())
                .ok_or_else(|| format!("recovery segment {session_id} has an unreadable frame"))?;
            self.volume
                .put(&format!("{prefix}/{}", frame_name(index + 1)), bytes);
            Ok(())
        }

        fn list_segments(&self) -> Result<Vec<String>, String> {
            let mut ids: Vec<String> = Vec::new();
            for key in self.volume.keys_with_prefix(&self.prefix) {
                let Some(rest) = key.strip_prefix(&format!("{}/", self.prefix)) else {
                    continue;
                };
                let Some((id, _)) = rest.split_once('/') else {
                    continue;
                };
                if validate_session_id(id).is_err() {
                    continue;
                }
                if ids.last().map(String::as_str) != Some(id) {
                    ids.push(id.to_string());
                }
            }
            ids.dedup();
            Ok(ids)
        }

        fn read_segment(&self, session_id: &str) -> Result<Option<Vec<u8>>, String> {
            let keys = self.frame_keys(session_id)?;
            if keys.is_empty() {
                return Ok(None);
            }
            let mut bytes = Vec::new();
            for key in keys {
                let Some(frame) = self.volume.get(&key) else {
                    continue;
                };
                bytes.extend_from_slice(&frame);
            }
            if !bytes.starts_with(RECOVERY_SEGMENT_MAGIC) {
                return Err(format!(
                    "recovery segment {session_id} is not an OpenDoc recovery segment"
                ));
            }
            Ok(Some(bytes))
        }

        fn remove_segment(&self, session_id: &str) -> Result<(), String> {
            self.volume.delete_prefix(&self.session_prefix(session_id)?);
            Ok(())
        }
    }
}

pub use volume_store::VolumeRecoveryJournalStore;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// A store that survives dropping the app, the way a disk survives
    /// `kill -9`.
    #[derive(Clone, Debug, Default)]
    struct MemoryStore(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);

    impl MemoryStore {
        fn segments(&self) -> BTreeMap<String, Vec<u8>> {
            self.0.lock().expect("store lock").clone()
        }

        fn only_segment_id(&self) -> String {
            let segments = self.segments();
            assert_eq!(segments.len(), 1, "expected exactly one recovery segment");
            segments.keys().next().expect("segment id").clone()
        }

        fn overwrite(&self, session_id: &str, bytes: Vec<u8>) {
            self.0
                .lock()
                .expect("store lock")
                .insert(session_id.to_string(), bytes);
        }
    }

    impl RecoveryJournalStore for MemoryStore {
        fn write_segment(&self, session_id: &str, bytes: &[u8]) -> Result<(), String> {
            validate_session_id(session_id)?;
            self.0
                .lock()
                .map_err(|_| "poisoned".to_string())?
                .insert(session_id.to_string(), bytes.to_vec());
            Ok(())
        }

        fn append_segment(&self, session_id: &str, bytes: &[u8]) -> Result<(), String> {
            validate_session_id(session_id)?;
            let mut guard = self.0.lock().map_err(|_| "poisoned".to_string())?;
            let segment = guard
                .get_mut(session_id)
                .ok_or_else(|| format!("recovery segment {session_id} is missing"))?;
            segment.extend_from_slice(bytes);
            Ok(())
        }

        fn list_segments(&self) -> Result<Vec<String>, String> {
            Ok(self
                .0
                .lock()
                .map_err(|_| "poisoned".to_string())?
                .keys()
                .cloned()
                .collect())
        }

        fn read_segment(&self, session_id: &str) -> Result<Option<Vec<u8>>, String> {
            validate_session_id(session_id)?;
            Ok(self
                .0
                .lock()
                .map_err(|_| "poisoned".to_string())?
                .get(session_id)
                .cloned())
        }

        fn remove_segment(&self, session_id: &str) -> Result<(), String> {
            validate_session_id(session_id)?;
            self.0
                .lock()
                .map_err(|_| "poisoned".to_string())?
                .remove(session_id);
            Ok(())
        }
    }

    const TEST_ED25519_PRIVATE_KEY: &str = r#"
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYgAAAJgAIAxdACAM
XQAAAAtzc2gtZWQyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYg
AAAEC2BsIi0QwW2uFscKTUUXNHLsYX4FxlaSDSblbAj7WR7bM+rvN+ot98qgEN796jTiQf
ZfG1KaT0PtFDJ/XFSqtiAAAAEHVzZXJAZXhhbXBsZS5jb20BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----
"#;

    /// An app journalling into `store`, with an empty document and three
    /// unsaved paragraphs. Everything goes through `dispatch_command`, which
    /// is the only hook the recovery journal has.
    fn crashed_session(store: &MemoryStore) -> AppDocument {
        let mut app = OpenDocApp::new_sample();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        app.dispatch_command("create_document", json!({ "title": "Crash test" }))
            .expect("create");
        assert!(
            store.segments().is_empty(),
            "a clean document leaves nothing to recover"
        );
        for text in ["alpha", "beta", "gamma"] {
            app.dispatch_command("add_paragraph", json!({ "text": text }))
                .expect("paragraph");
        }
        let document = app.document();
        assert!(document.has_unsaved_changes);
        document
        // `app` is dropped here: the process died, the store did not.
    }

    fn reopened(store: &MemoryStore) -> OpenDocApp {
        let mut app = OpenDocApp::new_sample();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        app
    }

    #[test]
    fn a_killed_session_replays_to_the_same_document_and_still_signs() {
        let store = MemoryStore::default();
        let crashed = crashed_session(&store);

        let mut app = reopened(&store);
        let offered = app.document();
        assert_eq!(offered.recovery_sessions.len(), 1);
        let session = &offered.recovery_sessions[0];
        assert_eq!(session.title, "Crash test");
        // Every unsaved change is named, including the one folded into the
        // segment's base snapshot.
        assert_eq!(session.operation_count, 3);
        assert!(!session.truncated);
        // Startup offers, it does not replay.
        assert_ne!(app.document().title, "Crash test");

        let session_id = session.id.clone();
        let recovered = app.recover_session(&session_id).expect("replay");
        assert_eq!(recovered.title, "Crash test");
        assert_eq!(recovered.visible_text, crashed.visible_text);
        assert_eq!(recovered.blocks.len(), crashed.blocks.len());
        assert!(recovered.has_unsaved_changes);
        assert!(recovered.recovery_sessions.is_empty());
        assert!(
            store.segments().is_empty(),
            "a replayed segment stops being an offer"
        );

        // Replayed state is ordinary state: it validates and it signs.
        app.snapshot_document()
            .validate_source()
            .expect("recovered source validates");
        let signed = app
            .sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Tester")
            .expect("recovered document signs");
        assert_eq!(signed.signature_state, "signed");
        assert_eq!(
            app.verify_current_signatures().expect("verify"),
            "signed".to_string()
        );
    }

    #[test]
    fn declining_recovery_keeps_the_segment_until_it_is_discarded() {
        let store = MemoryStore::default();
        crashed_session(&store);
        let session_id = store.only_segment_id();

        // Decline: open the app, look, and do nothing.
        let app = reopened(&store);
        assert_eq!(app.document().recovery_sessions.len(), 1);
        drop(app);
        assert_eq!(store.segments().len(), 1, "declining keeps the work");

        // Decline again, then discard deliberately.
        let mut app = reopened(&store);
        let after = app.discard_recovery_session(&session_id).expect("discard");
        assert!(after.recovery_sessions.is_empty());
        assert!(store.segments().is_empty());
        assert!(
            app.discard_recovery_session(&session_id).is_err(),
            "discarding twice is a not-found, not a silent success"
        );
    }

    #[test]
    fn a_torn_final_frame_is_dropped_and_reported() {
        let store = MemoryStore::default();
        crashed_session(&store);
        let session_id = store.only_segment_id();
        let bytes = store
            .read_segment(&session_id)
            .expect("read")
            .expect("segment");
        // The crash landed in the middle of writing the last envelope.
        store.overwrite(&session_id, bytes[..bytes.len() - 7].to_vec());

        let mut app = reopened(&store);
        let offered = app.document();
        let session = &offered.recovery_sessions[0];
        assert!(session.truncated);
        assert_eq!(session.operation_count, 2, "the torn change is not offered");

        let recovered = app.recover_session(session.id.clone()).expect("replay");
        assert!(recovered.visible_text.contains("beta"));
        assert!(!recovered.visible_text.contains("gamma"));
        assert!(recovered
            .warnings
            .iter()
            .any(|warning| warning.code == "recovery-journal-truncated"));
    }

    #[test]
    fn undo_rewinds_the_segment_so_recovery_cannot_resurrect_undone_work() {
        let store = MemoryStore::default();
        let mut app = OpenDocApp::new_sample();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        app.dispatch_command("create_document", json!({ "title": "Undo test" }))
            .expect("create");
        for text in ["kept", "undone"] {
            app.dispatch_command("add_paragraph", json!({ "text": text }))
                .expect("paragraph");
        }
        app.dispatch_command("undo_current_edit", json!({}))
            .expect("undo");
        let expected = app.document();
        assert!(expected.visible_text.contains("kept"));
        assert!(!expected.visible_text.contains("undone"));
        drop(app);

        let mut app = reopened(&store);
        let session_id = app.document().recovery_sessions[0].id.clone();
        let recovered = app.recover_session(&session_id).expect("replay");
        assert!(recovered.visible_text.contains("kept"));
        assert!(
            !recovered.visible_text.contains("undone"),
            "the recovery segment must not replay work the user undid"
        );
    }

    #[test]
    fn saving_clears_the_recovery_segment() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-recovery-save-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let store = MemoryStore::default();
        let mut app = OpenDocApp::new_sample();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        app.dispatch_command("create_document", json!({ "title": "Save test" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "durable" }))
            .expect("paragraph");
        assert_eq!(store.segments().len(), 1);

        app.dispatch_command(
            "save_local_repository",
            json!({ "path": root.to_string_lossy() }),
        )
        .expect("save");
        assert!(
            store.segments().is_empty(),
            "the repository holds it now; there is nothing to recover"
        );

        // Editing after the save starts a fresh segment based on the commit.
        app.dispatch_command("add_paragraph", json!({ "text": "after the save" }))
            .expect("paragraph");
        let segments = store.segments();
        assert_eq!(segments.len(), 1);
        let segment = decode_segment(segments.values().next().expect("segment")).expect("decode");
        assert!(segment.header.base_manifest.is_some());
        assert!(segment.header.base.visible_text.contains("durable"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn without_a_store_nothing_is_journalled() {
        let mut app = OpenDocApp::new_sample();
        app.dispatch_command("create_document", json!({ "title": "Browser" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "unprotected" }))
            .expect("paragraph");
        assert!(app.document().recovery_sessions.is_empty());
    }

    #[test]
    fn a_session_id_cannot_escape_the_store() {
        assert!(validate_session_id("recovery-0123abcd").is_ok());
        for bad in ["", "../escape", "a/b", "with space", "sub\\dir"] {
            assert!(validate_session_id(bad).is_err(), "{bad} must be refused");
        }
        let store = MemoryStore::default();
        let mut app = reopened(&store);
        assert!(app.recover_session("../../etc/passwd").is_err());
        assert!(app.discard_recovery_session("../../etc/passwd").is_err());
    }
}

// ---- Browser journal, minus the IndexedDB binding --------------------------

#[cfg(test)]
mod volume_journal_tests {
    //! `VolumeRecoveryJournalStore` is what gives the browser the crash
    //! protection the Tauri shell has. IndexedDB itself cannot be reached
    //! from `cargo test`, but everything above it can: the frame layout, the
    //! segment round trip, and the fact that a segment replays after the
    //! volume has been through a durable round trip.

    use super::*;
    use opendoc_store::MirroredVolume;
    use serde_json::json;
    use std::collections::BTreeMap;

    /// Stands in for IndexedDB: a map that changes only when the driver
    /// flushes a batch, and that outlives the volume it was flushed from.
    #[derive(Debug, Default)]
    struct DurableBytes(BTreeMap<String, Vec<u8>>);

    impl DurableBytes {
        fn flush(&mut self, volume: &MirroredVolume) {
            let pending = volume.pending();
            let Some(through) = pending.last().map(|mutation| mutation.seq) else {
                return;
            };
            for mutation in pending {
                match mutation.value {
                    Some(bytes) => {
                        self.0.insert(mutation.key, bytes);
                    }
                    None => {
                        self.0.remove(&mutation.key);
                    }
                }
            }
            volume.acknowledge(through);
        }

        fn reopen(&self) -> MirroredVolume {
            let volume = MirroredVolume::new();
            volume.hydrate(self.0.clone());
            volume
        }
    }

    fn store_over(volume: &MirroredVolume) -> Arc<VolumeRecoveryJournalStore> {
        Arc::new(VolumeRecoveryJournalStore::new(volume.clone(), "recovery"))
    }

    fn segment_bytes(operations: usize) -> Vec<u8> {
        let mut bytes = RECOVERY_SEGMENT_MAGIC.to_vec();
        for index in 0..operations {
            bytes.extend_from_slice(&(4u32).to_le_bytes());
            bytes.extend_from_slice(&(index as u32).to_le_bytes());
        }
        bytes
    }

    #[test]
    fn frames_round_trip_as_one_segment_in_key_order() {
        let volume = MirroredVolume::new();
        let store = store_over(&volume);
        store
            .write_segment("session-one", &segment_bytes(0))
            .expect("write");
        // Twelve appends, so the frame keys cross the width where a
        // non-padded index would sort "10" before "2".
        let mut expected = segment_bytes(0);
        for index in 0..12u32 {
            let mut frame = (4u32).to_le_bytes().to_vec();
            frame.extend_from_slice(&index.to_le_bytes());
            store.append_segment("session-one", &frame).expect("append");
            expected.extend_from_slice(&frame);
        }
        assert_eq!(
            store.read_segment("session-one").expect("read"),
            Some(expected)
        );
        assert_eq!(store.list_segments().expect("list"), vec!["session-one"]);
    }

    #[test]
    fn an_append_never_rewrites_the_frames_already_written() {
        // Why frames are separate keys: a segment's first frame carries a
        // whole document snapshot, and `append_segment` runs once per
        // gesture. A single-value segment would rewrite that snapshot into
        // IndexedDB on every keystroke.
        let volume = MirroredVolume::new();
        let store = store_over(&volume);
        let header = segment_bytes(0);
        store.write_segment("session-one", &header).expect("write");
        volume.acknowledge(volume.sequence());

        store
            .append_segment("session-one", b"tail")
            .expect("append");
        let pending = volume.pending();
        assert_eq!(pending.len(), 1, "an append queued {pending:?}");
        assert_eq!(pending[0].value.as_deref(), Some(&b"tail"[..]));
        assert!(
            pending[0].value.as_ref().expect("value").len() < header.len(),
            "the append must not carry the header's bytes again"
        );
    }

    #[test]
    fn segments_are_separate_and_removable() {
        let volume = MirroredVolume::new();
        let store = store_over(&volume);
        store.write_segment("aa", &segment_bytes(1)).expect("write");
        store
            .write_segment("aabb", &segment_bytes(2))
            .expect("write");
        assert_eq!(store.list_segments().expect("list"), vec!["aa", "aabb"]);

        store.remove_segment("aa").expect("remove");
        assert_eq!(store.list_segments().expect("list"), vec!["aabb"]);
        assert_eq!(store.read_segment("aa").expect("read"), None);
        assert!(store.read_segment("aabb").expect("read").is_some());

        // Reusing an id must not inherit the old tail.
        store
            .write_segment("aabb", &segment_bytes(0))
            .expect("write");
        assert_eq!(
            store.read_segment("aabb").expect("read"),
            Some(segment_bytes(0))
        );
    }

    #[test]
    fn the_store_refuses_what_the_file_store_refuses() {
        let volume = MirroredVolume::new();
        let store = store_over(&volume);
        assert!(store.append_segment("missing", b"tail").is_err());
        for bad in ["../escape", "a/b", "with space"] {
            assert!(store.write_segment(bad, b"x").is_err(), "{bad}");
            assert!(store.read_segment(bad).is_err(), "{bad}");
            assert!(store.remove_segment(bad).is_err(), "{bad}");
        }
        volume.put("recovery/notasegment/0000000000", b"not opendoc");
        assert!(store.read_segment("notasegment").is_err());
    }

    #[test]
    fn a_killed_browser_session_replays_from_the_durable_bytes() {
        let mut durable = DurableBytes::default();
        let crashed;
        {
            let volume = MirroredVolume::new();
            let mut app = OpenDocApp::new_sample();
            app.install_recovery_journal(store_over(&volume))
                .expect("journal installs");
            app.dispatch_command("create_document", json!({ "title": "Browser crash" }))
                .expect("create");
            for text in ["alpha", "beta", "gamma"] {
                app.dispatch_command("add_paragraph", json!({ "text": text }))
                    .expect("paragraph");
                // The driver flushes after every dispatched command.
                durable.flush(&volume);
            }
            crashed = app.document();
            assert!(crashed.has_unsaved_changes);
            // The tab is killed here; only the flushed bytes survive.
        }
        assert!(
            durable.0.keys().any(|key| key.starts_with("recovery/")),
            "nothing was journalled: {:?}",
            durable.0.keys().collect::<Vec<_>>()
        );

        let volume = durable.reopen();
        let mut app = OpenDocApp::new_empty_document();
        let offered = app
            .install_recovery_journal(store_over(&volume))
            .expect("journal installs");
        assert_eq!(offered.recovery_sessions.len(), 1);
        let session = &offered.recovery_sessions[0];
        assert_eq!(session.title, "Browser crash");
        assert_eq!(session.operation_count, 3);
        assert!(!session.truncated);

        let recovered = app.recover_session(session.id.clone()).expect("replay");
        assert_eq!(recovered.title, "Browser crash");
        assert_eq!(recovered.visible_text, crashed.visible_text);
        assert!(recovered.has_unsaved_changes);
    }

    #[test]
    fn a_crash_before_the_flush_loses_only_the_unflushed_tail() {
        // The honest cost of write-behind, stated as a test: the gesture that
        // never reached a transaction is gone, and the ones before it are not.
        let mut durable = DurableBytes::default();
        {
            let volume = MirroredVolume::new();
            let mut app = OpenDocApp::new_sample();
            app.install_recovery_journal(store_over(&volume))
                .expect("journal installs");
            app.dispatch_command("create_document", json!({ "title": "Partial" }))
                .expect("create");
            app.dispatch_command("add_paragraph", json!({ "text": "flushed" }))
                .expect("paragraph");
            durable.flush(&volume);
            app.dispatch_command("add_paragraph", json!({ "text": "unflushed" }))
                .expect("paragraph");
            assert!(volume.pending_len() > 0, "the tail must still be pending");
        }

        let volume = durable.reopen();
        let mut app = OpenDocApp::new_empty_document();
        let offered = app
            .install_recovery_journal(store_over(&volume))
            .expect("journal installs");
        assert_eq!(offered.recovery_sessions.len(), 1);
        let recovered = app
            .recover_session(offered.recovery_sessions[0].id.clone())
            .expect("replay");
        assert!(recovered.visible_text.contains("flushed"));
        assert!(!recovered.visible_text.contains("unflushed"));
    }

    #[test]
    fn closing_the_document_clears_the_segment_from_durable_storage() {
        // A closed or clean document has nothing to recover, so the next page
        // must not be offered a stale segment — and the deletion has to be
        // mirrored, not only applied in memory.
        let mut durable = DurableBytes::default();
        let volume = MirroredVolume::new();
        let mut app = OpenDocApp::new_sample();
        app.install_recovery_journal(store_over(&volume))
            .expect("journal installs");
        app.dispatch_command("create_document", json!({ "title": "Closed" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "work" }))
            .expect("paragraph");
        durable.flush(&volume);
        assert!(durable.0.keys().any(|key| key.starts_with("recovery/")));

        app.dispatch_command("close_document", json!({ "discardUnsavedChanges": true }))
            .expect("close");
        durable.flush(&volume);
        assert!(
            !durable.0.keys().any(|key| key.starts_with("recovery/")),
            "durable segment survived closing the document: {:?}",
            durable.0.keys().collect::<Vec<_>>()
        );
    }
}
