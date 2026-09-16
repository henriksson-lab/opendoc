//! Golden-byte fixtures for every persisted record.
//!
//! Everything this crate writes is **content addressed**: an object's name is
//! the hash of these bytes. Two builds that encode the same record differently
//! do not produce a compatibility warning — they produce two different objects
//! for one logical record, and a repository forks. The rest of the crate's
//! tests encode twice and round-trip, which is true of *any* deterministic
//! encoder: swapping `to_canonical_vec` for `cbor2::to_vec`, or exchanging two
//! adjacent same-typed fields in a binary `encode_body`, keeps every one of
//! them green. PLAN88 §7.
//!
//! So this module pins the bytes themselves. Two fixtures per record:
//!
//! * the `ODF0` binary envelope (`encode_record`), which is **positional** and
//!   carries no field names and no per-record version — only the one-byte tag.
//!   Exchanging two same-typed fields there is a silent misread of every
//!   record already written. Every fixture below therefore gives each field of
//!   a given type a **distinct** value, so any transposition moves bytes.
//! * the canonical CBOR encoding (`encode_canonical_cbor`), which is
//!   key-addressed and so immune to field reordering, but not to a
//!   non-deterministic map key order.
//!
//! Updating a fixture is not a formality. If a change to a record makes one of
//! these fail, the repository format changed, and the question to answer is
//! what happens to objects already written under the old bytes.

use super::*;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn hash(digest: &str) -> HashRef {
    HashRef::parse(&format!("sha256:{digest}")).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Assert `actual` is exactly the recorded bytes.
///
/// Prints the observed encoding on failure so that a deliberate format change
/// can be transcribed, and so that an accidental one is visible rather than
/// merely reported as "assertion failed".
#[track_caller]
fn assert_golden(label: &str, actual: &[u8], expected: &str) {
    let observed = hex(actual);
    assert_eq!(
        observed, expected,
        "{label}: encoding changed. Observed bytes:\n{observed}\n\
         These bytes are content-addressed: if this change is deliberate, say \
         what happens to objects already written under the old encoding."
    );
}

// ---------------------------------------------------------------------------
// fixtures
//
// Every field of a given type within one record holds a distinct value, so a
// transposition of two same-typed fields cannot leave the bytes unchanged.
// `created_at_ms`-style integers are deliberately byte-patterned so that a
// change of integer width or endianness shows up too.
// ---------------------------------------------------------------------------

fn manifest() -> ManifestRecord {
    ManifestRecord {
        document_uuid: "manifest-document-uuid".to_string(),
        branch: "manifest-branch".to_string(),
        parent: Some(hash("parentmanifest")),
        snapshot: hash("snapshotobject"),
        operation_segments: vec![hash("segmentone"), hash("segmenttwo")],
        signatures: vec![hash("signatureobject")],
        blobs: vec![hash("blobobject")],
        created_at_ms: 0x0102_0304_0506_0708,
    }
}

fn branch_head() -> BranchHeadRecord {
    BranchHeadRecord {
        document_uuid: "head-document-uuid".to_string(),
        branch: "head-branch".to_string(),
        manifest: hash("headmanifest"),
    }
}

fn signature() -> SignatureRecord {
    SignatureRecord {
        target: hash("signaturetarget"),
        signer: "ssh-ed25519 AAAASIGNERKEY".to_string(),
        signer_display: "Ada Lovelace".to_string(),
        title: "Signed manifest".to_string(),
        signed_at_ms: 0x1112_1314_1516_1718,
        signature: vec![0xde, 0xad, 0xbe, 0xef],
    }
}

fn version_label() -> VersionLabelRecord {
    VersionLabelRecord {
        manifest: hash("labelledmanifest"),
        document_uuid: "label-document-uuid".to_string(),
        branch: "label-branch".to_string(),
        label: "Q3 review".to_string(),
        author: "Grace Hopper".to_string(),
        created_at_ms: 0x2122_2324_2526_2728,
    }
}

fn lookup() -> LookupRecord {
    LookupRecord {
        document_uuid: "lookup-document-uuid".to_string(),
        branch: "lookup-branch".to_string(),
        manifest: hash("lookupmanifest"),
        aliases: vec![
            LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/example".to_string(),
            },
            LookupAliasRecord {
                scheme: "isbn".to_string(),
                value: "978-0-306-40615-7".to_string(),
            },
        ],
        created_at_ms: 0x3132_3334_3536_3738,
    }
}

