use opendoc_core::HashRef;
use serde::{Deserialize, Serialize};
use std::fmt;

const MAGIC: &[u8; 4] = b"ODF0";

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BranchHeadRecord {
    pub document_uuid: String,
    pub branch: String,
    #[serde(with = "hash_ref_serde")]
    pub manifest: HashRef,
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

    fn hash_vec(&mut self) -> Result<Vec<HashRef>, FormatError> {
        let len = self.u32()? as usize;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(self.hash()?);
        }
        Ok(out)
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

fn put_hash_vec(out: &mut Vec<u8>, values: &[HashRef]) {
    put_u32(out, values.len() as u32);
    for value in values {
        put_hash(out, value);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormatError {
    InvalidMagic,
    UnexpectedTag { expected: u8, actual: u8 },
    UnexpectedEof,
    TrailingBytes,
    InvalidUtf8,
    InvalidHash,
    InvalidOption,
    Overflow,
    Cbor(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FormatError {}

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
}
