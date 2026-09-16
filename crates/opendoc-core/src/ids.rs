//! Stable identity: document uuids, block/inline ids, and content hashes.

use crate::warning::ModelError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) static ID_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) static PROCESS_NONCE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

/// A 64-bit value that is unique per process (and, with overwhelming
/// probability, across machines). Stable ids combine it with a per-process
/// counter so that two replicas never mint the same block, inline, comment,
/// or actor id.
pub(crate) fn process_nonce() -> u64 {
    *PROCESS_NONCE.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(now_nanos());
        #[cfg(not(target_arch = "wasm32"))]
        hasher.write_u32(std::process::id());
        #[cfg(target_arch = "wasm32")]
        {
            // No process ids (and no OS entropy for RandomState) in browsers.
            hasher.write_u64((js_sys::Math::random() * u64::MAX as f64) as u64);
            hasher.write_u64((js_sys::Math::random() * u64::MAX as f64) as u64);
        }
        let value = hasher.finish();
        if value == 0 {
            1
        } else {
            value
        }
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct DocumentUuid(pub(crate) String);

impl DocumentUuid {
    pub fn new() -> Self {
        Self(format!("doc-{:016x}-{:016x}", now_nanos(), next_counter()))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::InvalidId("document uuid is empty"));
        }
        if value.trim() != value {
            return Err(ModelError::InvalidId(
                "document uuid has surrounding whitespace",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DocumentUuid {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DocumentUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct StableId(pub(crate) String);

impl StableId {
    pub fn new(prefix: &str) -> Self {
        Self(format!(
            "{prefix}-{:016x}{:08x}",
            process_nonce(),
            next_counter()
        ))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::InvalidId("stable id is empty"));
        }
        if value.trim() != value {
            return Err(ModelError::InvalidId(
                "stable id has surrounding whitespace",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct HashRef {
    algorithm: String,
    digest: String,
}

impl HashRef {
    pub fn new(
        algorithm: impl Into<String>,
        digest: impl Into<String>,
    ) -> Result<Self, ModelError> {
        let algorithm = algorithm.into();
        let digest = digest.into();
        if algorithm.trim().is_empty()
            || digest.trim().is_empty()
            || algorithm.trim() != algorithm
            || digest.trim() != digest
            || !is_hash_ref_component(&algorithm)
            || !is_hash_ref_component(&digest)
        {
            return Err(ModelError::InvalidHash);
        }
        Ok(Self { algorithm, digest })
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        let (algorithm, digest) = value.split_once(':').ok_or(ModelError::InvalidHash)?;
        Self::new(algorithm, digest)
    }

    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

pub fn digest_bytes(algorithm: &str, bytes: &[u8]) -> Result<HashRef, ModelError> {
    match algorithm {
        "sha256" => HashRef::new("sha256", lowercase_hex(&Sha256::digest(bytes))),
        _ => Err(ModelError::UnsupportedHashAlgorithm),
    }
}

pub(crate) fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn is_hash_ref_component(value: &str) -> bool {
    value != "."
        && value != ".."
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
}

impl fmt::Display for HashRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.digest)
    }
}

/// A [`StableId`] that is a pure function of the identities it is derived
/// from, rather than of a counter and a process nonce.
///
/// Merge sometimes has to invent a node that no operation created — the cell
/// where a concurrently inserted row crosses a concurrently inserted column,
/// or the placeholder row left when the last row of a table is deleted. Every
/// replica must invent the *same* node, or replicas that agree on every
/// character disagree on the bytes and so on the document hash. A derived id
/// is how that is done without a coordinator.
pub fn derived_stable_id(prefix: &str, parts: &[&str]) -> StableId {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    for part in parts {
        hasher.update([0u8]);
        hasher.update(part.as_bytes());
    }
    let digest = lowercase_hex(&hasher.finalize());
    StableId(format!("{prefix}-{}", &digest[..32]))
}

/// Mints a fresh list identity. One id per *list run* — a maximal sequence of
/// adjacent sibling list items — so two lists separated by a paragraph are
/// two lists and numbering restarts.
pub fn new_list_id() -> StableId {
    StableId::new("list")
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn now_nanos() -> u128 {
    (js_sys::Date::now() * 1_000_000.0) as u128
}

pub(crate) fn next_counter() -> u64 {
    ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn validate_stable_id(label: &'static str, id: &StableId) -> Result<(), ModelError> {
    if id.0.trim().is_empty() {
        Err(ModelError::InvalidDocument(match label {
            "block id" => "block id is empty",
            "inline id" => "inline id is empty",
            "table row id" => "table row id is empty",
            "table cell id" => "table cell id is empty",
            "equation id" => "equation id is empty",
            "footnote id" => "footnote id is empty",
            "footnote reference id" => "footnote reference id is empty",
            "comment thread id" => "comment thread id is empty",
            "comment id" => "comment id is empty",
            "suggestion id" => "suggestion id is empty",
            "citation id" => "citation id is empty",
            "bibliography reference id" => "bibliography reference id is empty",
            "citation group id" => "citation group id is empty",
            "citation item reference id" => "citation item reference id is empty",
            "footnote citation id" => "footnote citation id is empty",
            "nearest block anchor block id" => "nearest block anchor block id is empty",
            "text range start" => "text range start is empty",
            "text range end" => "text range end is empty",
            "list id" => "list id is empty",
            _ => "stable id is empty",
        }))
    } else if id.0.trim() != id.0 {
        Err(ModelError::InvalidDocument(
            "stable id has surrounding whitespace",
        ))
    } else {
        Ok(())
    }
}

impl DocumentUuid {
    pub(crate) fn validate(&self) -> Result<(), ModelError> {
        if self.0.trim().is_empty() {
            Err(ModelError::InvalidDocument("document uuid is empty"))
        } else if self.0.trim() != self.0 {
            Err(ModelError::InvalidDocument(
                "document uuid has surrounding whitespace",
            ))
        } else {
            Ok(())
        }
    }
}

/// Where a new sibling goes in an ordered list of siblings addressed by
/// [`StableId`].
///
/// The older spelling for this is `after: Option<StableId>`, where `None`
/// means *append*. That convention is load-bearing wherever it is still used
/// — it is what makes a replayed insert whose anchor has since been deleted
/// degrade to an append instead of vanishing — but it has one position it
/// cannot name: *before the first sibling*, because `None` is already spoken
/// for. Adding a second optional beside it would let a caller say two
/// contradictory things at once, so the position is one value instead.
///
/// [`InsertPosition::First`] is the position `After` cannot express; it never
/// degrades, because it names no anchor that could go missing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum InsertPosition {
    /// Before every existing sibling.
    First,
    /// Directly before the sibling with this id. If that sibling is gone by
    /// the time the operation is applied, the insert degrades to
    /// [`InsertPosition::Last`] and the applier says so.
    ///
    /// This is distinct from [`InsertPosition::First`]: `First` identifies a
    /// top-level of an otherwise unnamed sibling container, while `Before`
    /// retains the identity needed to restore the first block of a table cell.
    Before(StableId),
    /// Directly after the sibling with this id. If that sibling is gone by
    /// the time the operation is applied, the insert degrades to
    /// [`InsertPosition::Last`] and the applier says so.
    After(StableId),
    /// After every existing sibling.
    Last,
}

impl InsertPosition {
    /// The index the new sibling takes in a list of `len` siblings, given
    /// where the anchor currently sits. `None` for the anchor means it was not
    /// found, which is what the caller reports as a degraded anchor.
    pub fn index(&self, len: usize, anchor_index: Option<usize>) -> usize {
        match self {
            InsertPosition::First => 0,
            InsertPosition::Before(_) => anchor_index.unwrap_or(len),
            InsertPosition::After(_) => match anchor_index {
                Some(index) => index + 1,
                None => len,
            },
            InsertPosition::Last => len,
        }
    }

    /// The sibling this position is anchored to, when it is anchored at all.
    pub fn anchor(&self) -> Option<&StableId> {
        match self {
            InsertPosition::Before(id) | InsertPosition::After(id) => Some(id),
            InsertPosition::First | InsertPosition::Last => None,
        }
    }

    /// The position the older `after: Option<StableId>` spelling means: an
    /// anchored insert when there is an anchor, an append when there is not.
    ///
    /// This is the bridge from that spelling, and the only place the
    /// convention is written down. It is deliberately not a way to say
    /// [`InsertPosition::First`]: `None` keeps meaning *append*, exactly as it
    /// always did, so a caller that had no anchor cannot silently acquire a
    /// different meaning by being ported.
    pub fn after_or_last(after: Option<StableId>) -> Self {
        match after {
            Some(id) => InsertPosition::After(id),
            None => InsertPosition::Last,
        }
    }

    /// The spelling the command surface uses for [`InsertPosition::First`].
    ///
    /// The surface passes sibling ids verbatim, so the keyword has to be a
    /// string no sibling can be called. Ids are minted as `kind-nonce-counter`
    /// by [`StableId::new`], so a bare `"first"` never collides with one.
    pub const FIRST_KEYWORD: &'static str = "first";

    /// Reads the position off the command surface: the [`FIRST_KEYWORD`], the
    /// id of the sibling to follow, or nothing at all for an append.
    ///
    /// [`FIRST_KEYWORD`]: InsertPosition::FIRST_KEYWORD
    pub fn parse(anchor: Option<&str>) -> Result<Self, ModelError> {
        match anchor.map(str::trim) {
            None | Some("") => Ok(InsertPosition::Last),
            Some(value) if value == Self::FIRST_KEYWORD => Ok(InsertPosition::First),
            Some(value) => StableId::parse(value).map(InsertPosition::After),
        }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        match self {
            InsertPosition::Before(id) | InsertPosition::After(id) => {
                validate_stable_id("insert anchor", id)
            }
            InsertPosition::First | InsertPosition::Last => Ok(()),
        }
    }
}
