//! What a repository can prove about a *signed* version's history.
//!
//! `opendoc-sign` owns whether a signature verifies. This module owns the
//! other half: given the coverage record a signature was taken over, does this
//! repository still hold the manifests, snapshots and segments that record
//! names? A signature is a statement about bytes and says nothing about
//! availability, so deletion is invisible to it — the walk here is what makes
//! it visible.

use crate::local_store::process_tag;
use crate::*;
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{encode_record, ManifestRecord, SignatureRecord, VersionCoverageRecord};
use std::fs;
use std::path::PathBuf;

const DOCUMENT: &str = "coverage-doc";
const BRANCH: &str = "main";

fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("opendoc-coverage-{tag}-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    root
}

fn put(repo: &Repository<LocalObjectStore>, bytes: &[u8]) -> HashRef {
    let hash = digest_bytes("sha256", bytes).unwrap();
    repo.store().put_if_absent(&hash, bytes).unwrap();
    hash
}

/// A three-version chain with a real snapshot, segment and blob object behind
/// every manifest, so "missing" in a test means something was removed and not
/// merely never written.
struct Chain {
    manifests: Vec<HashRef>,
    snapshots: Vec<HashRef>,
    segments: Vec<HashRef>,
    blob: HashRef,
}

fn build_chain(repo: &Repository<LocalObjectStore>, versions: usize) -> Chain {
    let blob = put(repo, b"blob bytes");
    let mut chain = Chain {
        manifests: Vec::new(),
        snapshots: Vec::new(),
        segments: Vec::new(),
        blob: blob.clone(),
    };
    let mut parent = None;
    for index in 0..versions {
        let snapshot = put(repo, format!("snapshot {index}").as_bytes());
        let segment = put(repo, format!("segment {index}").as_bytes());
        let manifest = ManifestRecord {
            document_uuid: DOCUMENT.to_string(),
            branch: BRANCH.to_string(),
            parent: parent.clone(),
            snapshot: snapshot.clone(),
            operation_segments: vec![segment.clone()],
            signatures: Vec::new(),
            blobs: vec![blob.clone()],
            created_at_ms: 1_000 + index as u64,
        };
        let hash = repo
            .commit_manifest(&manifest, parent.as_ref())
            .unwrap()
            .unwrap();
        parent = Some(hash.clone());
        chain.manifests.push(hash);
        chain.snapshots.push(snapshot);
        chain.segments.push(segment);
    }
    chain
}

fn coverage_of(repo: &Repository<LocalObjectStore>, manifest: &HashRef) -> VersionCoverageRecord {
    VersionCoverageRecord::for_manifest(&repo.read_manifest(manifest).unwrap().unwrap()).unwrap()
}

fn signature_over(target: &HashRef, signer: &str) -> SignatureRecord {
    SignatureRecord {
        target: target.clone(),
        signer: signer.to_string(),
        signer_display: "Signer".to_string(),
        title: "Version".to_string(),
        signed_at_ms: 7,
        signature: b"not a real signature".to_vec(),
    }
}

#[test]
fn a_complete_chain_is_not_reported_as_truncated() {
    let root = temp_root("complete");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 3);
    let head = chain.manifests.last().unwrap();

    let audit = repo
        .audit_manifest_chain(&coverage_of(&repo, head))
        .unwrap();

    assert_eq!(audit.problems, Vec::new());
    assert!(audit.reaches_root);
    assert!(!audit.history_is_truncated());
    assert_eq!(
        audit.chain,
        chain.manifests.iter().rev().cloned().collect::<Vec<_>>(),
        "the walk reports the whole ancestry, newest first"
    );
    let _ = fs::remove_dir_all(root);
}

