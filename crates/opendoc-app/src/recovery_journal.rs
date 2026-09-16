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
/// Format of the segment header itself.
///
/// `v2` is `v1` plus the two things a `v1` header could not say about itself:
/// which record shape its `base` snapshot is
/// ([`RecoverySegmentHeader::base_format`]) and which document signatures the
/// crash caught unsaved ([`RecoverySegmentHeader::signatures`]). The frame
/// layout did not change, so [`RECOVERY_SEGMENT_MAGIC`] did not either.
pub(crate) const RECOVERY_SEGMENT_FORMAT: &str = "opendoc.recovery-segment.v2";
/// The pre-`base_format` format, still readable.
///
/// `v1` carries the numbering watermarks but embeds a whole `AppDocument` as
/// `base` without recording which `app-document` format that is, so one
/// `recovery-segment.v1` file can carry either shape and nothing inside the
/// segment can tell them apart. Read, and reported, rather than guessed at
/// silently.
pub(crate) const RECOVERY_SEGMENT_FORMAT_V1: &str = "opendoc.recovery-segment.v1";
/// The pre-split format, still readable.
///
/// In `v0` a single counter numbered envelopes and operations alike, so a
/// `v0` segment does not carry the watermarks — but it does not need to be
/// guessed at either: its shared numbering *is* both of them.
pub(crate) const RECOVERY_SEGMENT_FORMAT_V0: &str = "opendoc.recovery-segment.v0";

/// Warning code for a recovery offer that carries a signature the crash caught
/// before it was saved.
///
/// `AppRecoverySession` counts *operations*, and signing is not one, so a
/// session whose only unsaved work is a signature is offered as "0 changes".
/// Discarding it is then presented as throwing away nothing, when it in fact
/// destroys a signature. See `refresh_recovery_sessions`.
pub(crate) const UNSAVED_SIGNATURE_WARNING: &str = "recovery-journal-unsaved-signature";

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
    /// Which record shape [`RecoverySegmentHeader::base`] is — whatever
    /// `APP_DOCUMENT_FORMAT` says today, the same string a repository snapshot
    /// declares. An earlier format this build still reads is replayed and
    /// reported; one it does not is refused by name.
    ///
    /// A `v1` header embedded the whole `AppDocument` and said nothing about
    /// its format, so the next `AppDocument` bump would have been undetectable
    /// from inside a segment: the CBOR would decode into whatever fields
    /// happened to line up and the replay would produce a document nobody
    /// wrote. Declaring it is what makes that a refusal instead. Empty in a
    /// `v0`/`v1` segment, which is reported rather than assumed away.
    #[serde(default)]
    pub base_format: String,
    /// Document signatures held in memory when the base snapshot was taken.
    ///
    /// Signing is not a typed operation and it makes the document dirty, so a
    /// signature is exactly the kind of unsaved work this journal exists for —
    /// and it lived nowhere in a `v1` segment, so `recover_session` could only
    /// `clear()` it. A crash after signing and before saving silently lost the
    /// signature and offered the user an `unsigned` document with no warning.
    #[serde(default)]
    pub signatures: Vec<opendoc_format::SignatureRecord>,
    /// The envelope counter as it stood when this segment's base snapshot was
    /// taken.
    ///
    /// ADR 0005's invariant is that a segment replays to *exactly* the state
    /// it shadows, and the numbering watermarks are part of that state: a
    /// replica that resumed below one would re-mint an identity the repository
    /// already holds. They cannot be reconstructed from the segment's frames
    /// alone, because the frames start at the base snapshot and everything
    /// issued before it is behind that snapshot, not in front of it.
    ///
    /// Absent (zero) in a `v0` segment; see [`RECOVERY_SEGMENT_FORMAT_V0`].
    #[serde(default)]
    pub next_envelope_seq: u64,
    /// The document-operation counter at the same moment. See
    /// [`RecoverySegmentHeader::next_envelope_seq`].
    #[serde(default)]
    pub next_operation_seq: u64,
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

/// The numbering watermarks a recovered session continues from.
///
/// Both are a **maximum**, never a count: the segment's frames are the tail of
/// a journal whose head is behind the base snapshot, so counting what is
/// visible would re-mint identities the repository already holds.
///
/// A `v1` segment carries the watermarks as they stood when its base snapshot
/// was taken, and the appended frames can only have pushed them higher, so the
/// answer is the larger of the two. A `v0` segment predates the split and
/// carries neither — but nothing is guessed: in `v0` one counter numbered
/// envelopes and operations alike, so this actor's highest envelope number is
/// exactly what both counters stood at. That is an equality, not an
/// approximation, and it is still reported, because a segment written by an
/// older build is a fact the user is entitled to see rather than something to
/// read silently.
struct RecoveredSequenceCounters {
    next_envelope_seq: u64,
    next_operation_seq: u64,
    warning: Option<String>,
}