fn tombstone() -> TombstoneRecord {
    TombstoneRecord {
        object: hash("tombstonedobject"),
        archive_locator: "tape://library/pool/slot-7".to_string(),
        restore_hint: "request recall through the storage helpdesk".to_string(),
        created_at_ms: 0x4142_4344_4546_4748,
        signer: "ssh-ed25519 AAAATOMBSTONEKEY".to_string(),
        signature: vec![0x01, 0x02, 0x03, 0x04, 0x05],
    }
}

fn pack_index() -> PackIndexRecord {
    PackIndexRecord {
        pack: "main-pack".to_string(),
        entries: vec![
            PackIndexEntryRecord {
                hash: hash("packentryone"),
                offset: 4,
                length: 9,
            },
            PackIndexEntryRecord {
                hash: hash("packentrytwo"),
                offset: 13,
                length: 21,
            },
        ],
    }
}

fn snapshot() -> SnapshotRecord<Vec<u8>> {
    SnapshotRecord::new(
        "snapshot-document-uuid",
        "opendoc.app-document.v0",
        b"canonical source bytes".to_vec(),
    )
}

fn operation_segment() -> OperationSegmentRecord<Vec<u8>> {
    OperationSegmentRecord::new(
        "segment-document-uuid",
        "segment-branch",
        Some(hash("previoussegment")),
        Some(hash("basemanifest").to_string()),
        vec![b"operation-one".to_vec(), b"operation-two".to_vec()],
    )
}

// ---------------------------------------------------------------------------
// `ODF0` binary envelopes
// ---------------------------------------------------------------------------

#[test]
fn manifest_record_binary_bytes_are_pinned() {
    assert_golden(
        "ManifestRecord ODF0",
        &encode_record(&manifest()),
        "4f44463001000000166d616e69666573742d646f63756d656e742d7575696400\
         00000f6d616e69666573742d6272616e63680100000006736861323536000000\
         0e706172656e746d616e6966657374000000067368613235360000000e736e61\
         7073686f746f626a65637400000002000000067368613235360000000a736567\
         6d656e746f6e65000000067368613235360000000a7365676d656e7474776f00\
         000001000000067368613235360000000f7369676e61747572656f626a656374\
         00000001000000067368613235360000000a626c6f626f626a65637401020304\
         05060708",
    );
}

#[test]
fn branch_head_record_binary_bytes_are_pinned() {
    assert_golden(
        "BranchHeadRecord ODF0",
        &encode_record(&branch_head()),
        "4f4446300200000012686561642d646f63756d656e742d757569640000000b68\
         6561642d6272616e6368000000067368613235360000000c686561646d616e69\
         66657374",
    );
}

#[test]
fn signature_record_binary_bytes_are_pinned() {
    assert_golden(
        "SignatureRecord ODF0",
        &encode_record(&signature()),
        "4f44463003000000067368613235360000000f7369676e617475726574617267\
         6574000000197373682d6564323535313920414141415349474e45524b455900\
         00000c416461204c6f76656c6163650000000f5369676e6564206d616e696665\
         7374111213141516171800000004deadbeef",
    );
}

#[test]
fn version_label_record_binary_bytes_are_pinned() {
    assert_golden(
        "VersionLabelRecord ODF0",
        &encode_record(&version_label()),
        "4f4446300900000006736861323536000000106c6162656c6c65646d616e6966\
         657374000000136c6162656c2d646f63756d656e742d757569640000000c6c61\
         62656c2d6272616e6368000000095133207265766965770000000c4772616365\
         20486f707065722122232425262728",
    );
}

