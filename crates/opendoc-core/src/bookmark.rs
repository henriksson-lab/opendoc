//! Durable named navigation targets.
//!
//! A bookmark names a stable block identity, rather than a character offset.
//! Character offsets are projections of a concurrently edited text sequence;
//! a block id is source state and remains meaningful when text is rewritten.

use crate::ids::{validate_stable_id, StableId};
use crate::warning::ModelError;
use serde::{Deserialize, Serialize};

/// A named document-navigation target.
///
/// `revision`/`deleted` make this a small LWW register with a tombstone.  A
/// deletion is therefore durable and cannot be accidentally resurrected by an
/// older replica's save.  The target is intentionally allowed to be absent:
/// deleting a block must not make a valid document unsaveable, and an undo or
/// concurrent reinsert may restore it later.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: StableId,
    pub name: String,
    pub block_id: StableId,
    pub revision: u64,
    #[serde(default)]
    pub deleted: bool,
}

impl Bookmark {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("bookmark id", &self.id)?;
        validate_stable_id("bookmark target block id", &self.block_id)?;
        if self.name.is_empty() || self.name.trim() != self.name {
            return Err(ModelError::InvalidDocument(
                "bookmark name is empty or has surrounding whitespace",
            ));
        }
        if self.name.len() > 40
            || !self
                .name
                .bytes()
                .enumerate()
                .all(|(index, byte)| match byte {
                    b'A'..=b'Z' | b'a'..=b'z' | b'_' => true,
                    b'0'..=b'9' | b'-' => index != 0,
                    _ => false,
                })
        {
            return Err(ModelError::InvalidDocument(
                "bookmark name must be at most 40 ASCII letters, digits, hyphens, or underscores and cannot start with a digit or hyphen",
            ));
        }
        if self.revision == 0 {
            return Err(ModelError::InvalidDocument("bookmark revision is zero"));
        }
        Ok(())
    }
}
