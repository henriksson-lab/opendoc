//! The recent-documents list, and where it survives a restart.
//!
//! [`AppRecentDocument`] is a projection field (`AppDocument::recent_documents`)
//! that the home screen renders. The list itself used to be nothing but a
//! `Vec` in `state.rs`: saving a repository put an entry in it, and the
//! process exiting took every entry with it, so relaunching the native shell
//! showed an empty "Recent documents" panel however much work had been saved
//! (PLAN77, 2026-09-12).
//!
//! **Where recents belong.** A recents list is *user-level* state: it is not
//! document state (it outlives every document in it), not repository state (it
//! spans repositories, and a repository read-only to this user must not have
//! to be written to record that it was opened), and not view state (it is the
//! same list in every window). So it lives where the crash-recovery segments
//! live — a small value in storage the shell owns, beside
//! `$XDG_DATA_HOME/org.opendoc.prototype/recovery/` on a Tauri build (ADR
//! 0005) and in the IndexedDB-backed volume in a browser build (ADR 0008).
//! Which directory that is, is the shell's knowledge and not this crate's:
//! the app-data path differs per platform and comes from the bundle
//! identifier, so — exactly as ADR 0005 decided for recovery — a store is
//! *installed* by the runtime rather than guessed at here.
//!
//! Storage is therefore behind [`RecentDocumentStore`], which is one value in
//! and one value out. Unlike a recovery segment the list is small, bounded by
//! [`RECENT_DOCUMENT_LIMIT`], and always rewritten whole, so it needs no
//! append path and no frame layout: [`FileRecentDocumentStore`] replaces a
//! file atomically and [`VolumeRecentDocumentStore`] replaces one volume key.
//!
//! A runtime that installs no store keeps the old behaviour — the list works
//! for as long as the process lives and is gone after it — which is an honest
//! answer for a runtime with nowhere to put it, and is reported by
//! [`RecentDocuments::is_durable`] rather than looking the same as a working
//! one.
//!
//! # Who wires this up
//!
//! The list is `AppState::recent_documents`, so `record_recent_document` and
//! the scan path in `repository_io.rs` store a change by making it — there is
//! no second "save the recents" call for a new call site to forget. Both
//! shells install a store at startup, beside the recovery journal they
//! already install:
//!
//! * `apps/desktop/src-tauri/src/main.rs` installs
//!   [`FileRecentDocumentStore`] over `<app data dir>/recent-documents`,
//!   beside the `recovery/` directory of ADR 0005.
//! * `opendoc-wasm`'s storage setup installs [`VolumeRecentDocumentStore`]
//!   over the key `recent/documents` of the IndexedDB-backed volume, and only
//!   once that volume is actually mirrored — a volume with mirroring disabled
//!   is memory, and installing over it would make [`RecentDocuments::is_durable`]
//!   claim a restart will remember when a reload will not.
//!
//! Neither shell can turn a storage problem into a failure to start:
//! [`OpenDocApp::install_recent_documents`] returns no error at all and
//! reports an unreadable or corrupt stored list as a model warning, because
//! losing the recents list must cost the user their recents and not their
//! session.

use crate::{AppDocument, OpenDocApp};
use opendoc_format::{decode_cbor, encode_canonical_cbor};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// How many entries the list keeps. The oldest fall off the end.
pub const RECENT_DOCUMENT_LIMIT: usize = 20;

/// Marks a stored recents list and pins its layout.
const RECENT_DOCUMENTS_MAGIC: &[u8] = b"opendoc-recents-v0\n";
/// `format` of the stored record.
const RECENT_DOCUMENTS_FORMAT: &str = "opendoc.recent-documents.v0";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppRecentDocument {
    pub uuid: String,
    pub title: String,
    pub doi: Option<String>,
    pub repository_root: String,
    pub repository_backend: String,
    pub repository_namespace: Option<String>,
    pub last_manifest: Option<String>,
    pub updated_at_ms: u64,
}