#[test]
fn lookup_record_binary_bytes_are_pinned() {
    assert_golden("LookupRecord ODF0", &encode_record(&lookup()), "4f44463004000000146c6f6f6b75702d646f63756d656e742d75756964000000\
                                                                   0d6c6f6f6b75702d6272616e6368000000067368613235360000000e6c6f6f6b\
                                                                   75706d616e69666573740000000200000003646f690000000f31302e31323334\
                                                                   2f6578616d706c65000000046973626e000000113937382d302d3330362d3430\
                                                                   3631352d373132333435363738");
}

#[test]
fn tombstone_record_binary_bytes_are_pinned() {
    assert_golden(
        "TombstoneRecord ODF0",
        &encode_record(&tombstone()),
        "4f444630050000000673686132353600000010746f6d6273746f6e65646f626a\
         6563740000001a746170653a2f2f6c6962726172792f706f6f6c2f736c6f742d\
         370000002b7265717565737420726563616c6c207468726f7567682074686520\
         73746f726167652068656c706465736b41424344454647480000001c7373682d\
         656432353531392041414141544f4d4253544f4e454b45590000000501020304\
         05",
    );
}

#[test]
fn pack_index_record_binary_bytes_are_pinned() {
    assert_golden(
        "PackIndexRecord ODF0",
        &encode_record(&pack_index()),
        "4f44463006000000096d61696e2d7061636b0000000200000006736861323536\
         0000000c7061636b656e7472796f6e6500000000000000040000000000000009\
         000000067368613235360000000c7061636b656e74727974776f000000000000\
         000d0000000000000015",
    );
}

#[test]
fn snapshot_record_binary_bytes_are_pinned() {
    assert_golden(
        "SnapshotRecord ODF0",
        &encode_record(&snapshot()),
        "4f44463007000000136f70656e646f632e736e617073686f742e763000000016\
         736e617073686f742d646f63756d656e742d75756964000000176f70656e646f\
         632e6170702d646f63756d656e742e76300000001663616e6f6e6963616c2073\
         6f75726365206279746573",
    );
}

#[test]
fn operation_segment_record_binary_bytes_are_pinned() {
    assert_golden(
        "OperationSegmentRecord ODF0",
        &encode_record(&operation_segment()),
        "4f444630080000001c6f70656e646f632e6f7065726174696f6e2d7365676d65\
         6e742e7630000000157365676d656e742d646f63756d656e742d757569640000\
         000e7365676d656e742d6272616e636801000000067368613235360000000f70\
         726576696f75737365676d656e7401000000137368613235363a626173656d61\
         6e6966657374000000020000000d6f7065726174696f6e2d6f6e650000000d6f\
         7065726174696f6e2d74776f",
    );
}

// ---------------------------------------------------------------------------
// canonical CBOR
// ---------------------------------------------------------------------------

#[test]
fn manifest_record_canonical_cbor_bytes_are_pinned() {
    assert_golden(
        "ManifestRecord CBOR",
        &encode_canonical_cbor(&manifest()).unwrap(),
        "a865626c6f627381717368613235363a626c6f626f626a656374666272616e63\
         686f6d616e69666573742d6272616e636866706172656e74757368613235363a\
         706172656e746d616e696665737468736e617073686f74757368613235363a73\
         6e617073686f746f626a6563746a7369676e6174757265738176736861323536\
         3a7369676e61747572656f626a6563746d637265617465645f61745f6d731b01\
         020304050607086d646f63756d656e745f75756964766d616e69666573742d64\
         6f63756d656e742d75756964726f7065726174696f6e5f7365676d656e747382\
         717368613235363a7365676d656e746f6e65717368613235363a7365676d656e\
         7474776f",
    );
}

#[test]
fn branch_head_record_canonical_cbor_bytes_are_pinned() {
    assert_golden(
        "BranchHeadRecord CBOR",
        &encode_canonical_cbor(&branch_head()).unwrap(),
        "a3666272616e63686b686561642d6272616e6368686d616e6966657374737368\
         613235363a686561646d616e69666573746d646f63756d656e745f7575696472\
         686561642d646f63756d656e742d75756964",
    );
}

