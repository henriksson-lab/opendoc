use opendoc_core::HashRef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

const MAGIC: &[u8; 4] = b"ODF0";
const MAX_RECORD_VEC_ITEMS: u32 = 1_000_000;

pub const SNAPSHOT_KIND: &str = "opendoc.snapshot.v0";
pub const OPERATION_SEGMENT_KIND: &str = "opendoc.operation-segment.v0";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestRecord {
    pub document_uuid: String,
    pub branch: String,
    #[serde(with = "hash_ref_opt_serde")]
    pub parent: Option<HashRef>,
    #[serde(with = "hash_ref_serde")]
    pub snapshot: HashRef,
    #[serde(with = "hash_ref_vec_serde")]
    pub operation_segments: Vec<HashRef>,
    #[serde(with = "hash_ref_vec_serde")]
    pub signatures: Vec<HashRef>,
    #[serde(with = "hash_ref_vec_serde")]
    pub blobs: Vec<HashRef>,
    pub created_at_ms: u64,
}

impl ManifestRecord {
    pub fn validate(&self) -> Result<(), FormatError> {
        require_canonical_document_uuid("manifest document_uuid", &self.document_uuid)?;
        require_repository_key_segment("manifest branch", &self.branch)?;
        require_unique_hash_refs("manifest operation segment", &self.operation_segments)?;
        require_unique_hash_refs("manifest signature", &self.signatures)?;
        require_unique_hash_refs("manifest blob", &self.blobs)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SnapshotRecord<T> {
    pub kind: String,
    pub document_uuid: String,
    pub source_format: String,
    pub source: T,
}

impl<T> SnapshotRecord<T> {
    pub fn new(
        document_uuid: impl Into<String>,
        source_format: impl Into<String>,
        source: T,
    ) -> Self {
        Self {
            kind: SNAPSHOT_KIND.to_string(),
            document_uuid: document_uuid.into(),
            source_format: source_format.into(),
            source,
        }
    }

    pub fn validate_kind(&self) -> Result<(), FormatError> {
        if self.kind == SNAPSHOT_KIND {
            Ok(())
        } else {
            Err(FormatError::UnsupportedKind(self.kind.clone()))
        }
    }

    pub fn validate(&self) -> Result<(), FormatError> {
        self.validate_kind()?;
        require_canonical_document_uuid("snapshot document_uuid", &self.document_uuid)?;
        require_no_surrounding_whitespace("snapshot source_format", &self.source_format)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationSegmentRecord<T> {
    pub kind: String,
    pub document_uuid: String,
    pub branch: String,
    #[serde(with = "hash_ref_opt_serde")]
    pub previous_segment: Option<HashRef>,
    pub base_manifest: Option<String>,
    pub operations: Vec<T>,
}

impl<T> OperationSegmentRecord<T> {
    pub fn new(
        document_uuid: impl Into<String>,
        branch: impl Into<String>,
        previous_segment: Option<HashRef>,
        base_manifest: Option<String>,
        operations: Vec<T>,
    ) -> Self {
        Self {
            kind: OPERATION_SEGMENT_KIND.to_string(),
            document_uuid: document_uuid.into(),
            branch: branch.into(),
            previous_segment,
            base_manifest,
            operations,
        }
    }

    pub fn validate_kind(&self) -> Result<(), FormatError> {
        if self.kind == OPERATION_SEGMENT_KIND {
            Ok(())
        } else {
            Err(FormatError::UnsupportedKind(self.kind.clone()))
        }
    }

    pub fn validate(&self) -> Result<(), FormatError> {
        self.validate_kind()?;
        require_canonical_document_uuid("operation segment document_uuid", &self.document_uuid)?;
        require_repository_key_segment("operation segment branch", &self.branch)?;
        if self.operations.is_empty() {
            return Err(FormatError::InvalidRecord(
                "operation segment operations are empty".to_string(),
            ));
        }
        if self.operations.len() > MAX_RECORD_VEC_ITEMS as usize {
            return Err(FormatError::InvalidRecord(format!(
                "operation segment operations length exceeds supported limit {MAX_RECORD_VEC_ITEMS}"
            )));
        }
        if let Some(base_manifest) = &self.base_manifest {
            require_no_surrounding_whitespace("operation segment base_manifest", base_manifest)?;
            HashRef::parse(base_manifest).map_err(|_| {
                FormatError::InvalidRecord(
                    "operation segment base_manifest is not a hash reference".to_string(),
                )
            })?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BranchHeadRecord {
    pub document_uuid: String,
    pub branch: String,
    #[serde(with = "hash_ref_serde")]
    pub manifest: HashRef,
}

impl BranchHeadRecord {
    pub fn validate(&self) -> Result<(), FormatError> {
        require_canonical_document_uuid("branch head document_uuid", &self.document_uuid)?;
        require_repository_key_segment("branch head branch", &self.branch)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SignatureRecord {
    #[serde(with = "hash_ref_serde")]
    pub target: HashRef,
    pub signer: String,
    pub signer_display: String,
    pub title: String,
    pub signed_at_ms: u64,
    pub signature: Vec<u8>,
}

impl SignatureRecord {
    pub fn validate(&self) -> Result<(), FormatError> {
        require_no_surrounding_whitespace("signature signer", &self.signer)?;
        require_no_surrounding_whitespace("signature signer_display", &self.signer_display)?;
        require_no_surrounding_whitespace("signature title", &self.title)?;
        require_non_empty_bytes("signature bytes", &self.signature)?;
        Ok(())
    }
}

/// Human label attached to an already-committed manifest.
///
/// Manifests are content addressed, so a label can never live inside the
/// manifest it names: writing one would change the manifest hash and orphan
/// every child that points at the old one. Labels are therefore sidecars keyed
/// by the target manifest hash, exactly like blob signature sidecars.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VersionLabelRecord {
    #[serde(with = "hash_ref_serde")]
    pub manifest: HashRef,
    pub document_uuid: String,
    pub branch: String,
    pub label: String,
    pub author: String,
    pub created_at_ms: u64,
}

impl VersionLabelRecord {
    pub fn validate(&self) -> Result<(), FormatError> {
        require_canonical_document_uuid("version label document_uuid", &self.document_uuid)?;
        require_repository_key_segment("version label branch", &self.branch)?;
        require_no_surrounding_whitespace("version label label", &self.label)?;
        require_no_surrounding_whitespace("version label author", &self.author)?;
        if self.label.is_empty() {
            return Err(FormatError::InvalidRecord(
                "version label label is empty".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LookupRecord {
    pub document_uuid: String,
    pub branch: String,
    #[serde(with = "hash_ref_serde")]
    pub manifest: HashRef,
    pub aliases: Vec<LookupAliasRecord>,
    pub created_at_ms: u64,
}

impl LookupRecord {
    pub fn validate(&self) -> Result<(), FormatError> {
        require_canonical_document_uuid("lookup document_uuid", &self.document_uuid)?;
        require_repository_key_segment("lookup branch", &self.branch)?;
        let mut aliases = BTreeSet::new();
        for alias in &self.aliases {
            alias.validate()?;
            let alias_key = lookup_alias_key(alias);
            if !aliases.insert(alias_key.clone()) {
                return Err(FormatError::InvalidRecord(format!(
                    "duplicate lookup alias {}:{}",
                    alias_key.0, alias_key.1
                )));
            }
        }
        Ok(())
    }
}

fn lookup_alias_key(alias: &LookupAliasRecord) -> (String, String) {
    let scheme = alias.scheme.trim().to_ascii_lowercase();
    if scheme == "doi" {
        (scheme, alias.value.trim().to_ascii_lowercase())
    } else {
        (scheme, alias.value.clone())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LookupAliasRecord {
    pub scheme: String,
    pub value: String,
}

impl LookupAliasRecord {
    pub fn validate(&self) -> Result<(), FormatError> {
        require_repository_key_segment("lookup alias scheme", &self.scheme)?;
        require_no_surrounding_whitespace("lookup alias value", &self.value)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TombstoneRecord {
    #[serde(with = "hash_ref_serde")]
    pub object: HashRef,
    pub archive_locator: String,
    pub restore_hint: String,
    pub created_at_ms: u64,
    pub signer: String,
    pub signature: Vec<u8>,
}

impl TombstoneRecord {
    pub fn validate(&self) -> Result<(), FormatError> {
        require_no_surrounding_whitespace("tombstone archive_locator", &self.archive_locator)?;
        require_no_surrounding_whitespace("tombstone restore_hint", &self.restore_hint)?;
        require_no_surrounding_whitespace("tombstone signer", &self.signer)?;
        require_non_empty_bytes("tombstone signature", &self.signature)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PackIndexRecord {
    pub pack: String,
    pub entries: Vec<PackIndexEntryRecord>,
}

impl PackIndexRecord {
    pub fn validate(&self) -> Result<(), FormatError> {
        require_pack_name("pack index pack", &self.pack)?;
        let mut hashes = BTreeSet::new();
        let mut ranges = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            entry.validate()?;
            if !hashes.insert(entry.hash.to_string()) {
                return Err(FormatError::InvalidRecord(format!(
                    "duplicate pack index entry hash {}",
                    entry.hash
                )));
            }
            let end = entry.offset.checked_add(entry.length).ok_or_else(|| {
                FormatError::InvalidRecord(format!(
                    "pack index entry {} byte range overflows",
                    entry.hash
                ))
            })?;
            ranges.push((entry.offset, end, entry.hash.to_string()));
        }
        ranges.sort();
        let mut previous_end = 0;
        for (offset, end, hash) in ranges {
            if offset < previous_end {
                return Err(FormatError::InvalidRecord(format!(
                    "pack index entry {} overlaps previous pack entry",
                    hash
                )));
            }
            previous_end = end;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PackIndexEntryRecord {
    #[serde(with = "hash_ref_serde")]
    pub hash: HashRef,
    pub offset: u64,
    pub length: u64,
}

impl PackIndexEntryRecord {
    pub fn validate(&self) -> Result<(), FormatError> {
        if self.offset < 4 {
            Err(FormatError::InvalidRecord(
                "pack index entry offset is before pack payload".to_string(),
            ))
        } else if self.length == 0 {
            Err(FormatError::InvalidRecord(
                "pack index entry length is zero".to_string(),
            ))
        } else {
            Ok(())
        }
    }
}

pub trait BinaryRecord: Sized {
    const TAG: u8;
    fn encode_body(&self, out: &mut Vec<u8>);
    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError>;
}

pub fn encode_record<T: BinaryRecord>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(T::TAG);
    value.encode_body(&mut out);
    out
}

pub fn encode_canonical_cbor<T: Serialize>(value: &T) -> Result<Vec<u8>, FormatError> {
    cbor2::to_canonical_vec(value).map_err(|err| FormatError::Cbor(err.to_string()))
}

pub fn decode_cbor<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, FormatError> {
    cbor2::from_slice(bytes).map_err(|err| FormatError::Cbor(err.to_string()))
}

pub fn decode_record<T: BinaryRecord>(bytes: &[u8]) -> Result<T, FormatError> {
    let mut reader = Reader::new(bytes);
    reader.expect(MAGIC)?;
    let tag = reader.u8()?;
    if tag != T::TAG {
        return Err(FormatError::UnexpectedTag {
            expected: T::TAG,
            actual: tag,
        });
    }
    let value = T::decode_body(&mut reader)?;
    if !reader.is_empty() {
        return Err(FormatError::TrailingBytes);
    }
    Ok(value)
}

impl BinaryRecord for ManifestRecord {
    const TAG: u8 = 1;

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_str(out, &self.document_uuid);
        put_str(out, &self.branch);
        put_opt_hash(out, self.parent.as_ref());
        put_hash(out, &self.snapshot);
        put_hash_vec(out, &self.operation_segments);
        put_hash_vec(out, &self.signatures);
        put_hash_vec(out, &self.blobs);
        put_u64(out, self.created_at_ms);
    }

    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            document_uuid: input.string()?,
            branch: input.string()?,
            parent: input.opt_hash()?,
            snapshot: input.hash()?,
            operation_segments: input.hash_vec()?,
            signatures: input.hash_vec()?,
            blobs: input.hash_vec()?,
            created_at_ms: input.u64()?,
        })
    }
}

impl BinaryRecord for BranchHeadRecord {
    const TAG: u8 = 2;

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_str(out, &self.document_uuid);
        put_str(out, &self.branch);
        put_hash(out, &self.manifest);
    }

    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            document_uuid: input.string()?,
            branch: input.string()?,
            manifest: input.hash()?,
        })
    }
}

impl BinaryRecord for SignatureRecord {
    const TAG: u8 = 3;

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_hash(out, &self.target);
        put_str(out, &self.signer);
        put_str(out, &self.signer_display);
        put_str(out, &self.title);
        put_u64(out, self.signed_at_ms);
        put_bytes(out, &self.signature);
    }

    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            target: input.hash()?,
            signer: input.string()?,
            signer_display: input.string()?,
            title: input.string()?,
            signed_at_ms: input.u64()?,
            signature: input.bytes()?,
        })
    }
}

impl BinaryRecord for LookupRecord {
    const TAG: u8 = 4;

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_str(out, &self.document_uuid);
        put_str(out, &self.branch);
        put_hash(out, &self.manifest);
        put_lookup_alias_vec(out, &self.aliases);
        put_u64(out, self.created_at_ms);
    }

    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            document_uuid: input.string()?,
            branch: input.string()?,
            manifest: input.hash()?,
            aliases: input.lookup_alias_vec()?,
            created_at_ms: input.u64()?,
        })
    }
}

impl BinaryRecord for TombstoneRecord {
    const TAG: u8 = 5;

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_hash(out, &self.object);
        put_str(out, &self.archive_locator);
        put_str(out, &self.restore_hint);
        put_u64(out, self.created_at_ms);
        put_str(out, &self.signer);
        put_bytes(out, &self.signature);
    }

    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            object: input.hash()?,
            archive_locator: input.string()?,
            restore_hint: input.string()?,
            created_at_ms: input.u64()?,
            signer: input.string()?,
            signature: input.bytes()?,
        })
    }
}