impl AppRecentDocument {
    /// What makes two entries the same document in the same place.
    ///
    /// The uuid alone is not it: the same document saved into two
    /// repositories is two ways to reopen it, and both are worth offering.
    fn same_target(&self, other: &AppRecentDocument) -> bool {
        self.uuid == other.uuid
            && self.repository_root == other.repository_root
            && self.repository_backend == other.repository_backend
            && self.repository_namespace == other.repository_namespace
    }
}

/// Durable storage for the recents list: one value, read and replaced whole.
///
/// Deliberately free of document types — a browser adapter over IndexedDB has
/// only to move this byte string around — and deliberately not append-only,
/// because the list is rewritten in full on every change. `&self` for the
/// same reason [`crate::RecoveryJournalStore`] uses it: the file (or key) is
/// the state, and the app holds the store behind an `Arc`.
pub trait RecentDocumentStore: std::fmt::Debug + Send + Sync {
    /// The stored value, or `None` when nothing has been stored yet.
    fn read_recent_documents(&self) -> Result<Option<Vec<u8>>, String>;
    /// Replace the stored value.
    fn write_recent_documents(&self, bytes: &[u8]) -> Result<(), String>;
}

/// Encode a list for storage: magic, then canonical CBOR.
pub(crate) fn encode_recent_documents(documents: &[AppRecentDocument]) -> Result<Vec<u8>, String> {
    let record = RecentDocumentsRecord {
        format: RECENT_DOCUMENTS_FORMAT.to_string(),
        documents: documents.to_vec(),
    };
    let payload = encode_canonical_cbor(&record).map_err(|err| err.to_string())?;
    let mut bytes = RECENT_DOCUMENTS_MAGIC.to_vec();
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Decode a stored list, refusing anything this build does not understand.
pub(crate) fn decode_recent_documents(bytes: &[u8]) -> Result<Vec<AppRecentDocument>, String> {
    if !bytes.starts_with(RECENT_DOCUMENTS_MAGIC) {
        return Err("stored value is not an OpenDoc recent-documents list".to_string());
    }
    let record: RecentDocumentsRecord =
        decode_cbor(&bytes[RECENT_DOCUMENTS_MAGIC.len()..]).map_err(|err| err.to_string())?;
    if record.format != RECENT_DOCUMENTS_FORMAT {
        return Err(format!(
            "unsupported recent-documents format {}",
            record.format
        ));
    }
    Ok(record.documents)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RecentDocumentsRecord {
    format: String,
    documents: Vec<AppRecentDocument>,
}

/// The recents list together with the storage that outlives the process.
///
/// Every mutation goes through this type, and every mutation that changes the
/// list writes it back, so there is no separate "save the recents" step that a
/// new call site could forget — which is the shape the list has to have,
/// because the moments worth remembering (a save, an open, a scan) are spread
/// across the repository paths.
///
/// Reads go through `Deref`, so this is a `[AppRecentDocument]` everywhere the
/// projection needs one.
#[derive(Clone, Default)]
pub struct RecentDocuments {
    documents: Vec<AppRecentDocument>,
    store: Option<Arc<dyn RecentDocumentStore>>,
}

impl std::fmt::Debug for RecentDocuments {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecentDocuments")
            .field("documents", &self.documents)
            .field("durable", &self.store.is_some())
            .finish()
    }
}

impl std::ops::Deref for RecentDocuments {
    type Target = [AppRecentDocument];

    fn deref(&self) -> &Self::Target {
        &self.documents
    }
}

impl RecentDocuments {
    /// Install durable storage and adopt what it already holds.
    ///
    /// Called once, at startup, by the runtime that owns the storage. Entries
    /// already in memory are from *this* session and stay in front of the
    /// stored ones.
    ///
    /// An unreadable or corrupt stored value is reported and then replaced:
    /// losing the recents list must cost the user their recents, not their
    /// session, so this never fails in a way that could stop a shell from
    /// starting. The store stays installed either way, so the next recorded
    /// document writes a well-formed list over the damaged one.
    pub fn install(&mut self, store: Arc<dyn RecentDocumentStore>) -> Result<(), String> {
        self.store = Some(store.clone());
        let stored = match store.read_recent_documents() {
            Ok(Some(bytes)) => decode_recent_documents(&bytes)?,
            Ok(None) => Vec::new(),
            Err(err) => return Err(err),
        };
        for entry in stored {
            if self
                .documents
                .iter()
                .any(|existing| existing.same_target(&entry))
            {
                continue;
            }
            self.documents.push(entry);
        }
        self.documents.truncate(RECENT_DOCUMENT_LIMIT);
        Ok(())
    }

    /// Is this list stored anywhere, or only held for as long as the process
    /// lives? A runtime with no store installed answers false, and the UI is
    /// entitled to say so rather than to imply a restart will remember.
    pub fn is_durable(&self) -> bool {
        self.store.is_some()
    }

    /// Remember one document as the most recently used, and store the list.
    ///
    /// The same document in the same repository moves to the front instead of
    /// appearing twice; see [`AppRecentDocument::same_target`].
    pub fn record(&mut self, entry: AppRecentDocument) -> Result<(), String> {
        self.documents
            .retain(|existing| !existing.same_target(&entry));
        self.documents.insert(0, entry);
        self.documents.truncate(RECENT_DOCUMENT_LIMIT);
        self.flush()
    }

    /// Fold in documents found by scanning a repository, newest first.
    ///
    /// A scan is a use of every document it found, so the results go to the
    /// front in their own order; anything already listed is replaced by the
    /// freshly read entry rather than being listed twice.
    pub fn merge(&mut self, discovered: Vec<AppRecentDocument>) -> Result<(), String> {
        for entry in discovered.into_iter().rev() {
            self.documents
                .retain(|existing| !existing.same_target(&entry));
            self.documents.insert(0, entry);
        }
        self.documents.truncate(RECENT_DOCUMENT_LIMIT);
        self.flush()
    }

    /// Forget everything, including in storage.
    pub fn clear(&mut self) -> Result<(), String> {
        self.documents.clear();
        self.flush()
    }

    fn flush(&self) -> Result<(), String> {
        let Some(store) = self.store.as_ref() else {
            return Ok(());
        };
        store.write_recent_documents(&encode_recent_documents(&self.documents)?)
    }
}

impl OpenDocApp {
    /// Install durable storage for the recents list and adopt what it holds.
    ///
    /// Called once, at startup, by the runtime that owns the storage —
    /// exactly as ADR 0005 decided for the recovery journal, and for the same
    /// reason: which file or key this is, is the shell's knowledge.
    ///
    /// **There is deliberately no error to propagate.** A shell that got a
    /// `Result` here would have to decide what to do with it, and the only
    /// right answer is "carry on": a recents list that cannot be read is worth
    /// a warning, never a refusal to start. So an unreadable or corrupt stored
    /// value is reported into `document.warnings` and the store stays
    /// installed, which means the next recorded document writes a well-formed
    /// list over the damaged one.
    ///
    /// The returned document carries the adopted `recent_documents`, so a
    /// shell that renders the home screen from it needs no second call.
    pub fn install_recent_documents(&mut self, store: Arc<dyn RecentDocumentStore>) -> AppDocument {
        if let Err(err) = self.recent_documents.install(store) {
            self.push_model_warning(
                "recent-documents-unreadable",
                format!("the stored recent-documents list could not be read: {err}"),
            );
        }
        self.document()
    }

    /// Is the recents list stored anywhere, or only held for this process?
    pub fn recent_documents_are_durable(&self) -> bool {
        self.recent_documents.is_durable()
    }
}

// ---- Filesystem store ------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
mod file_store {
    use super::RecentDocumentStore;
    use std::path::PathBuf;

    /// The recents list as one file, for a native shell.
    ///
    /// The shell picks the path (its app-data directory, beside the recovery
    /// segments); this type only owns how it is written. A write goes to a
    /// sibling temporary file and is renamed over the real one, because a
    /// crash halfway through rewriting the list in place would leave a
    /// truncated file that the next start cannot decode — and the whole point
    /// of this file is to be readable after an unclean exit.
    #[derive(Clone, Debug)]
    pub struct FileRecentDocumentStore {
        path: PathBuf,
    }

    impl FileRecentDocumentStore {
        pub fn new(path: impl Into<PathBuf>) -> Self {
            Self { path: path.into() }
        }

        pub fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl RecentDocumentStore for FileRecentDocumentStore {
        fn read_recent_documents(&self) -> Result<Option<Vec<u8>>, String> {
            match std::fs::read(&self.path) {
                Ok(bytes) => Ok(Some(bytes)),
                // Nothing stored yet is not a problem to report; it is a first
                // run, and it must not produce a warning on the home screen.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(err) => Err(format!("{}: {err}", self.path.display())),
            }
        }

        fn write_recent_documents(&self, bytes: &[u8]) -> Result<(), String> {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("{}: {err}", parent.display()))?;
            }
            let mut temporary = self.path.clone().into_os_string();
            temporary.push(".writing");
            let temporary = PathBuf::from(temporary);
            std::fs::write(&temporary, bytes)
                .map_err(|err| format!("{}: {err}", temporary.display()))?;
            std::fs::rename(&temporary, &self.path).map_err(|err| {
                let _ = std::fs::remove_file(&temporary);
                format!("{}: {err}", self.path.display())
            })
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use file_store::FileRecentDocumentStore;

// ---- Key/value volume store ------------------------------------------------

mod volume_store {
    use super::RecentDocumentStore;
    use opendoc_store::MirroredVolume;

    /// The recents list as one key in a [`MirroredVolume`], which is how a
    /// browser build gets the same memory the desktop one has (ADR 0008).
    ///
    /// One key, not a subtree: the list is small and always written whole, so
    /// a write is a single small insert the volume's driver mirrors into
    /// IndexedDB with everything else.
    #[derive(Clone, Debug)]
    pub struct VolumeRecentDocumentStore {
        volume: MirroredVolume,
        key: String,
    }

    impl VolumeRecentDocumentStore {
        /// `key` is the volume key the list owns. Repositories and recovery
        /// segments live under their own prefixes, so no repository path and
        /// no session id can name it.
        pub fn new(volume: MirroredVolume, key: impl Into<String>) -> Self {
            Self {
                volume,
                key: key.into().trim_matches('/').to_string(),
            }
        }

        pub fn volume(&self) -> &MirroredVolume {
            &self.volume
        }
    }

    impl RecentDocumentStore for VolumeRecentDocumentStore {
        fn read_recent_documents(&self) -> Result<Option<Vec<u8>>, String> {
            Ok(self.volume.get(&self.key))
        }

        fn write_recent_documents(&self, bytes: &[u8]) -> Result<(), String> {
            self.volume.put(&self.key, bytes);
            Ok(())
        }
    }
}

pub use volume_store::VolumeRecentDocumentStore;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Stands in for the shell's storage: bytes that outlive the
    /// `RecentDocuments` that wrote them, which is what a restart is.
    #[derive(Debug, Default)]
    struct MemoryStore {
        bytes: Mutex<Option<Vec<u8>>>,
        writes: Mutex<usize>,
    }

    impl RecentDocumentStore for MemoryStore {
        fn read_recent_documents(&self) -> Result<Option<Vec<u8>>, String> {
            Ok(self.bytes.lock().expect("lock").clone())
        }

        fn write_recent_documents(&self, bytes: &[u8]) -> Result<(), String> {
            *self.bytes.lock().expect("lock") = Some(bytes.to_vec());
            *self.writes.lock().expect("lock") += 1;
            Ok(())
        }
    }

    #[derive(Debug)]
    struct RefusingStore;

    impl RecentDocumentStore for RefusingStore {
        fn read_recent_documents(&self) -> Result<Option<Vec<u8>>, String> {
            Ok(None)
        }

        fn write_recent_documents(&self, _bytes: &[u8]) -> Result<(), String> {
            Err("storage is read-only".to_string())
        }
    }

    fn entry(uuid: &str, root: &str, updated_at_ms: u64) -> AppRecentDocument {
        AppRecentDocument {
            uuid: uuid.to_string(),
            title: format!("Document {uuid}"),
            doi: None,
            repository_root: root.to_string(),
            repository_backend: "local".to_string(),
            repository_namespace: None,
            last_manifest: Some("sha256:aaaa".to_string()),
            updated_at_ms,
        }
    }

    #[test]
    fn recents_survive_a_simulated_restart() {
        let store = Arc::new(MemoryStore::default());
        let mut first = RecentDocuments::default();
        first.install(store.clone()).expect("install");
        assert!(first.is_durable());
        first
            .record(entry("doc-1", "/repo/one", 10))
            .expect("record");
        first
            .record(entry("doc-2", "/repo/two", 20))
            .expect("record");
        assert_eq!(first.len(), 2);
        // The process ends here: everything in memory goes with it.
        drop(first);

        let mut restarted = RecentDocuments::default();
        restarted.install(store).expect("install");
        assert_eq!(
            restarted
                .iter()
                .map(|recent| recent.uuid.as_str())
                .collect::<Vec<_>>(),
            vec!["doc-2", "doc-1"],
            "a restart must see the recents the last session recorded, newest first"
        );
        assert_eq!(restarted[0].repository_root, "/repo/two");
        assert_eq!(restarted[0].last_manifest.as_deref(), Some("sha256:aaaa"));
    }

    #[test]
    fn a_list_with_no_store_is_not_durable_and_says_so() {
        let mut volatile = RecentDocuments::default();
        assert!(!volatile.is_durable());
        // Recording still works; it is just not remembered anywhere, which is
        // the honest answer for a runtime with nowhere to put it.
        volatile
            .record(entry("doc-1", "/repo/one", 10))
            .expect("record");
        assert_eq!(volatile.len(), 1);
    }

    #[test]
    fn the_same_document_in_the_same_repository_moves_to_the_front() {
        let store = Arc::new(MemoryStore::default());
        let mut recents = RecentDocuments::default();
        recents.install(store.clone()).expect("install");
        recents
            .record(entry("doc-1", "/repo/one", 10))
            .expect("record");
        recents
            .record(entry("doc-2", "/repo/two", 20))
            .expect("record");
        recents
            .record(entry("doc-1", "/repo/one", 30))
            .expect("record");
        assert_eq!(recents.len(), 2, "a second save is not a second entry");
        assert_eq!(recents[0].uuid, "doc-1");
        assert_eq!(recents[0].updated_at_ms, 30);

        // The same document in a *different* repository is a different way to
        // reopen it, and both are worth keeping.
        recents
            .record(entry("doc-1", "/repo/three", 40))
            .expect("record");
        assert_eq!(recents.len(), 3);

        let mut restarted = RecentDocuments::default();
        restarted.install(store).expect("install");
        assert_eq!(restarted.len(), 3, "storage holds exactly what memory held");
        assert_eq!(restarted[0].repository_root, "/repo/three");
    }

    #[test]
    fn the_list_is_capped_and_the_cap_is_what_is_stored() {
        let store = Arc::new(MemoryStore::default());
        let mut recents = RecentDocuments::default();
        recents.install(store.clone()).expect("install");
        for index in 0..(RECENT_DOCUMENT_LIMIT + 5) {
            recents
                .record(entry(&format!("doc-{index}"), "/repo/one", index as u64))
                .expect("record");
        }
        assert_eq!(recents.len(), RECENT_DOCUMENT_LIMIT);

        let mut restarted = RecentDocuments::default();
        restarted.install(store).expect("install");
        assert_eq!(restarted.len(), RECENT_DOCUMENT_LIMIT);
        assert_eq!(
            restarted[0].uuid,
            format!("doc-{}", RECENT_DOCUMENT_LIMIT + 4)
        );
    }

    #[test]
    fn a_scan_merges_its_results_in_front_without_duplicating_them() {
        let store = Arc::new(MemoryStore::default());
        let mut recents = RecentDocuments::default();
        recents.install(store.clone()).expect("install");
        recents
            .record(entry("doc-1", "/repo/one", 10))
            .expect("record");
        recents
            .merge(vec![
                entry("doc-9", "/repo/nine", 90),
                entry("doc-1", "/repo/one", 11),
            ])
            .expect("merge");
        assert_eq!(
            recents
                .iter()
                .map(|recent| recent.uuid.as_str())
                .collect::<Vec<_>>(),
            vec!["doc-9", "doc-1"],
            "a rescan re-orders the list, it does not lengthen it"
        );
        assert_eq!(
            recents[1].updated_at_ms, 11,
            "the rescanned entry is the fresh one"
        );

        let mut restarted = RecentDocuments::default();
        restarted.install(store).expect("install");
        assert_eq!(restarted.len(), 2);
    }

    #[test]
    fn installing_keeps_this_sessions_entries_in_front_of_the_stored_ones() {
        let store = Arc::new(MemoryStore::default());
        let mut first = RecentDocuments::default();
        first.install(store.clone()).expect("install");
        first
            .record(entry("doc-1", "/repo/one", 10))
            .expect("record");
        drop(first);

        let mut late = RecentDocuments::default();
        late.record(entry("doc-2", "/repo/two", 20))
            .expect("record");
        late.install(store).expect("install");
        assert_eq!(
            late.iter()
                .map(|recent| recent.uuid.as_str())
                .collect::<Vec<_>>(),
            vec!["doc-2", "doc-1"],
            "work this session did is more recent than anything on disk"
        );
    }

    #[test]
    fn a_corrupt_stored_list_is_reported_and_then_replaced() {
        let store = Arc::new(MemoryStore::default());
        store
            .write_recent_documents(b"this is not a recents list")
            .expect("seed");
        let mut recents = RecentDocuments::default();
        let error = recents.install(store.clone()).expect_err("must report");
        assert!(
            error.contains("not an OpenDoc recent-documents list"),
            "unexpected error: {error}"
        );
        // The store is still installed, so the next document recorded writes a
        // well-formed list over the damaged one.
        assert!(recents.is_durable());
        recents
            .record(entry("doc-1", "/repo/one", 10))
            .expect("record");
        let mut restarted = RecentDocuments::default();
        restarted.install(store).expect("install");
        assert_eq!(restarted.len(), 1);
    }

    #[test]
    fn a_store_that_refuses_a_write_reports_it_and_keeps_the_list() {
        let mut recents = RecentDocuments::default();
        recents.install(Arc::new(RefusingStore)).expect("install");
        let error = recents
            .record(entry("doc-1", "/repo/one", 10))
            .expect_err("must report");
        assert!(error.contains("read-only"), "unexpected error: {error}");
        assert_eq!(
            recents.len(),
            1,
            "the session keeps the recent it just used even if it cannot be stored"
        );
    }

    #[test]
    fn a_stored_list_round_trips_through_the_codec() {
        let documents = vec![
            entry("doc-1", "/repo/one", 10),
            entry("doc-2", "/repo/two", 20),
        ];
        let bytes = encode_recent_documents(&documents).expect("encode");
        assert!(bytes.starts_with(RECENT_DOCUMENTS_MAGIC));
        assert_eq!(decode_recent_documents(&bytes).expect("decode"), documents);
        assert!(decode_recent_documents(&bytes[1..]).is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_file_store_survives_a_restart_and_rewrites_in_place() {
        let directory = std::env::temp_dir().join(format!(
            "opendoc-recents-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let path = directory.join("recent-documents");
        let store = Arc::new(FileRecentDocumentStore::new(&path));
        assert_eq!(
            store.read_recent_documents().expect("first run"),
            None,
            "a first run has nothing stored and that is not an error"
        );

        let mut recents = RecentDocuments::default();
        recents.install(store.clone()).expect("install");
        recents
            .record(entry("doc-1", "/repo/one", 10))
            .expect("record");
        recents
            .record(entry("doc-2", "/repo/two", 20))
            .expect("record");
        assert!(path.exists(), "the list is written where the shell asked");
        assert!(
            !path.with_file_name("recent-documents.writing").exists(),
            "the temporary file is renamed, never left behind"
        );

        let mut restarted = RecentDocuments::default();
        restarted
            .install(Arc::new(FileRecentDocumentStore::new(&path)))
            .expect("install");
        assert_eq!(
            restarted
                .iter()
                .map(|recent| recent.uuid.as_str())
                .collect::<Vec<_>>(),
            vec!["doc-2", "doc-1"]
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_volume_store_survives_a_durable_round_trip() {
        use opendoc_store::MirroredVolume;

        // The browser's storage, minus IndexedDB: a map that only changes when
        // the driver flushes, and that outlives the volume it flushed from.
        let volume = MirroredVolume::new();
        let mut recents = RecentDocuments::default();
        recents
            .install(Arc::new(VolumeRecentDocumentStore::new(
                volume.clone(),
                "recent/documents",
            )))
            .expect("install");
        recents
            .record(entry("doc-1", "/repo/one", 10))
            .expect("record");

        let pending = volume.pending();
        assert_eq!(pending.len(), 1, "one recorded document is one small write");
        let durable = pending
            .into_iter()
            .map(|mutation| (mutation.key, mutation.value.expect("a write, not a delete")))
            .collect::<Vec<_>>();
        assert_eq!(durable[0].0, "recent/documents");

        let reopened = MirroredVolume::new();
        reopened.hydrate(durable);
        let mut restarted = RecentDocuments::default();
        restarted
            .install(Arc::new(VolumeRecentDocumentStore::new(
                reopened,
                "recent/documents",
            )))
            .expect("install");
        assert_eq!(restarted.len(), 1);
        assert_eq!(restarted[0].uuid, "doc-1");
    }

    // ---- The wiring: a real save, through the real app, across a restart ---
    //
    // Everything above tests the list and its storage. These test that the
    // app actually uses them — the three edits PLAN77 named: the field type in
    // `state.rs`, `record_recent_document` and the scan merge in
    // `repository_io.rs`, and `install_recent_documents` for the shells. Without
    // any one of them a save still leaves nothing behind.

    /// A temporary directory nobody else in this process is using.
    #[cfg(not(target_arch = "wasm32"))]
    fn scratch(label: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "opendoc-recents-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_saved_repository_is_still_listed_after_a_restart() {
        use serde_json::json;

        let scratch = scratch("save");
        let repository = scratch.join("repo");
        let store = Arc::new(FileRecentDocumentStore::new(
            scratch.join("recent-documents"),
        ));

        let mut app = OpenDocApp::new_empty_document();
        app.install_recent_documents(store.clone());
        app.dispatch_command("create_document", json!({ "title": "Remembered" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "saved work" }))
            .expect("paragraph");
        app.dispatch_command(
            "save_local_repository",
            json!({ "path": repository.to_string_lossy() }),
        )
        .expect("save");
        let saved = app.document();
        assert_eq!(saved.recent_documents.len(), 1);
        assert_eq!(saved.recent_documents[0].title, "Remembered");
        let uuid = saved.recent_documents[0].uuid.clone();
        // The process ends here. This is exactly what used to lose the list:
        // the Tauri shell starts from `new_empty_document()`.
        drop(app);

        let mut restarted = OpenDocApp::new_empty_document();
        let booted = restarted.install_recent_documents(Arc::new(FileRecentDocumentStore::new(
            scratch.join("recent-documents"),
        )));
        assert_eq!(
            booted
                .recent_documents
                .iter()
                .map(|recent| (recent.uuid.as_str(), recent.title.as_str()))
                .collect::<Vec<_>>(),
            vec![(uuid.as_str(), "Remembered")],
            "a relaunched shell must remember which repository was used"
        );
        assert_eq!(
            booted.recent_documents[0].repository_root,
            repository.to_string_lossy()
        );
        assert!(restarted.recent_documents_are_durable());
        assert!(
            booted
                .warnings
                .iter()
                .all(|warning| !warning.code.starts_with("recent-documents")),
            "a healthy list warns about nothing: {:?}",
            booted.warnings
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_folder_scan_is_stored_too_so_a_restart_still_offers_what_it_found() {
        use serde_json::json;

        // The scan path is the other half: opening a folder repopulates the
        // list from the repository's lookup index, and that merge has to be
        // stored as well or a restart forgets the folder was ever opened.
        let scratch = scratch("scan");
        let repository = scratch.join("repo");
        let recents = scratch.join("recent-documents");

        let mut app = OpenDocApp::new_empty_document();
        app.install_recent_documents(Arc::new(FileRecentDocumentStore::new(&recents)));
        app.dispatch_command("create_document", json!({ "title": "Scanned" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "scanned work" }))
            .expect("paragraph");
        app.dispatch_command(
            "save_local_repository",
            json!({ "path": repository.to_string_lossy() }),
        )
        .expect("save");
        drop(app);

        // A second process that has never seen the folder: forget the list, so
        // the only thing that can put the document back is the scan.
        let mut scanner = OpenDocApp::new_empty_document();
        scanner.install_recent_documents(Arc::new(FileRecentDocumentStore::new(&recents)));
        scanner.recent_documents.clear().expect("clear");
        let scanned = scanner
            .scan_local_repository(repository.clone())
            .expect("scan");
        assert_eq!(scanned.recent_documents.len(), 1);
        assert_eq!(scanned.recent_documents[0].title, "Scanned");
        drop(scanner);

        let mut restarted = OpenDocApp::new_empty_document();
        let booted =
            restarted.install_recent_documents(Arc::new(FileRecentDocumentStore::new(&recents)));
        assert_eq!(
            booted
                .recent_documents
                .iter()
                .map(|recent| recent.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Scanned"],
            "the scan merge must be stored, not only held in memory"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_recents_store_that_refuses_a_write_warns_and_does_not_fail_the_save() {
        use serde_json::json;

        // The document reached the repository; only the memory of having
        // opened it did not. That must not look like a failed save, and it
        // must not be silent either.
        let scratch = scratch("refused");
        let mut app = OpenDocApp::new_empty_document();
        app.install_recent_documents(Arc::new(RefusingStore));
        app.dispatch_command("create_document", json!({ "title": "Refused" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "work" }))
            .expect("paragraph");
        app.dispatch_command(
            "save_local_repository",
            json!({ "path": scratch.join("repo").to_string_lossy() }),
        )
        .expect("the save itself succeeds");

        let document = app.document();
        assert!(
            document
                .warnings
                .iter()
                .any(|warning| warning.code == "recent-documents-unwritable"),
            "a list that could not be stored must say so: {:?}",
            document.warnings
        );
        assert_eq!(
            document.recent_documents.len(),
            1,
            "the session still knows what it just saved"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_corrupt_stored_list_is_a_warning_and_never_a_failure_to_start() {
        let store = Arc::new(MemoryStore::default());
        store
            .write_recent_documents(b"written by something else")
            .expect("seed");
        let mut app = OpenDocApp::new_empty_document();
        // No `Result` to ignore: the shell cannot turn this into a refusal to
        // start even by accident.
        let booted = app.install_recent_documents(store);
        assert!(booted.recent_documents.is_empty());
        assert!(
            booted
                .warnings
                .iter()
                .any(|warning| warning.code == "recent-documents-unreadable"),
            "unexpected warnings: {:?}",
            booted.warnings
        );
        assert!(
            app.recent_documents_are_durable(),
            "the store stays installed, so the next save repairs the damaged list"
        );
    }
}