#[test]
fn signature_record_canonical_cbor_bytes_are_pinned() {
    assert_golden(
        "SignatureRecord CBOR",
        &encode_canonical_cbor(&signature()).unwrap(),
        "a6657469746c656f5369676e6564206d616e6966657374667369676e65727819\
         7373682d6564323535313920414141415349474e45524b455966746172676574\
         767368613235363a7369676e6174757265746172676574697369676e61747572\
         658418de18ad18be18ef6c7369676e65645f61745f6d731b1112131415161718\
         6e7369676e65725f646973706c61796c416461204c6f76656c616365",
    );
}

#[test]
fn version_label_record_canonical_cbor_bytes_are_pinned() {
    assert_golden(
        "VersionLabelRecord CBOR",
        &encode_canonical_cbor(&version_label()).unwrap(),
        "a6656c6162656c6951332072657669657766617574686f726c47726163652048\
         6f70706572666272616e63686c6c6162656c2d6272616e6368686d616e696665\
         7374777368613235363a6c6162656c6c65646d616e69666573746d6372656174\
         65645f61745f6d731b21222324252627286d646f63756d656e745f7575696473\
         6c6162656c2d646f63756d656e742d75756964",
    );
}

#[test]
fn lookup_record_canonical_cbor_bytes_are_pinned() {
    assert_golden(
        "LookupRecord CBOR",
        &encode_canonical_cbor(&lookup()).unwrap(),
        "a5666272616e63686d6c6f6f6b75702d6272616e636867616c696173657382a2\
         6576616c75656f31302e313233342f6578616d706c6566736368656d6563646f\
         69a26576616c7565713937382d302d3330362d34303631352d3766736368656d\
         65646973626e686d616e6966657374757368613235363a6c6f6f6b75706d616e\
         69666573746d637265617465645f61745f6d731b31323334353637386d646f63\
         756d656e745f75756964746c6f6f6b75702d646f63756d656e742d75756964",
    );
}

#[test]
fn tombstone_record_canonical_cbor_bytes_are_pinned() {
    assert_golden(
        "TombstoneRecord CBOR",
        &encode_canonical_cbor(&tombstone()).unwrap(),
        "a6666f626a656374777368613235363a746f6d6273746f6e65646f626a656374\
         667369676e6572781c7373682d656432353531392041414141544f4d4253544f\
         4e454b4559697369676e61747572658501020304056c726573746f72655f6869\
         6e74782b7265717565737420726563616c6c207468726f756768207468652073\
         746f726167652068656c706465736b6d637265617465645f61745f6d731b4142\
         4344454647486f617263686976655f6c6f6361746f72781a746170653a2f2f6c\
         6962726172792f706f6f6c2f736c6f742d37",
    );
}

#[test]
fn pack_index_record_canonical_cbor_bytes_are_pinned() {
    assert_golden(
        "PackIndexRecord CBOR",
        &encode_canonical_cbor(&pack_index()).unwrap(),
        "a2647061636b696d61696e2d7061636b67656e747269657382a3646861736873\
         7368613235363a7061636b656e7472796f6e65666c656e67746809666f666673\
         657404a36468617368737368613235363a7061636b656e74727974776f666c65\
         6e67746815666f66667365740d",
    );
}

#[test]
fn snapshot_record_canonical_cbor_bytes_are_pinned() {
    assert_golden(
        "SnapshotRecord CBOR",
        &encode_canonical_cbor(&snapshot()).unwrap(),
        "a4646b696e64736f70656e646f632e736e617073686f742e763066736f757263\
         659618631861186e186f186e186918631861186c18201873186f187518721863\
         18651820186218791874186518736d646f63756d656e745f7575696476736e61\
         7073686f742d646f63756d656e742d757569646d736f757263655f666f726d61\
         74776f70656e646f632e6170702d646f63756d656e742e7630",
    );
}

