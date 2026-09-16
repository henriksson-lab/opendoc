//! Is what the encoder emits actually *canonical* CBOR?
//!
//! `golden_tests` pins the bytes of every record, which catches a change to
//! the encoder. It does not say what is wrong with the new bytes, and it says
//! nothing at all about a record whose fixture has not been written yet — a
//! new field, a new record type, a `Document` on its way into a snapshot. The
//! rest of the crate's CBOR tests encode twice and compare, which is true of
//! any deterministic encoder.
//!
//! Canonicality is not a style preference here. Objects are named by the hash
//! of these bytes and signatures are taken over them, so an encoder that emits
//! the same *value* two ways forks a repository between two builds and
//! invalidates a signature that was never tampered with. RFC 8949 §4.2.1 names
//! the three requirements, and this module checks all three directly on the
//! bytes:
//!
//! * definite lengths only — no indefinite-length strings, arrays or maps,
//! * preferred (shortest) argument encodings for every integer and length,
//! * map keys in the bytewise lexicographic order of their *encodings*, with
//!   no duplicates.
//!
//! `scan_canonical` is deliberately written from the RFC rather than from
//! `cbor2`, so it is a second opinion and not a restatement of the encoder.
//! `the_scanner_rejects_every_non_canonical_form_it_is_meant_to_catch` proves
//! it can say no; without that this file would be exactly the kind of test
//! PLAN88 §7 is about.

use super::*;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// the checker
// ---------------------------------------------------------------------------

struct Canonical<'a> {
    bytes: &'a [u8],
    pos: usize,
}

struct Head {
    major: u8,
    info: u8,
    argument: u64,
    argument_bytes: usize,
}

/// Check that `bytes` is one complete RFC 8949 §4.2.1 core-deterministic CBOR
/// item and nothing else.
fn scan_canonical(bytes: &[u8]) -> Result<(), String> {
    let mut scan = Canonical { bytes, pos: 0 };
    scan.item()?;
    if scan.pos != bytes.len() {
        return Err(format!(
            "{} trailing byte(s) after the top-level item",
            bytes.len() - scan.pos
        ));
    }
    Ok(())
}

