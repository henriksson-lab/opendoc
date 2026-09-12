//! An object store for runtimes whose durable storage is asynchronous.
//!
//! The browser is the motivating case: IndexedDB is callback/promise based and
//! `OpenDocApp::dispatch_command` is synchronous all the way down, so an
//! adapter cannot simply "write to IndexedDB" inside [`ObjectStore::put_named`].
//! Rather than fake synchrony (there is no way to block a browser's main
//! thread on a transaction, and pretending otherwise deadlocks the page), this
//! module splits the problem in two:
//!
//! * [`MirroredVolume`] is the synchronous store of record — a flat
//!   `key -> bytes` map held in memory. Every read and write the app performs
//!   is answered from it, immediately, with no I/O.
//! * Every mutation is also appended to an ordered, monotonically sequenced
//!   *pending* list. A runtime-specific driver (`opendoc-wasm`'s IndexedDB
//!   binding) drains that list asynchronously and reports back, via
//!   [`MirroredVolume::acknowledge`], how far durability has actually reached.
//!
//! The durability watermark is therefore explicit rather than assumed: the app
//! never believes a write is durable, it only knows the volume holds it and
//! that `durable_seq` lags `sequence` by however much the driver has not
//! written yet.
//!
//! ## Why this is safe for head pointers
//!
//! Content-addressed objects are immutable, so a write-behind cache cannot
//! corrupt them — a late write only means the object is missing, never wrong.
//! Branch heads are mutable, and a head that lands durably *before* the objects
//! it names would be a dangling pointer: a lost document.
//!
//! Two properties rule that out:
//!
//! 1. [`MirroredVolume::pending`] always returns *every* unacknowledged
//!    mutation, so a drained batch is a prefix of the mutation sequence. The
//!    commit path writes each object before it swaps the head
//!    (`Repository::commit_manifest`), so any batch containing the head swap
//!    also contains every object written before it.
//! 2. The driver is required to apply one batch atomically (an IndexedDB
//!    `readwrite` transaction) and to serialize batches. A torn batch is
//!    therefore not a state the durable store can be left in.
//!
//! The cost, stated plainly: a crash between a mutation and its flush loses the
//! tail of the sequence — the document reverts to an earlier *consistent*
//! state, never to an inconsistent one.
//!
//! ## Per-key coalescing
//!
//! `pending` is keyed by key, not by sequence number: a second write to the
//! same key replaces the first and carries the newer sequence number. That
//! bounds memory by the number of distinct keys rather than by the number of
//! writes (the head key is rewritten on every save), and it is safe precisely
//! because a batch is applied atomically, so no intermediate value of a key is
//! ever observable in durable storage. `acknowledge` compares per-entry
//! sequence numbers, so a key rewritten *while* a flush was in flight is not
//! dropped by that flush's acknowledgement.

use crate::error::StoreError;
use crate::keys::clean_relative_path;
use crate::local_store::parse_head_value;
use crate::object_store::{ObjectStore, ObjectStoreLayout, StoreCapabilities};
use opendoc_core::{digest_bytes, HashRef};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// One durable mutation, in sequence order. `value` is `None` for a deletion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeMutation {
    pub seq: u64,
    pub key: String,
    pub value: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