impl BinaryRecord for PackIndexRecord {
    const TAG: u8 = 6;

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_str(out, &self.pack);
        put_pack_index_entry_vec(out, &self.entries);
    }

    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            pack: input.string()?,
            entries: input.pack_index_entry_vec()?,
        })
    }
}

impl BinaryRecord for SnapshotRecord<Vec<u8>> {
    const TAG: u8 = 7;

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_str(out, &self.kind);
        put_str(out, &self.document_uuid);
        put_str(out, &self.source_format);
        put_bytes(out, &self.source);
    }

    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            kind: input.string()?,
            document_uuid: input.string()?,
            source_format: input.string()?,
            source: input.bytes()?,
        })
    }
}

impl BinaryRecord for OperationSegmentRecord<Vec<u8>> {
    const TAG: u8 = 8;

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_str(out, &self.kind);
        put_str(out, &self.document_uuid);
        put_str(out, &self.branch);
        put_opt_hash(out, self.previous_segment.as_ref());
        put_opt_str(out, self.base_manifest.as_deref());
        put_bytes_vec(out, &self.operations);
    }

    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            kind: input.string()?,
            document_uuid: input.string()?,
            branch: input.string()?,
            previous_segment: input.opt_hash()?,
            base_manifest: input.opt_string()?,
            operations: input.bytes_vec()?,
        })
    }
}

