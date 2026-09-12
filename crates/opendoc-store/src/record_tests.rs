use crate::keys::{blob_signature_path, tombstone_path, uuid_lookup_path};
use crate::local_store::process_tag;
use crate::*;
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{
    encode_record, BranchHeadRecord, LookupAliasRecord, LookupRecord, ManifestRecord,
    SignatureRecord, TombstoneRecord,
};
use std::fs;

#[test]
fn repository_rejects_semantically_invalid_binary_records() {
    let root = std::env::temp_dir().join(format!("opendoc-repo-invalid-records-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let repo = Repository::new(LocalObjectStore::new(&root));
    let manifest_hash = HashRef::parse("sha256:aaa").unwrap();

    let invalid_manifest = ManifestRecord {
        document_uuid: String::new(),
        branch: "main".to_string(),
        parent: None,
        snapshot: manifest_hash.clone(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    assert!(matches!(
        repo.write_manifest(&invalid_manifest),
        Err(StoreError::Format(message)) if message.contains("manifest document_uuid is empty")
    ));
    let whitespace_manifest = ManifestRecord {
        document_uuid: " doc ".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: manifest_hash.clone(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    assert!(matches!(
        repo.write_manifest(&whitespace_manifest),
        Err(StoreError::Format(message))
            if message.contains("manifest document_uuid has surrounding whitespace")
    ));
    let invalid_branch_manifest = ManifestRecord {
        document_uuid: "doc".to_string(),
        branch: "main branch".to_string(),
        parent: None,
        snapshot: manifest_hash.clone(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    assert!(matches!(
        repo.write_manifest(&invalid_branch_manifest),
        Err(StoreError::Format(message))
            if message.contains("manifest branch is not a repository key segment")
    ));
    let duplicate_ref_manifest = ManifestRecord {
        document_uuid: "doc".to_string(),
        branch: "main".to_string(),
        parent: None,
        snapshot: manifest_hash.clone(),
        operation_segments: vec![manifest_hash.clone(), manifest_hash.clone()],
        signatures: Vec::new(),
        blobs: Vec::new(),
        created_at_ms: 1,
    };
    assert!(matches!(
        repo.write_manifest(&duplicate_ref_manifest),
        Err(StoreError::Format(message))
            if message.contains("duplicate manifest operation segment reference")
    ));
    let duplicate_ref_bytes = encode_record(&duplicate_ref_manifest);
    let duplicate_ref_hash = digest_bytes("sha256", &duplicate_ref_bytes).unwrap();
    repo.store()
        .put_if_absent(&duplicate_ref_hash, &duplicate_ref_bytes)
        .unwrap();
    assert!(matches!(
        repo.read_manifest(&duplicate_ref_hash),
        Err(StoreError::Format(message))
            if message.contains("duplicate manifest operation segment reference")
    ));

    let invalid_head = BranchHeadRecord {
        document_uuid: "doc".to_string(),
        branch: String::new(),
        manifest: manifest_hash.clone(),
    };
    assert!(matches!(
        repo.write_candidate_head(&invalid_head),
        Err(StoreError::Format(message)) if message.contains("branch head branch is empty")
    ));
    let invalid_path_head = BranchHeadRecord {
        document_uuid: "doc".to_string(),
        branch: "..".to_string(),
        manifest: manifest_hash.clone(),
    };
    assert!(matches!(
        repo.write_candidate_head(&invalid_path_head),
        Err(StoreError::Format(message))
            if message.contains("branch head branch is not a repository key segment")
    ));
    let whitespace_head = BranchHeadRecord {
        document_uuid: " doc ".to_string(),
        branch: "main".to_string(),
        manifest: manifest_hash.clone(),
    };
    assert!(matches!(
        repo.write_candidate_head(&whitespace_head),
        Err(StoreError::Format(message))
            if message.contains("branch head document_uuid has surrounding whitespace")
    ));

    let invalid_lookup = LookupRecord {
        document_uuid: "doc".to_string(),
        branch: "main".to_string(),
        manifest: manifest_hash.clone(),
        aliases: vec![LookupAliasRecord {
            scheme: "doi".to_string(),
            value: String::new(),
        }],
        created_at_ms: 2,
    };
    assert!(matches!(
        repo.write_lookup_record(&invalid_lookup),
        Err(StoreError::Format(message)) if message.contains("lookup alias value is empty")
    ));
    assert!(matches!(
        repo.read_uuid_lookup("../doc"),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        repo.read_doi_lookup(" "),
        Err(StoreError::InvalidPath)
    ));
    let invalid_path_lookup = LookupRecord {
        document_uuid: "../doc".to_string(),
        branch: "main".to_string(),
        manifest: manifest_hash.clone(),
        aliases: Vec::new(),
        created_at_ms: 2,
    };
    assert!(matches!(
        repo.write_lookup_record(&invalid_path_lookup),
        Err(StoreError::InvalidPath)
    ));
    let whitespace_lookup = LookupRecord {
        document_uuid: " doc ".to_string(),
        branch: "main".to_string(),
        manifest: manifest_hash.clone(),
        aliases: Vec::new(),
        created_at_ms: 2,
    };
    assert!(matches!(
        repo.write_lookup_record(&whitespace_lookup),
        Err(StoreError::Format(message))
            if message.contains("lookup document_uuid has surrounding whitespace")
    ));
    let invalid_branch_lookup = LookupRecord {
        document_uuid: "doc".to_string(),
        branch: "main/branch".to_string(),
        manifest: manifest_hash.clone(),
        aliases: Vec::new(),
        created_at_ms: 2,
    };
    assert!(matches!(
        repo.write_lookup_record(&invalid_branch_lookup),
        Err(StoreError::Format(message))
            if message.contains("lookup branch is not a repository key segment")
    ));

    let mut lookup = invalid_lookup.clone();
    lookup.aliases[0].value = "10.1234/example".to_string();
    let path = uuid_lookup_path(&lookup.document_uuid).unwrap();
    repo.store()
        .put_named(&path, &encode_record(&invalid_lookup))
        .unwrap();
    assert!(matches!(
        repo.read_uuid_lookup("doc"),
        Err(StoreError::Format(message)) if message.contains("lookup alias value is empty")
    ));
    repo.store()
        .put_named(
            &uuid_lookup_path("doc").unwrap(),
            &encode_record(&whitespace_lookup),
        )
        .unwrap();
    assert!(matches!(
        repo.read_uuid_lookup("doc"),
        Err(StoreError::Format(message))
            if message.contains("lookup document_uuid has surrounding whitespace")
    ));

    let whitespace_alias_lookup = LookupRecord {
        document_uuid: "doc-whitespace-alias".to_string(),
        branch: "main".to_string(),
        manifest: manifest_hash.clone(),
        aliases: vec![LookupAliasRecord {
            scheme: "doi".to_string(),
            value: " 10.1234/example ".to_string(),
        }],
        created_at_ms: 2,
    };
    assert!(matches!(
        repo.write_lookup_record(&whitespace_alias_lookup),
        Err(StoreError::Format(message))
            if message.contains("lookup alias value has surrounding whitespace")
    ));
    repo.store()
        .put_named(
            &uuid_lookup_path(&whitespace_alias_lookup.document_uuid).unwrap(),
            &encode_record(&whitespace_alias_lookup),
        )
        .unwrap();
    assert!(matches!(
        repo.read_uuid_lookup("doc-whitespace-alias"),
        Err(StoreError::Format(message))
            if message.contains("lookup alias value has surrounding whitespace")
    ));

    let duplicate_lookup = LookupRecord {
        document_uuid: "doc-duplicate".to_string(),
        branch: "main".to_string(),
        manifest: manifest_hash.clone(),
        aliases: vec![
            LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/example".to_string(),
            },
            LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/example".to_string(),
            },
        ],
        created_at_ms: 2,
    };
    assert!(matches!(
        repo.write_lookup_record(&duplicate_lookup),
        Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
    ));
    let path = uuid_lookup_path(&duplicate_lookup.document_uuid).unwrap();
    repo.store()
        .put_named(&path, &encode_record(&duplicate_lookup))
        .unwrap();
    assert!(matches!(
        repo.read_uuid_lookup("doc-duplicate"),
        Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
    ));

    let duplicate_doi_lookup = LookupRecord {
        document_uuid: "doc-duplicate-doi".to_string(),
        branch: "main".to_string(),
        manifest: manifest_hash.clone(),
        aliases: vec![
            LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/Example".to_string(),
            },
            LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/example".to_string(),
            },
        ],
        created_at_ms: 2,
    };
    assert!(matches!(
        repo.write_lookup_record(&duplicate_doi_lookup),
        Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
    ));
    let path = uuid_lookup_path(&duplicate_doi_lookup.document_uuid).unwrap();
    repo.store()
        .put_named(&path, &encode_record(&duplicate_doi_lookup))
        .unwrap();
    assert!(matches!(
        repo.read_uuid_lookup("doc-duplicate-doi"),
        Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
    ));

    let duplicate_doi_scheme_lookup = LookupRecord {
        document_uuid: "doc-duplicate-doi-scheme".to_string(),
        branch: "main".to_string(),
        manifest: manifest_hash.clone(),
        aliases: vec![
            LookupAliasRecord {
                scheme: "DOI".to_string(),
                value: "10.1234/Example".to_string(),
            },
            LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/example".to_string(),
            },
        ],
        created_at_ms: 2,
    };
    assert!(matches!(
        repo.write_lookup_record(&duplicate_doi_scheme_lookup),
        Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
    ));
    let path = uuid_lookup_path(&duplicate_doi_scheme_lookup.document_uuid).unwrap();
    repo.store()
        .put_named(&path, &encode_record(&duplicate_doi_scheme_lookup))
        .unwrap();
    assert!(matches!(
        repo.read_uuid_lookup("doc-duplicate-doi-scheme"),
        Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
    ));

    let invalid_tombstone = TombstoneRecord {
        object: HashRef::parse("sha256:bbb").unwrap(),
        archive_locator: "tape://pool/object".to_string(),
        restore_hint: "request recall".to_string(),
        created_at_ms: 3,
        signer: "ssh-ed25519 AAAA".to_string(),
        signature: Vec::new(),
    };
    assert!(matches!(
        repo.write_tombstone(&invalid_tombstone),
        Err(StoreError::Format(message)) if message.contains("tombstone signature is empty")
    ));

    let mut valid_tombstone = invalid_tombstone.clone();
    valid_tombstone.signature = vec![1];
    let path = tombstone_path(&valid_tombstone.object);
    repo.store()
        .put_named(&path, &encode_record(&invalid_tombstone))
        .unwrap();
    assert!(matches!(
        repo.read_tombstone(&valid_tombstone.object),
        Err(StoreError::Format(message)) if message.contains("tombstone signature is empty")
    ));
    let padded_tombstone = TombstoneRecord {
        object: HashRef::parse("sha256:bbd").unwrap(),
        archive_locator: " tape://pool/object".to_string(),
        restore_hint: "request recall".to_string(),
        created_at_ms: 3,
        signer: "ssh-ed25519 AAAA".to_string(),
        signature: vec![1],
    };
    assert!(matches!(
        repo.write_tombstone(&padded_tombstone),
        Err(StoreError::Format(message))
            if message.contains("tombstone archive_locator has surrounding whitespace")
    ));
    repo.store()
        .put_named(
            &tombstone_path(&padded_tombstone.object),
            &encode_record(&padded_tombstone),
        )
        .unwrap();
    assert!(matches!(
        repo.read_tombstone(&padded_tombstone.object),
        Err(StoreError::Format(message))
            if message.contains("tombstone archive_locator has surrounding whitespace")
    ));

    let invalid_signature = SignatureRecord {
        target: HashRef::parse("sha256:ccc").unwrap(),
        signer: String::new(),
        signer_display: "Alice".to_string(),
        title: "blob".to_string(),
        signed_at_ms: 4,
        signature: vec![1],
    };
    assert!(matches!(
        repo.write_blob_signature(&invalid_signature.target, &invalid_signature),
        Err(StoreError::Format(message)) if message.contains("signature signer is empty")
    ));

    repo.store()
        .put_named(
            &blob_signature_path(&invalid_signature.target),
            &encode_record(&invalid_signature),
        )
        .unwrap();
    assert!(matches!(
        repo.read_blob_signature(&invalid_signature.target),
        Err(StoreError::Format(message)) if message.contains("signature signer is empty")
    ));

    let padded_signature = SignatureRecord {
        target: HashRef::parse("sha256:ddd").unwrap(),
        signer: " ssh-ed25519 AAAA".to_string(),
        signer_display: "Alice".to_string(),
        title: "blob".to_string(),
        signed_at_ms: 5,
        signature: vec![1],
    };
    assert!(matches!(
        repo.write_blob_signature(&padded_signature.target, &padded_signature),
        Err(StoreError::Format(message))
            if message.contains("signature signer has surrounding whitespace")
    ));
    repo.store()
        .put_named(
            &blob_signature_path(&padded_signature.target),
            &encode_record(&padded_signature),
        )
        .unwrap();
    assert!(matches!(
        repo.read_blob_signature(&padded_signature.target),
        Err(StoreError::Format(message))
            if message.contains("signature signer has surrounding whitespace")
    ));
    let _ = fs::remove_dir_all(root);
}