fn assert_canonical(label: &str, bytes: &[u8]) {
    if let Err(reason) = scan_canonical(bytes) {
        panic!(
            "{label} is not canonical CBOR: {reason}\n  bytes: {}",
            hex(bytes)
        );
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl<'a> Canonical<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let all = self.bytes;
        if self.pos + count > all.len() {
            return Err(format!(
                "item at byte {} runs {} byte(s) past the end",
                self.pos,
                self.pos + count - all.len()
            ));
        }
        let slice = &all[self.pos..self.pos + count];
        self.pos += count;
        Ok(slice)
    }

    fn head(&mut self) -> Result<Head, String> {
        let at = self.pos;
        let initial = self.take(1)?[0];
        let major = initial >> 5;
        let info = initial & 0x1f;
        let (argument, argument_bytes) = match info {
            0..=23 => (u64::from(info), 0usize),
            24 => (u64::from(self.take(1)?[0]), 1),
            25 => {
                let raw = self.take(2)?;
                (u64::from(u16::from_be_bytes([raw[0], raw[1]])), 2)
            }
            26 => {
                let raw = self.take(4)?;
                (
                    u64::from(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]])),
                    4,
                )
            }
            27 => {
                let raw = self.take(8)?;
                let mut wide = [0u8; 8];
                wide.copy_from_slice(raw);
                (u64::from_be_bytes(wide), 8)
            }
            28..=30 => {
                return Err(format!(
                    "reserved additional information {info} (major type {major}) at byte {at}"
                ))
            }
            _ => {
                return Err(format!(
                    "indefinite-length item (major type {major}) at byte {at}"
                ))
            }
        };
        // Major type 7 uses the same slots for float widths, which are a
        // property of the value and are checked separately.
        if major != 7 {
            let shortest = match argument {
                0..=23 => 0usize,
                24..=0xff => 1,
                0x100..=0xffff => 2,
                0x1_0000..=0xffff_ffff => 4,
                _ => 8,
            };
            if argument_bytes != shortest {
                return Err(format!(
                    "argument {argument} (major type {major}) is written in {argument_bytes} \
                     byte(s) at byte {at}; the preferred encoding uses {shortest}"
                ));
            }
        }
        Ok(Head {
            major,
            info,
            argument,
            argument_bytes,
        })
    }

    fn item(&mut self) -> Result<(), String> {
        let all = self.bytes;
        let head = self.head()?;
        match head.major {
            0 | 1 => Ok(()),
            2 | 3 => {
                let length = usize::try_from(head.argument)
                    .map_err(|_| format!("string length {} does not fit", head.argument))?;
                self.take(length)?;
                Ok(())
            }
            4 => {
                for _ in 0..head.argument {
                    self.item()?;
                }
                Ok(())
            }
            5 => {
                let mut previous: Option<&'a [u8]> = None;
                for _ in 0..head.argument {
                    let key_start = self.pos;
                    self.item()?;
                    let key = &all[key_start..self.pos];
                    if let Some(earlier) = previous {
                        match earlier.cmp(key) {
                            Ordering::Less => {}
                            Ordering::Equal => {
                                return Err(format!(
                                    "duplicate map key {} at byte {key_start}",
                                    hex(key)
                                ))
                            }
                            Ordering::Greater => {
                                return Err(format!(
                                    "map keys out of order at byte {key_start}: {} follows {}",
                                    hex(key),
                                    hex(earlier)
                                ))
                            }
                        }
                    }
                    previous = Some(key);
                    self.item()?;
                }
                Ok(())
            }
            6 => self.item(),
            _ => self.simple_or_float(&head),
        }
    }

    fn simple_or_float(&mut self, head: &Head) -> Result<(), String> {
        let at = self.pos - head.argument_bytes - 1;
        match head.info {
            0..=23 => Ok(()),
            24 => {
                if head.argument < 32 {
                    Err(format!(
                        "simple value {} at byte {at} must use the one-byte form",
                        head.argument
                    ))
                } else {
                    Ok(())
                }
            }
            25 => Ok(()), // f16 is already the narrowest width
            26 => {
                let value = f32::from_bits(head.argument as u32);
                if value.is_nan() {
                    return Err(format!("NaN at byte {at} must be encoded as f16"));
                }
                if f16_values().contains(&value.to_bits()) {
                    return Err(format!(
                        "float {value} at byte {at} is written as f32 but is exactly \
                         representable as f16"
                    ));
                }
                Ok(())
            }
            27 => {
                let value = f64::from_bits(head.argument);
                if value.is_nan() {
                    return Err(format!("NaN at byte {at} must be encoded as f16"));
                }
                if ((value as f32) as f64).to_bits() == value.to_bits() {
                    return Err(format!(
                        "float {value} at byte {at} is written as f64 but is exactly \
                         representable as f32"
                    ));
                }
                Ok(())
            }
            _ => Err(format!(
                "reserved simple-value encoding {} at byte {at}",
                head.info
            )),
        }
    }
}

/// Every `f32` bit pattern that some `f16` denotes exactly.
fn f16_values() -> &'static HashSet<u32> {
    static VALUES: OnceLock<HashSet<u32>> = OnceLock::new();
    VALUES.get_or_init(|| {
        (0..=u16::MAX)
            .map(|bits| f16_to_f32(bits).to_bits())
            .collect()
    })
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let fraction = u32::from(bits & 0x3ff);
    let assembled = if exponent == 0 {
        if fraction == 0 {
            sign
        } else {
            // Subnormal: shift the fraction up until the implicit bit appears,
            // which costs one binade of exponent per shift.
            let mut shifted = fraction;
            let mut shifts = 0u32;
            while shifted & 0x400 == 0 {
                shifted <<= 1;
                shifts += 1;
            }
            sign | ((113 - shifts) << 23) | ((shifted & 0x3ff) << 13)
        }
    } else if exponent == 0x1f {
        sign | (0xff << 23) | (fraction << 13)
    } else {
        sign | ((exponent + 112) << 23) | (fraction << 13)
    };
    f32::from_bits(assembled)
}

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

fn hash_ref(digest: &str) -> HashRef {
    HashRef::parse(&format!("sha256:{digest}")).unwrap()
}

fn manifest() -> ManifestRecord {
    ManifestRecord {
        document_uuid: "canonical-document-uuid".to_string(),
        branch: "main".to_string(),
        parent: Some(hash_ref("parentmanifest")),
        snapshot: hash_ref("snapshotobject"),
        operation_segments: vec![hash_ref("segmentone"), hash_ref("segmenttwo")],
        signatures: vec![hash_ref("signatureobject")],
        blobs: vec![hash_ref("blobobject")],
        created_at_ms: 0x0102_0304_0506_0708,
    }
}