struct PendingEntry {
    seq: u64,
    value: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
struct VolumeState {
    entries: BTreeMap<String, Vec<u8>>,
    pending: BTreeMap<String, PendingEntry>,
    seq: u64,
    durable_seq: u64,
    /// False once a runtime has told the volume that nothing is draining it,
    /// so recording mutations would only leak memory.
    mirroring: bool,
}

/// A synchronous in-memory volume whose mutations are mirrored, in order, to
/// an asynchronous durable store. See the module documentation.
///
/// Deliberately not `Default`: a volume either has a driver draining it
/// ([`MirroredVolume::new`]) or knowingly has none
/// ([`MirroredVolume::memory_only`]), and defaulting to one of those silently
/// would be defaulting to a durability claim.
#[derive(Clone, Debug)]
pub struct MirroredVolume {
    state: Arc<Mutex<VolumeState>>,
}

impl MirroredVolume {
    /// A volume whose mutations a driver will drain. Deliberately without a
    /// `Default` impl: defaulting would be defaulting to a durability claim.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(VolumeState {
                mirroring: true,
                ..VolumeState::default()
            })),
        }
    }

    /// A volume that records nothing for a driver to drain: reads and writes
    /// work, but they live and die with the page.
    pub fn memory_only() -> Self {
        Self {
            state: Arc::new(Mutex::new(VolumeState::default())),
        }
    }

    /// Lock recovery: a panic inside one of these very short critical sections
    /// must not take the whole volume — and with it the user's document —
    /// down with it.
    fn lock(&self) -> MutexGuard<'_, VolumeState> {
        self.state.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// Load durable contents into the volume. Hydration is not a mutation: it
    /// describes what the durable store already holds, so it neither advances
    /// the sequence nor queues anything to write back.
    pub fn hydrate(&self, entries: impl IntoIterator<Item = (String, Vec<u8>)>) {
        let mut state = self.lock();
        for (key, bytes) in entries {
            state.entries.insert(key, bytes);
        }
    }

    /// Stop recording mutations, and drop those already recorded, because no
    /// driver is draining them (a browser with IndexedDB unavailable, say).
    /// The volume keeps working; it is simply not durable, and says so.
    pub fn disable_mirroring(&self) {
        let mut state = self.lock();
        state.mirroring = false;
        state.pending.clear();
        state.durable_seq = state.seq;
    }

    pub fn is_mirroring(&self) -> bool {
        self.lock().mirroring
    }

    /// Every mutation not yet acknowledged, oldest first.
    pub fn pending(&self) -> Vec<VolumeMutation> {
        let state = self.lock();
        let mut out: Vec<VolumeMutation> = state
            .pending
            .iter()
            .map(|(key, entry)| VolumeMutation {
                seq: entry.seq,
                key: key.clone(),
                value: entry.value.clone(),
            })
            .collect();
        out.sort_by_key(|mutation| mutation.seq);
        out
    }

    /// Report that every mutation up to and including `through_seq` is durable.
    ///
    /// Entries rewritten after the flush began carry a higher sequence number
    /// and survive, so an in-flight flush cannot acknowledge away a write it
    /// never saw.
    pub fn acknowledge(&self, through_seq: u64) {
        let mut state = self.lock();
        state.pending.retain(|_, entry| entry.seq > through_seq);
        if through_seq > state.durable_seq {
            state.durable_seq = through_seq.min(state.seq);
        }
    }

    /// Sequence number of the most recent mutation.
    pub fn sequence(&self) -> u64 {
        self.lock().seq
    }

    /// Sequence number the driver has reported as durable.
    pub fn durable_seq(&self) -> u64 {
        self.lock().durable_seq
    }

    pub fn pending_len(&self) -> usize {
        self.lock().pending.len()
    }

    pub fn pending_bytes(&self) -> usize {
        self.lock()
            .pending
            .iter()
            .map(|(key, entry)| key.len() + entry.value.as_ref().map_or(0, |bytes| bytes.len()))
            .sum()
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.lock().entries.get(key).cloned()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.lock().entries.contains_key(key)
    }

    pub fn put(&self, key: &str, bytes: &[u8]) {
        let mut state = self.lock();
        state.entries.insert(key.to_string(), bytes.to_vec());
        record(&mut state, key, Some(bytes.to_vec()));
    }

    pub fn delete(&self, key: &str) {
        let mut state = self.lock();
        if state.entries.remove(key).is_none() && !state.pending.contains_key(key) {
            return;
        }
        record(&mut state, key, None);
    }

    /// Delete every key under `prefix` (which is matched as a path prefix, so
    /// `a/b` matches `a/b/c` but never `a/bc`).
    pub fn delete_prefix(&self, prefix: &str) {
        let boundary = prefix_boundary(prefix);
        let keys: Vec<String> = {
            let state = self.lock();
            state
                .entries
                .range(boundary.clone()..)
                .take_while(|(key, _)| key.starts_with(&boundary))
                .map(|(key, _)| key.clone())
                .collect()
        };
        for key in keys {
            self.delete(&key);
        }
    }

    /// Keys under `prefix`, in full (not relative) form, sorted.
    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        let boundary = prefix_boundary(prefix);
        let state = self.lock();
        state
            .entries
            .range(boundary.clone()..)
            .take_while(|(key, _)| key.starts_with(&boundary))
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Everything the volume holds, for a driver that wants to rewrite the
    /// durable store from scratch, and for tests.
    pub fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        self.lock().entries.clone()
    }

    /// An [`ObjectStore`] view rooted at `root`, so one volume can hold several
    /// repositories the way one filesystem holds several directories.
    pub fn object_store(&self, root: &str) -> Result<MirroredObjectStore, StoreError> {
        Ok(MirroredObjectStore {
            volume: self.clone(),
            root: normalize_root(root)?,
        })
    }
}