impl BinaryRecord for VersionLabelRecord {
    const TAG: u8 = 9;

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_hash(out, &self.manifest);
        put_str(out, &self.document_uuid);
        put_str(out, &self.branch);
        put_str(out, &self.label);
        put_str(out, &self.author);
        put_u64(out, self.created_at_ms);
    }

    fn decode_body(input: &mut Reader<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            manifest: input.hash()?,
            document_uuid: input.string()?,
            branch: input.string()?,
            label: input.string()?,
            author: input.string()?,
            created_at_ms: input.u64()?,
        })
    }
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn expect(&mut self, expected: &[u8]) -> Result<(), FormatError> {
        let actual = self.take(expected.len())?;
        if actual != expected {
            return Err(FormatError::InvalidMagic);
        }
        Ok(())
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], FormatError> {
        let end = self.offset.checked_add(len).ok_or(FormatError::Overflow)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(FormatError::UnexpectedEof)?;
        self.offset = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(*self.take(1)?.first().unwrap())
    }

    fn u32(&mut self) -> Result<u32, FormatError> {
        let mut buf = [0; 4];
        buf.copy_from_slice(self.take(4)?);
        Ok(u32::from_be_bytes(buf))
    }

    fn u64(&mut self) -> Result<u64, FormatError> {
        let mut buf = [0; 8];
        buf.copy_from_slice(self.take(8)?);
        Ok(u64::from_be_bytes(buf))
    }

    fn string(&mut self) -> Result<String, FormatError> {
        let bytes = self.bytes()?;
        String::from_utf8(bytes).map_err(|_| FormatError::InvalidUtf8)
    }

    fn bytes(&mut self) -> Result<Vec<u8>, FormatError> {
        let len = self.u32()? as usize;
        Ok(self.take(len)?.to_vec())
    }

    fn hash(&mut self) -> Result<HashRef, FormatError> {
        let algorithm = self.string()?;
        let digest = self.string()?;
        HashRef::new(algorithm, digest).map_err(|_| FormatError::InvalidHash)
    }

    fn opt_hash(&mut self) -> Result<Option<HashRef>, FormatError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.hash()?)),
            _ => Err(FormatError::InvalidOption),
        }
    }

    fn opt_string(&mut self) -> Result<Option<String>, FormatError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.string()?)),
            _ => Err(FormatError::InvalidOption),
        }
    }

    fn hash_vec(&mut self) -> Result<Vec<HashRef>, FormatError> {
        let len = self.bounded_vec_len("hash vector")?;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(self.hash()?);
        }
        Ok(out)
    }

    fn bytes_vec(&mut self) -> Result<Vec<Vec<u8>>, FormatError> {
        let len = self.bounded_vec_len("bytes vector")?;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(self.bytes()?);
        }
        Ok(out)
    }

    fn lookup_alias_vec(&mut self) -> Result<Vec<LookupAliasRecord>, FormatError> {
        let len = self.bounded_vec_len("lookup alias vector")?;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(LookupAliasRecord {
                scheme: self.string()?,
                value: self.string()?,
            });
        }
        Ok(out)
    }

    fn pack_index_entry_vec(&mut self) -> Result<Vec<PackIndexEntryRecord>, FormatError> {
        let len = self.bounded_vec_len("pack index entry vector")?;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(PackIndexEntryRecord {
                hash: self.hash()?,
                offset: self.u64()?,
                length: self.u64()?,
            });
        }
        Ok(out)
    }

    fn bounded_vec_len(&mut self, label: &str) -> Result<usize, FormatError> {
        let len = self.u32()?;
        if len > MAX_RECORD_VEC_ITEMS {
            return Err(FormatError::InvalidRecord(format!(
                "{label} length exceeds supported limit {MAX_RECORD_VEC_ITEMS}"
            )));
        }
        Ok(len as usize)
    }
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    put_u32(out, value.len() as u32);
    out.extend_from_slice(value);
}

