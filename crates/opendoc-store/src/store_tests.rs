use crate::keys::blob_signature_path;
use crate::local_store::process_tag;
use crate::pack::{decode_pack_index, encode_pack_index};
use crate::*;
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{encode_record, PackIndexEntryRecord, PackIndexRecord};
use std::fs;
use std::io::{Seek, SeekFrom, Write};

#[test]
fn local_store_round_trips_object_and_head() {
    let root = std::env::temp_dir().join(format!("opendoc-store-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    assert_eq!(
        store.capabilities(),
        StoreCapabilities {
            idempotent_content_put: true,
            compare_and_swap_head: true,
            list_prefix: true,
            atomic_named_overwrite: true,
            local_pack_files: true,
        }
    );
    let hash = digest_bytes("sha256", b"hello").unwrap();
    assert!(store.put_if_absent(&hash, b"hello").unwrap());
    assert!(!store.put_if_absent(&hash, b"hello").unwrap());
    assert_eq!(store.get(&hash).unwrap(), Some(b"hello".to_vec()));
    assert!(store
        .compare_and_swap_head("doc", "main", None, &hash)
        .unwrap());
    assert_eq!(store.read_head("doc", "main").unwrap(), Some(hash.clone()));
    let other = HashRef::parse("sha256:123456").unwrap();
    assert!(!store
        .compare_and_swap_head("doc", "main", None, &other)
        .unwrap());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn object_store_layout_is_backend_neutral_and_s3_safe() {
    let hash = digest_bytes("sha256", b"layout").unwrap();
    assert_eq!(
        ObjectStoreLayout::object_key(&hash),
        format!("objects/sha256/{}/{}", &hash.digest()[..2], hash.digest())
    );
    assert_eq!(
        ObjectStoreLayout::blob_signature_key(&hash),
        format!(
            "objects/sha256/{}/{}.sig",
            &hash.digest()[..2],
            hash.digest()
        )
    );
    assert_eq!(
        ObjectStoreLayout::head_key("doc-1", "main").unwrap(),
        "documents/doc-1/heads/main.head"
    );
    assert_eq!(
        ObjectStoreLayout::candidate_head_key("doc-1", "main", &hash).unwrap(),
        format!(
            "documents/doc-1/head-candidates/main/sha256/{}.head",
            hash.digest()
        )
    );
    assert!(matches!(
        ObjectStoreLayout::head_key("../doc", "main"),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        ObjectStoreLayout::head_key("..", "main"),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        ObjectStoreLayout::head_key("doc", "feature/x"),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        ObjectStoreLayout::candidate_head_key("doc", "..", &hash),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        ObjectStoreLayout::uuid_lookup_key("../doc"),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        ObjectStoreLayout::uuid_lookup_key(".."),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        ObjectStoreLayout::doi_lookup_key(" "),
        Err(StoreError::InvalidPath)
    ));
}

#[test]
fn local_store_satisfies_reusable_object_store_contract() {
    let root = std::env::temp_dir().join(format!("opendoc-store-conformance-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    verify_object_store_contract(&store, "local").unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn flat_store_satisfies_reusable_object_store_contract() {
    let root =
        std::env::temp_dir().join(format!("opendoc-flat-store-conformance-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = FlatObjectStore::new(&root, "/bucket/prefix/").unwrap();
    assert_eq!(store.namespace(), "bucket/prefix");
    assert_eq!(
        store.capabilities(),
        StoreCapabilities {
            idempotent_content_put: true,
            compare_and_swap_head: true,
            list_prefix: true,
            atomic_named_overwrite: true,
            local_pack_files: true,
        }
    );
    verify_object_store_contract(&store, "flat").unwrap();
    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "opendal")]
#[test]
fn opendal_fs_store_satisfies_reusable_object_store_contract() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-opendal-fs-store-conformance-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let store = OpenDalObjectStore::from_fs_root(&root, "/bucket/prefix/").unwrap();
    assert_eq!(store.namespace(), "bucket/prefix");
    assert_eq!(
        store.capabilities(),
        StoreCapabilities {
            idempotent_content_put: true,
            compare_and_swap_head: false,
            list_prefix: true,
            atomic_named_overwrite: false,
            local_pack_files: false,
        }
    );
    verify_object_store_contract(&store, "opendal").unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn flat_store_uses_backend_neutral_namespaced_key_layout() {
    let root = std::env::temp_dir().join(format!("opendoc-flat-store-layout-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = FlatObjectStore::new(&root, "bucket/prefix").unwrap();
    let bytes = b"s3-shaped flat object";
    let hash = digest_bytes("sha256", bytes).unwrap();

    assert!(store.put_if_absent(&hash, bytes).unwrap());
    assert!(root
        .join("bucket/prefix")
        .join(ObjectStoreLayout::object_key(&hash))
        .exists());
    store
        .put_named("documents/doc/metadata.bin", b"named")
        .unwrap();
    assert_eq!(
        fs::read(root.join("bucket/prefix/documents/doc/metadata.bin")).unwrap(),
        b"named"
    );
    assert_eq!(
        store.list_prefix("documents").unwrap(),
        vec!["doc/metadata.bin".to_string()]
    );
    assert!(matches!(
        FlatObjectStore::new(&root, "../bad"),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        FlatObjectStore::new(&root, "bucket//bad"),
        Err(StoreError::InvalidPath)
    ));
    assert!(matches!(
        store.put_if_absent(&hash, b"wrong bytes"),
        Err(StoreError::HashMismatch)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_store_rejects_hash_mismatched_loose_object_writes_and_reads() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-local-store-loose-integrity-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    let bytes = b"local loose integrity";
    let hash = digest_bytes("sha256", bytes).unwrap();

    assert!(matches!(
        store.put_if_absent(&hash, b"wrong bytes"),
        Err(StoreError::HashMismatch)
    ));
    assert!(store.put_if_absent(&hash, bytes).unwrap());
    fs::write(store.object_path(&hash), b"tampered loose bytes").unwrap();

    assert!(matches!(store.get(&hash), Err(StoreError::HashMismatch)));
    assert!(matches!(store.exists(&hash), Err(StoreError::HashMismatch)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn flat_store_rejects_corrupt_content_addressed_object_reads() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-flat-store-object-integrity-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = FlatObjectStore::new(&root, "bucket/prefix").unwrap();
    let bytes = b"flat object integrity";
    let hash = digest_bytes("sha256", bytes).unwrap();

    assert!(store.put_if_absent(&hash, bytes).unwrap());
    fs::write(
        store
            .key_path(&ObjectStoreLayout::object_key(&hash))
            .unwrap(),
        b"tampered flat object",
    )
    .unwrap();

    assert!(matches!(store.get(&hash), Err(StoreError::HashMismatch)));
    assert!(matches!(store.exists(&hash), Err(StoreError::HashMismatch)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn stores_reject_corrupt_branch_head_records() {
    let local_root =
        std::env::temp_dir().join(format!("opendoc-store-corrupt-head-{}", process_tag()));
    let flat_root =
        std::env::temp_dir().join(format!("opendoc-flat-store-corrupt-head-{}", process_tag()));
    let _ = fs::remove_dir_all(&local_root);
    let _ = fs::remove_dir_all(&flat_root);

    let local = LocalObjectStore::new(&local_root);
    let local_head = local.head_path("doc-corrupt", "main");
    fs::create_dir_all(local_head.parent().unwrap()).unwrap();
    fs::write(&local_head, b"not-a-hash").unwrap();
    assert!(matches!(
        local.read_head("doc-corrupt", "main"),
        Err(StoreError::CorruptHead)
    ));
    let padded = HashRef::parse("sha256:abc123").unwrap();
    fs::write(&local_head, format!(" {padded} ")).unwrap();
    assert!(matches!(
        local.read_head("doc-corrupt", "main"),
        Err(StoreError::CorruptHead)
    ));

    let flat = FlatObjectStore::new(&flat_root, "bucket/prefix").unwrap();
    flat.put_named(
        &ObjectStoreLayout::head_key("doc-corrupt", "main").unwrap(),
        b"not-a-hash",
    )
    .unwrap();
    assert!(matches!(
        flat.read_head("doc-corrupt", "main"),
        Err(StoreError::CorruptHead)
    ));
    flat.put_named(
        &ObjectStoreLayout::head_key("doc-corrupt", "main").unwrap(),
        format!("\n{padded}").as_bytes(),
    )
    .unwrap();
    assert!(matches!(
        flat.read_head("doc-corrupt", "main"),
        Err(StoreError::CorruptHead)
    ));

    let _ = fs::remove_dir_all(local_root);
    let _ = fs::remove_dir_all(flat_root);
}

#[test]
fn local_store_reads_objects_from_pack_after_loose_files_are_removed() {
    let root = std::env::temp_dir().join(format!("opendoc-store-pack-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    let first = digest_bytes("sha256", b"first packed object").unwrap();
    let second = digest_bytes("sha256", b"second packed object").unwrap();
    assert!(store.put_if_absent(&first, b"first packed object").unwrap());
    assert!(store
        .put_if_absent(&second, b"second packed object")
        .unwrap());

    let stats = store.compact_loose_objects_to_pack("main-pack").unwrap();
    assert_eq!(stats.pack, "main-pack");
    assert_eq!(stats.objects, 2);
    assert!(stats.bytes > 4);
    let index_bytes = fs::read(root.join("packs/main-pack.idx")).unwrap();
    assert_eq!(index_bytes.get(0..4), Some(&b"ODF0"[..]));
    assert_eq!(decode_pack_index(&index_bytes).unwrap().len(), 2);

    assert!(!store.object_path(&first).exists());
    assert!(!store.object_path(&second).exists());
    assert!(store.exists(&first).unwrap());
    assert_eq!(
        store.get(&first).unwrap(),
        Some(b"first packed object".to_vec())
    );
    assert_eq!(
        store.get(&second).unwrap(),
        Some(b"second packed object".to_vec())
    );
    assert!(!store.put_if_absent(&first, b"first packed object").unwrap());
    assert!(matches!(
        store.compact_loose_objects_to_pack("../bad"),
        Err(StoreError::InvalidPath)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn flat_store_reads_namespaced_objects_from_pack_after_loose_files_are_removed() {
    let root = std::env::temp_dir().join(format!("opendoc-flat-store-pack-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = FlatObjectStore::new(&root, "bucket/prefix").unwrap();
    assert!(store.capabilities().local_pack_files);
    let first = digest_bytes("sha256", b"first flat packed object").unwrap();
    let second = digest_bytes("sha256", b"second flat packed object").unwrap();
    assert!(store
        .put_if_absent(&first, b"first flat packed object")
        .unwrap());
    assert!(store
        .put_if_absent(&second, b"second flat packed object")
        .unwrap());

    let stats = store.compact_loose_objects_to_pack("flat-pack").unwrap();

    assert_eq!(stats.pack, "flat-pack");
    assert_eq!(stats.objects, 2);
    assert!(stats.bytes > 4);
    let index_path = root.join("bucket/prefix/packs/flat-pack.idx");
    let index_bytes = fs::read(index_path).unwrap();
    assert_eq!(index_bytes.get(0..4), Some(&b"ODF0"[..]));
    assert_eq!(decode_pack_index(&index_bytes).unwrap().len(), 2);
    assert!(!store
        .key_path(&ObjectStoreLayout::object_key(&first))
        .unwrap()
        .exists());
    assert!(!store
        .key_path(&ObjectStoreLayout::object_key(&second))
        .unwrap()
        .exists());
    assert!(store.exists(&first).unwrap());
    assert_eq!(
        store.get(&first).unwrap(),
        Some(b"first flat packed object".to_vec())
    );
    assert_eq!(
        store.get(&second).unwrap(),
        Some(b"second flat packed object".to_vec())
    );
    assert!(!store
        .put_if_absent(&first, b"first flat packed object")
        .unwrap());
    assert!(matches!(
        store.compact_loose_objects_to_pack("../bad"),
        Err(StoreError::InvalidPath)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_pack_recompaction_preserves_existing_packed_objects() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-store-pack-rewrite-preserve-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    let first = digest_bytes("sha256", b"first packed object").unwrap();
    store.put_if_absent(&first, b"first packed object").unwrap();
    let first_stats = store.compact_loose_objects_to_pack("main-pack").unwrap();
    assert_eq!(first_stats.objects, 1);
    assert!(!store.object_path(&first).exists());

    let second = digest_bytes("sha256", b"second packed object").unwrap();
    store
        .put_if_absent(&second, b"second packed object")
        .unwrap();
    let second_stats = store.compact_loose_objects_to_pack("main-pack").unwrap();

    assert_eq!(second_stats.objects, 2);
    assert!(!store.object_path(&second).exists());
    assert_eq!(
        store.get(&first).unwrap(),
        Some(b"first packed object".to_vec())
    );
    assert_eq!(
        store.get(&second).unwrap(),
        Some(b"second packed object".to_vec())
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_pack_write_recovers_from_stale_temp_files() {
    let root =
        std::env::temp_dir().join(format!("opendoc-store-pack-stale-temp-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    let hash = digest_bytes("sha256", b"recoverable packed object").unwrap();
    store
        .put_if_absent(&hash, b"recoverable packed object")
        .unwrap();
    let pack_dir = root.join("packs");
    fs::create_dir_all(&pack_dir).unwrap();
    let stale_pack = pack_dir.join(format!("main-pack.pack.tmp-{}", process_tag()));
    let stale_index = pack_dir.join(format!("main-pack.idx.tmp-{}", process_tag()));
    fs::write(&stale_pack, b"interrupted pack write").unwrap();
    fs::write(&stale_index, b"interrupted index write").unwrap();

    let stats = store.compact_loose_objects_to_pack("main-pack").unwrap();

    assert_eq!(stats.objects, 1);
    assert!(!stale_pack.exists());
    assert!(!stale_index.exists());
    assert_eq!(
        store.get(&hash).unwrap(),
        Some(b"recoverable packed object".to_vec())
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_pack_write_validates_hashes_before_creating_temp_files() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-store-pack-bad-hash-no-temp-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    let hash = digest_bytes("sha256", b"expected packed object").unwrap();

    assert!(matches!(
        store.write_pack("main-pack", &[(hash, b"different bytes".to_vec())]),
        Err(StoreError::HashMismatch)
    ));

    let pack_dir = root.join("packs");
    assert!(pack_dir.exists());
    assert!(fs::read_dir(&pack_dir).unwrap().next().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_store_rejects_corrupt_packed_object_bytes() {
    let root = std::env::temp_dir().join(format!("opendoc-store-corrupt-pack-{}", process_tag()));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    let hash = digest_bytes("sha256", b"packed integrity object").unwrap();
    assert!(store
        .put_if_absent(&hash, b"packed integrity object")
        .unwrap());

    let stats = store
        .compact_loose_objects_to_pack("integrity-pack")
        .unwrap();
    assert_eq!(stats.objects, 1);
    assert!(!store.object_path(&hash).exists());

    let index = fs::read(root.join("packs/integrity-pack.idx")).unwrap();
    let entries = decode_pack_index(&index).unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.hash == hash)
        .expect("packed object is indexed");
    let mut pack = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("packs/integrity-pack.pack"))
        .unwrap();
    pack.seek(SeekFrom::Start(entry.offset)).unwrap();
    pack.write_all(b"X").unwrap();
    pack.sync_all().unwrap();

    assert!(matches!(store.get(&hash), Err(StoreError::HashMismatch)));
    assert!(matches!(store.exists(&hash), Err(StoreError::HashMismatch)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_store_rejects_corrupt_binary_pack_index() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-store-corrupt-pack-index-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    let hash = digest_bytes("sha256", b"packed index object").unwrap();
    assert!(store.put_if_absent(&hash, b"packed index object").unwrap());

    let stats = store.compact_loose_objects_to_pack("index-pack").unwrap();
    assert_eq!(stats.objects, 1);
    assert!(!store.object_path(&hash).exists());
    fs::write(
        root.join("packs/index-pack.idx"),
        b"not a binary pack index",
    )
    .unwrap();

    assert!(matches!(
        store.get(&hash),
        Err(StoreError::Format(message)) if message.contains("InvalidMagic")
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_store_rejects_pack_index_that_targets_different_pack() {
    let root = std::env::temp_dir().join(format!(
        "opendoc-store-wrong-pack-index-target-{}",
        process_tag()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalObjectStore::new(&root);
    let hash = digest_bytes("sha256", b"packed wrong target object").unwrap();
    assert!(store
        .put_if_absent(&hash, b"packed wrong target object")
        .unwrap());

    let stats = store.compact_loose_objects_to_pack("main-pack").unwrap();
    assert_eq!(stats.objects, 1);
    assert!(!store.object_path(&hash).exists());
    let index_path = root.join("packs/main-pack.idx");
    let entries = decode_pack_index(&fs::read(&index_path).unwrap()).unwrap();
    fs::write(&index_path, encode_pack_index("other-pack", &entries)).unwrap();

    assert!(matches!(
        store.get(&hash),
        Err(StoreError::Format(message))
            if message.contains("pack index file targets unexpected pack other-pack")
    ));
    fs::write(
        &index_path,
        encode_record(&PackIndexRecord {
            pack: "other.pack".to_string(),
            entries: entries
                .iter()
                .map(|entry| PackIndexEntryRecord {
                    hash: entry.hash.clone(),
                    offset: entry.offset,
                    length: entry.length,
                })
                .collect(),
        }),
    )
    .unwrap();
    assert!(matches!(
        store.get(&hash),
        Err(StoreError::Format(message))
            if message.contains("pack index pack is not a pack name")
    ));
    let mut invalid_offset_entries = entries.clone();
    invalid_offset_entries[0].offset = 0;
    fs::write(
        &index_path,
        encode_pack_index("main-pack", &invalid_offset_entries),
    )
    .unwrap();
    assert!(matches!(
        store.get(&hash),
        Err(StoreError::Format(message))
            if message.contains("pack index entry offset is before pack payload")
    ));
    let mut overflowing_entries = entries.clone();
    overflowing_entries[0].offset = u64::MAX;
    overflowing_entries[0].length = 1;
    fs::write(
        &index_path,
        encode_pack_index("main-pack", &overflowing_entries),
    )
    .unwrap();
    assert!(matches!(
        store.get(&hash),
        Err(StoreError::Format(message))
            if message.contains("byte range overflows")
    ));
    let mut oversized_entries = entries.clone();
    oversized_entries[0].length = 1_000_000;
    fs::write(
        &index_path,
        encode_pack_index("main-pack", &oversized_entries),
    )
    .unwrap();
    assert!(matches!(
        store.get(&hash),
        Err(StoreError::Format(message))
            if message.contains("pack index entry range exceeds pack size")
    ));
    let mut duplicate_entries = entries.clone();
    duplicate_entries.push(entries[0].clone());
    fs::write(
        &index_path,
        encode_pack_index("main-pack", &duplicate_entries),
    )
    .unwrap();
    assert!(matches!(
        store.get(&hash),
        Err(StoreError::Format(message))
            if message.contains("duplicate pack index entry hash")
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_object_store_layout_is_the_literal_set_of_paths_the_format_promises() {
    // Every path below is written out by hand from the layout the repository
    // format documents, against a hash whose digest is spelled in the source.
    // Nothing here calls the function it is checking a second time, so a
    // change to the sharding, the suffix or the directory — any of which
    // orphans every sidecar already written — fails here rather than being
    // restated as the expected value. PLAN88 §7.
    let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let object = HashRef::parse(&format!("sha256:{digest}")).unwrap();

    assert_eq!(
        ObjectStoreLayout::version_label_key(&object),
        "objects/sha256/01/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.label"
    );
    assert_eq!(
        blob_signature_path(&object),
        "objects/sha256/01/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.sig"
    );
    assert_eq!(
        ObjectStoreLayout::tombstone_key(&object),
        "archive/tombstones/sha256/01/\
         0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.tombstone"
    );
    assert_eq!(
        ObjectStoreLayout::uuid_lookup_key("document-uuid").unwrap(),
        "indexes/by-uuid/do/document-uuid.idx"
    );

    assert_eq!(
        ObjectStoreLayout::version_coverage_key(&object),
        "objects/sha256/01/\
         0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.coverage"
    );
    // The signer is part of the key, so two signers do not overwrite each
    // other. The digest below is `sha256("ssh-ed25519 AAAA")`, computed
    // outside this crate.
    assert_eq!(
        ObjectStoreLayout::version_signature_key(&object, "ssh-ed25519 AAAA").unwrap(),
        "signatures/versions/sha256/01/\
         0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/\
         9de8a23119db6492ef40542eb343c0d884c627f46fbd999bf4d9c5f884f905cd.vsig"
    );
    assert_ne!(
        ObjectStoreLayout::version_signature_key(&object, "ssh-ed25519 AAAA").unwrap(),
        ObjectStoreLayout::version_signature_key(&object, "ssh-ed25519 BBBB").unwrap()
    );
    assert!(ObjectStoreLayout::version_signature_key(&object, "  ").is_err());

    // The three sidecars share a shard directory with the object they name and
    // are told apart only by their suffix, so two of them must never collide.
    let label = ObjectStoreLayout::version_label_key(&object);
    let signature = blob_signature_path(&object);
    let coverage = ObjectStoreLayout::version_coverage_key(&object);
    assert_ne!(label, signature);
    assert_ne!(label, coverage);
    assert_ne!(signature, coverage);
    assert_eq!(
        label.trim_end_matches(".label"),
        signature.trim_end_matches(".sig"),
        "the label and signature sidecars stopped sharing a shard"
    );
    assert_eq!(
        label.trim_end_matches(".label"),
        coverage.trim_end_matches(".coverage"),
        "the coverage sidecar stopped sharing a shard with the manifest it names"
    );

    // A different digest goes to a different shard: the prefix is the first
    // two characters of *this* digest, not a constant.
    let other = HashRef::parse(&format!("sha256:fe{}", &digest[2..])).unwrap();
    assert!(
        ObjectStoreLayout::version_label_key(&other).starts_with("objects/sha256/fe/"),
        "{}",
        ObjectStoreLayout::version_label_key(&other)
    );
}