fn record(state: &mut VolumeState, key: &str, value: Option<Vec<u8>>) {
    state.seq += 1;
    if !state.mirroring {
        state.durable_seq = state.seq;
        return;
    }
    let seq = state.seq;
    state
        .pending
        .insert(key.to_string(), PendingEntry { seq, value });
}

fn prefix_boundary(prefix: &str) -> String {
    let trimmed = prefix.trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

/// A volume root is a path-shaped name, not a filesystem path: the browser has
/// no filesystem, and a repository "at" `opendoc-repo` is a key prefix. Leading
/// and trailing slashes are tolerated (a user typing `/docs` means `docs`), but
/// `..` is refused so one repository cannot address another's keys.
fn normalize_root(root: &str) -> Result<String, StoreError> {
    let mut parts = Vec::new();
    for part in root.split(['/', '\\']) {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(StoreError::InvalidPath);
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

/// The [`ObjectStore`] implementation over a [`MirroredVolume`].
#[derive(Clone, Debug)]
pub struct MirroredObjectStore {
    volume: MirroredVolume,
    root: String,
}

impl MirroredObjectStore {
    pub fn volume(&self) -> &MirroredVolume {
        &self.volume
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    fn key(&self, relative: &str) -> String {
        if self.root.is_empty() {
            relative.to_string()
        } else {
            format!("{}/{relative}", self.root)
        }
    }

    fn named_key(&self, path: &str) -> Result<String, StoreError> {
        let relative = clean_relative_path(path)?
            .to_string_lossy()
            .replace('\\', "/");
        if relative.is_empty() {
            return Err(StoreError::InvalidPath);
        }
        Ok(self.key(&relative))
    }

    fn object_key(&self, hash: &HashRef) -> String {
        self.key(&ObjectStoreLayout::object_key(hash))
    }

    fn head_key(&self, document_uuid: &str, branch: &str) -> Result<String, StoreError> {
        Ok(self.key(&ObjectStoreLayout::head_key(document_uuid, branch)?))
    }
}

impl ObjectStore for MirroredObjectStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            idempotent_content_put: true,
            // The volume is the single writer of record in its runtime, and
            // every mutation on it is serialized by one lock, so a
            // read-compare-write is atomic with respect to every other caller.
            compare_and_swap_head: true,
            list_prefix: true,
            atomic_named_overwrite: true,
            // Pack files are a filesystem optimisation; a key/value volume has
            // nothing to compact.
            local_pack_files: false,
        }
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        let actual =
            digest_bytes(hash.algorithm(), bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
        let key = self.object_key(hash);
        if self.volume.contains(&key) {
            return Ok(false);
        }
        self.volume.put(&key, bytes);
        Ok(true)
    }

    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        let Some(bytes) = self.volume.get(&self.object_key(hash)) else {
            return Ok(None);
        };
        let actual =
            digest_bytes(hash.algorithm(), &bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
        Ok(Some(bytes))
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        Ok(self.volume.contains(&self.object_key(hash)))
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let key = self.named_key(path)?;
        self.volume.put(&key, bytes);
        Ok(())
    }

    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.volume.get(&self.named_key(path)?))
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let relative = clean_relative_path(prefix)?
            .to_string_lossy()
            .replace('\\', "/");
        let base = self.key(&relative);
        let boundary = prefix_boundary(&base);
        Ok(self
            .volume
            .keys_with_prefix(&base)
            .into_iter()
            .filter_map(|key| key.strip_prefix(&boundary).map(str::to_string))
            .collect())
    }

    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError> {
        let key = self.head_key(document_uuid, branch)?;
        let current = match self.volume.get(&key) {
            Some(bytes) => Some(parse_head_value(
                std::str::from_utf8(&bytes).map_err(|_| StoreError::CorruptHead)?,
            )?),
            None => None,
        };
        if current.as_ref() != expected {
            return Ok(false);
        }
        self.volume.put(&key, new.to_string().as_bytes());
        Ok(true)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        let key = self.head_key(document_uuid, branch)?;
        match self.volume.get(&key) {
            Some(bytes) => Ok(Some(parse_head_value(
                std::str::from_utf8(&bytes).map_err(|_| StoreError::CorruptHead)?,
            )?)),
            None => Ok(None),
        }
    }
}