fn put_str(out: &mut Vec<u8>, value: &str) {
    put_bytes(out, value.as_bytes());
}

fn put_hash(out: &mut Vec<u8>, value: &HashRef) {
    put_str(out, value.algorithm());
    put_str(out, value.digest());
}

fn put_opt_hash(out: &mut Vec<u8>, value: Option<&HashRef>) {
    match value {
        Some(hash) => {
            out.push(1);
            put_hash(out, hash);
        }
        None => out.push(0),
    }
}

fn put_opt_str(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            put_str(out, value);
        }
        None => out.push(0),
    }
}

fn put_hash_vec(out: &mut Vec<u8>, values: &[HashRef]) {
    put_u32(out, values.len() as u32);
    for value in values {
        put_hash(out, value);
    }
}

fn put_bytes_vec(out: &mut Vec<u8>, values: &[Vec<u8>]) {
    put_u32(out, values.len() as u32);
    for value in values {
        put_bytes(out, value);
    }
}

fn put_lookup_alias_vec(out: &mut Vec<u8>, values: &[LookupAliasRecord]) {
    put_u32(out, values.len() as u32);
    for value in values {
        put_str(out, &value.scheme);
        put_str(out, &value.value);
    }
}

fn put_pack_index_entry_vec(out: &mut Vec<u8>, values: &[PackIndexEntryRecord]) {
    put_u32(out, values.len() as u32);
    for value in values {
        put_hash(out, &value.hash);
        put_u64(out, value.offset);
        put_u64(out, value.length);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormatError {
    InvalidMagic,
    UnexpectedTag { expected: u8, actual: u8 },
    UnsupportedKind(String),
    UnexpectedEof,
    TrailingBytes,
    InvalidUtf8,
    InvalidHash,
    InvalidOption,
    Overflow,
    Cbor(String),
    InvalidRecord(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FormatError {}

fn require_non_empty(label: &str, value: &str) -> Result<(), FormatError> {
    if value.trim().is_empty() {
        Err(FormatError::InvalidRecord(format!("{label} is empty")))
    } else {
        Ok(())
    }
}

fn require_canonical_document_uuid(label: &str, value: &str) -> Result<(), FormatError> {
    require_non_empty(label, value)?;
    if value.trim() != value {
        Err(FormatError::InvalidRecord(format!(
            "{label} has surrounding whitespace"
        )))
    } else {
        Ok(())
    }
}

fn require_no_surrounding_whitespace(label: &str, value: &str) -> Result<(), FormatError> {
    require_non_empty(label, value)?;
    if value.trim() != value {
        Err(FormatError::InvalidRecord(format!(
            "{label} has surrounding whitespace"
        )))
    } else {
        Ok(())
    }
}

fn require_repository_key_segment(label: &str, value: &str) -> Result<(), FormatError> {
    require_no_surrounding_whitespace(label, value)?;
    let valid = value != "."
        && value != ".."
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.');
    if valid {
        Ok(())
    } else {
        Err(FormatError::InvalidRecord(format!(
            "{label} is not a repository key segment"
        )))
    }
}

fn require_pack_name(label: &str, value: &str) -> Result<(), FormatError> {
    require_no_surrounding_whitespace(label, value)?;
    let valid = !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if valid {
        Ok(())
    } else {
        Err(FormatError::InvalidRecord(format!(
            "{label} is not a pack name"
        )))
    }
}

fn require_unique_hash_refs(label: &str, values: &[HashRef]) -> Result<(), FormatError> {
    let mut seen = BTreeSet::new();
    for value in values {
        let key = value.to_string();
        if !seen.insert(key.clone()) {
            return Err(FormatError::InvalidRecord(format!(
                "duplicate {label} reference {key}"
            )));
        }
    }
    Ok(())
}

fn require_non_empty_bytes(label: &str, value: &[u8]) -> Result<(), FormatError> {
    if value.is_empty() {
        Err(FormatError::InvalidRecord(format!("{label} is empty")))
    } else {
        Ok(())
    }
}

mod hash_ref_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &HashRef, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<HashRef, D::Error> {
        let value = String::deserialize(deserializer)?;
        HashRef::parse(&value).map_err(serde::de::Error::custom)
    }
}

mod hash_ref_opt_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Option<HashRef>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .as_ref()
            .map(ToString::to_string)
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<HashRef>, D::Error> {
        let value = Option::<String>::deserialize(deserializer)?;
        value
            .map(|item| HashRef::parse(&item).map_err(serde::de::Error::custom))
            .transpose()
    }
}

mod hash_ref_vec_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &[HashRef], serializer: S) -> Result<S::Ok, S::Error> {
        value
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<HashRef>, D::Error> {
        Vec::<String>::deserialize(deserializer)?
            .into_iter()
            .map(|item| HashRef::parse(&item).map_err(serde::de::Error::custom))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips_deterministically() {
        let record = ManifestRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: vec![HashRef::parse("sha256:bbb").unwrap()],
            signatures: vec![HashRef::parse("sha256:ddd").unwrap()],
            blobs: vec![HashRef::parse("sha256:ccc").unwrap()],
            created_at_ms: 7,
        };
        let one = encode_record(&record);
        let two = encode_record(&record);
        assert_eq!(one, two);
        assert_eq!(decode_record::<ManifestRecord>(&one).unwrap(), record);
    }

    #[test]
    fn manifest_encodes_as_canonical_cbor() {
        let record = ManifestRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: vec![HashRef::parse("sha256:bbb").unwrap()],
            signatures: vec![HashRef::parse("sha256:ddd").unwrap()],
            blobs: vec![HashRef::parse("sha256:ccc").unwrap()],
            created_at_ms: 7,
        };
        let one = encode_canonical_cbor(&record).unwrap();
        let two = encode_canonical_cbor(&record).unwrap();
        assert_eq!(one, two);
        assert_eq!(decode_cbor::<ManifestRecord>(&one).unwrap(), record);
    }

    #[test]
    fn snapshot_and_operation_segments_are_canonical_cbor() {
        let snapshot =
            SnapshotRecord::new("doc", "opendoc.app-document.v0", "source-state".to_string());
        let one = encode_canonical_cbor(&snapshot).unwrap();
        let two = encode_canonical_cbor(&snapshot).unwrap();
        assert_eq!(one, two);
        let decoded: SnapshotRecord<String> = decode_cbor(&one).unwrap();
        assert_eq!(decoded, snapshot);
        decoded.validate().unwrap();

        let segment = OperationSegmentRecord::new(
            "doc",
            "main",
            Some(HashRef::parse("sha256:previous").unwrap()),
            Some("sha256:manifest".to_string()),
            vec!["insert paragraph".to_string(), "format range".to_string()],
        );
        let one = encode_canonical_cbor(&segment).unwrap();
        let two = encode_canonical_cbor(&segment).unwrap();
        assert_eq!(one, two);
        let decoded: OperationSegmentRecord<String> = decode_cbor(&one).unwrap();
        assert_eq!(decoded, segment);
        decoded.validate().unwrap();
    }

    #[test]
    fn snapshot_and_operation_segments_have_native_binary_envelopes() {
        let snapshot = SnapshotRecord::new(
            "doc",
            "opendoc.app-document.v0",
            b"canonical source bytes".to_vec(),
        );
        let first = encode_record(&snapshot);
        let second = encode_record(&snapshot);
        assert_eq!(first, second);
        assert_eq!(
            first[..5],
            [b'O', b'D', b'F', b'0', SnapshotRecord::<Vec<u8>>::TAG]
        );
        let decoded = decode_record::<SnapshotRecord<Vec<u8>>>(&first).unwrap();
        assert_eq!(decoded, snapshot);
        decoded.validate().unwrap();

        let base = HashRef::parse("sha256:ccc").unwrap();
        let segment = OperationSegmentRecord::new(
            "doc",
            "main",
            Some(base.clone()),
            Some(base.to_string()),
            vec![b"operation-1".to_vec(), b"operation-2".to_vec()],
        );
        let first = encode_record(&segment);
        let second = encode_record(&segment);
        assert_eq!(first, second);
        assert_eq!(
            first[..5],
            [
                b'O',
                b'D',
                b'F',
                b'0',
                OperationSegmentRecord::<Vec<u8>>::TAG
            ]
        );
        let decoded = decode_record::<OperationSegmentRecord<Vec<u8>>>(&first).unwrap();
        assert_eq!(decoded, segment);
        decoded.validate().unwrap();
    }

    #[test]
    fn operation_segment_binary_envelope_rejects_oversized_operation_vectors() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.push(OperationSegmentRecord::<Vec<u8>>::TAG);
        put_str(&mut bytes, OPERATION_SEGMENT_KIND);
        put_str(&mut bytes, "doc");
        put_str(&mut bytes, "main");
        put_opt_hash(&mut bytes, None);
        put_opt_str(&mut bytes, None);
        put_u32(&mut bytes, MAX_RECORD_VEC_ITEMS + 1);

        assert!(matches!(
            decode_record::<OperationSegmentRecord<Vec<u8>>>(&bytes),
            Err(FormatError::InvalidRecord(message))
                if message == "bytes vector length exceeds supported limit 1000000"
        ));
    }

    #[test]
    fn snapshot_and_operation_segment_records_validate_required_envelope_fields() {
        let mut snapshot =
            SnapshotRecord::new("doc", "opendoc.app-document.v0", "source-state".to_string());
        snapshot.validate().unwrap();
        snapshot.kind = "other".to_string();
        assert!(matches!(
            snapshot.validate(),
            Err(FormatError::UnsupportedKind(kind)) if kind == "other"
        ));

        let mut snapshot =
            SnapshotRecord::new("doc", "opendoc.app-document.v0", "source-state".to_string());
        snapshot.document_uuid.clear();
        assert!(matches!(
            snapshot.validate(),
            Err(FormatError::InvalidRecord(message)) if message == "snapshot document_uuid is empty"
        ));

        let mut snapshot =
            SnapshotRecord::new("doc", "opendoc.app-document.v0", "source-state".to_string());
        snapshot.source_format = " ".to_string();
        assert!(matches!(
            snapshot.validate(),
            Err(FormatError::InvalidRecord(message)) if message == "snapshot source_format is empty"
        ));

        let snapshot = SnapshotRecord::new(
            "doc",
            " opendoc.app-document.v0 ",
            "source-state".to_string(),
        );
        assert!(matches!(
            snapshot.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "snapshot source_format has surrounding whitespace"
        ));

        let mut segment = OperationSegmentRecord::new(
            "doc",
            "main",
            Some(HashRef::parse("sha256:previous").unwrap()),
            Some("sha256:manifest".to_string()),
            vec!["insert paragraph".to_string()],
        );
        segment.validate().unwrap();
        segment.branch.clear();
        assert!(matches!(
            segment.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "operation segment branch is empty"
        ));

        let segment: OperationSegmentRecord<String> =
            OperationSegmentRecord::new("doc", "main", None, None, Vec::new());
        assert!(matches!(
            segment.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "operation segment operations are empty"
        ));

        let segment = OperationSegmentRecord::new("doc", "main", None, None, vec![(); 1_000_001]);
        assert!(matches!(
            segment.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "operation segment operations length exceeds supported limit 1000000"
        ));

        let segment = OperationSegmentRecord::new(
            "doc",
            "main",
            None,
            Some(" ".to_string()),
            vec!["insert paragraph".to_string()],
        );
        assert!(matches!(
            segment.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "operation segment base_manifest is empty"
        ));

        let segment = OperationSegmentRecord::new(
            "doc",
            "main",
            None,
            Some(" sha256:manifest ".to_string()),
            vec!["insert paragraph".to_string()],
        );
        assert!(matches!(
            segment.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "operation segment base_manifest has surrounding whitespace"
        ));

        let segment = OperationSegmentRecord::new(
            "doc",
            "main",
            None,
            Some("not-a-hash-ref".to_string()),
            vec!["insert paragraph".to_string()],
        );
        assert!(matches!(
            segment.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "operation segment base_manifest is not a hash reference"
        ));
    }

    #[test]
    fn lookup_and_tombstone_records_are_binary_and_deterministic() {
        let lookup = LookupRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            manifest: HashRef::parse("sha256:aaa").unwrap(),
            aliases: vec![LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/example".to_string(),
            }],
            created_at_ms: 9,
        };
        let one = encode_record(&lookup);
        let two = encode_record(&lookup);
        assert_eq!(one, two);
        assert_eq!(decode_record::<LookupRecord>(&one).unwrap(), lookup);

        let tombstone = TombstoneRecord {
            object: HashRef::parse("sha256:bbb").unwrap(),
            archive_locator: "tape://library/pool/slot".to_string(),
            restore_hint: "request recall through storage helpdesk".to_string(),
            created_at_ms: 10,
            signer: "ssh-ed25519 AAAA".to_string(),
            signature: vec![1, 2, 3],
        };
        let one = encode_record(&tombstone);
        let two = encode_record(&tombstone);
        assert_eq!(one, two);
        assert_eq!(decode_record::<TombstoneRecord>(&one).unwrap(), tombstone);
    }

    #[test]
    fn pack_index_record_is_binary_and_deterministic() {
        let record = PackIndexRecord {
            pack: "main-pack".to_string(),
            entries: vec![PackIndexEntryRecord {
                hash: HashRef::parse("sha256:abc").unwrap(),
                offset: 4,
                length: 9,
            }],
        };
        let one = encode_record(&record);
        let two = encode_record(&record);
        assert_eq!(one, two);
        assert_eq!(one.get(0..4), Some(&b"ODF0"[..]));
        assert_eq!(decode_record::<PackIndexRecord>(&one).unwrap(), record);

        let mut invalid = PackIndexRecord {
            pack: " ".to_string(),
            entries: Vec::new(),
        };
        assert!(matches!(
            invalid.validate(),
            Err(FormatError::InvalidRecord(message)) if message == "pack index pack is empty"
        ));
        invalid.pack = "main-pack".to_string();
        invalid.entries.push(PackIndexEntryRecord {
            hash: HashRef::parse("sha256:abc").unwrap(),
            offset: 4,
            length: 0,
        });
        assert!(matches!(
            invalid.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "pack index entry length is zero"
        ));
    }

    #[test]
    fn binary_record_decode_rejects_unbounded_vector_lengths_before_allocation() {
        let mut manifest = Vec::new();
        manifest.extend_from_slice(MAGIC);
        manifest.push(ManifestRecord::TAG);
        put_str(&mut manifest, "doc");
        put_str(&mut manifest, "main");
        put_opt_hash(&mut manifest, None);
        put_hash(&mut manifest, &HashRef::parse("sha256:aaa").unwrap());
        put_u32(&mut manifest, MAX_RECORD_VEC_ITEMS + 1);
        assert!(matches!(
            decode_record::<ManifestRecord>(&manifest),
            Err(FormatError::InvalidRecord(message))
                if message == "hash vector length exceeds supported limit 1000000"
        ));

        let mut lookup = Vec::new();
        lookup.extend_from_slice(MAGIC);
        lookup.push(LookupRecord::TAG);
        put_str(&mut lookup, "doc");
        put_str(&mut lookup, "main");
        put_hash(&mut lookup, &HashRef::parse("sha256:aaa").unwrap());
        put_u32(&mut lookup, MAX_RECORD_VEC_ITEMS + 1);
        assert!(matches!(
            decode_record::<LookupRecord>(&lookup),
            Err(FormatError::InvalidRecord(message))
                if message == "lookup alias vector length exceeds supported limit 1000000"
        ));

        let mut pack = Vec::new();
        pack.extend_from_slice(MAGIC);
        pack.push(PackIndexRecord::TAG);
        put_str(&mut pack, "main-pack");
        put_u32(&mut pack, MAX_RECORD_VEC_ITEMS + 1);
        assert!(matches!(
            decode_record::<PackIndexRecord>(&pack),
            Err(FormatError::InvalidRecord(message))
                if message == "pack index entry vector length exceeds supported limit 1000000"
        ));
    }

    #[test]
    fn decoded_repository_records_validate_semantic_required_fields() {
        let hash = HashRef::parse("sha256:aaa").unwrap();
        let mut snapshot = SnapshotRecord::new("doc", "opendoc.source.v0", "source");
        snapshot.validate().unwrap();
        snapshot.document_uuid = " doc".to_string();
        assert!(matches!(
            snapshot.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "snapshot document_uuid has surrounding whitespace"
        ));

        let mut segment =
            OperationSegmentRecord::new("doc", "main", None, Some(hash.to_string()), vec!["op"]);
        segment.validate().unwrap();
        segment.document_uuid = "doc ".to_string();
        assert!(matches!(
            segment.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "operation segment document_uuid has surrounding whitespace"
        ));
        segment.document_uuid = "doc".to_string();
        segment.branch = "../main".to_string();
        assert!(matches!(
            segment.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "operation segment branch is not a repository key segment"
        ));

        let mut manifest = ManifestRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: hash.clone(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        manifest.validate().unwrap();
        manifest.document_uuid = " ".to_string();
        assert!(matches!(
            manifest.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "manifest document_uuid is empty"
        ));
        manifest.document_uuid = " doc ".to_string();
        assert!(matches!(
            manifest.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "manifest document_uuid has surrounding whitespace"
        ));
        manifest.document_uuid = "doc".to_string();
        manifest.branch = "main branch".to_string();
        assert!(matches!(
            manifest.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "manifest branch is not a repository key segment"
        ));
        manifest.branch = "main".to_string();
        manifest.operation_segments = vec![hash.clone(), hash.clone()];
        assert!(matches!(
            manifest.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "duplicate manifest operation segment reference sha256:aaa"
        ));
        manifest.operation_segments.clear();
        manifest.signatures = vec![hash.clone(), hash.clone()];
        assert!(matches!(
            manifest.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "duplicate manifest signature reference sha256:aaa"
        ));
        manifest.signatures.clear();
        manifest.blobs = vec![hash.clone(), hash.clone()];
        assert!(matches!(
            manifest.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "duplicate manifest blob reference sha256:aaa"
        ));

        let mut head = BranchHeadRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            manifest: hash.clone(),
        };
        head.validate().unwrap();
        head.document_uuid = "\tdoc".to_string();
        assert!(matches!(
            head.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "branch head document_uuid has surrounding whitespace"
        ));
        head.document_uuid = "doc".to_string();
        head.branch.clear();
        assert!(matches!(
            head.validate(),
            Err(FormatError::InvalidRecord(message)) if message == "branch head branch is empty"
        ));
        head.branch = "..".to_string();
        assert!(matches!(
            head.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "branch head branch is not a repository key segment"
        ));

        let mut lookup = LookupRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            manifest: hash.clone(),
            aliases: vec![LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/example".to_string(),
            }],
            created_at_ms: 2,
        };
        lookup.validate().unwrap();
        lookup.document_uuid = "doc\n".to_string();
        assert!(matches!(
            lookup.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "lookup document_uuid has surrounding whitespace"
        ));
        lookup.document_uuid = "doc".to_string();
        lookup.branch = " main".to_string();
        assert!(matches!(
            lookup.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "lookup branch has surrounding whitespace"
        ));
        lookup.branch = "main".to_string();
        lookup.aliases[0].value.clear();
        assert!(matches!(
            lookup.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "lookup alias value is empty"
        ));
        lookup.aliases[0].value = " 10.1234/example ".to_string();
        assert!(matches!(
            lookup.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "lookup alias value has surrounding whitespace"
        ));
        lookup.aliases[0].value = "10.1234/example".to_string();
        lookup.aliases[0].scheme = " doi".to_string();
        assert!(matches!(
            lookup.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "lookup alias scheme has surrounding whitespace"
        ));
        lookup.aliases[0].scheme = "do/i".to_string();
        assert!(matches!(
            lookup.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "lookup alias scheme is not a repository key segment"
        ));

        let mut signature = SignatureRecord {
            target: hash.clone(),
            signer: "ssh-ed25519 AAAA".to_string(),
            signer_display: "Alice".to_string(),
            title: "Signed manifest".to_string(),
            signed_at_ms: 3,
            signature: vec![1],
        };
        signature.validate().unwrap();
        signature.signature.clear();
        assert!(matches!(
            signature.validate(),
            Err(FormatError::InvalidRecord(message)) if message == "signature bytes is empty"
        ));
        signature.signature = vec![1];
        signature.signer = " ssh-ed25519 AAAA".to_string();
        assert!(matches!(
            signature.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "signature signer has surrounding whitespace"
        ));
        signature.signer = "ssh-ed25519 AAAA".to_string();
        signature.signer_display = "Alice ".to_string();
        assert!(matches!(
            signature.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "signature signer_display has surrounding whitespace"
        ));
        signature.signer_display = "Alice".to_string();
        signature.title = "\tSigned manifest".to_string();
        assert!(matches!(
            signature.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "signature title has surrounding whitespace"
        ));

        let duplicate_lookup = LookupRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            manifest: hash.clone(),
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
            duplicate_lookup.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "duplicate lookup alias doi:10.1234/example"
        ));

        let duplicate_doi_lookup = LookupRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            manifest: hash.clone(),
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
            duplicate_doi_lookup.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "duplicate lookup alias doi:10.1234/example"
        ));

        let duplicate_doi_scheme_lookup = LookupRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            manifest: hash.clone(),
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
            duplicate_doi_scheme_lookup.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "duplicate lookup alias doi:10.1234/example"
        ));

        let mut tombstone = TombstoneRecord {
            object: hash.clone(),
            archive_locator: "tape://pool/object".to_string(),
            restore_hint: "request recall".to_string(),
            created_at_ms: 3,
            signer: "ssh-ed25519 AAAA".to_string(),
            signature: vec![1],
        };
        tombstone.validate().unwrap();
        tombstone.signature.clear();
        assert!(matches!(
            tombstone.validate(),
            Err(FormatError::InvalidRecord(message)) if message == "tombstone signature is empty"
        ));
        tombstone.signature = vec![1];
        tombstone.archive_locator = " tape://pool/object".to_string();
        assert!(matches!(
            tombstone.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "tombstone archive_locator has surrounding whitespace"
        ));
        tombstone.archive_locator = "tape://pool/object".to_string();
        tombstone.restore_hint = "request recall ".to_string();
        assert!(matches!(
            tombstone.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "tombstone restore_hint has surrounding whitespace"
        ));
        tombstone.restore_hint = "request recall".to_string();
        tombstone.signer = "\tarchive-indexer".to_string();
        assert!(matches!(
            tombstone.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "tombstone signer has surrounding whitespace"
        ));

        let mut pack_index = PackIndexRecord {
            pack: "main_pack".to_string(),
            entries: vec![PackIndexEntryRecord {
                hash: hash.clone(),
                offset: 4,
                length: 1,
            }],
        };
        pack_index.validate().unwrap();
        pack_index.pack = "main.pack".to_string();
        assert!(matches!(
            pack_index.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "pack index pack is not a pack name"
        ));
        pack_index.pack = " main-pack".to_string();
        assert!(matches!(
            pack_index.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "pack index pack has surrounding whitespace"
        ));
        pack_index.pack = String::new();
        assert!(matches!(
            pack_index.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "pack index pack is empty"
        ));
        pack_index.pack = "main-pack".to_string();
        pack_index.entries[0].offset = 0;
        assert!(matches!(
            pack_index.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "pack index entry offset is before pack payload"
        ));
        pack_index.entries[0].offset = 4;
        pack_index.entries[0].length = 0;
        assert!(matches!(
            pack_index.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "pack index entry length is zero"
        ));
        pack_index.entries[0].length = 1;
        pack_index.entries.push(PackIndexEntryRecord {
            hash: hash.clone(),
            offset: 5,
            length: 1,
        });
        assert!(matches!(
            pack_index.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "duplicate pack index entry hash sha256:aaa"
        ));
        let other_hash = HashRef::parse("sha256:bbb").unwrap();
        pack_index.entries[1].hash = other_hash.clone();
        pack_index.validate().unwrap();
        pack_index.entries[1].offset = 4;
        assert!(matches!(
            pack_index.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "pack index entry sha256:bbb overlaps previous pack entry"
        ));
        pack_index.entries[1].offset = u64::MAX;
        pack_index.entries[1].length = 2;
        assert!(matches!(
            pack_index.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "pack index entry sha256:bbb byte range overflows"
        ));

        let mut signature = SignatureRecord {
            target: hash,
            signer: "ssh-ed25519 AAAA".to_string(),
            signer_display: "Alice".to_string(),
            title: "blob".to_string(),
            signed_at_ms: 4,
            signature: vec![1],
        };
        signature.validate().unwrap();
        signature.signer = " ".to_string();
        assert!(matches!(
            signature.validate(),
            Err(FormatError::InvalidRecord(message)) if message == "signature signer is empty"
        ));
        signature.signer = " ssh-ed25519 AAAA".to_string();
        assert!(matches!(
            signature.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "signature signer has surrounding whitespace"
        ));
        signature.signer = "ssh-ed25519 AAAA".to_string();
        signature.signer_display = "Alice ".to_string();
        assert!(matches!(
            signature.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "signature signer_display has surrounding whitespace"
        ));
        signature.signer_display = "Alice".to_string();
        signature.title = "\tblob".to_string();
        assert!(matches!(
            signature.validate(),
            Err(FormatError::InvalidRecord(message))
                if message == "signature title has surrounding whitespace"
        ));
    }
}