fn branch_head() -> BranchHeadRecord {
    BranchHeadRecord {
        document_uuid: "head-document-uuid".to_string(),
        branch: "head-branch".to_string(),
        manifest: hash_ref("headmanifest"),
    }
}

fn signature() -> SignatureRecord {
    SignatureRecord {
        target: hash_ref("signaturetarget"),
        signer: "ssh-ed25519 AAAASIGNERKEY".to_string(),
        signer_display: "Ada Lovelace".to_string(),
        title: "Signed manifest".to_string(),
        signed_at_ms: 24,
        signature: vec![1, 2, 3, 4],
    }
}

fn version_label() -> VersionLabelRecord {
    VersionLabelRecord {
        manifest: hash_ref("labelmanifest"),
        document_uuid: "label-document-uuid".to_string(),
        branch: "label-branch".to_string(),
        label: "Before the rewrite".to_string(),
        author: "Ada".to_string(),
        created_at_ms: 65_536,
    }
}

fn lookup() -> LookupRecord {
    LookupRecord {
        document_uuid: "lookup-document-uuid".to_string(),
        branch: "lookup-branch".to_string(),
        manifest: hash_ref("lookupmanifest"),
        aliases: vec![LookupAliasRecord {
            scheme: "doi".to_string(),
            value: "10.1000/xyz".to_string(),
        }],
        created_at_ms: 255,
    }
}

fn tombstone() -> TombstoneRecord {
    TombstoneRecord {
        object: hash_ref("tombstonedobject"),
        archive_locator: "s3://archive/object".to_string(),
        restore_hint: "ask the archivist".to_string(),
        created_at_ms: 256,
        signer: "ssh-ed25519 AAAASIGNERKEY".to_string(),
        signature: vec![9, 9, 9],
    }
}

fn pack_index() -> PackIndexRecord {
    PackIndexRecord {
        pack: "pack-one".to_string(),
        entries: vec![PackIndexEntryRecord {
            hash: hash_ref("packedobject"),
            offset: 4_294_967_296,
            length: 23,
        }],
    }
}

fn version_coverage() -> VersionCoverageRecord {
    VersionCoverageRecord::for_manifest(&manifest()).unwrap()
}

fn snapshot() -> SnapshotRecord<Vec<u8>> {
    SnapshotRecord::new(
        "snapshot-document-uuid",
        "opendoc.document.v0",
        b"canonical source bytes".to_vec(),
    )
}

fn operation_segment() -> OperationSegmentRecord<Vec<u8>> {
    OperationSegmentRecord::new(
        "segment-document-uuid",
        "segment-branch",
        Some(hash_ref("previoussegment")),
        Some("sha256:basemanifest".to_string()),
        vec![b"operation-1".to_vec(), b"operation-2".to_vec()],
    )
}

/// Every record this crate persists, encoded canonically, with its name.
fn every_record_encoded() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "ManifestRecord",
            encode_canonical_cbor(&manifest()).unwrap(),
        ),
        (
            "BranchHeadRecord",
            encode_canonical_cbor(&branch_head()).unwrap(),
        ),
        (
            "SignatureRecord",
            encode_canonical_cbor(&signature()).unwrap(),
        ),
        (
            "VersionLabelRecord",
            encode_canonical_cbor(&version_label()).unwrap(),
        ),
        ("LookupRecord", encode_canonical_cbor(&lookup()).unwrap()),
        (
            "TombstoneRecord",
            encode_canonical_cbor(&tombstone()).unwrap(),
        ),
        (
            "PackIndexRecord",
            encode_canonical_cbor(&pack_index()).unwrap(),
        ),
        (
            "SnapshotRecord",
            encode_canonical_cbor(&snapshot()).unwrap(),
        ),
        (
            "OperationSegmentRecord",
            encode_canonical_cbor(&operation_segment()).unwrap(),
        ),
        (
            "VersionCoverageRecord",
            version_coverage().signing_payload().unwrap(),
        ),
    ]
}

// ---------------------------------------------------------------------------
// the checker can say no
// ---------------------------------------------------------------------------

/// Bytes assembled by hand, never by the encoder under test.
fn bytes_of(hex: &str) -> Vec<u8> {
    let stripped: String = hex.chars().filter(|ch| !ch.is_whitespace()).collect();
    (0..stripped.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&stripped[index..index + 2], 16).unwrap())
        .collect()
}

