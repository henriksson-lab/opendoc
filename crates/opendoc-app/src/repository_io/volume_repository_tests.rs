//! The browser's persistence path, minus the IndexedDB binding.
//!
//! `local_object_store` resolves to a [`MirroredVolume`] only on
//! `wasm32`, where `cargo test` cannot reach it. Everything below the
//! resolution — the store, the repository, the save and open paths, and
//! the write-behind boundary — is target independent, so it is exercised
//! here against the same types the browser runs.

use super::*;
use opendoc_store::MirroredVolume;
use serde_json::json;
use std::collections::BTreeMap;

/// Stands in for IndexedDB: a map that changes only when the driver
/// flushes a batch, and that outlives the volume it was flushed from.
#[derive(Debug, Default)]
struct DurableBytes(BTreeMap<String, Vec<u8>>);

impl DurableBytes {
    /// One flush: take every pending mutation, apply it, acknowledge it.
    /// Exactly what `opendoc-wasm`'s `flush_batch` does with a
    /// `readwrite` transaction.
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

    /// A new page over the same durable store.
    fn reopen(&self) -> MirroredVolume {
        let volume = MirroredVolume::new();
        volume.hydrate(self.0.clone());
        volume
    }
}

fn repository_for(volume: &MirroredVolume) -> Repository<Box<dyn ObjectStore>> {
    Repository::new(Box::new(
        volume
            .object_store("repositories/opendoc-repo")
            .expect("volume root"),
    ) as Box<dyn ObjectStore>)
}

fn edited_app(text: &str) -> OpenDocApp {
    let mut app = OpenDocApp::new_sample();
    app.dispatch_command("create_document", json!({ "title": "Browser document" }))
        .expect("create");
    app.dispatch_command("add_paragraph", json!({ "text": text }))
        .expect("paragraph");
    app
}

#[test]
fn a_document_saved_into_a_volume_reopens_from_the_durable_bytes() {
    let mut durable = DurableBytes::default();
    let uuid;
    let visible_text;
    {
        let volume = MirroredVolume::new();
        let mut app = edited_app("persisted across a page load");
        let saved = app
            .repository_service()
            .save_to_repository_inner(
                PathBuf::from("opendoc-repo"),
                repository_for(&volume),
                false,
                "local",
                None,
            )
            .expect("save");
        assert!(!saved.has_unsaved_changes);
        uuid = saved.uuid.clone();
        visible_text = saved.visible_text();
        // The save is only in memory until the driver drains it.
        assert!(volume.pending_len() > 0);
        assert!(durable.0.is_empty());
        durable.flush(&volume);
        assert_eq!(volume.pending_len(), 0);
        assert_eq!(volume.durable_seq(), volume.sequence());
    }

    // A new page: a fresh app, a fresh volume, hydrated from IndexedDB.
    let volume = durable.reopen();
    let mut app = OpenDocApp::new_empty_document();
    let opened = app
        .repository_service()
        .open_projection_from_repository(
            PathBuf::from("opendoc-repo"),
            repository_for(&volume),
            &uuid,
            "local",
            None,
        )
        .expect("open");
    assert_eq!(opened.uuid, uuid);
    assert_eq!(opened.visible_text(), visible_text);
    assert!(opened
        .visible_text()
        .contains("persisted across a page load"));
    assert!(!opened.has_unsaved_changes);
}

#[test]
fn a_scan_of_a_hydrated_volume_finds_the_saved_document() {
    // How the browser's "Open repository" works: nothing remembers the
    // document across a page load except the repository's own index.
    let mut durable = DurableBytes::default();
    let volume = MirroredVolume::new();
    let mut app = edited_app("findable");
    let saved = app
        .repository_service()
        .save_to_repository_inner(
            PathBuf::from("opendoc-repo"),
            repository_for(&volume),
            false,
            "local",
            None,
        )
        .expect("save");
    durable.flush(&volume);

    let reopened = durable.reopen();
    let scan = repository_for(&reopened)
        .scan_lookup_entries()
        .expect("scan");
    assert!(
        scan.records
            .iter()
            .any(|record| record.document_uuid == saved.uuid),
        "hydrated volume held {} lookup records",
        scan.records.len()
    );
}

#[test]
fn an_unflushed_commit_is_lost_whole_rather_than_half_written() {
    // The write-behind bargain: a crash before the flush reverts the
    // durable store to its last *consistent* state. What must never
    // happen is a head pointing at objects that never arrived.
    let mut durable = DurableBytes::default();
    let volume = MirroredVolume::new();
    let mut app = edited_app("first save");
    let saved = app
        .repository_service()
        .save_to_repository_inner(
            PathBuf::from("opendoc-repo"),
            repository_for(&volume),
            false,
            "local",
            None,
        )
        .expect("save");
    durable.flush(&volume);
    let after_first = durable.0.clone();
    let first_manifest = saved.last_manifest.clone().expect("first manifest");

    app.dispatch_command("add_paragraph", json!({ "text": "never made it" }))
        .expect("paragraph");
    let second = app
        .repository_service()
        .save_to_repository_inner(
            PathBuf::from("opendoc-repo"),
            repository_for(&volume),
            false,
            "local",
            None,
        )
        .expect("save");
    assert_ne!(second.last_manifest, saved.last_manifest);
    // The tab dies here: the second batch never reaches the transaction.
    assert!(volume.pending_len() > 0);
    assert_eq!(durable.0, after_first);

    let reopened = durable.reopen();
    let repo = repository_for(&reopened);
    let head = repo
        .store()
        .read_head(saved.uuid.as_str(), SNAPSHOT_BRANCH)
        .expect("head")
        .expect("the flushed commit is durable");
    assert_eq!(
        head.to_string(),
        first_manifest,
        "the durable head must be the last flushed commit, not the lost one"
    );
    let manifest = repo
        .read_manifest(&head)
        .expect("manifest")
        .expect("present");
    // Every object the durable head names is durable too.
    let audit = repo
        .audit_manifest_dependencies(&manifest)
        .expect("dependency audit");
    assert!(
        audit.missing_hashes().is_empty(),
        "durable head named missing objects: {:?}",
        audit.missing_hashes()
    );

    let mut next_page = OpenDocApp::new_empty_document();
    let opened = next_page
        .repository_service()
        .open_projection_from_repository(
            PathBuf::from("opendoc-repo"),
            repository_for(&reopened),
            &saved.uuid,
            "local",
            None,
        )
        .expect("open");
    assert!(opened.visible_text().contains("first save"));
    assert!(!opened.visible_text().contains("never made it"));
}