fn recovered_sequence_counters(
    segment: &RecoverySegment,
    actor_id: &str,
) -> RecoveredSequenceCounters {
    let highest_envelope = segment
        .operations
        .iter()
        .filter(|envelope| envelope.record.actor == actor_id)
        .map(|envelope| envelope.record.seq)
        .max()
        .unwrap_or(0);
    if segment.header.format == RECOVERY_SEGMENT_FORMAT_V0 {
        let next = highest_envelope + 1;
        return RecoveredSequenceCounters {
            next_envelope_seq: next,
            next_operation_seq: next,
            warning: Some(format!(
                "recovery segment {} was written before envelope and operation numbering were separated; both continue from sequence {next}, which is what its single counter stood at",
                segment.header.session_id
            )),
        };
    }
    let highest_operation = segment
        .operations
        .iter()
        .filter_map(|envelope| envelope.operation.as_ref())
        .filter(|operation| operation.id.actor.0 == actor_id)
        .map(|operation| operation.id.seq)
        .max()
        .unwrap_or(0);
    RecoveredSequenceCounters {
        next_envelope_seq: segment
            .header
            .next_envelope_seq
            .max(highest_envelope + 1)
            .max(1),
        next_operation_seq: segment
            .header
            .next_operation_seq
            .max(highest_operation + 1)
            .max(1),
        warning: None,
    }
}

