//! Durable identities for the characters inside an editable inline run.
//!
//! The existing merge CRDT derives equivalent atoms temporarily while it
//! resolves offset-addressed legacy operations.  These types are deliberately
//! model-owned: a saved annotation must name a token or a gap, never a screen
//! offset.  Wiring them into `Document` and token-addressed operations is the
//! next migration; keeping the vocabulary here first prevents that migration
//! from depending on `opendoc-merge` (which already depends on this crate).

use crate::ids::validate_stable_id;
use crate::{DocumentUuid, ModelError, StableId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Immutable provenance of one Unicode scalar in an editable run.
///
/// `Baseline` ids are deterministic for imported/legacy source. `Operation`
/// ids reserve the durable spelling used once text edits mint their own
/// tokens; the core cannot use merge's `OperationId` without a dependency
/// cycle, so its two stable components live directly in source state.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum TextTokenId {
    Baseline {
        document_uuid: DocumentUuid,
        inline_id: StableId,
        ordinal: u32,
    },
    Operation {
        actor: String,
        sequence: u64,
        ordinal: u32,
    },
}

impl TextTokenId {
    pub fn validate(&self) -> Result<(), ModelError> {
        match self {
            Self::Baseline {
                document_uuid,
                inline_id,
                ..
            } => {
                document_uuid.validate()?;
                validate_stable_id("text token inline id", inline_id)?;
            }
            Self::Operation { actor, .. } => {
                if actor.trim().is_empty() {
                    return Err(ModelError::InvalidDocument("text token actor is empty"));
                }
                if actor.trim() != actor {
                    return Err(ModelError::InvalidDocument(
                        "text token actor has surrounding whitespace",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The affinity of a zero-width token gap under concurrent insertion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TextGapBias {
    /// Concurrent content in the original gap is projected after this gap.
    Before,
    /// Concurrent content in the original gap is projected before this gap.
    After,
}

/// An endpoint between two token identities. Either side may be the run
/// boundary. Endpoints are retained even if their neighbouring token is
/// tombstoned.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextGap {
    pub left: Option<TextTokenId>,
    pub right: Option<TextTokenId>,
    pub bias: TextGapBias,
}

/// A character-granular interval inside one durable editable run.
///
/// The endpoints are gaps rather than scalar offsets.  They deliberately keep
/// naming tombstoned neighbouring tokens: deleting selected text must not make
/// a comment, suggestion, bookmark, or internal target silently drift to a
/// surviving character.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextTokenRange {
    pub inline_id: StableId,
    pub start: TextGap,
    pub end: TextGap,
}

impl TextTokenRange {
    /// Validate fields which do not need the owning document's token map.
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("token range inline id", &self.inline_id)?;
        self.start.validate()?;
        self.end.validate()
    }

    /// Confirm that both gaps name retained tokens in this exact run and that
    /// the interval is ordered in its current deterministic linearization.
    pub fn validate_against(&self, sequence: &TextSequence) -> Result<(), ModelError> {
        self.validate()?;
        if !sequence.tokens.is_empty()
            && ([&self.start, &self.end]
                .into_iter()
                .any(|gap| gap.left.is_none() && gap.right.is_none()))
        {
            return Err(ModelError::InvalidDocument(
                "empty token gap belongs only to an empty text sequence",
            ));
        }
        let Some(start) = sequence.visible_offset_of_gap(&self.start) else {
            return Err(ModelError::InvalidDocument(
                "token range start is not in its text sequence",
            ));
        };
        let Some(end) = sequence.visible_offset_of_gap(&self.end) else {
            return Err(ModelError::InvalidDocument(
                "token range end is not in its text sequence",
            ));
        };
        if start > end {
            return Err(ModelError::InvalidDocument(
                "token range endpoints are reversed",
            ));
        }
        Ok(())
    }
}

impl TextGap {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.left.is_none() && self.right.is_none() {
            // This is the empty run's one valid gap. Its bias remains
            // meaningful once an operation inserts at the boundary.
            return Ok(());
        }
        if self.left == self.right {
            return Err(ModelError::InvalidDocument(
                "text gap has identical endpoints",
            ));
        }
        if let Some(left) = &self.left {
            left.validate()?;
        }
        if let Some(right) = &self.right {
            right.validate()?;
        }
        Ok(())
    }
}

/// A scalar atom retained in saved sequence source. Tombstoned atoms remain
/// addressable so annotations and later insertions cannot silently drift.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextToken {
    pub id: TextTokenId,
    pub predecessor: Option<TextTokenId>,
    pub scalar: char,
    #[serde(default)]
    pub tombstoned: bool,
}

/// The durable RGA-shaped sequence for exactly one text/link inline.
///
/// The order here is its deterministic current linearization. `predecessor`
/// retains the insertion edge needed by a later RGA-aware operation layer;
/// it is not discarded when a token is tombstoned.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct TextSequence {
    pub tokens: Vec<TextToken>,
}

impl TextSequence {
    /// Deterministically materialize tokens for a legacy string. This is the
    /// only permitted baseline spelling, so independent readers of an old
    /// document agree before a signed migration persists the sequence.
    pub fn materialize_legacy(
        document_uuid: &DocumentUuid,
        inline_id: &StableId,
        text: &str,
    ) -> Self {
        let mut predecessor = None;
        let tokens = text
            .chars()
            .enumerate()
            .map(|(ordinal, scalar)| {
                let id = TextTokenId::Baseline {
                    document_uuid: document_uuid.clone(),
                    inline_id: inline_id.clone(),
                    ordinal: ordinal as u32,
                };
                let token = TextToken {
                    id: id.clone(),
                    predecessor: predecessor.clone(),
                    scalar,
                    tombstoned: false,
                };
                predecessor = Some(id);
                token
            })
            .collect();
        Self { tokens }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        let mut ids = BTreeSet::new();
        for token in &self.tokens {
            token.id.validate()?;
            if !ids.insert(token.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate text token id"));
            }
            if let Some(predecessor) = &token.predecessor {
                predecessor.validate()?;
                if predecessor == &token.id {
                    return Err(ModelError::InvalidDocument("text token precedes itself"));
                }
                if !ids.contains(predecessor) {
                    return Err(ModelError::InvalidDocument(
                        "text token predecessor is not earlier in sequence",
                    ));
                }
            }
        }
        Ok(())
    }