#[test]
fn operation_segment_record_canonical_cbor_bytes_are_pinned() {
    assert_golden(
        "OperationSegmentRecord CBOR",
        &encode_canonical_cbor(&operation_segment()).unwrap(),
        "a6646b696e64781c6f70656e646f632e6f7065726174696f6e2d7365676d656e\
         742e7630666272616e63686e7365676d656e742d6272616e63686a6f70657261\
         74696f6e73828d186f187018651872186118741869186f186e182d186f186e18\
         658d186f187018651872186118741869186f186e182d18741877186f6d626173\
         655f6d616e6966657374737368613235363a626173656d616e69666573746d64\
         6f63756d656e745f75756964757365676d656e742d646f63756d656e742d7575\
         69647070726576696f75735f7365676d656e74767368613235363a7072657669\
         6f75737365676d656e74",
    );
}

// ---------------------------------------------------------------------------
// what the fixtures are worth
// ---------------------------------------------------------------------------

#[test]
fn every_fixture_is_a_record_that_would_really_be_written() {
    // A golden fixture over an invalid record would pin bytes no repository
    // ever contains. Each of these is a record the writer would accept.
    manifest().validate().unwrap();
    branch_head().validate().unwrap();
    signature().validate().unwrap();
    version_label().validate().unwrap();
    lookup().validate().unwrap();
    tombstone().validate().unwrap();
    pack_index().validate().unwrap();
    snapshot().validate().unwrap();
    operation_segment().validate().unwrap();
}

#[test]
fn every_pinned_record_still_decodes_to_the_fixture_it_came_from() {
    // The goldens pin the *writer*. This pins the reader against the same
    // bytes, so a writer and reader cannot be changed in step and stay green
    // while every object already on disk becomes unreadable.
    assert_eq!(
        decode_record::<ManifestRecord>(&encode_record(&manifest())).unwrap(),
        manifest()
    );
    assert_eq!(
        decode_record::<BranchHeadRecord>(&encode_record(&branch_head())).unwrap(),
        branch_head()
    );
    assert_eq!(
        decode_record::<SignatureRecord>(&encode_record(&signature())).unwrap(),
        signature()
    );
    assert_eq!(
        decode_record::<VersionLabelRecord>(&encode_record(&version_label())).unwrap(),
        version_label()
    );
    assert_eq!(
        decode_record::<LookupRecord>(&encode_record(&lookup())).unwrap(),
        lookup()
    );
    assert_eq!(
        decode_record::<TombstoneRecord>(&encode_record(&tombstone())).unwrap(),
        tombstone()
    );
    assert_eq!(
        decode_record::<PackIndexRecord>(&encode_record(&pack_index())).unwrap(),
        pack_index()
    );
    assert_eq!(
        decode_record::<SnapshotRecord<Vec<u8>>>(&encode_record(&snapshot())).unwrap(),
        snapshot()
    );
    assert_eq!(
        decode_record::<OperationSegmentRecord<Vec<u8>>>(&encode_record(&operation_segment()))
            .unwrap(),
        operation_segment()
    );
}

#[test]
fn the_record_tag_is_the_only_discriminator_and_every_one_is_distinct() {
    // A binary record carries no version of its own — `ODF0` plus a one-byte
    // tag is the whole header — so the tag is the only thing standing between
    // a `SignatureRecord` and a `TombstoneRecord` being read as each other.
    // Pin the assignment: reusing a retired number for a new record type is a
    // silent misread of every object written under the old one.
    let tags: Vec<(&str, u8)> = vec![
        ("ManifestRecord", ManifestRecord::TAG),
        ("BranchHeadRecord", BranchHeadRecord::TAG),
        ("SignatureRecord", SignatureRecord::TAG),
        ("LookupRecord", LookupRecord::TAG),
        ("TombstoneRecord", TombstoneRecord::TAG),
        ("PackIndexRecord", PackIndexRecord::TAG),
        ("SnapshotRecord", SnapshotRecord::<Vec<u8>>::TAG),
        (
            "OperationSegmentRecord",
            OperationSegmentRecord::<Vec<u8>>::TAG,
        ),
        ("VersionLabelRecord", VersionLabelRecord::TAG),
    ];
    assert_eq!(
        tags,
        vec![
            ("ManifestRecord", 1),
            ("BranchHeadRecord", 2),
            ("SignatureRecord", 3),
            ("LookupRecord", 4),
            ("TombstoneRecord", 5),
            ("PackIndexRecord", 6),
            ("SnapshotRecord", 7),
            ("OperationSegmentRecord", 8),
            ("VersionLabelRecord", 9),
        ]
    );
    let distinct: BTreeSet<u8> = tags.iter().map(|(_, tag)| *tag).collect();
    assert_eq!(distinct.len(), tags.len(), "two records share a tag byte");
}

