use crate::keys::{doi_lookup_path, uuid_lookup_path};
use crate::local_store::process_tag;
use crate::*;
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{
    encode_record, BranchHeadRecord, LookupAliasRecord, LookupRecord, ManifestRecord,
    SignatureRecord, TombstoneRecord,
};
use std::fs;

#[test]
fn repository_commits_and_reads_manifest() {
    let root = std::env::temp_dir().join(format!("opendoc-repo-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let manifest = ManifestRecord {
        document_uuid: "doc".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let hash = repo.commit_manifest(&manifest, None).unwrap().unwrap();
    assert_eq!(repo.read_manifest(&hash).unwrap(), Some(manifest));
    assert_eq!(repo.store().read_head("doc", "main").unwrap(), Some(hash));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_rejects_corrupt_manifest_object_bytes() {
    let root =
        std::env::temp_dir().join(format!("opendoc-repo-corrupt-manifest-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    let repo = Repository::new(store.clone());
    let manifest = ManifestRecord {
        document_uuid: "doc-corrupt-manifest".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let hash = repo.commit_manifest(&manifest, None).unwrap().unwrap();
    fs::write(store.object_path(&hash), b"corrupted manifest bytes").unwrap();

    assert!(matches!(
        repo.read_manifest(&hash),
        Err(StoreError::HashMismatch)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_commits_and_plans_candidates_on_flat_store() {
    let root = std::env::temp_dir().join(format!("opendoc-flat-repo-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(FlatObjectStore::new(&root, "bucket/repo").unwrap());
    let base = ManifestRecord {
        document_uuid: "doc-flat".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
    let current = ManifestRecord {
        document_uuid: "doc-flat".to_string(),
        branch: "main".to_string(),
        parent: Some(base_hash.clone()),
        snapshot: HashRef::parse("sha256:bbb").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };
    let current_hash = repo
        .commit_manifest(&current, Some(&base_hash))
        .unwrap()
        .unwrap();
    let candidate = ManifestRecord {
        document_uuid: "doc-flat".to_string(),
        branch: "main".to_string(),
        parent: Some(base_hash.clone()),
        snapshot: HashRef::parse("sha256:ccc").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 3,
    };
    let candidate_hash = match repo
        .commit_manifest_or_candidate(&candidate, Some(&base_hash))
        .unwrap()
    {
        CommitOutcome::Candidate { manifest, .. } => manifest,
        other => panic!("expected stale flat-store commit to become candidate, got {other:?}"),
    };

    assert_eq!(
        repo.store().read_head("doc-flat", "main").unwrap(),
        Some(current_hash.clone())
    );
    assert_eq!(
        repo.read_manifest(&candidate_hash).unwrap(),
        Some(candidate)
    );
    let plans = repo.plan_candidate_merges("doc-flat", "main").unwrap();
    assert_eq!(
        plans.plans,
        vec![CandidateMergePlan {
            current: Some(current_hash.clone()),
            candidate: candidate_hash.clone(),
            merge_base: Some(base_hash),
            current_since_base: vec![current_hash],
            candidate_since_base: vec![candidate_hash],
        }]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_can_write_candidate_head_when_cas_fails() {
    let root = std::env::temp_dir().join(format!("opendoc-repo-candidate-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let first = ManifestRecord {
        document_uuid: "doc-candidate".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let second = ManifestRecord {
        document_uuid: "doc-candidate".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:bbb").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };

    let first_hash = match repo.commit_manifest_or_candidate(&first, None).unwrap() {
        CommitOutcome::Committed(hash) => hash,
        other => panic!("expected committed first manifest, got {other:?}"),
    };
    let outcome = repo.commit_manifest_or_candidate(&second, None).unwrap();
    let (second_hash, path) = match outcome {
        CommitOutcome::Candidate { manifest, path } => (manifest, path),
        other => panic!("expected candidate second manifest, got {other:?}"),
    };

    assert_eq!(
        repo.store().read_head("doc-candidate", "main").unwrap(),
        Some(first_hash)
    );
    assert!(path.contains("/head-candidates/main/sha256/"));
    let candidates = repo.list_candidate_heads("doc-candidate", "main").unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].manifest, second_hash);
    assert_eq!(repo.read_manifest(&second_hash).unwrap(), Some(second));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_resolves_candidate_heads_deterministically() {
    let root =
        std::env::temp_dir().join(format!("opendoc-repo-resolve-candidates-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let base = ManifestRecord {
        document_uuid: "doc-resolve".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
    let fast_forward = ManifestRecord {
        document_uuid: "doc-resolve".to_string(),
        branch: "main".to_string(),
        parent: Some(base_hash.clone()),
        snapshot: HashRef::parse("sha256:bbb").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };
    let divergent = ManifestRecord {
        document_uuid: "doc-resolve".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:ccc").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 3,
    };
    let fast_forward_hash = repo.write_manifest(&fast_forward).unwrap();
    let divergent_hash = repo.write_manifest(&divergent).unwrap();
    for manifest in [
        fast_forward_hash.clone(),
        divergent_hash.clone(),
        base_hash.clone(),
    ] {
        repo.write_candidate_head(&BranchHeadRecord {
            document_uuid: "doc-resolve".to_string(),
            branch: "main".to_string(),
            manifest,
        })
        .unwrap();
    }
    let missing_hash = HashRef::parse("sha256:dddddd").unwrap();
    repo.write_candidate_head(&BranchHeadRecord {
        document_uuid: "doc-resolve".to_string(),
        branch: "main".to_string(),
        manifest: missing_hash.clone(),
    })
    .unwrap();

    let resolution = repo.resolve_candidate_heads("doc-resolve", "main").unwrap();
    assert_eq!(resolution.current, Some(base_hash.clone()));
    assert_eq!(
        resolution
            .candidates
            .iter()
            .map(|candidate| (&candidate.manifest, candidate.status))
            .collect::<Vec<_>>(),
        vec![
            (&fast_forward_hash, CandidateStatus::FastForward),
            (&divergent_hash, CandidateStatus::NeedsMerge),
            (&base_hash, CandidateStatus::AlreadyCurrent),
            (&missing_hash, CandidateStatus::MissingManifest),
        ]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_resolves_candidate_heads_while_reporting_invalid_records() {
    let root =
        std::env::temp_dir().join(format!("opendoc-repo-invalid-candidates-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let base = ManifestRecord {
        document_uuid: "doc-invalid-candidates".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
    let fast_forward = ManifestRecord {
        document_uuid: "doc-invalid-candidates".to_string(),
        branch: "main".to_string(),
        parent: Some(base_hash.clone()),
        snapshot: HashRef::parse("sha256:bbb").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };
    let fast_forward_hash = repo.write_manifest(&fast_forward).unwrap();
    repo.write_candidate_head(&BranchHeadRecord {
        document_uuid: "doc-invalid-candidates".to_string(),
        branch: "main".to_string(),
        manifest: fast_forward_hash.clone(),
    })
    .unwrap();

    let malformed_hash = HashRef::parse("sha256:cccccc").unwrap();
    repo.store()
        .put_named(
            &ObjectStoreLayout::candidate_head_key(
                "doc-invalid-candidates",
                "main",
                &malformed_hash,
            )
            .unwrap(),
            b"not a branch head record",
        )
        .unwrap();
    let misplaced_hash = HashRef::parse("sha256:dddddd").unwrap();
    repo.store()
        .put_named(
            &ObjectStoreLayout::candidate_head_key(
                "doc-invalid-candidates",
                "main",
                &misplaced_hash,
            )
            .unwrap(),
            &encode_record(&BranchHeadRecord {
                document_uuid: "other-doc".to_string(),
                branch: "main".to_string(),
                manifest: misplaced_hash,
            }),
        )
        .unwrap();
    let path_hash = HashRef::parse("sha256:eeeeee").unwrap();
    let record_hash = HashRef::parse("sha256:ffffff").unwrap();
    repo.store()
        .put_named(
            &ObjectStoreLayout::candidate_head_key("doc-invalid-candidates", "main", &path_hash)
                .unwrap(),
            &encode_record(&BranchHeadRecord {
                document_uuid: "doc-invalid-candidates".to_string(),
                branch: "main".to_string(),
                manifest: record_hash,
            }),
        )
        .unwrap();

    let resolution = repo
        .resolve_candidate_heads("doc-invalid-candidates", "main")
        .unwrap();
    assert_eq!(resolution.current, Some(base_hash));
    assert_eq!(resolution.candidates.len(), 1);
    assert_eq!(resolution.candidates[0].manifest, fast_forward_hash);
    assert_eq!(
        resolution.candidates[0].status,
        CandidateStatus::FastForward
    );
    assert_eq!(resolution.invalid_candidates.len(), 3);
    assert!(resolution
        .invalid_candidates
        .iter()
        .any(|candidate| candidate.reason.contains("InvalidMagic")));
    assert!(resolution
        .invalid_candidates
        .iter()
        .any(|candidate| candidate.reason.contains("other-doc:main")));
    assert!(resolution
        .invalid_candidates
        .iter()
        .any(|candidate| candidate.reason.contains(
            "candidate head path targets sha256:eeeeee but record targets sha256:ffffff"
        )));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_lists_valid_candidate_heads_while_ignoring_invalid_records() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-repo-list-valid-candidates-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let valid_manifest = HashRef::parse("sha256:abc123").unwrap();
    repo.write_candidate_head(&BranchHeadRecord {
        document_uuid: "doc-list-candidates".to_string(),
        branch: "main".to_string(),
        manifest: valid_manifest.clone(),
    })
    .unwrap();
    let invalid_manifest = HashRef::parse("sha256:def456").unwrap();
    repo.store()
        .put_named(
            &ObjectStoreLayout::candidate_head_key(
                "doc-list-candidates",
                "main",
                &invalid_manifest,
            )
            .unwrap(),
            b"not a branch head record",
        )
        .unwrap();

    let listed = repo
        .list_candidate_heads("doc-list-candidates", "main")
        .unwrap();
    assert_eq!(
        listed,
        vec![BranchHeadRecord {
            document_uuid: "doc-list-candidates".to_string(),
            branch: "main".to_string(),
            manifest: valid_manifest,
        }]
    );
    let resolution = repo
        .resolve_candidate_heads("doc-list-candidates", "main")
        .unwrap();
    assert_eq!(resolution.invalid_candidates.len(), 1);
    assert!(resolution.invalid_candidates[0]
        .reason
        .contains("InvalidMagic"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_fast_forwards_candidate_heads_with_cas() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-repo-fast-forward-candidates-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let base = ManifestRecord {
        document_uuid: "doc-advance".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
    let fast_forward = ManifestRecord {
        document_uuid: "doc-advance".to_string(),
        branch: "main".to_string(),
        parent: Some(base_hash.clone()),
        snapshot: HashRef::parse("sha256:bbb").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };
    let divergent = ManifestRecord {
        document_uuid: "doc-advance".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:ccc").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 3,
    };
    let fast_forward_hash = repo.write_manifest(&fast_forward).unwrap();
    let divergent_hash = repo.write_manifest(&divergent).unwrap();
    for manifest in [fast_forward_hash.clone(), divergent_hash.clone()] {
        repo.write_candidate_head(&BranchHeadRecord {
            document_uuid: "doc-advance".to_string(),
            branch: "main".to_string(),
            manifest,
        })
        .unwrap();
    }

    assert_eq!(
        repo.try_fast_forward_candidate("doc-advance", "main", &fast_forward_hash)
            .unwrap(),
        CandidateAdvance::Advanced(fast_forward_hash.clone())
    );
    assert_eq!(
        repo.store().read_head("doc-advance", "main").unwrap(),
        Some(fast_forward_hash.clone())
    );
    assert_eq!(
        repo.try_fast_forward_candidate("doc-advance", "main", &fast_forward_hash)
            .unwrap(),
        CandidateAdvance::AlreadyCurrent(fast_forward_hash)
    );
    assert_eq!(
        repo.try_fast_forward_candidate("doc-advance", "main", &divergent_hash)
            .unwrap(),
        CandidateAdvance::NeedsMerge(divergent_hash)
    );
    let missing_hash = HashRef::parse("sha256:dddddd").unwrap();
    assert_eq!(
        repo.try_fast_forward_candidate("doc-advance", "main", &missing_hash)
            .unwrap(),
        CandidateAdvance::MissingManifest(missing_hash)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_reconciles_fast_forward_candidate_chain() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-repo-reconcile-candidates-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let base = ManifestRecord {
        document_uuid: "doc-reconcile".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
    let first = ManifestRecord {
        document_uuid: "doc-reconcile".to_string(),
        branch: "main".to_string(),
        parent: Some(base_hash.clone()),
        snapshot: HashRef::parse("sha256:bbb").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };
    let first_hash = repo.write_manifest(&first).unwrap();
    let second = ManifestRecord {
        document_uuid: "doc-reconcile".to_string(),
        branch: "main".to_string(),
        parent: Some(first_hash.clone()),
        snapshot: HashRef::parse("sha256:ccc").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 3,
    };
    let divergent = ManifestRecord {
        document_uuid: "doc-reconcile".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:ddd").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 4,
    };
    let second_hash = repo.write_manifest(&second).unwrap();
    let divergent_hash = repo.write_manifest(&divergent).unwrap();
    for manifest in [
        first_hash.clone(),
        second_hash.clone(),
        divergent_hash.clone(),
    ] {
        repo.write_candidate_head(&BranchHeadRecord {
            document_uuid: "doc-reconcile".to_string(),
            branch: "main".to_string(),
            manifest,
        })
        .unwrap();
    }

    let reconciliation = repo
        .reconcile_candidate_heads("doc-reconcile", "main")
        .unwrap();
    assert_eq!(reconciliation.initial.current, Some(base_hash));
    assert_eq!(
        reconciliation.advanced,
        vec![first_hash.clone(), second_hash.clone()]
    );
    assert_eq!(
        repo.store().read_head("doc-reconcile", "main").unwrap(),
        Some(second_hash.clone())
    );
    let statuses = reconciliation
        .final_resolution
        .candidates
        .iter()
        .map(|candidate| (&candidate.manifest, candidate.status))
        .collect::<Vec<_>>();
    assert!(statuses.contains(&(&first_hash, CandidateStatus::IntegratedAncestor)));
    assert!(statuses.contains(&(&second_hash, CandidateStatus::AlreadyCurrent)));
    assert!(statuses.contains(&(&divergent_hash, CandidateStatus::NeedsMerge)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn flat_repository_reconciles_fast_forward_candidate_chain() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-flat-reconcile-candidates-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(FlatObjectStore::new(&root, "bucket/repo").unwrap());
    let base = ManifestRecord {
        document_uuid: "doc-flat-reconcile".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
    let first = ManifestRecord {
        document_uuid: "doc-flat-reconcile".to_string(),
        branch: "main".to_string(),
        parent: Some(base_hash.clone()),
        snapshot: HashRef::parse("sha256:bbb").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };
    let first_hash = repo.write_manifest(&first).unwrap();
    let second = ManifestRecord {
        document_uuid: "doc-flat-reconcile".to_string(),
        branch: "main".to_string(),
        parent: Some(first_hash.clone()),
        snapshot: HashRef::parse("sha256:ccc").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 3,
    };
    let divergent = ManifestRecord {
        document_uuid: "doc-flat-reconcile".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:ddd").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 4,
    };
    let second_hash = repo.write_manifest(&second).unwrap();
    let divergent_hash = repo.write_manifest(&divergent).unwrap();
    for manifest in [
        first_hash.clone(),
        second_hash.clone(),
        divergent_hash.clone(),
    ] {
        repo.write_candidate_head(&BranchHeadRecord {
            document_uuid: "doc-flat-reconcile".to_string(),
            branch: "main".to_string(),
            manifest,
        })
        .unwrap();
    }

    let reconciliation = repo
        .reconcile_candidate_heads("doc-flat-reconcile", "main")
        .unwrap();

    assert_eq!(reconciliation.initial.current, Some(base_hash));
    assert_eq!(
        reconciliation.advanced,
        vec![first_hash.clone(), second_hash.clone()]
    );
    assert_eq!(
        repo.store()
            .read_head("doc-flat-reconcile", "main")
            .unwrap(),
        Some(second_hash.clone())
    );
    let statuses = reconciliation
        .final_resolution
        .candidates
        .iter()
        .map(|candidate| (&candidate.manifest, candidate.status))
        .collect::<Vec<_>>();
    assert!(statuses.contains(&(&first_hash, CandidateStatus::IntegratedAncestor)));
    assert!(statuses.contains(&(&second_hash, CandidateStatus::AlreadyCurrent)));
    assert!(statuses.contains(&(&divergent_hash, CandidateStatus::NeedsMerge)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_plans_candidate_merge_from_common_base() {
    let root = std::env::temp_dir().join(format!("opendoc-repo-plan-merge-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let base = ManifestRecord {
        document_uuid: "doc-plan".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
    let current = ManifestRecord {
        document_uuid: "doc-plan".to_string(),
        branch: "main".to_string(),
        parent: Some(base_hash.clone()),
        snapshot: HashRef::parse("sha256:bbb").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };
    let current_hash = repo
        .commit_manifest(&current, Some(&base_hash))
        .unwrap()
        .unwrap();
    let candidate = ManifestRecord {
        document_uuid: "doc-plan".to_string(),
        branch: "main".to_string(),
        parent: Some(base_hash.clone()),
        snapshot: HashRef::parse("sha256:ccc").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 3,
    };
    let candidate_hash = repo.write_manifest(&candidate).unwrap();
    repo.write_candidate_head(&BranchHeadRecord {
        document_uuid: "doc-plan".to_string(),
        branch: "main".to_string(),
        manifest: candidate_hash.clone(),
    })
    .unwrap();

    let plans = repo.plan_candidate_merges("doc-plan", "main").unwrap();
    assert_eq!(plans.plans.len(), 1);
    assert_eq!(
        plans.plans[0],
        CandidateMergePlan {
            current: Some(current_hash.clone()),
            candidate: candidate_hash.clone(),
            merge_base: Some(base_hash),
            current_since_base: vec![current_hash],
            candidate_since_base: vec![candidate_hash],
        }
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_plans_candidate_merge_without_common_base() {
    let root = std::env::temp_dir().join(format!("opendoc-repo-plan-root-merge-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let current = ManifestRecord {
        document_uuid: "doc-root-plan".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:aaa").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    let current_hash = repo.commit_manifest(&current, None).unwrap().unwrap();
    let candidate = ManifestRecord {
        document_uuid: "doc-root-plan".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: HashRef::parse("sha256:bbb").unwrap(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 2,
    };
    let candidate_hash = repo.write_manifest(&candidate).unwrap();
    repo.write_candidate_head(&BranchHeadRecord {
        document_uuid: "doc-root-plan".to_string(),
        branch: "main".to_string(),
        manifest: candidate_hash.clone(),
    })
    .unwrap();

    let plans = repo.plan_candidate_merges("doc-root-plan", "main").unwrap();
    assert_eq!(plans.plans.len(), 1);
    assert_eq!(
        plans.plans[0],
        CandidateMergePlan {
            current: Some(current_hash.clone()),
            candidate: candidate_hash.clone(),
            merge_base: None,
            current_since_base: vec![current_hash],
            candidate_since_base: vec![candidate_hash],
        }
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_writes_lookup_indexes_and_tombstones() {
    let root = std::env::temp_dir().join(format!("opendoc-repo-lookup-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let manifest = HashRef::parse("sha256:aaa").unwrap();
    let lookup = LookupRecord {
        document_uuid: "doc-lookup".to_string(),
        branch: "main".to_string(),
        manifest: manifest.clone(),
        aliases: vec![LookupAliasRecord {
            scheme: "doi".to_string(),
            value: "10.1234/Example".to_string(),
        }],
        created_at_ms: 12,
    };

    let paths = repo.write_lookup_record(&lookup).unwrap();
    assert_eq!(paths.len(), 2);
    assert_eq!(
        repo.read_uuid_lookup("doc-lookup").unwrap(),
        Some(lookup.clone())
    );
    assert_eq!(
        repo.read_doi_lookup("10.1234/example").unwrap(),
        Some(lookup.clone())
    );
    assert_eq!(repo.scan_lookup_records().unwrap(), vec![lookup]);

    let upper_scheme_lookup = LookupRecord {
        document_uuid: "doc-upper-doi".to_string(),
        branch: "main".to_string(),
        manifest: manifest.clone(),
        aliases: vec![LookupAliasRecord {
            scheme: "DOI".to_string(),
            value: "10.5678/Upper".to_string(),
        }],
        created_at_ms: 12,
    };
    let paths = repo.write_lookup_record(&upper_scheme_lookup).unwrap();
    assert_eq!(paths.len(), 2);
    assert_eq!(
        repo.read_doi_lookup(" 10.5678/upper ").unwrap(),
        Some(upper_scheme_lookup.clone())
    );
    assert!(repo
        .scan_lookup_records()
        .unwrap()
        .contains(&upper_scheme_lookup));

    let multi_alias_lookup = LookupRecord {
        document_uuid: "doc-multi-doi".to_string(),
        branch: "main".to_string(),
        manifest: manifest.clone(),
        aliases: vec![
            LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.7777/Primary".to_string(),
            },
            LookupAliasRecord {
                scheme: "DOI".to_string(),
                value: "10.7777/Secondary".to_string(),
            },
        ],
        created_at_ms: 13,
    };
    let paths = repo.write_lookup_record(&multi_alias_lookup).unwrap();
    assert_eq!(paths.len(), 3);
    assert_eq!(
        repo.read_doi_lookup("10.7777/primary").unwrap(),
        Some(multi_alias_lookup.clone())
    );
    assert_eq!(
        repo.read_doi_lookup("10.7777/secondary").unwrap(),
        Some(multi_alias_lookup.clone())
    );
    let scanned = repo.scan_lookup_records().unwrap();
    assert_eq!(
        scanned
            .iter()
            .filter(|record| record.document_uuid == "doc-multi-doi")
            .count(),
        1
    );

    let tombstone = TombstoneRecord {
        object: HashRef::parse("sha256:bbb").unwrap(),
        archive_locator: "tape://pool/slot/object".to_string(),
        restore_hint: "request recall".to_string(),
        created_at_ms: 13,
        signer: "ssh-ed25519 AAAA".to_string(),
        signature: vec![4, 5, 6],
    };
    let path = repo.write_tombstone(&tombstone).unwrap();
    assert!(path.starts_with("archive/tombstones/sha256/bb/"));
    assert_eq!(
        repo.read_tombstone(&tombstone.object).unwrap(),
        Some(tombstone.clone())
    );
    assert_eq!(
        repo.scan_tombstone_records().unwrap(),
        vec![tombstone.clone()]
    );
    let extra_tombstone = TombstoneRecord {
        object: HashRef::parse("sha256:eeeeee").unwrap(),
        archive_locator: "tape://pool/slot/extra-object".to_string(),
        restore_hint: "request extra recall".to_string(),
        created_at_ms: 14,
        signer: "ssh-ed25519 AAAA".to_string(),
        signature: vec![4, 5, 6],
    };
    repo.write_tombstone(&extra_tombstone).unwrap();
    let wrong_tombstone = TombstoneRecord {
        object: HashRef::parse("sha256:ddd").unwrap(),
        archive_locator: "tape://pool/slot/object".to_string(),
        restore_hint: "request recall".to_string(),
        created_at_ms: 13,
        signer: "ssh-ed25519 AAAA".to_string(),
        signature: vec![4, 5, 6],
    };
    repo.store()
        .put_named(&path, &encode_record(&wrong_tombstone))
        .unwrap();
    assert!(matches!(
        repo.read_tombstone(&tombstone.object),
        Err(StoreError::HashMismatch)
    ));
    assert!(matches!(
        repo.scan_tombstone_records(),
        Err(StoreError::HashMismatch)
    ));
    let scan = repo.scan_tombstone_entries().unwrap();
    assert_eq!(scan.records, vec![extra_tombstone]);
    assert_eq!(scan.invalid.len(), 1);
    assert!(scan.invalid[0].reason.contains("HashMismatch"));

    let signature = SignatureRecord {
        target: HashRef::parse("sha256:ccc").unwrap(),
        signer: "ssh-ed25519 AAAA".to_string(),
        signer_display: "Alice".to_string(),
        title: "blob".to_string(),
        signed_at_ms: 14,
        signature: vec![7, 8, 9],
    };
    let path = repo
        .write_blob_signature(&signature.target, &signature)
        .unwrap();
    assert_eq!(path, "objects/sha256/cc/ccc.sig");
    assert_eq!(
        repo.read_blob_signature(&signature.target).unwrap(),
        Some(signature.clone())
    );
    let wrong_signature = SignatureRecord {
        target: HashRef::parse("sha256:ddd").unwrap(),
        signer: "ssh-ed25519 AAAA".to_string(),
        signer_display: "Alice".to_string(),
        title: "blob".to_string(),
        signed_at_ms: 14,
        signature: vec![7, 8, 9],
    };
    assert!(matches!(
        repo.write_blob_signature(&signature.target, &wrong_signature),
        Err(StoreError::HashMismatch)
    ));
    repo.store()
        .put_named(&path, &encode_record(&wrong_signature))
        .unwrap();
    assert!(matches!(
        repo.read_blob_signature(&signature.target),
        Err(StoreError::HashMismatch)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_audits_manifest_dependencies_for_shallow_clone_recovery() {
    let root =
        std::env::temp_dir().join(format!("opendoc-repo-dependency-audit-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));

    let snapshot_bytes = b"canonical snapshot";
    let snapshot = digest_bytes("sha256", snapshot_bytes).unwrap();
    repo.store()
        .put_if_absent(&snapshot, snapshot_bytes)
        .unwrap();
    let present_segment_bytes = b"operation segment one";
    let present_segment = digest_bytes("sha256", present_segment_bytes).unwrap();
    repo.store()
        .put_if_absent(&present_segment, present_segment_bytes)
        .unwrap();
    let missing_segment = digest_bytes("sha256", b"missing operation segment").unwrap();
    let signature_bytes = b"version signature record";
    let version_signature = digest_bytes("sha256", signature_bytes).unwrap();
    repo.store()
        .put_if_absent(&version_signature, signature_bytes)
        .unwrap();
    let present_blob_bytes = b"present image blob";
    let present_blob = digest_bytes("sha256", present_blob_bytes).unwrap();
    repo.store()
        .put_if_absent(&present_blob, present_blob_bytes)
        .unwrap();
    let present_blob_signature = SignatureRecord {
        target: present_blob.clone(),
        signer: "ssh-ed25519 AAAA".to_string(),
        signer_display: "Blob Signer".to_string(),
        title: "present image".to_string(),
        signed_at_ms: 15,
        signature: vec![1, 2, 3],
    };
    repo.write_blob_signature(&present_blob, &present_blob_signature)
        .unwrap();
    let recoverable_blob = digest_bytes("sha256", b"archived image blob").unwrap();
    let recoverable_tombstone = TombstoneRecord {
        object: recoverable_blob.clone(),
        archive_locator: "tape://pool/slot/recoverable-image".to_string(),
        restore_hint: "request tape recall".to_string(),
        created_at_ms: 16,
        signer: "ssh-ed25519 AAAA".to_string(),
        signature: vec![4, 5, 6],
    };
    repo.write_tombstone(&recoverable_tombstone).unwrap();
    let unrecoverable_blob = digest_bytes("sha256", b"unrecoverable image blob").unwrap();
    let manifest = ManifestRecord {
        document_uuid: "doc-dependency-audit".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: snapshot.clone(),
        operation_segments: vec![present_segment.clone(), missing_segment.clone()],
        signatures: vec![version_signature.clone()],
        blobs: vec![
            present_blob.clone(),
            recoverable_blob.clone(),
            unrecoverable_blob.clone(),
        ],
        created_at_ms: 17,
    };

    let audit = repo.audit_manifest_dependencies(&manifest).unwrap();

    assert_eq!(
        audit.snapshot,
        ObjectDependencyStatus {
            hash: snapshot,
            present: true,
        }
    );
    assert_eq!(
        audit.operation_segments,
        vec![
            ObjectDependencyStatus {
                hash: present_segment,
                present: true,
            },
            ObjectDependencyStatus {
                hash: missing_segment.clone(),
                present: false,
            },
        ]
    );
    assert_eq!(
        audit.version_signatures,
        vec![ObjectDependencyStatus {
            hash: version_signature,
            present: true,
        }]
    );
    assert_eq!(audit.blobs.len(), 3);
    assert_eq!(audit.blobs[0].hash, present_blob);
    assert!(audit.blobs[0].bytes_present);
    assert!(audit.blobs[0].signature_sidecar_present);
    assert_eq!(audit.blobs[0].archive_tombstone, None);
    assert_eq!(audit.blobs[1].hash, recoverable_blob);
    assert!(!audit.blobs[1].bytes_present);
    assert!(!audit.blobs[1].signature_sidecar_present);
    assert_eq!(
        audit.blobs[1].archive_tombstone,
        Some(recoverable_tombstone)
    );
    assert_eq!(audit.blobs[2].hash, unrecoverable_blob);
    assert!(!audit.blobs[2].bytes_present);
    let mut expected_missing = vec![
        missing_segment,
        recoverable_blob.clone(),
        unrecoverable_blob,
    ];
    expected_missing.sort_by_key(|hash| hash.to_string());
    assert_eq!(audit.missing_hashes(), expected_missing);
    assert_eq!(audit.recoverable_missing_blobs(), vec![recoverable_blob]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_rejects_lookup_records_from_wrong_index_paths() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-repo-lookup-path-mismatch-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let lookup = LookupRecord {
        document_uuid: "doc-correct".to_string(),
        branch: "main".to_string(),
        manifest: HashRef::parse("sha256:aaa").unwrap(),
        aliases: vec![LookupAliasRecord {
            scheme: "doi".to_string(),
            value: "10.1234/correct".to_string(),
        }],
        created_at_ms: 12,
    };

    repo.store()
        .put_named(
            &uuid_lookup_path("doc-wrong").unwrap(),
            &encode_record(&lookup),
        )
        .unwrap();
    assert!(matches!(
        repo.read_uuid_lookup("doc-wrong"),
        Err(StoreError::LookupMismatch)
    ));
    assert!(repo.scan_lookup_records().unwrap().is_empty());

    repo.store()
        .put_named(
            &doi_lookup_path("10.1234/wrong").unwrap(),
            &encode_record(&lookup),
        )
        .unwrap();
    assert!(matches!(
        repo.read_doi_lookup("10.1234/wrong"),
        Err(StoreError::LookupMismatch)
    ));
    assert!(repo.scan_lookup_records().unwrap().is_empty());

    repo.write_lookup_record(&lookup).unwrap();
    assert_eq!(
        repo.read_uuid_lookup("doc-correct").unwrap(),
        Some(lookup.clone())
    );
    assert_eq!(
        repo.read_doi_lookup("10.1234/correct").unwrap(),
        Some(lookup.clone())
    );
    assert_eq!(repo.scan_lookup_records().unwrap(), vec![lookup]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repository_scans_valid_lookup_records_while_reporting_invalid_indexes() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-repo-lookup-scan-invalid-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let lookup = LookupRecord {
        document_uuid: "doc-scan-valid".to_string(),
        branch: "main".to_string(),
        manifest: HashRef::parse("sha256:aaa").unwrap(),
        aliases: vec![LookupAliasRecord {
            scheme: "doi".to_string(),
            value: "10.1234/scan-valid".to_string(),
        }],
        created_at_ms: 3,
    };
    repo.write_lookup_record(&lookup).unwrap();
    repo.store()
        .put_named("indexes/by-uuid/zz/corrupt.idx", b"not a lookup record")
        .unwrap();
    let mismatched = LookupRecord {
        document_uuid: "doc-scan-mismatch".to_string(),
        branch: "main".to_string(),
        manifest: HashRef::parse("sha256:bbb").unwrap(),
        aliases: Vec::new(),
        created_at_ms: 4,
    };
    repo.store()
        .put_named(
            "indexes/by-uuid/zz/mismatch.idx",
            &encode_record(&mismatched),
        )
        .unwrap();

    assert_eq!(repo.scan_lookup_records().unwrap(), vec![lookup.clone()]);
    let scan = repo.scan_lookup_entries().unwrap();
    assert_eq!(scan.records, vec![lookup]);
    assert_eq!(scan.invalid.len(), 2);
    assert!(scan
        .invalid
        .iter()
        .any(|problem| problem.reason.contains("InvalidMagic")));
    assert!(scan
        .invalid
        .iter()
        .any(|problem| problem.reason == "lookup record does not match index path"));
    let _ = fs::remove_dir_all(root);
}
