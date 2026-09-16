//! The durable operation log: what survives, and in what order it is written.

use crate::error::ServiceError;
use crate::log::{DocumentLog, SERVICE_BRANCH};
use crate::store::SharedStore;
use crate::test_support::{insert_text, seeded_document, TempRoot};
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{encode_canonical_cbor, encode_record, BranchHeadRecord};
use opendoc_merge::{ActorId, CausalContext, Operation, OperationId, OperationKind};
use opendoc_store::{LocalObjectStore, ObjectStore, Repository};

fn repository(root: &TempRoot) -> Repository<SharedStore<LocalObjectStore>> {
    Repository::new(SharedStore::new(root.store()))
}

fn operation(actor: &str, seq: u64, kind: OperationKind, observed: &[Operation]) -> Operation {
    Operation::in_context(
        OperationId {
            actor: ActorId(actor.to_string()),
            seq,
        },
        kind,
        CausalContext::observing(observed.iter()),
    )
}

#[test]
fn a_created_document_starts_at_commit_zero_with_the_base_as_its_snapshot() {
    let root = TempRoot::new("log-create");
    let (document, _block, _inline) = seeded_document("Genesis");
    let log = DocumentLog::create(repository(&root), document.clone()).unwrap();
    assert_eq!(log.commit_seq(), 0);
    assert_eq!(log.base(), &document);
    assert_eq!(log.document(), &document);
    assert!(log.operations().is_empty());
    assert!(log.head().is_some());
}

#[test]
fn creating_a_document_that_already_has_a_head_is_refused() {
    let root = TempRoot::new("log-recreate");
    let (document, _block, _inline) = seeded_document("Twice");
    DocumentLog::create(repository(&root), document.clone()).unwrap();
    let error = DocumentLog::create(repository(&root), document)
        .err()
        .expect("creating a second time must fail");
    assert!(matches!(error, ServiceError::Conflict(_)));
}

#[test]
fn an_appended_operation_survives_a_reload_from_storage_alone() {
    let root = TempRoot::new("log-reload");
    let (document, _block, inline) = seeded_document("Durable");
    let uuid = document.uuid.as_str().to_string();

    let expected_bytes = {
        let mut log = DocumentLog::create(repository(&root), document).unwrap();
        let first = operation("actor-alice", 1, insert_text(&inline, 0, "X"), &[]);
        let second = operation(
            "actor-alice",
            2,
            insert_text(&inline, 1, "Y"),
            std::slice::from_ref(&first),
        );
        log.append(vec![first]).unwrap();
        log.append(vec![second]).unwrap();
        assert_eq!(log.commit_seq(), 2);
        encode_canonical_cbor(log.document()).unwrap()
    };

    // Nothing but the object store crosses this boundary.
    let reloaded = DocumentLog::load(repository(&root), &uuid).unwrap();
    assert_eq!(reloaded.commit_seq(), 2);
    assert_eq!(reloaded.operations().len(), 2);
    assert_eq!(
        encode_canonical_cbor(reloaded.document()).unwrap(),
        expected_bytes,
        "a reloaded log must materialise byte-identical canonical CBOR"
    );
}

#[test]
fn the_merge_base_stays_genesis_across_commits() {
    // ADR 0007: character identities are reconstructed from (base, operation
    // set) on every merge. Re-basing on the previous commit's output would
    // silently re-anchor late operations positionally.
    let root = TempRoot::new("log-base");
    let (document, _block, inline) = seeded_document("Base");
    let mut log = DocumentLog::create(repository(&root), document.clone()).unwrap();
    let first = operation("actor-alice", 1, insert_text(&inline, 0, "X"), &[]);
    log.append(vec![first.clone()]).unwrap();
    log.append(vec![operation(
        "actor-alice",
        2,
        insert_text(&inline, 1, "Y"),
        std::slice::from_ref(&first),
    )])
    .unwrap();
    assert_eq!(log.base(), &document);

    let reloaded = DocumentLog::load(repository(&root), document.uuid.as_str()).unwrap();
    assert_eq!(reloaded.base(), &document);
}

#[test]
fn every_object_the_durable_head_names_is_already_durable() {
    // ADR 0008's head-safety rule, asserted rather than assumed: walk the
    // whole chain the head points at and require each object to be present.
    let root = TempRoot::new("log-head-safety");
    let (document, _block, inline) = seeded_document("Head safety");
    let mut log = DocumentLog::create(repository(&root), document.clone()).unwrap();
    let mut observed: Vec<Operation> = Vec::new();
    for seq in 1..=3u64 {
        let next = operation(
            "actor-alice",
            seq,
            insert_text(&inline, 0, "z"),
            &observed.clone(),
        );
        log.append(vec![next.clone()]).unwrap();
        observed.push(next);
    }

    let repository = repository(&root);
    let head = repository
        .store()
        .read_head(document.uuid.as_str(), SERVICE_BRANCH)
        .unwrap()
        .expect("a durable head");
    let mut cursor = Some(head);
    let mut visited = 0usize;
    while let Some(hash) = cursor {
        let manifest = repository
            .read_manifest(&hash)
            .unwrap()
            .expect("the head names a manifest that exists");
        assert!(
            repository.store().exists(&manifest.snapshot).unwrap(),
            "manifest {hash} names a snapshot that is not durable"
        );
        for segment in &manifest.operation_segments {
            assert!(
                repository.store().exists(segment).unwrap(),
                "manifest {hash} names an operation segment that is not durable"
            );
        }
        cursor = manifest.parent.clone();
        visited += 1;
    }
    assert_eq!(visited, 4, "genesis plus three commits");
}