#[test]
fn a_record_body_is_positional_so_the_tag_alone_cannot_catch_a_field_swap() {
    // The hazard the goldens above exist for, stated as a test rather than as
    // a comment: two `String` fields exchanged in `encode_body` produce a
    // record that still decodes, still round-trips, and still carries the same
    // tag — it just means something else. Here the writer's output for a
    // signature whose signer and display name are exchanged is a perfectly
    // well-formed `SignatureRecord`.
    let mut swapped = signature();
    std::mem::swap(&mut swapped.signer, &mut swapped.signer_display);
    let bytes = encode_record(&swapped);
    let decoded = decode_record::<SignatureRecord>(&bytes).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, swapped);
    // Nothing about the envelope distinguishes it from the fixture …
    assert_eq!(bytes[..5], encode_record(&signature())[..5]);
    // … and only a byte-level fixture separates them at all.
    assert_ne!(bytes, encode_record(&signature()));
}

#[test]
fn canonical_cbor_orders_map_keys_by_their_encoded_bytes_not_by_the_string() {
    // RFC 8949 §4.2.1 deterministic encoding sorts map keys by the bytewise
    // order of their *encoded* form, so a shorter key always sorts before a
    // longer one. `"aa"` sorts before `"b"` as a string and after it as an
    // encoding, which is the smallest input that tells a canonical encoder
    // from a merely deterministic one: `cbor2::to_vec` emits these in
    // `BTreeMap` order, `to_canonical_vec` emits `"b"` first.
    let mut map: BTreeMap<String, u8> = BTreeMap::new();
    map.insert("aa".to_string(), 1);
    map.insert("b".to_string(), 2);
    assert_golden(
        "two-key map",
        &encode_canonical_cbor(&map).unwrap(),
        "a261620262616101",
    );
}

#[test]
fn canonical_cbor_is_stable_across_the_order_a_map_was_built_in() {
    // The property that makes content addressing work at all, over an input
    // whose iteration order is not already the answer.
    let keys = ["gamma", "a", "beta", "zz", "b"];
    let mut forward: BTreeMap<String, u8> = BTreeMap::new();
    let mut backward: BTreeMap<String, u8> = BTreeMap::new();
    for (index, key) in keys.iter().enumerate() {
        forward.insert((*key).to_string(), index as u8);
    }
    for (index, key) in keys.iter().enumerate().rev() {
        backward.insert((*key).to_string(), index as u8);
    }
    let encoded = encode_canonical_cbor(&forward).unwrap();
    assert_eq!(encoded, encode_canonical_cbor(&backward).unwrap());
    // …and the order really is by encoded bytes: the one-character keys first.
    let order: Vec<&str> = ["a", "b", "zz", "beta", "gamma"].to_vec();
    let mut positions = Vec::new();
    for key in &order {
        let needle: Vec<u8> = std::iter::once(0x60 | key.len() as u8)
            .chain(key.bytes())
            .collect();
        positions.push(
            encoded
                .windows(needle.len())
                .position(|window| window == needle)
                .unwrap_or_else(|| panic!("key {key} missing from {}", hex(&encoded))),
        );
    }
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "canonical key order was not by encoded bytes: {positions:?} in {}",
        hex(&encoded)
    );
}