#[test]
fn the_scanner_rejects_every_non_canonical_form_it_is_meant_to_catch() {
    // Each row is a hand-written encoding of a value the encoder can produce,
    // paired with the canonical encoding of the *same* value. The canonical
    // one must pass and the other must fail with the named reason — a scanner
    // that answered "fine" to everything, or "broken" to everything, fails
    // here rather than silently blessing the crate's real output.
    let cases: [(&str, &str, &str, &str); 9] = [
        (
            "indefinite-length array",
            "9f0102ff",
            "820102",
            "indefinite-length item",
        ),
        (
            "indefinite-length map",
            "bf6161 01 ff",
            "a1616101",
            "indefinite-length item",
        ),
        (
            "indefinite-length (segmented) text string",
            "7f616161 62ff",
            "626162",
            "indefinite-length item",
        ),
        (
            "one-byte argument for a value below 24",
            "1817",
            "17",
            "preferred encoding uses 0",
        ),
        (
            "two-byte argument for a value that fits in one",
            "1900ff",
            "18ff",
            "preferred encoding uses 1",
        ),
        (
            "eight-byte argument for a value that fits in four",
            "1b00000000ffffffff",
            "1affffffff",
            "preferred encoding uses 4",
        ),
        (
            "non-shortest string length",
            "7803616263",
            "63616263",
            "preferred encoding uses 0",
        ),
        (
            // "aa" sorts before "b" as a string and after it as an encoding.
            "map keys in string order rather than encoded order",
            "a262616101616202",
            "a2616202 62616101",
            "map keys out of order",
        ),
        (
            "duplicate map key",
            "a2616101616102",
            "a1616101",
            "duplicate map key",
        ),
    ];
    for (name, bad, good, reason) in cases {
        assert_canonical(
            &format!("the canonical counterpart of {name}"),
            &bytes_of(good),
        );
        let error = scan_canonical(&bytes_of(bad))
            .expect_err(&format!("{name} was accepted as canonical CBOR"));
        assert!(
            error.contains(reason),
            "{name}: expected an error naming {reason:?}, got {error:?}"
        );
    }
}

#[test]
fn the_scanner_rejects_a_float_wider_than_its_value_needs() {
    // 1.0 is exactly an f16, so neither the f32 nor the f64 spelling of it is
    // canonical. 1e300 needs all 64 bits and 1e30 needs 32, so those are.
    assert_canonical("f16 1.0", &bytes_of("f93c00"));
    assert_canonical("f64 1e300", &bytes_of("fb7e37e43c8800759c"));
    assert_canonical("f32 1e30", &bytes_of("fa7149f2ca"));
    for (name, encoded, reason) in [
        ("f32 1.0", "fa3f800000", "representable as f16"),
        ("f64 1.0", "fb3ff0000000000000", "representable as f32"),
        ("f64 NaN", "fb7ff8000000000000", "must be encoded as f16"),
    ] {
        let error = scan_canonical(&bytes_of(encoded)).expect_err(&format!("{name} was accepted"));
        assert!(
            error.contains(reason),
            "{name}: expected an error naming {reason:?}, got {error:?}"
        );
    }
}

#[test]
fn the_scanner_reads_the_whole_item_and_nothing_after_it() {
    assert_canonical("a bare integer", &bytes_of("01"));
    let error = scan_canonical(&bytes_of("0102")).expect_err("trailing bytes were accepted");
    assert!(error.contains("trailing byte"), "{error}");
    let error = scan_canonical(&bytes_of("8201")).expect_err("a truncated array was accepted");
    assert!(error.contains("past the end"), "{error}");
}

// ---------------------------------------------------------------------------
// …and the crate's own output passes it
// ---------------------------------------------------------------------------

#[test]
fn every_persisted_record_encodes_as_core_deterministic_cbor() {
    for (name, encoded) in every_record_encoded() {
        assert_canonical(name, &encoded);
    }
}

#[test]
fn nested_and_wide_values_encode_as_core_deterministic_cbor() {
    // The records above are flat maps of short keys. Deep nesting, a map with
    // keys either side of every argument-width boundary, and a string long
    // enough to need a two-byte length exercise the paths they do not.
    let mut wide: BTreeMap<u64, String> = BTreeMap::new();
    for key in [
        0u64,
        23,
        24,
        255,
        256,
        65_535,
        65_536,
        4_294_967_295,
        4_294_967_296,
    ] {
        wide.insert(key, "x".repeat(300));
    }
    assert_canonical(
        "a map spanning every argument width",
        &encode_canonical_cbor(&wide).unwrap(),
    );

    let nested = vec![vec![vec![manifest(), manifest()], Vec::new()]];
    assert_canonical("nested records", &encode_canonical_cbor(&nested).unwrap());

    let mut strings: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for key in ["gamma", "a", "beta", "zz", "b", "aa"] {
        strings.insert(key.to_string(), vec![0u8; 70_000]);
    }
    assert_canonical(
        "long byte strings",
        &encode_canonical_cbor(&strings).unwrap(),
    );
}