#[test]
fn a_head_moved_by_another_writer_makes_the_commit_refuse() {
    let root = TempRoot::new("log-cas");
    let (document, _block, inline) = seeded_document("Race");
    let uuid = document.uuid.as_str().to_string();
    let mut log = DocumentLog::create(repository(&root), document).unwrap();

    // Something else takes the head. The log must not write over it.
    let repository = repository(&root);
    let current = repository.store().read_head(&uuid, SERVICE_BRANCH).unwrap();
    let intruder = BranchHeadRecord {
        document_uuid: uuid.clone(),
        branch: SERVICE_BRANCH.to_string(),
        manifest: HashRef::parse(
            "sha256:0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap(),
    };
    let intruder_hash = digest_bytes("sha256", &encode_record(&intruder)).unwrap();
    repository
        .store()
        .put_if_absent(&intruder_hash, &encode_record(&intruder))
        .unwrap();
    assert!(repository
        .store()
        .compare_and_swap_head(&uuid, SERVICE_BRANCH, current.as_ref(), &intruder.manifest)
        .unwrap());

    let error = log
        .append(vec![operation(
            "actor-alice",
            1,
            insert_text(&inline, 0, "X"),
            &[],
        )])
        .unwrap_err();
    assert!(
        matches!(error, ServiceError::Conflict(_)),
        "a lost compare-and-swap must refuse, not overwrite: {error:?}"
    );
    // And the in-memory log did not advance behind the failure.
    assert_eq!(log.commit_seq(), 0);
    assert!(log.operations().is_empty());
}

#[test]
fn an_empty_commit_is_refused() {
    let root = TempRoot::new("log-empty");
    let (document, _block, _inline) = seeded_document("Empty");
    let mut log = DocumentLog::create(repository(&root), document).unwrap();
    assert!(matches!(
        log.append(Vec::new()).unwrap_err(),
        ServiceError::BadRequest(_)
    ));
}

#[test]
fn loading_a_document_with_no_head_reports_not_found() {
    let root = TempRoot::new("log-missing");
    let error = DocumentLog::load(repository(&root), "doc-dead-beef")
        .err()
        .expect("loading a document with no head must fail");
    assert!(matches!(error, ServiceError::NotFound(_)));
}

#[test]
fn open_or_create_creates_once_and_loads_afterwards() {
    let root = TempRoot::new("log-open-or-create");
    let (document, _block, inline) = seeded_document("Once");
    let mut log = DocumentLog::open_or_create(repository(&root), document.clone()).unwrap();
    log.append(vec![operation(
        "actor-alice",
        1,
        insert_text(&inline, 0, "X"),
        &[],
    )])
    .unwrap();

    // A second call with a document carrying the same uuid must not reset it.
    let reopened = DocumentLog::open_or_create(repository(&root), document).unwrap();
    assert_eq!(reopened.commit_seq(), 1);
    assert_eq!(reopened.operations().len(), 1);
}

#[test]
fn the_log_reports_which_operation_ids_it_already_holds() {
    let root = TempRoot::new("log-contains");
    let (document, _block, inline) = seeded_document("Contains");
    let mut log = DocumentLog::create(repository(&root), document).unwrap();
    let logged = operation("actor-alice", 1, insert_text(&inline, 0, "X"), &[]);
    log.append(vec![logged.clone()]).unwrap();
    assert!(log.contains(&logged));
    assert_eq!(log.logged(&logged), Some(&logged));

    let different_payload = operation("actor-alice", 1, insert_text(&inline, 0, "Q"), &[]);
    assert!(log.contains(&different_payload));
    assert_ne!(log.logged(&different_payload), Some(&different_payload));
}

#[test]
fn a_stored_log_with_a_sequence_gap_is_refused_on_load() {
    // `VectorClock::observed` reads "seq >= n" as "every one of that actor's
    // operations up to n". Intake enforces that on the way in; this is the
    // matching check on the way back out of storage.
    let root = TempRoot::new("log-gap");
    let (document, _block, inline) = seeded_document("Gap");
    let uuid = document.uuid.as_str().to_string();
    let mut log = DocumentLog::create(repository(&root), document).unwrap();
    let first = operation("actor-alice", 1, insert_text(&inline, 0, "X"), &[]);
    log.append(vec![first.clone()]).unwrap();
    log.append(vec![operation(
        "actor-alice",
        3,
        insert_text(&inline, 1, "Y"),
        std::slice::from_ref(&first),
    )])
    .unwrap();

    let error = DocumentLog::load(repository(&root), &uuid)
        .err()
        .expect("a log with a sequence gap must not load");
    assert!(
        matches!(error, ServiceError::Storage(_)),
        "expected a storage error, got {error:?}"
    );
}