// ---- The runtime's single volume -------------------------------------------

static BROWSER_VOLUME: OnceLock<Mutex<Option<MirroredVolume>>> = OnceLock::new();

fn browser_volume_slot() -> &'static Mutex<Option<MirroredVolume>> {
    BROWSER_VOLUME.get_or_init(|| Mutex::new(None))
}

/// Install the volume that stands in for local storage in this runtime.
///
/// There is exactly one of these per page, because a browser origin has
/// exactly one IndexedDB database behind it and one WebAssembly instance in
/// front of it. It is a process global for the same reason the filesystem is:
/// the repository code below it takes a root, not a device.
pub fn install_browser_volume(volume: MirroredVolume) {
    let mut slot = browser_volume_slot()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    *slot = Some(volume);
}

/// The installed volume, if this runtime has one.
pub fn browser_volume() -> Option<MirroredVolume> {
    browser_volume_slot()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

impl fmt::Display for VolumeMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            Some(bytes) => write!(
                formatter,
                "#{} put {} ({}B)",
                self.seq,
                self.key,
                bytes.len()
            ),
            None => write!(formatter, "#{} delete {}", self.seq, self.key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify_object_store_contract;

    fn hash_of(bytes: &[u8]) -> HashRef {
        digest_bytes("sha256", bytes).expect("sha256")
    }

    #[test]
    fn mirrored_object_store_satisfies_the_store_contract() {
        let volume = MirroredVolume::new();
        let store = volume.object_store("conformance-root").expect("root");
        verify_object_store_contract(&store, "mirrored").expect("contract");
    }

    #[test]
    fn two_roots_on_one_volume_do_not_see_each_other() {
        let volume = MirroredVolume::new();
        let left = volume.object_store("left").expect("root");
        let right = volume.object_store("right").expect("root");
        let bytes = b"shared content".to_vec();
        let hash = hash_of(&bytes);
        assert!(left.put_if_absent(&hash, &bytes).expect("put"));
        assert!(left.exists(&hash).expect("exists"));
        assert!(!right.exists(&hash).expect("exists"));
        assert!(right.put_if_absent(&hash, &bytes).expect("put"));
    }

    #[test]
    fn a_root_cannot_escape_into_another() {
        let volume = MirroredVolume::new();
        assert!(matches!(
            volume.object_store("../escape"),
            Err(StoreError::InvalidPath)
        ));
        let store = volume.object_store("/tolerated/slashes/").expect("root");
        assert_eq!(store.root(), "tolerated/slashes");
        assert!(matches!(
            store.put_named("../outside.bin", b"x"),
            Err(StoreError::InvalidPath)
        ));
    }

    #[test]
    fn every_mutation_is_queued_in_order_and_nothing_else_is() {
        let volume = MirroredVolume::new();
        let store = volume.object_store("repo").expect("root");
        let bytes = b"object".to_vec();
        let hash = hash_of(&bytes);

        store.put_if_absent(&hash, &bytes).expect("put");
        // A second put of the same content is a no-op, so it must not queue.
        store.put_if_absent(&hash, &bytes).expect("put");
        store.put_named("indexes/one.idx", b"index").expect("named");
        store
            .compare_and_swap_head("doc-1", "main", None, &hash)
            .expect("cas");

        let pending = volume.pending();
        assert_eq!(pending.len(), 3, "queued {pending:?}");
        assert_eq!(
            pending[0].key,
            format!("repo/{}", ObjectStoreLayout::object_key(&hash))
        );
        assert_eq!(pending[1].key, "repo/indexes/one.idx");
        assert_eq!(
            pending[2].key,
            format!(
                "repo/{}",
                ObjectStoreLayout::head_key("doc-1", "main").expect("head key")
            )
        );
        assert!(pending.windows(2).all(|pair| pair[0].seq < pair[1].seq));
        assert_eq!(
            volume.durable_seq(),
            0,
            "nothing is durable until a driver says so"
        );
    }

    #[test]
    fn the_head_swap_never_precedes_the_objects_it_names() {
        // The safety property the whole write-behind design rests on: a flush
        // takes a *prefix* of the sequence, so a batch holding the head swap
        // also holds everything written before it.
        let volume = MirroredVolume::new();
        let store = volume.object_store("repo").expect("root");
        let object = b"manifest bytes".to_vec();
        let hash = hash_of(&object);
        store.put_if_absent(&hash, &object).expect("put");
        store
            .compare_and_swap_head("doc-1", "main", None, &hash)
            .expect("cas");

        let batch = volume.pending();
        let head_key = format!(
            "repo/{}",
            ObjectStoreLayout::head_key("doc-1", "main").expect("head key")
        );
        let head_seq = batch
            .iter()
            .find(|mutation| mutation.key == head_key)
            .expect("head mutation")
            .seq;
        let object_seq = batch
            .iter()
            .find(|mutation| mutation.key != head_key)
            .expect("object mutation")
            .seq;
        assert!(object_seq < head_seq);
        assert!(
            batch.iter().all(|mutation| mutation.seq <= head_seq),
            "a batch containing the head must contain every earlier mutation"
        );
    }

    #[test]
    fn acknowledging_a_flush_keeps_writes_made_while_it_was_in_flight() {
        let volume = MirroredVolume::new();
        volume.put("heads/main", b"first");
        let batch = volume.pending();
        let through = batch.last().expect("mutation").seq;

        // The page keeps running while the transaction is open.
        volume.put("heads/main", b"second");
        volume.acknowledge(through);

        let still_pending = volume.pending();
        assert_eq!(still_pending.len(), 1, "pending {still_pending:?}");
        assert_eq!(still_pending[0].value.as_deref(), Some(&b"second"[..]));
    }

    #[test]
    fn repeated_writes_to_one_key_coalesce_to_the_latest_value() {
        let volume = MirroredVolume::new();
        for round in 0..50u8 {
            volume.put("heads/main", &[round]);
        }
        let pending = volume.pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].value.as_deref(), Some(&[49u8][..]));
        assert_eq!(
            volume.sequence(),
            50,
            "coalescing must not lose the ordering"
        );
    }

    #[test]
    fn deletes_are_mirrored_and_prefixes_delete_on_a_path_boundary() {
        let volume = MirroredVolume::new();
        volume.put("recovery/abc/0", b"header");
        volume.put("recovery/abc/1", b"frame");
        volume.put("recovery/abcdef/0", b"other session");
        volume.acknowledge(volume.sequence());

        volume.delete_prefix("recovery/abc");
        assert_eq!(
            volume.keys_with_prefix("recovery"),
            vec!["recovery/abcdef/0"]
        );
        let pending = volume.pending();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|mutation| mutation.value.is_none()));
    }

    #[test]
    fn hydration_is_not_a_mutation() {
        let volume = MirroredVolume::new();
        volume.hydrate([("repo/objects/sha256/aa/aabb".to_string(), b"bytes".to_vec())]);
        assert_eq!(volume.sequence(), 0);
        assert!(volume.pending().is_empty());
        assert_eq!(
            volume.get("repo/objects/sha256/aa/aabb"),
            Some(b"bytes".to_vec())
        );
    }

    #[test]
    fn a_hydrated_volume_serves_what_a_previous_session_wrote() {
        let first = MirroredVolume::new();
        let store = first.object_store("repo").expect("root");
        let bytes = b"survives the page".to_vec();
        let hash = hash_of(&bytes);
        store.put_if_absent(&hash, &bytes).expect("put");
        store
            .compare_and_swap_head("doc-1", "main", None, &hash)
            .expect("cas");

        // Durable storage keeps what the driver flushed; the page goes away.
        let durable = first.snapshot();
        let second = MirroredVolume::new();
        second.hydrate(durable);
        let reopened = second.object_store("repo").expect("root");
        assert_eq!(reopened.get(&hash).expect("get"), Some(bytes));
        assert_eq!(
            reopened.read_head("doc-1", "main").expect("head"),
            Some(hash)
        );
    }

    #[test]
    fn a_corrupt_object_is_refused_rather_than_returned() {
        let volume = MirroredVolume::new();
        let store = volume.object_store("repo").expect("root");
        let bytes = b"trusted".to_vec();
        let hash = hash_of(&bytes);
        store.put_if_absent(&hash, &bytes).expect("put");
        volume.put(
            &format!("repo/{}", ObjectStoreLayout::object_key(&hash)),
            b"tampered",
        );
        assert!(matches!(store.get(&hash), Err(StoreError::HashMismatch)));
        assert!(matches!(
            store.put_if_absent(&hash, b"not the content"),
            Err(StoreError::HashMismatch)
        ));
    }

    #[test]
    fn a_stale_expected_head_loses_the_swap() {
        let volume = MirroredVolume::new();
        let store = volume.object_store("repo").expect("root");
        let first = hash_of(b"first");
        let second = hash_of(b"second");
        assert!(store
            .compare_and_swap_head("doc-1", "main", None, &first)
            .expect("cas"));
        assert!(!store
            .compare_and_swap_head("doc-1", "main", None, &second)
            .expect("cas"));
        assert!(store
            .compare_and_swap_head("doc-1", "main", Some(&first), &second)
            .expect("cas"));
        assert_eq!(
            store.read_head("doc-1", "main").expect("head"),
            Some(second)
        );
    }

    #[test]
    fn a_volume_with_no_driver_records_nothing() {
        let volume = MirroredVolume::memory_only();
        volume.put("heads/main", b"value");
        assert!(volume.pending().is_empty());
        assert!(!volume.is_mirroring());
        assert_eq!(volume.durable_seq(), volume.sequence());
        assert_eq!(volume.get("heads/main"), Some(b"value".to_vec()));
    }

    #[test]
    fn list_prefix_reports_paths_relative_to_the_prefix() {
        let volume = MirroredVolume::new();
        let store = volume.object_store("repo").expect("root");
        store
            .put_named("indexes/by-uuid/aa/one.idx", b"1")
            .expect("named");
        store
            .put_named("indexes/by-uuid/bb/two.idx", b"2")
            .expect("named");
        store.put_named("elsewhere/three.idx", b"3").expect("named");
        let mut listed = store.list_prefix("indexes").expect("list");
        listed.sort();
        assert_eq!(listed, vec!["by-uuid/aa/one.idx", "by-uuid/bb/two.idx"]);
    }
}