#[test]
fn integer_arguments_are_written_in_their_shortest_form() {
    // Hand-computed from RFC 8949 §3: 0x17, then the 1-, 2-, 4- and 8-byte
    // argument forms at the first value that needs each. An encoder that
    // widened `u64` fields to eight bytes — the obvious way to write one —
    // produces `1b0000000000000017` for the first entry.
    assert_eq!(
        hex(&encode_canonical_cbor(&vec![
            23u64,
            24,
            255,
            256,
            65_535,
            65_536,
            4_294_967_295,
            4_294_967_296,
        ])
        .unwrap()),
        "88\
         17\
         1818\
         18ff\
         190100\
         19ffff\
         1a00010000\
         1affffffff\
         1b0000000100000000"
    );
    // …and negative integers, which encode -1-n.
    assert_eq!(
        hex(&encode_canonical_cbor(&vec![-1i64, -24, -25, -256, -257]).unwrap()),
        "85 20 37 3818 38ff 390100".replace(' ', "")
    );
}

// ---------------------------------------------------------------------------
// decode → encode is the identity on canonical bytes
// ---------------------------------------------------------------------------

#[test]
fn re_encoding_a_decoded_record_reproduces_the_same_bytes() {
    // Content addressing needs the round trip to be exact at the *byte* level,
    // not merely to preserve the value: a repository reads a record, writes it
    // back, and the object must keep its name. Comparing the decoded values
    // instead — which the rest of the crate's tests do — passes for an encoder
    // that emits map keys in a different order each run.
    macro_rules! round_trip {
        ($name:literal, $record:expr, $type:ty) => {{
            let encoded = encode_canonical_cbor(&$record).unwrap();
            let decoded: $type = decode_cbor(&encoded).unwrap();
            assert_eq!($record, decoded, "{} did not survive the round trip", $name);
            assert_eq!(
                hex(&encode_canonical_cbor(&decoded).unwrap()),
                hex(&encoded),
                "{} re-encoded to different bytes",
                $name
            );
        }};
    }
    round_trip!("ManifestRecord", manifest(), ManifestRecord);
    round_trip!("BranchHeadRecord", branch_head(), BranchHeadRecord);
    round_trip!("SignatureRecord", signature(), SignatureRecord);
    round_trip!("VersionLabelRecord", version_label(), VersionLabelRecord);
    round_trip!("LookupRecord", lookup(), LookupRecord);
    round_trip!("TombstoneRecord", tombstone(), TombstoneRecord);
    round_trip!("PackIndexRecord", pack_index(), PackIndexRecord);
    round_trip!("SnapshotRecord", snapshot(), SnapshotRecord<Vec<u8>>);
    round_trip!(
        "OperationSegmentRecord",
        operation_segment(),
        OperationSegmentRecord<Vec<u8>>
    );
}

#[test]
fn a_non_canonically_encoded_record_is_normalised_when_it_is_re_encoded() {
    // The bytes below are a `BranchHeadRecord` written by hand, by a peer that
    // is not this encoder: an indefinite-length map, keys in struct
    // declaration order instead of encoded order, one key length written in
    // the two-byte form, and one value as a segmented text string. `cbor2`
    // accepts all of it — "be liberal in what you accept" — so the guarantee
    // the repository actually needs is that what comes back *out* is the one
    // canonical spelling, whatever went in.
    let non_canonical = bytes_of(concat!(
        "bf", // map, indefinite length
        "79000d",
        "646f63756d656e745f75756964", // "document_uuid", 2-byte length
        "72",
        "686561642d646f63756d656e742d75756964",
        "66",
        "6272616e6368", // "branch"
        "6b",
        "686561642d6272616e6368",
        "68",
        "6d616e6966657374", // "manifest"
        "73",
        "7368613235363a686561646d616e6966657374",
        "ff"
    ));
    // The fixture has to really be non-canonical, or this test proves nothing.
    let reason = scan_canonical(&non_canonical).expect_err("the fixture is already canonical");
    assert!(reason.contains("indefinite-length item"), "{reason}");

    let decoded: BranchHeadRecord =
        decode_cbor(&non_canonical).expect("cbor2 accepts a liberally-encoded record");
    assert_eq!(
        decoded,
        branch_head(),
        "the record decoded to the wrong value"
    );

    let re_encoded = encode_canonical_cbor(&decoded).unwrap();
    assert_canonical("the re-encoded record", &re_encoded);
    assert_eq!(
        hex(&re_encoded),
        hex(&encode_canonical_cbor(&branch_head()).unwrap()),
        "a non-canonical input did not normalise to the canonical bytes"
    );
    assert_ne!(
        hex(&re_encoded),
        hex(&non_canonical),
        "the input was passed through unchanged"
    );
}