    /// The reader-visible text; tombstones deliberately do not alter token
    /// identity or its position in this source sequence.
    pub fn visible_text(&self) -> String {
        self.tokens
            .iter()
            .filter(|token| !token.tombstoned)
            .map(|token| token.scalar)
            .collect()
    }

    /// Convert a current Unicode-scalar selection boundary to a durable gap.
    /// The offset is clamped exactly as legacy text edits are. This helper is
    /// for a future operation authoring boundary; it intentionally does not
    /// write an offset into an annotation.
    pub fn gap_at_visible_offset(&self, offset: usize, bias: TextGapBias) -> TextGap {
        let visible = self
            .tokens
            .iter()
            .filter(|token| !token.tombstoned)
            .collect::<Vec<_>>();
        let offset = offset.min(visible.len());
        TextGap {
            left: offset.checked_sub(1).map(|index| visible[index].id.clone()),
            right: visible.get(offset).map(|token| token.id.clone()),
            bias,
        }
    }

    /// Resolve an existing durable gap to a visible scalar boundary without
    /// retargeting through a tombstone. A missing endpoint denotes a stale
    /// generation and returns `None`; a tombstoned endpoint still resolves to
    /// its retained sequence position.
    pub fn visible_offset_of_gap(&self, gap: &TextGap) -> Option<usize> {
        if gap.validate().is_err() {
            return None;
        }
        let left = match &gap.left {
            Some(id) => Some(self.tokens.iter().position(|token| &token.id == id)?),
            None => None,
        };
        let right = match &gap.right {
            Some(id) => Some(self.tokens.iter().position(|token| &token.id == id)?),
            None => None,
        };
        if let (Some(left), Some(right)) = (left, right) {
            if left >= right {
                return None;
            }
        }
        let boundary = match (left, right) {
            (_, Some(right)) => right,
            (Some(left), None) => left + 1,
            (None, None) => 0,
        };
        Some(
            self.tokens[..boundary]
                .iter()
                .filter(|token| !token.tombstoned)
                .count(),
        )
    }
}
