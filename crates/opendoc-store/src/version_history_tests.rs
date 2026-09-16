use crate::local_store::process_tag;
use crate::*;
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{encode_record, ManifestRecord, VersionLabelRecord};
use std::fs;

/// Build `count` chained commits on `doc`/`main`, each with its own snapshot
/// object present, and return the manifest hashes oldest-first.
fn commit_version_chain(repo: &Repository<LocalObjectStore>, count: usize) -> Vec<HashRef> {
    let mut hashes = Vec::new();
    let mut parent: Option<HashRef> = None;
    for index in 0..count {
        let snapshot_bytes = format!("snapshot-{index}").into_bytes();
        let snapshot = digest_bytes("sha256", &snapshot_bytes).unwrap();
        repo.store()
            .put_if_absent(&snapshot, &snapshot_bytes)
            .unwrap();
        let manifest = ManifestRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            parent: parent.clone(),
            snapshot,
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1_000 + index as u64,
        };
        let hash = repo
            .commit_manifest(&manifest, parent.as_ref())
            .unwrap()
            .unwrap();
        parent = Some(hash.clone());
        hashes.push(hash);
    }
    hashes
}

#[test]
fn version_history_walks_manifest_parents_newest_first() {
    let root = std::env::temp_dir().join(format!("opendoc-versions-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = commit_version_chain(&repo, 3);

    let history = repo.list_versions("doc", "main", None);
    assert_eq!(history.head.as_ref(), chain.last());
    assert!(!history.truncated);
    assert!(history.problems.is_empty(), "{:?}", history.problems);
    let listed: Vec<_> = history
        .entries
        .iter()
        .map(|entry| entry.manifest.clone())
        .collect();
    let mut expected = chain.clone();
    expected.reverse();
    assert_eq!(listed, expected);
    assert!(history.entries[0].is_head);
    assert!(!history.entries[1].is_head);
    assert_eq!(history.entries[0].created_at_ms, 1_002);
    assert_eq!(history.entries[2].parent, None);
    assert!(history.entries.iter().all(|entry| entry.snapshot_present));
    assert!(history.entries.iter().all(|entry| entry.label.is_none()));

    let history = repo.list_versions("doc", "main", Some(2));
    assert_eq!(history.entries.len(), 2);
    assert!(history.truncated);

    let empty = repo.list_versions("other-doc", "main", None);
    assert!(empty.head.is_none());
    assert!(empty.entries.is_empty());
    assert!(empty.problems.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn version_history_degrades_when_an_ancestor_manifest_is_missing() {
    let root = std::env::temp_dir().join(format!("opendoc-versions-gap-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = commit_version_chain(&repo, 3);

    // Remove the middle manifest object: the chain is now broken below the
    // head, which must degrade to a warning instead of an error.
    let middle = root.join(ObjectStoreLayout::object_key(&chain[1]));
    fs::remove_file(&middle).unwrap();

    let history = repo.list_versions("doc", "main", None);
    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].manifest, chain[2]);
    assert!(history.truncated);
    assert_eq!(history.problems.len(), 1);
    assert_eq!(history.problems[0].manifest.as_ref(), Some(&chain[1]));
    assert!(
        history.problems[0]
            .reason
            .contains("manifest object is missing"),
        "{}",
        history.problems[0].reason
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn version_history_reports_a_missing_snapshot_but_still_lists_the_version() {
    let root = std::env::temp_dir().join(format!("opendoc-versions-snap-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = commit_version_chain(&repo, 2);
    let oldest = repo.read_manifest(&chain[0]).unwrap().unwrap();
    fs::remove_file(root.join(ObjectStoreLayout::object_key(&oldest.snapshot))).unwrap();

    let history = repo.list_versions("doc", "main", None);
    assert_eq!(history.entries.len(), 2, "the whole chain is still listed");
    assert!(history.entries[0].snapshot_present);
    assert!(!history.entries[1].snapshot_present);
    assert!(!history.truncated);
    assert_eq!(history.problems.len(), 1);
    assert!(history.problems[0]
        .reason
        .contains("snapshot object is missing"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn version_history_degrades_when_a_manifest_object_is_corrupt() {
    let root = std::env::temp_dir().join(format!("opendoc-versions-corrupt-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = commit_version_chain(&repo, 2);
    // Overwrite the parent object's bytes: reading it now fails the digest
    // check rather than returning `None`.
    fs::write(
        root.join(ObjectStoreLayout::object_key(&chain[0])),
        b"corrupt",
    )
    .unwrap();

    let history = repo.list_versions("doc", "main", None);
    assert_eq!(history.entries.len(), 1);
    assert!(history.truncated);
    assert_eq!(history.problems.len(), 1);
    assert!(
        history.problems[0]
            .reason
            .contains("manifest object could not be read"),
        "{}",
        history.problems[0].reason
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn version_labels_round_trip_as_manifest_sidecars() {
    let root = std::env::temp_dir().join(format!("opendoc-versions-label-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = commit_version_chain(&repo, 2);

    assert_eq!(repo.read_version_label(&chain[0]).unwrap(), None);
    let label = VersionLabelRecord {
        manifest: chain[0].clone(),
        document_uuid: "doc".to_string(),
        branch: "main".to_string(),
        label: "Before the rewrite".to_string(),
        author: "Ada".to_string(),
        created_at_ms: 7,
    };
    let path = repo.write_version_label(&label).unwrap();
    // Derived from the manifest hash's own text, not by re-calling the
    // function that produced the answer: comparing `write_version_label`'s
    // path against `ObjectStoreLayout::version_label_key` — the very call it
    // makes — cannot fail, and passed for any layout at all. PLAN88 §7.
    let printed = chain[0].to_string();
    let (algorithm, digest) = printed
        .split_once(':')
        .expect("a hash ref prints as algorithm:digest");
    assert_eq!(algorithm, "sha256");
    assert_eq!(
        path,
        format!("objects/sha256/{}/{}.label", &digest[..2], digest),
        "the sidecar is not sharded beside the manifest object it names"
    );
    // …and it really is a file at that name, so the string and the bytes on
    // disk cannot drift apart.
    assert!(
        root.join(&path).is_file(),
        "nothing was written at {path} under {}",
        root.display()
    );
    assert_eq!(repo.read_version_label(&chain[0]).unwrap(), Some(label));

    // Naming a version never rewrites it: the manifest chain is untouched.
    let history = repo.list_versions("doc", "main", None);
    assert_eq!(history.head.as_ref(), chain.last());
    assert_eq!(history.entries.len(), 2);
    assert!(history.entries[0].label.is_none());
    assert_eq!(
        history.entries[1]
            .label
            .as_ref()
            .map(|record| record.label.as_str()),
        Some("Before the rewrite")
    );

    // A sidecar naming a different manifest is not trusted.
    let mismatched = VersionLabelRecord {
        manifest: chain[1].clone(),
        document_uuid: "doc".to_string(),
        branch: "main".to_string(),
        label: "Wrong target".to_string(),
        author: "Ada".to_string(),
        created_at_ms: 8,
    };
    repo.store()
        .put_named(
            &ObjectStoreLayout::version_label_key(&chain[0]),
            &encode_record(&mismatched),
        )
        .unwrap();
    assert!(matches!(
        repo.read_version_label(&chain[0]),
        Err(StoreError::HashMismatch)
    ));
    let history = repo.list_versions("doc", "main", None);
    assert_eq!(history.entries.len(), 2, "a bad label never hides history");
    assert!(history
        .problems
        .iter()
        .any(|problem| problem.reason.contains("version label could not be read")));

    let empty = VersionLabelRecord {
        manifest: chain[0].clone(),
        document_uuid: "doc".to_string(),
        branch: "main".to_string(),
        label: String::new(),
        author: "Ada".to_string(),
        created_at_ms: 9,
    };
    assert!(matches!(
        repo.write_version_label(&empty),
        Err(StoreError::Format(_))
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn named_paths_cannot_escape_repository_root() {
    let root = std::env::temp_dir().join(format!("opendoc-repo-path-safety-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    assert!(matches!(
        store.put_named("../outside", b"nope"),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        store.get_named("/absolute"),
        Err(StoreError::InvalidPath)
    ));
    let _ = fs::remove_dir_all(root);
}