/// The case the version signature alone cannot see: the head manifest is
/// byte-for-byte what was signed, and its parent is gone.
#[test]
fn a_deleted_ancestor_manifest_is_named_as_truncation() {
    let root = temp_root("deleted-ancestor");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 3);
    let head = chain.manifests[2].clone();
    let coverage = coverage_of(&repo, &head);

    fs::remove_file(repo.store().object_path(&chain.manifests[1])).unwrap();

    // The signed head is untouched, so nothing about the signature changes.
    assert_eq!(
        coverage_of(&repo, &head),
        coverage,
        "the signed manifest still derives the same coverage"
    );

    let audit = repo.audit_manifest_chain(&coverage).unwrap();
    assert!(audit.history_is_truncated());
    assert!(!audit.reaches_root);
    assert_eq!(audit.missing_manifests(), vec![chain.manifests[1].clone()]);
    assert_eq!(audit.chain, vec![head.clone()]);
    assert!(
        audit
            .problems
            .contains(&ManifestChainProblem::ManifestMissing {
                manifest: chain.manifests[1].clone(),
                referenced_by: Some(head),
            }),
        "{:?}",
        audit.problems
    );
    let _ = fs::remove_dir_all(root);
}

/// The whole signed version is gone and only its signed coverage survives.
/// That record still names a parent, so truncation is still provable.
#[test]
fn a_deleted_signed_manifest_still_reports_what_it_claimed() {
    let root = temp_root("deleted-head");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 2);
    let head = chain.manifests[1].clone();
    let coverage = coverage_of(&repo, &head);
    repo.write_version_coverage(&coverage).unwrap();

    fs::remove_file(repo.store().object_path(&head)).unwrap();
    assert!(repo.read_manifest(&head).unwrap().is_none());

    let stored = repo.read_version_coverage(&head).unwrap().expect("sidecar");
    assert_eq!(stored, coverage);
    assert_eq!(
        stored.parent,
        Some(chain.manifests[0].clone()),
        "the surviving record still says this version had a parent"
    );

    let audit = repo.audit_manifest_chain(&stored).unwrap();
    assert!(audit.history_is_truncated());
    assert_eq!(audit.chain, Vec::new());
    assert_eq!(audit.missing_manifests(), vec![head.clone()]);
    assert!(
        audit
            .problems
            .contains(&ManifestChainProblem::ManifestMissing {
                manifest: head,
                referenced_by: None,
            }),
        "{:?}",
        audit.problems
    );
    let _ = fs::remove_dir_all(root);
}

/// ADR 0002 makes deferring large blob downloads a supported mode, so a
/// repository without blob bytes is a shallow clone, not a tampered one.
#[test]
fn absent_blob_bytes_are_reported_without_being_called_truncation() {
    let root = temp_root("shallow");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 2);
    let head = chain.manifests[1].clone();

    fs::remove_file(repo.store().object_path(&chain.blob)).unwrap();

    let audit = repo
        .audit_manifest_chain(&coverage_of(&repo, &head))
        .unwrap();
    assert!(
        !audit.history_is_truncated(),
        "a shallow clone is not a truncated history: {:?}",
        audit.problems
    );
    assert!(audit.reaches_root);
    assert_eq!(
        audit.absent_blobs(),
        vec![chain.blob.clone(), chain.blob],
        "every manifest that named the blob reports it"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_deleted_snapshot_or_segment_in_the_chain_is_truncation() {
    let root = temp_root("deleted-objects");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 3);
    let head = chain.manifests[2].clone();
    let coverage = coverage_of(&repo, &head);

    fs::remove_file(repo.store().object_path(&chain.snapshots[0])).unwrap();
    fs::remove_file(repo.store().object_path(&chain.segments[1])).unwrap();

    let audit = repo.audit_manifest_chain(&coverage).unwrap();
    assert!(audit.history_is_truncated());
    assert!(
        audit.reaches_root,
        "the manifest chain itself is intact; what is gone is what it named"
    );
    assert!(
        audit
            .problems
            .contains(&ManifestChainProblem::SnapshotMissing {
                manifest: chain.manifests[0].clone(),
                snapshot: chain.snapshots[0].clone(),
            }),
        "the deleted snapshot of the oldest version is not named: {:?}",
        audit.problems
    );
    assert!(
        audit
            .problems
            .contains(&ManifestChainProblem::OperationSegmentMissing {
                manifest: chain.manifests[1].clone(),
                segment: chain.segments[1].clone(),
            }),
        "the deleted operation segment of the middle version is not named: {:?}",
        audit.problems
    );
    let _ = fs::remove_dir_all(root);
}