pub(crate) fn decode_segment(bytes: &[u8]) -> Result<RecoverySegment, AppApiError> {
    let (frames, truncated) = split_frames(bytes)?;
    let mut frames = frames.into_iter();
    let header_bytes = frames
        .next()
        .ok_or_else(|| AppApiError::Format("recovery segment has no header frame".to_string()))?;
    let header: RecoverySegmentHeader = decode_cbor(header_bytes)
        .map_err(|err| AppApiError::Format(format!("recovery segment header: {err}")))?;
    if header.format != RECOVERY_SEGMENT_FORMAT
        && header.format != RECOVERY_SEGMENT_FORMAT_V1
        && header.format != RECOVERY_SEGMENT_FORMAT_V0
    {
        return Err(AppApiError::Format(format!(
            "unsupported recovery segment format {}",
            header.format
        )));
    }
    // A `v2` header says what shape its base snapshot is, so a segment written
    // against a different `app-document` format is refused by name instead of
    // being decoded into whichever fields happen to line up. `v0`/`v1` headers
    // carry no such declaration; that is reported at replay
    // (`recovery-journal-undeclared-base-format`), not guessed at here.
    //
    // An *earlier* app-document format this build still reads is not one of
    // those cases. Refusing it would throw away the unsaved work a crash
    // caught for no better reason than that this crate's encoder changed
    // between the crash and the restart — which is precisely when a recovery
    // segment matters most. It is replayed and reported instead
    // (`recovery-journal-earlier-base-format`).
    if !header.base_format.is_empty()
        && header.base_format != crate::APP_DOCUMENT_FORMAT
        && !crate::is_superseded_app_document_format(&header.base_format)
    {
        return Err(AppApiError::Format(format!(
            "recovery segment {} carries a {} base snapshot, which this build cannot replay",
            header.session_id, header.base_format
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
        // Owned here rather than accumulated, because this warning describes
        // exactly the set of segments currently on offer: recovering or
        // discarding one has to take its warning with it, and the offers are
        // rebuilt from the store on every call anyway.
        self.document
            .warnings
            .retain(|warning| warning.code != UNSAVED_SIGNATURE_WARNING);
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
                    // Signing is not a typed operation, so a crash that caught
                    // a signature and nothing else is offered as a session
                    // with *zero* changes — and the user is asked whether to
                    // throw away "nothing" when a signature is what is at
                    // stake. The offer DTO has no field for this, so it is
                    // said in the one channel that reaches the user without a
                    // contract change.
                    let unsaved_signatures = segment.header.signatures.len();
                    let operation_count = operations.len();
                    sessions.push(AppRecoverySession {
                        id: id.clone(),
                        document_uuid: segment.header.document_uuid.clone(),
                        title: segment.header.title.clone(),
                        started_at_ms: segment.header.started_at_ms,
                        operation_count,
                        repository_root: segment.header.repository_root.clone(),
                        repository_backend: segment.header.repository_backend.clone(),
                        base_manifest: segment.header.base_manifest.clone(),
                        operations,
                        truncated: segment.truncated,
                    });
                    if unsaved_signatures > 0 {
                        self.push_model_warning(
                            UNSAVED_SIGNATURE_WARNING,
                            format!(
                                "recovery session {id} holds {unsaved_signatures} signature(s) that never reached the repository; they are not among its {operation_count} recorded change(s), so recovering it brings them back and discarding it destroys them"
                            ),
                        );
                    }
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
            // Drop the cursor. It names a segment this process could not
            // write — most likely because another runtime over the same store
            // was offered this process's *live* journal as a crash and
            // discarded it, which deletes the file `append_segment` opens.
            // Without this the cursor stayed, every later sync re-tried the
            // same failing append, and `push_model_warning` deduped the
            // warning after the first one: crash protection was gone for the
            // rest of the session, permanently and silently. Clearing it makes
            // the next dispatch re-snapshot under a fresh session id, so the
            // failure heals itself.
            //
            // This is not the fix for two runtimes sharing a store — that is
            // leader election, PLAN77 F2, and ADR 0005/0008 both record "one
            // session at a time" as a known limitation. It is the difference
            // between a limitation and a permanent, non-self-healing failure.
            self.recovery.cursor = None;
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
            base_format: crate::APP_DOCUMENT_FORMAT.to_string(),
            // Signing makes the document dirty, so a segment exists precisely
            // when a just-minted signature has not reached the repository yet.
            // ADR 0005's invariant is that a segment replays to exactly the
            // state it shadows, and a signature is part of that state.
            signatures: self.signatures.clone(),
            next_envelope_seq: self.next_envelope_seq,
            next_operation_seq: self.next_operation_seq,
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

        let counters = recovered_sequence_counters(&segment, &self.actor_id);
        self.document = replayed.document;
        self.workbook = workbook.evaluated();
        self.blobs = blobs;
        self.blob_bytes.clear();
        self.blob_signatures.clear();
        self.blob_tombstones.clear();
        self.blob_tombstone_records.clear();
        // Carried, not cleared. A segment exists only while the document
        // differs from the repository, and signing is one of the things that
        // makes it differ — so a signature found in a segment is by
        // construction one the crash caught before it was saved. Clearing it
        // here handed the user a document that said `unsigned`, with
        // `warnings: []`, and nothing anywhere to say a signature had been
        // thrown away.
        self.signatures = segment.header.signatures.clone();
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
        self.next_envelope_seq = counters.next_envelope_seq;
        self.next_operation_seq = counters.next_operation_seq;
        self.repository_root = segment.header.repository_root.clone().map(Into::into);
        self.repository_backend = segment.header.repository_backend.clone();
        self.repository_namespace = segment.header.repository_namespace.clone();
        self.last_manifest = segment.header.base_manifest.clone();

        if let Some(message) = counters.warning {
            self.push_model_warning("recovery-journal-legacy-numbering", message);
        }
        if crate::is_superseded_app_document_format(&segment.header.base_format) {
            self.push_model_warning(
                "recovery-journal-earlier-base-format",
                format!(
                    "recovery segment {session_id} holds a {} base snapshot, which this build reads but no longer writes; it was replayed as {}",
                    segment.header.base_format,
                    crate::APP_DOCUMENT_FORMAT
                ),
            );
        }
        if segment.header.base_format.is_empty() {
            self.push_model_warning(
                "recovery-journal-undeclared-base-format",
                format!(
                    "recovery segment {session_id} does not record which snapshot format its base state is; it was replayed as {}",
                    crate::APP_DOCUMENT_FORMAT
                ),
            );
        }
        if !self.signatures.is_empty() && !self.document_signatures_cover_current_state() {
            self.push_model_warning(
                "recovery-journal-broken-signature",
                "a signature recovered from the crash journal does not cover the recovered state; sign again before saving",
            );
        }
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

    /// The segment replays to *exactly* the in-memory state, and the numbering
    /// watermarks are part of that state.
    ///
    /// The case that proves it: a session whose only unsaved envelope is a
    /// blob upload, from an actor whose earlier operations are behind the base
    /// snapshot. Reconstructing the counters from the segment's frames would
    /// find no operation at all and restart at 1 — re-minting ids the
    /// repository already holds. The header's watermark is what makes it
    /// impossible.
    #[test]
    fn a_recovered_session_continues_the_numbering_the_header_recorded() {
        let store = MemoryStore::default();
        {
            let mut app = OpenDocApp::new_empty_document();
            app.install_recovery_journal(Arc::new(store.clone()))
                .expect("journal installs");
            app.dispatch_command("create_document", json!({ "title": "Watermarks" }))
                .expect("create");
            for text in ["alpha", "beta"] {
                app.dispatch_command("add_paragraph", json!({ "text": text }))
                    .expect("paragraph");
            }
            // Pretend the repository already holds everything so far, then add
            // one envelope that carries no operation. The segment that follows
            // starts from a snapshot and holds only that envelope.
            app.saved_operation_count = app.operation_envelopes.len();
            app.dispatch_command(
                "add_binary_blob",
                json!({ "name": "n.txt", "mediaType": "text/plain", "bytes": [1] }),
            )
            .expect("blob");
            assert_eq!(app.next_operation_seq, 3);
        }

        let session_id = store.only_segment_id();
        let segment = decode_segment(&store.read_segment(&session_id).unwrap().unwrap())
            .expect("the segment decodes");
        assert!(
            !segment
                .operations
                .iter()
                .any(|envelope| envelope.operation.is_some()),
            "the segment carries no typed operation, which is the point"
        );

        let mut app = OpenDocApp::new_empty_document();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        app.recover_session(&session_id).expect("recovery");
        assert_eq!(
            app.next_operation_seq, 3,
            "the operation counter must not restart under the repository's own history"
        );
        assert!(app.next_envelope_seq >= 4);
        assert!(
            !app.document()
                .warnings
                .iter()
                .any(|warning| warning.code == "recovery-journal-legacy-numbering"),
            "a segment this build wrote is not a legacy segment"
        );
    }

    /// A segment written before the counters were split still replays, and it
    /// is not read silently.
    ///
    /// Nothing is guessed: in that format one counter numbered envelopes and
    /// operations alike, so the shared numbering *is* both watermarks,
    /// exactly. The warning is there because a file written by an older build
    /// is a fact the user is entitled to see.
    #[test]
    fn a_pre_split_recovery_segment_replays_and_says_which_format_it_was() {
        let store = MemoryStore::default();
        let session_id = {
            let mut app = OpenDocApp::new_empty_document();
            app.install_recovery_journal(Arc::new(store.clone()))
                .expect("journal installs");
            app.dispatch_command("create_document", json!({ "title": "Legacy" }))
                .expect("create");
            app.saved_operation_count = app.operation_envelopes.len();
            for text in ["alpha", "beta"] {
                app.dispatch_command("add_paragraph", json!({ "text": text }))
                    .expect("paragraph");
            }
            store.only_segment_id()
        };

        // Rewrite the segment the way the previous format wrote it: one
        // counter, so every envelope's record number is also its operation's,
        // and the header carries no watermark at all.
        let bytes = store.read_segment(&session_id).unwrap().unwrap();
        let segment = decode_segment(&bytes).expect("the segment decodes");
        let mut header = segment.header.clone();
        header.format = RECOVERY_SEGMENT_FORMAT_V0.to_string();
        header.next_envelope_seq = 0;
        header.next_operation_seq = 0;
        let mut legacy = encode_segment_header(&header).expect("header");
        let mut rewritten = Vec::new();
        let mut highest = 0u64;
        for envelope in &segment.operations {
            let mut envelope = envelope.clone();
            if let Some(operation) = envelope.operation.as_mut() {
                operation.id.seq = envelope.record.seq;
            }
            highest = highest.max(envelope.record.seq);
            legacy.extend_from_slice(&encode_operation_frame(&envelope).expect("frame"));
            rewritten.push(envelope);
        }
        store.overwrite(&session_id, legacy);

        let mut app = OpenDocApp::new_empty_document();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        let document = app.recover_session(&session_id).expect("recovery");
        assert_eq!(
            ["alpha", "beta"]
                .into_iter()
                .filter(|text| document.visible_text().contains(text))
                .count(),
            2,
            "a pre-split segment still replays its work"
        );
        let warning = document
            .warnings
            .iter()
            .find(|warning| warning.code == "recovery-journal-legacy-numbering")
            .expect("the older format is named, not read silently");
        assert!(warning.message.contains(&session_id), "{warning:?}");

        // And the watermarks a replica with that segment's own actor would
        // continue from are exactly what the single counter stood at. (A
        // recovering process mints its own actor id, so it numbers from 1
        // under a name nothing else has used; this is the case where it does
        // not.)
        let legacy_segment = RecoverySegment {
            header: header.clone(),
            operations: rewritten,
            truncated: false,
        };
        let counters = recovered_sequence_counters(&legacy_segment, &header.actor);
        assert_eq!(counters.next_envelope_seq, highest + 1);
        assert_eq!(counters.next_operation_seq, highest + 1);
        assert!(counters.warning.is_some());

        // The same segment under this build's format keeps the two apart.
        let mut modern = legacy_segment.clone();
        modern.header.format = RECOVERY_SEGMENT_FORMAT.to_string();
        modern.header.next_envelope_seq = 9;
        modern.header.next_operation_seq = 4;
        let counters = recovered_sequence_counters(&modern, &header.actor);
        assert_eq!(
            counters.next_envelope_seq,
            9.max(highest + 1),
            "the header's watermark is a floor, never a ceiling"
        );
        assert_eq!(counters.next_operation_seq, 4.max(highest + 1));
        assert!(counters.warning.is_none());
    }

    /// A segment in a format this build does not know is refused, not guessed
    /// at.
    #[test]
    fn an_unknown_recovery_segment_format_is_refused() {
        let store = MemoryStore::default();
        let session_id = {
            let mut app = OpenDocApp::new_empty_document();
            app.install_recovery_journal(Arc::new(store.clone()))
                .expect("journal installs");
            app.dispatch_command("create_document", json!({ "title": "Future" }))
                .expect("create");
            app.dispatch_command("add_paragraph", json!({ "text": "alpha" }))
                .expect("paragraph");
            store.only_segment_id()
        };
        let segment = decode_segment(&store.read_segment(&session_id).unwrap().unwrap())
            .expect("the segment decodes");
        let mut header = segment.header.clone();
        header.format = "opendoc.recovery-segment.v99".to_string();
        store.overwrite(&session_id, encode_segment_header(&header).expect("header"));

        let mut app = OpenDocApp::new_empty_document();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        let error = app
            .recover_session(&session_id)
            .expect_err("an unknown format is refused");
        assert!(
            matches!(&error, AppApiError::Format(message) if message.contains("v99")),
            "{error:?}"
        );
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
        assert_eq!(recovered.visible_text(), crashed.visible_text());
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

    /// A signature caught by the crash comes back with the work it covers.
    ///
    /// Signing is not a typed operation and it makes the document dirty, so a
    /// segment exists precisely when a just-minted signature has not reached
    /// the repository yet. `recover_session` used to `clear()` the signature
    /// list and the header had nowhere to carry one, so the user was handed an
    /// `unsigned` document with `warnings: []` — nothing anywhere said a
    /// signature had been thrown away.
    #[test]
    fn a_signature_caught_by_the_crash_is_recovered_with_the_document() {
        let store = MemoryStore::default();
        let signed_target = {
            let mut app = OpenDocApp::new_empty_document();
            app.install_recovery_journal(Arc::new(store.clone()))
                .expect("journal installs");
            app.dispatch_command("create_document", json!({ "title": "Signed crash" }))
                .expect("create");
            app.dispatch_command("add_paragraph", json!({ "text": "the signed prose" }))
                .expect("paragraph");
            // Pretend the repository holds everything so far, so the signature
            // is the only unsaved change left.
            app.saved_operation_count = app.operation_envelopes.len();
            app.sync_recovery_journal();
            let signed = app
                .sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Tester")
                .expect("sign");
            assert_eq!(signed.signature_state, "signed");
            assert!(signed.has_unsaved_changes);
            app.sync_recovery_journal();
            app.signatures[0].target.clone()
            // The process dies here. The signature was never saved.
        };

        let session_id = store.only_segment_id();
        let segment = decode_segment(&store.read_segment(&session_id).unwrap().unwrap())
            .expect("the segment decodes");
        assert_eq!(
            segment.header.signatures.len(),
            1,
            "the segment carries the unsaved signature"
        );

        let mut app = OpenDocApp::new_empty_document();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        let recovered = app.recover_session(&session_id).expect("replay");
        assert_eq!(
            recovered.signatures.len(),
            1,
            "recovery discarded the signature"
        );
        assert_eq!(app.signatures[0].target, signed_target);
        assert!(
            app.document_signatures_cover_current_state(),
            "the recovered signature must still cover the recovered state"
        );
        assert!(
            recovered.has_unsaved_changes,
            "the signature has still not reached the repository"
        );
    }

    /// The *offer* says a signature is at stake, because its change count
    /// cannot.
    ///
    /// Signing is not a typed operation, so a crash that caught a signature
    /// and nothing else is offered as a session with zero changes and an empty
    /// operation list — the UI reads that as "this session still had 0 unsaved
    /// changes" and the user decides whether to discard a signature on the
    /// strength of it. The warning is the only channel that can say otherwise
    /// without a contract change, and it belongs to the set of segments
    /// currently on offer: discarding one takes its warning with it.
    #[test]
    fn a_recovery_offer_names_the_signature_its_change_count_cannot_show() {
        let store = MemoryStore::default();
        {
            let mut app = OpenDocApp::new_empty_document();
            app.install_recovery_journal(Arc::new(store.clone()))
                .expect("journal installs");
            app.dispatch_command("create_document", json!({ "title": "Signed crash" }))
                .expect("create");
            app.dispatch_command("add_paragraph", json!({ "text": "the signed prose" }))
                .expect("paragraph");
            // Everything so far is in the repository, so the signature is the
            // only unsaved work the crash catches.
            app.saved_operation_count = app.operation_envelopes.len();
            app.sync_recovery_journal();
            app.sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Tester")
                .expect("sign");
            app.sync_recovery_journal();
            // The process dies here.
        }

        let mut app = OpenDocApp::new_empty_document();
        let offered = app
            .install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        let session_id = store.only_segment_id();
        assert_eq!(
            offered.recovery_sessions.len(),
            1,
            "the crash left exactly one session"
        );
        assert_eq!(
            offered.recovery_sessions[0].operation_count, 0,
            "the fixture must be the case the change count cannot describe"
        );
        let warning = offered
            .warnings
            .iter()
            .find(|warning| warning.code == UNSAVED_SIGNATURE_WARNING)
            .unwrap_or_else(|| {
                panic!(
                    "a recovery offer holding an unsaved signature must say so: {:?}",
                    offered.warnings
                )
            });
        assert!(
            warning.message.contains(&session_id),
            "the warning must name the session it is about: {}",
            warning.message
        );

        // Discarding the offer takes the warning with it: the signature is
        // gone and there is no longer anything at stake to report.
        app.discard_recovery_session(&session_id)
            .expect("the offer is discarded");
        let after = app.document();
        assert!(
            !after
                .warnings
                .iter()
                .any(|warning| warning.code == UNSAVED_SIGNATURE_WARNING),
            "a discarded offer must not keep warning about its signature: {:?}",
            after.warnings
        );
    }

    /// The negative control for the offer warning: an ordinary crash with no
    /// signature must not claim one.
    ///
    /// Without this an implementation that warns unconditionally passes the
    /// test above, and the warning would appear on every recovery offer — which
    /// is the same as saying nothing.
    #[test]
    fn a_recovery_offer_without_a_signature_does_not_claim_one() {
        let store = MemoryStore::default();
        crashed_session(&store);
        let app = reopened(&store);
        let document = app.document();
        assert_eq!(
            document.recovery_sessions.len(),
            1,
            "the crash left exactly one session"
        );
        assert!(
            document.recovery_sessions[0].operation_count > 0,
            "this crash caught ordinary edits"
        );
        assert!(
            !document
                .warnings
                .iter()
                .any(|warning| warning.code == UNSAVED_SIGNATURE_WARNING),
            "nothing was signed, so nothing may say a signature is at stake: {:?}",
            document.warnings
        );
    }

    /// A segment says which snapshot format its base state is, and a segment
    /// that names one this build cannot replay is refused rather than decoded
    /// into whichever fields happen to line up.
    #[test]
    fn a_recovery_segment_declares_the_format_of_its_base_snapshot() {
        let store = MemoryStore::default();
        let session_id = {
            let mut app = OpenDocApp::new_empty_document();
            app.install_recovery_journal(Arc::new(store.clone()))
                .expect("journal installs");
            app.dispatch_command("create_document", json!({ "title": "Formats" }))
                .expect("create");
            app.dispatch_command("add_paragraph", json!({ "text": "alpha" }))
                .expect("paragraph");
            store.only_segment_id()
        };
        let segment = decode_segment(&store.read_segment(&session_id).unwrap().unwrap())
            .expect("the segment decodes");
        assert_eq!(segment.header.base_format, crate::APP_DOCUMENT_FORMAT);

        // A base snapshot in a format this build does not know.
        let mut header = segment.header.clone();
        header.base_format = "opendoc.app-document.v99".to_string();
        let mut foreign = encode_segment_header(&header).expect("header");
        for envelope in &segment.operations {
            foreign.extend_from_slice(&encode_operation_frame(envelope).expect("frame"));
        }
        store.overwrite(&session_id, foreign);

        let mut app = OpenDocApp::new_empty_document();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        let error = app
            .recover_session(&session_id)
            .expect_err("a foreign base snapshot format is refused");
        assert!(
            matches!(&error, AppApiError::Format(message) if message.contains("v99")),
            "{error:?}"
        );

        // A segment written before the declaration existed still replays, and
        // says that it was read on an assumption.
        let mut header = segment.header.clone();
        header.format = RECOVERY_SEGMENT_FORMAT_V1.to_string();
        header.base_format = String::new();
        let mut undeclared = encode_segment_header(&header).expect("header");
        for envelope in &segment.operations {
            undeclared.extend_from_slice(&encode_operation_frame(envelope).expect("frame"));
        }
        store.overwrite(&session_id, undeclared);

        let mut app = OpenDocApp::new_empty_document();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        let recovered = app.recover_session(&session_id).expect("replay");
        assert!(recovered.visible_text().contains("alpha"));
        assert!(
            recovered
                .warnings
                .iter()
                .any(|warning| warning.code == "recovery-journal-undeclared-base-format"),
            "{:?}",
            recovered.warnings
        );
    }

    /// A crash journal written by the *previous* build replays, and says so.
    ///
    /// The base snapshot in a segment is an `AppDocument`, and the format bump
    /// that reaches this crate's signature reader reaches here too. Refusing a
    /// segment because its base declares the format this build read last week
    /// throws away the unsaved work a crash caught — for no reason except that
    /// our own encoder changed in between, which is exactly the moment a user
    /// has to restart and exactly when a recovery segment is worth most.
    #[test]
    fn a_recovery_segment_from_an_earlier_payload_format_replays_and_says_so() {
        let store = MemoryStore::default();
        let session_id = {
            let mut app = OpenDocApp::new_empty_document();
            app.install_recovery_journal(Arc::new(store.clone()))
                .expect("journal installs");
            app.dispatch_command("create_document", json!({ "title": "Formats" }))
                .expect("create");
            app.dispatch_command("add_paragraph", json!({ "text": "unsaved prose" }))
                .expect("paragraph");
            store.only_segment_id()
        };
        let segment = decode_segment(&store.read_segment(&session_id).unwrap().unwrap())
            .expect("the segment decodes");

        // Spelled out, not read back from the list under test: this is the
        // string the build before the bump actually wrote.
        let earlier = "opendoc.app-document.v1";
        assert_ne!(
            earlier,
            crate::APP_DOCUMENT_FORMAT,
            "the fixture has to name a format this build no longer writes"
        );
        let mut header = segment.header.clone();
        header.base_format = earlier.to_string();
        let mut restated = encode_segment_header(&header).expect("header");
        for envelope in &segment.operations {
            restated.extend_from_slice(&encode_operation_frame(envelope).expect("frame"));
        }
        store.overwrite(&session_id, restated);

        let mut app = OpenDocApp::new_empty_document();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        let recovered = app
            .recover_session(&session_id)
            .expect("a segment from the previous build still replays");
        assert!(
            recovered.visible_text().contains("unsaved prose"),
            "the unsaved work came back"
        );
        let warning = recovered
            .warnings
            .iter()
            .find(|warning| warning.code == "recovery-journal-earlier-base-format")
            .unwrap_or_else(|| panic!("the format difference is named: {:?}", recovered.warnings));
        assert!(
            warning.message.contains(earlier)
                && warning.message.contains(crate::APP_DOCUMENT_FORMAT),
            "{warning:?}"
        );
        assert!(
            !recovered
                .warnings
                .iter()
                .any(|warning| warning.code == "recovery-journal-undeclared-base-format"),
            "the segment did declare its format: {:?}",
            recovered.warnings
        );
    }

    /// A segment this process could not append to does not end crash
    /// protection for the rest of the session.
    ///
    /// The way it happens in the wild: a second runtime over the same store is
    /// offered this process's *live* journal as a crash — `refresh_recovery_sessions`
    /// only skips its own cursor — and discarding it deletes the file the
    /// first process is appending to. The cursor used to stay on that dead
    /// segment, every later sync re-tried the same failing append, and
    /// `push_model_warning` deduped the warning after the first one: journal
    /// writing was over, silently, until the document was closed.
    ///
    /// Leader election is the fix for two runtimes sharing a store (PLAN77 F2;
    /// ADR 0005 and 0008 both record "one session at a time"). This is the
    /// difference between that limitation and a permanent failure.
    #[test]
    fn a_segment_deleted_under_a_live_session_does_not_end_crash_protection() {
        let store = MemoryStore::default();
        let mut app = OpenDocApp::new_empty_document();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        app.dispatch_command("create_document", json!({ "title": "Two windows" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "alpha" }))
            .expect("paragraph");
        let first_session = store.only_segment_id();

        // A second runtime is offered the live journal as a crash — the
        // limitation ADR 0005 records — and discards it.
        let mut second = OpenDocApp::new_empty_document();
        second
            .install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        assert_eq!(
            second.document().recovery_sessions.len(),
            1,
            "the fixture must reproduce the second window seeing the live segment"
        );
        second
            .discard_recovery_session(&first_session)
            .expect("discard");
        assert!(store.segments().is_empty());

        // The first window keeps working. Its next append cannot land, and it
        // says so — once.
        app.dispatch_command("add_paragraph", json!({ "text": "beta" }))
            .expect("paragraph");
        assert!(
            app.document()
                .warnings
                .iter()
                .any(|warning| warning.code == "recovery-journal-unavailable"),
            "a failed journal write must not be silent"
        );

        // And the very next gesture re-snapshots under a fresh session id, so
        // the work after the discard is protected again.
        app.dispatch_command("add_paragraph", json!({ "text": "gamma" }))
            .expect("paragraph");
        let session_id = store.only_segment_id();
        assert_ne!(session_id, first_session);

        let mut recovered = OpenDocApp::new_empty_document();
        recovered
            .install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        let document = recovered.recover_session(&session_id).expect("replay");
        for text in ["alpha", "beta", "gamma"] {
            assert!(
                document.visible_text().contains(text),
                "crash protection did not resume: {text} is missing"
            );
        }
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
        assert!(recovered.visible_text().contains("beta"));
        assert!(!recovered.visible_text().contains("gamma"));
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
        assert!(expected.visible_text().contains("kept"));
        assert!(!expected.visible_text().contains("undone"));
        drop(app);

        let mut app = reopened(&store);
        let session_id = app.document().recovery_sessions[0].id.clone();
        let recovered = app.recover_session(&session_id).expect("replay");
        assert!(recovered.visible_text().contains("kept"));
        assert!(
            !recovered.visible_text().contains("undone"),
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
        assert!(segment.header.base.visible_text().contains("durable"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A seeded collaboration session: one paragraph holding one run, with a
    /// recovery journal installed.
    fn collaborating(store: &MemoryStore) -> OpenDocApp {
        let mut app = OpenDocApp::new_sample();
        app.install_recovery_journal(Arc::new(store.clone()))
            .expect("journal installs");
        let mut base = opendoc_core::Document::new("Shared");
        base.blocks.push(opendoc_core::Block {
            id: opendoc_core::StableId::parse("blk-recovery-0001").expect("block id"),
            kind: opendoc_core::BlockKind::Paragraph,
            properties: Default::default(),
            content: vec![opendoc_core::Inline::Text {
                id: opendoc_core::StableId::parse("inl-recovery-0001").expect("run id"),
                text: "abcdefgh".to_string(),
                marks: Vec::new(),
            }],
        });
        app.join_collaboration_session(
            crate::OpenDocServiceSession::new(
                "local",
                "actor-local",
                "doc-recovery",
                crate::OpenDocServiceRole::Editor,
            ),
            base,
            Vec::new(),
        )
        .expect("joining a session");
        app
    }

    fn remote_insert(seq: u64, offset: usize, text: &str) -> crate::Operation {
        crate::Operation::in_context(
            crate::OperationId {
                actor: crate::ActorId("actor-remote".to_string()),
                seq,
            },
            crate::OperationKind::InsertText {
                inline_id: opendoc_core::StableId::parse("inl-recovery-0001").expect("run id"),
                offset,
                text: text.to_string(),
            },
            crate::CausalContext::default(),
        )
    }

    /// Remote work was durable on the service before it was acknowledged, so
    /// there is nothing here that only this process has. Writing a recovery
    /// segment for it would claim otherwise.
    #[test]
    fn remote_work_alone_leaves_no_recovery_segment() {
        let store = MemoryStore::default();
        let mut app = collaborating(&store);
        assert!(store.segments().is_empty());

        app.apply_remote_operations(vec![remote_insert(1, 3, "XY")], None)
            .expect("the operation merges");

        assert!(
            store.segments().is_empty(),
            "someone else's keystroke is not this process's to recover"
        );
    }

    /// Once there *is* local work to recover, the segment has to carry the
    /// remote operations too: ADR 0005's invariant is that a segment replays to
    /// exactly the in-memory state, and leaving them out would roll a
    /// collaborator's edits back on recovery.
    #[test]
    fn a_segment_covering_local_work_also_covers_the_remote_work_under_it() {
        let store = MemoryStore::default();
        let mut app = collaborating(&store);
        app.dispatch_command("add_paragraph", json!({ "text": "local work" }))
            .expect("a local command");
        assert_eq!(store.segments().len(), 1);
        let before_frames = store
            .read_segment(&store.only_segment_id())
            .expect("read")
            .expect("segment")
            .len();

        app.apply_remote_operations(vec![remote_insert(1, 3, "XY")], None)
            .expect("the operation merges");

        let after = store
            .read_segment(&store.only_segment_id())
            .expect("read")
            .expect("segment");
        assert!(
            after.len() > before_frames,
            "the remote operation must be journalled next to the local one"
        );
        let live = app.document();

        // And a replay of that segment reaches the document that was in memory,
        // remote work included.
        let session_id = store.only_segment_id();
        drop(app);
        let mut reopened = reopened(&store);
        let recovered = reopened.recover_session(&session_id).expect("replay");
        assert_eq!(recovered.visible_text(), live.visible_text());
        assert!(
            recovered.visible_text().contains("abcXYdefgh"),
            "{:?}",
            recovered.visible_text()
        );
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
        assert_eq!(recovered.visible_text(), crashed.visible_text());
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
        assert!(recovered.visible_text().contains("flushed"));
        assert!(!recovered.visible_text().contains("unflushed"));
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