#[test]
fn a_segmented_string_is_refused_rather_than_silently_accepted() {
    // The other half of "rejected or normalised". `cbor2`'s own documentation
    // says decoding "handles indefinite-length items, segmented strings" — its
    // *serde* path does not, and that is the path this crate decodes through.
    // Refusal is a fine answer for content-addressed storage; being wrong
    // about which answer it is, is not, so it is pinned here.
    let segmented_value = bytes_of(concat!(
        "a3",
        "66",
        "6272616e6368", // "branch"
        "7f",
        "6468656164",
        "662d6272616e6368",
        "ff", // "head" + "-branch"
        "68",
        "6d616e6966657374", // "manifest"
        "73",
        "7368613235363a686561646d616e6966657374",
        "6d",
        "646f63756d656e745f75756964", // "document_uuid"
        "72",
        "686561642d646f63756d656e742d75756964",
    ));
    let reason = scan_canonical(&segmented_value).expect_err("the fixture is already canonical");
    assert!(reason.contains("indefinite-length item"), "{reason}");
    let refused = decode_cbor::<BranchHeadRecord>(&segmented_value);
    assert!(
        refused.is_err(),
        "a segmented string decoded to {:?}; if that ever starts working, the \
         re-encoding path has to be checked the way the map above is",
        refused
    );
}

#[test]
fn a_merely_deterministic_encoder_would_not_pass_this_module() {
    // The negative control for the whole file. `cbor2::to_vec` is
    // deterministic — it round-trips, and encoding twice agrees — and it is
    // the natural thing to reach for. It emits struct fields in declaration
    // order, which for `ManifestRecord` is not the canonical key order, so
    // swapping it in would fork every repository written by a build that had
    // not.
    let deterministic = cbor2::to_vec(&manifest()).unwrap();
    assert_eq!(
        deterministic,
        cbor2::to_vec(&manifest()).unwrap(),
        "the negative control is not even deterministic"
    );
    let decoded: ManifestRecord = decode_cbor(&deterministic).unwrap();
    assert_eq!(
        decoded,
        manifest(),
        "the negative control does not round-trip"
    );

    let reason = scan_canonical(&deterministic)
        .expect_err("cbor2::to_vec already emits canonical CBOR, so this module tests nothing");
    assert!(reason.contains("map keys out of order"), "{reason}");
    assert_ne!(
        hex(&deterministic),
        hex(&encode_canonical_cbor(&manifest()).unwrap()),
        "the two encoders agree, so the fixture cannot tell them apart"
    );
}

/// A version signature is taken over these bytes, so "the payload" has to be
/// one sequence of bytes and not a family of encodings that happen to decode
/// alike. `from_signing_payload` is where that is enforced.
#[test]
fn a_version_coverage_payload_that_is_not_canonical_is_refused() {
    let coverage = version_coverage();
    let canonical = coverage.signing_payload().unwrap();
    assert_canonical("the version coverage payload", &canonical);
    assert_eq!(
        VersionCoverageRecord::from_signing_payload(&canonical).unwrap(),
        coverage
    );

    // `cbor2::to_vec` writes the same value with map keys in struct
    // declaration order rather than encoded order: a different byte string
    // that decodes to the same record.
    let non_canonical = cbor2::to_vec(&coverage).unwrap();
    let reason = scan_canonical(&non_canonical).expect_err("the fixture is already canonical");
    assert!(reason.contains("out of order"), "{reason}");
    assert_eq!(
        decode_cbor::<VersionCoverageRecord>(&non_canonical).unwrap(),
        coverage,
        "the fixture has to decode to the same record, or it proves nothing"
    );

    let err = VersionCoverageRecord::from_signing_payload(&non_canonical)
        .expect_err("non-canonical payload bytes were accepted");
    assert!(
        matches!(&err, FormatError::InvalidRecord(message) if message.contains("not canonical")),
        "{err:?}"
    );
}