/// A parent link that leaves the document is not a shortcut into someone
/// else's history.
#[test]
fn a_chain_that_leaves_the_document_stops_and_says_so() {
    let root = temp_root("foreign");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let foreign_snapshot = put(&repo, b"foreign snapshot");
    let foreign = ManifestRecord {
        document_uuid: "other-document".to_string(),
        branch: BRANCH.to_string(),
        parent: None,
        snapshot: foreign_snapshot,
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let foreign_hash = repo.write_manifest(&foreign).unwrap();
    let snapshot = put(&repo, b"our snapshot");
    let ours = ManifestRecord {
        document_uuid: DOCUMENT.to_string(),
        branch: BRANCH.to_string(),
        parent: Some(foreign_hash.clone()),
        snapshot,
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };
    let hash = repo.write_manifest(&ours).unwrap();

    let audit = repo
        .audit_manifest_chain(&VersionCoverageRecord::for_manifest(&ours).unwrap())
        .unwrap();
    assert!(audit.history_is_truncated());
    assert!(!audit.reaches_root);
    assert_eq!(audit.chain, vec![hash]);
    assert!(
        audit
            .problems
            .contains(&ManifestChainProblem::WrongDocument {
                manifest: foreign_hash,
                document_uuid: "other-document".to_string(),
                branch: BRANCH.to_string(),
            }),
        "{:?}",
        audit.problems
    );
    let _ = fs::remove_dir_all(root);
}

/// The walk is bounded, and hitting the bound is reported as not knowing the
/// rest of the history rather than as having seen all of it.
#[test]
fn a_chain_longer_than_the_traversal_limit_stops_and_reports_it() {
    // An in-memory volume: 4097 manifests on disk would dominate the suite's
    // runtime, and nothing here is about the filesystem.
    let volume = MirroredVolume::new();
    let repo = Repository::new(volume.object_store("repo").unwrap());
    let snapshot_bytes = b"shared snapshot";
    let snapshot = digest_bytes("sha256", snapshot_bytes).unwrap();
    repo.store()
        .put_if_absent(&snapshot, snapshot_bytes)
        .unwrap();
    let mut parent = None;
    let mut last = None;
    for index in 0..=VERSION_HISTORY_TRAVERSAL_LIMIT {
        let manifest = ManifestRecord {
            document_uuid: DOCUMENT.to_string(),
            branch: BRANCH.to_string(),
            parent: parent.clone(),
            snapshot: snapshot.clone(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: index as u64,
        };
        let hash = repo.write_manifest(&manifest).unwrap();
        parent = Some(hash.clone());
        last = Some(manifest);
    }
    let coverage = VersionCoverageRecord::for_manifest(&last.unwrap()).unwrap();

    let audit = repo.audit_manifest_chain(&coverage).unwrap();
    assert_eq!(audit.chain.len(), VERSION_HISTORY_TRAVERSAL_LIMIT);
    assert!(!audit.reaches_root);
    assert!(audit.history_is_truncated());
    assert!(
        audit
            .problems
            .iter()
            .any(|problem| matches!(problem, ManifestChainProblem::TraversalLimit { .. })),
        "{:?}",
        audit.problems
    );
}

#[test]
fn version_signatures_accumulate_per_signer_and_refuse_a_foreign_target() {
    let root = temp_root("sidecars");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 2);
    let head = chain.manifests[1].clone();
    let coverage = coverage_of(&repo, &head);

    assert_eq!(repo.read_signed_version(&head).unwrap(), None);

    repo.write_version_signature(&coverage, &signature_over(&head, "ssh-ed25519 AAAA-alice"))
        .unwrap();
    repo.write_version_signature(&coverage, &signature_over(&head, "ssh-ed25519 BBBB-bob"))
        .unwrap();

    let signed = repo.read_signed_version(&head).unwrap().expect("signed");
    assert_eq!(signed.coverage, coverage);
    let mut signers = signed
        .signatures
        .iter()
        .map(|record| record.signer.clone())
        .collect::<Vec<_>>();
    signers.sort();
    assert_eq!(
        signers,
        vec![
            "ssh-ed25519 AAAA-alice".to_string(),
            "ssh-ed25519 BBBB-bob".to_string()
        ],
        "ADR 0003: a version may have multiple signatures"
    );

    // Writing the same signer again replaces, it does not duplicate.
    repo.write_version_signature(&coverage, &signature_over(&head, "ssh-ed25519 AAAA-alice"))
        .unwrap();
    assert_eq!(repo.read_version_signatures(&head).unwrap().len(), 2);

    // A signature naming another manifest is not this version's signature.
    let other = chain.manifests[0].clone();
    assert!(matches!(
        repo.write_version_signature(&coverage, &signature_over(&other, "ssh-ed25519 CCCC-eve")),
        Err(StoreError::HashMismatch)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_coverage_sidecar_is_refused_when_it_is_not_canonical_or_names_another_manifest() {
    let root = temp_root("bad-sidecar");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 2);
    let head = chain.manifests[1].clone();
    let coverage = coverage_of(&repo, &head);
    let path = repo.write_version_coverage(&coverage).unwrap();
    assert_eq!(path, ObjectStoreLayout::version_coverage_key(&head));
    assert_eq!(repo.read_version_coverage(&head).unwrap(), Some(coverage));

    // Non-canonical CBOR that decodes to the same record.
    let non_canonical = cbor2::to_vec(&coverage_of(&repo, &head)).unwrap();
    repo.store().put_named(&path, &non_canonical).unwrap();
    assert!(matches!(
        repo.read_version_coverage(&head),
        Err(StoreError::Format(message)) if message.contains("not canonical")
    ));

    // Authentic bytes, wrong slot: the coverage of the parent filed under the
    // head's hash.
    let parent_coverage = coverage_of(&repo, &chain.manifests[0]);
    repo.store()
        .put_named(&path, &parent_coverage.signing_payload().unwrap())
        .unwrap();
    assert!(matches!(
        repo.read_version_coverage(&head),
        Err(StoreError::HashMismatch)
    ));

    // And a record that is not a coverage record at all.
    let mut foreign = parent_coverage;
    foreign.kind = "opendoc.snapshot.v0".to_string();
    repo.store()
        .put_named(&path, &cbor2::to_canonical_vec(&foreign).unwrap())
        .unwrap();
    assert!(matches!(
        repo.read_version_coverage(&head),
        Err(StoreError::Format(message)) if message.contains("opendoc.snapshot.v0")
    ));
    let _ = fs::remove_dir_all(root);
}

/// A version signature sidecar filed under one manifest but naming another is
/// refused on the way out as well as on the way in, because a store is not
/// only written through this API.
#[test]
fn a_version_signature_sidecar_filed_under_the_wrong_manifest_is_refused_on_read() {
    let root = temp_root("lifted-sidecar");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 2);
    let head = chain.manifests[1].clone();
    let parent = chain.manifests[0].clone();
    let parent_coverage = coverage_of(&repo, &parent);
    repo.write_version_signature(
        &parent_coverage,
        &signature_over(&parent, "ssh-ed25519 AAAA-alice"),
    )
    .unwrap();

    // Copy the parent's signature sidecar into the head's slot by hand.
    let from = ObjectStoreLayout::version_signature_key(&parent, "ssh-ed25519 AAAA-alice").unwrap();
    let to = ObjectStoreLayout::version_signature_key(&head, "ssh-ed25519 AAAA-alice").unwrap();
    let bytes = repo.store().get_named(&from).unwrap().unwrap();
    repo.store().put_named(&to, &bytes).unwrap();

    assert!(matches!(
        repo.read_version_signatures(&head),
        Err(StoreError::HashMismatch)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn writing_a_version_signature_writes_the_payload_it_was_taken_over() {
    let root = temp_root("payload-written");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 2);
    let head = chain.manifests[1].clone();
    let coverage = coverage_of(&repo, &head);

    repo.write_version_signature(&coverage, &signature_over(&head, "ssh-ed25519 AAAA-alice"))
        .unwrap();

    let stored = repo
        .store()
        .get_named(&ObjectStoreLayout::version_coverage_key(&head))
        .unwrap()
        .expect("the coverage record is written alongside the signature");
    assert_eq!(
        stored,
        coverage.signing_payload().unwrap(),
        "the stored sidecar is byte-identical to the signed payload"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn encoded_signature_records_are_unchanged_by_the_version_sidecar_path() {
    // A guard on the assumption `read_version_signatures` makes: the sidecar
    // holds a plain binary `SignatureRecord`, the same encoding the manifest's
    // own signature objects use.
    let root = temp_root("encoding");
    let repo = Repository::new(LocalObjectStore::new(&root));
    let chain = build_chain(&repo, 1);
    let head = chain.manifests[0].clone();
    let coverage = coverage_of(&repo, &head);
    let record = signature_over(&head, "ssh-ed25519 AAAA-alice");
    let path = repo.write_version_signature(&coverage, &record).unwrap();

    assert_eq!(
        repo.store().get_named(&path).unwrap().unwrap(),
        encode_record(&record)
    );
    let _ = fs::remove_dir_all(root);
}

/// `read_version_signatures` lists a prefix, and every backend spells that
/// differently. A sidecar that only works on the local store is a sidecar that
/// vanishes in the browser.
#[test]
fn version_sidecars_round_trip_on_every_object_store() {
    fn exercise<S: ObjectStore>(store: S, label: &str) {
        let repo = Repository::new(store);
        let snapshot_bytes = b"snapshot";
        let snapshot = digest_bytes("sha256", snapshot_bytes).unwrap();
        repo.store()
            .put_if_absent(&snapshot, snapshot_bytes)
            .unwrap();
        let manifest = ManifestRecord {
            document_uuid: DOCUMENT.to_string(),
            branch: BRANCH.to_string(),
            parent: None,
            snapshot,
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let hash = repo.write_manifest(&manifest).unwrap();
        let coverage = VersionCoverageRecord::for_manifest(&manifest).unwrap();
        repo.write_version_signature(&coverage, &signature_over(&hash, "ssh-ed25519 AAAA-alice"))
            .unwrap();
        repo.write_version_signature(&coverage, &signature_over(&hash, "ssh-ed25519 BBBB-bob"))
            .unwrap();

        let signed = repo
            .read_signed_version(&hash)
            .unwrap()
            .unwrap_or_else(|| panic!("{label}: the sidecars did not read back"));
        assert_eq!(signed.coverage, coverage, "{label}");
        assert_eq!(
            signed.signatures.len(),
            2,
            "{label}: both signers did not survive the round trip"
        );
        assert!(!repo
            .audit_manifest_chain(&signed.coverage)
            .unwrap()
            .history_is_truncated());
    }

    let root = temp_root("backends");
    exercise(LocalObjectStore::new(&root), "LocalObjectStore");
    let flat_root = temp_root("backends-flat");
    exercise(
        FlatObjectStore::new(&flat_root, "repo").unwrap(),
        "FlatObjectStore",
    );
    let volume = MirroredVolume::new();
    exercise(volume.object_store("repo").unwrap(), "MirroredObjectStore");
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(flat_root);
}
