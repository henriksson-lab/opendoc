//! List commands: list items, indentation and the list marker vocabulary.

use super::*;

impl OpenDocApp {
    pub fn add_list_item(
        &mut self,
        text: impl Into<String>,
        level: u8,
        list_kind_name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let kind = list_kind(list_kind_name.as_ref())?;
        if level > 8 {
            return Err(AppApiError::Format(format!(
                "list item level {level} is outside 0..=8"
            )));
        }
        let after = self.document.blocks.last().map(|block| block.id.clone());
        let list_id = list_id_for_new_item(&self.document.blocks, after.as_ref());
        self.apply(
            "insert-block",
            "list item",
            OperationKind::InsertBlock {
                position: InsertPosition::after_or_last(after),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::ListItem {
                        list_id,
                        level,
                        kind,
                    },
                    content: vec![Inline::text(text)],
                    properties: BlockProperties::default(),
                },
            },
        )
    }

    pub fn insert_list_item_after(
        &mut self,
        after_block_id: impl AsRef<str>,
        text: impl Into<String>,
        level: u8,
        list_kind_name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let kind = list_kind(list_kind_name.as_ref())?;
        if level > 8 {
            return Err(AppApiError::Format(format!(
                "list item level {level} is outside 0..=8"
            )));
        }
        let after = parse_id(after_block_id.as_ref())?;
        if find_block_in_blocks(&self.document.blocks, &after).is_none() {
            return Err(AppApiError::NotFound(format!(
                "top-level block {after} was not found"
            )));
        }
        let list_id = list_id_for_new_item(&self.document.blocks, Some(&after));
        self.apply(
            "insert-block",
            "list item after block",
            OperationKind::InsertBlock {
                position: InsertPosition::After(after),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::ListItem {
                        list_id,
                        level,
                        kind,
                    },
                    content: vec![Inline::text(text)],
                    properties: BlockProperties::default(),
                },
            },
        )
    }

    pub fn update_list_item(
        &mut self,
        block_id: impl AsRef<str>,
        level: u8,
        list_kind_name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let kind = list_kind(list_kind_name.as_ref())?;
        if level > 8 {
            return Err(AppApiError::Format(format!(
                "list item level {level} is outside 0..=8"
            )));
        }
        let block_id = parse_id(block_id.as_ref())?;
        let Some(block) = find_block_in_blocks(&self.document.blocks, &block_id) else {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        };
        if !matches!(block.kind, BlockKind::ListItem { .. }) {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not a list item"
            )));
        }
        self.apply(
            "update-list-item",
            "update list item",
            OperationKind::UpdateListItem {
                block_id,
                level,
                kind,
            },
        )
    }

    /// Sets the number at which this item's ordered list wrapper begins.
    ///
    /// The public command takes an item id rather than a raw `list_id`: that
    /// makes the target visible to an editor and prevents a UI from creating a
    /// dormant property for a run it cannot see. The journalled operation is
    /// nevertheless keyed by `(list_id, level)`, per ADR 0021.
    pub fn set_ordered_list_start(
        &mut self,
        block_id: impl AsRef<str>,
        start: u32,
    ) -> Result<AppDocument, AppApiError> {
        if start == 0 {
            return Err(AppApiError::Format(
                "list numbering start must be positive".to_string(),
            ));
        }
        let block_id = parse_id(block_id.as_ref())?;
        let Some(block) = find_block_in_blocks(&self.document.blocks, &block_id) else {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        };
        let BlockKind::ListItem {
            list_id,
            level,
            kind,
        } = &block.kind
        else {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not a list item"
            )));
        };
        if !kind.is_ordered() {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not an ordered list item"
            )));
        }
        self.apply(
            "set-list-start",
            "set ordered list start",
            OperationKind::SetListStart {
                list_id: list_id.clone(),
                level: *level,
                start,
            },
        )
    }

    /// Sets the durable counter style of this item's ordered-list wrapper.
    /// Like the start command, this takes a visible item id rather than
    /// allowing clients to create properties for a nonexistent run.
    pub fn set_ordered_list_format(
        &mut self,
        block_id: impl AsRef<str>,
        format: opendoc_core::OrderedListFormat,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        let Some(block) = find_block_in_blocks(&self.document.blocks, &block_id) else {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        };
        let BlockKind::ListItem {
            list_id,
            level,
            kind,
        } = &block.kind
        else {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not a list item"
            )));
        };
        if !kind.is_ordered() {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not an ordered list item"
            )));
        }
        self.apply(
            "set-list-format",
            "set ordered list format",
            OperationKind::SetListFormat {
                list_id: list_id.clone(),
                level: *level,
                format,
            },
        )
    }

    pub fn set_ordered_list_format_name(
        &mut self,
        block_id: impl AsRef<str>,
        format: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let format = opendoc_core::OrderedListFormat::parse(format.as_ref()).ok_or_else(|| {
            AppApiError::Format(format!(
                "unsupported ordered list format {:?}",
                format.as_ref()
            ))
        })?;
        self.set_ordered_list_format(block_id, format)
    }

    /// Sets the durable marker glyph of this item's unordered-list wrapper.
    /// A visible item id keeps callers from creating hidden run properties.
    pub fn set_bullet_list_marker_name(
        &mut self,
        block_id: impl AsRef<str>,
        marker: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let marker = opendoc_core::BulletListMarker::parse(marker.as_ref()).ok_or_else(|| {
            AppApiError::Format(format!(
                "unsupported bullet list marker {:?}",
                marker.as_ref()
            ))
        })?;
        let block_id = parse_id(block_id.as_ref())?;
        let Some(block) = find_block_in_blocks(&self.document.blocks, &block_id) else {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        };
        let BlockKind::ListItem {
            list_id,
            level,
            kind,
        } = &block.kind
        else {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not a list item"
            )));
        };
        if !matches!(kind, ListKind::Bullet) {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not a bulleted list item"
            )));
        }
        self.apply(
            "set-list-bullet-marker",
            "set bullet list marker",
            OperationKind::SetListBulletMarker {
                list_id: list_id.clone(),
                level: *level,
                marker,
            },
        )
    }

    pub fn adjust_editor_selection_list_indent(
        &mut self,
        selection: EditorSelection,
        delta: i8,
    ) -> Result<AppDocument, AppApiError> {
        let block_ids = self
            .describe_editor_selection(selection)?
            .selected_block_ids
            .into_iter()
            .map(|id| parse_id(&id))
            .collect::<Result<Vec<_>, _>>()?;
        let operations = block_ids
            .into_iter()
            .filter_map(|block_id| {
                let block = find_block_in_blocks(&self.document.blocks, &block_id)?;
                let BlockKind::ListItem { level, kind, .. } = block.kind else {
                    return None;
                };
                let level = (i16::from(level) + i16::from(delta)).clamp(0, 8) as u8;
                // Indenting must not silently retype the marker, so the kind
                // (checkbox state included) travels through unchanged.
                Some((
                    "update-list-item",
                    "update list item",
                    OperationKind::UpdateListItem {
                        block_id,
                        level,
                        kind,
                    },
                ))
            })
            .collect();
        self.apply_batch(operations)
    }
}

/// The list marker a command asked for, by name: `"bullet"`, `"ordered"` or
/// `"checklist"`.
///
/// A checklist created or converted through a style command starts unchecked.
/// The checkbox moves only through [`OpenDocApp::set_list_item_checked`], so
/// converting a list to bullets and back cannot resurrect a stale checked
/// state — there is deliberately no "remember the checkbox" heuristic, which
/// would make a checklist impossible to convert away from cleanly.
pub(crate) fn list_kind(name: &str) -> Result<ListKind, AppApiError> {
    ListKind::parse(name.trim(), false).map_err(|err| AppApiError::Format(err.to_string()))
}

/// `SetBlockTextStyle` operations that move the tail of each cut list run onto
/// a fresh run id. See [`list_run_split_reassignments`].
pub(crate) fn list_run_split_operations(
    blocks: &[Block],
    leaving: &BTreeSet<StableId>,
) -> Vec<(&'static str, &'static str, OperationKind)> {
    list_run_split_reassignments(blocks, leaving)
        .into_iter()
        .map(|(block_id, list_id, level, kind)| {
            (
                "set-block-text-style",
                "split list run",
                OperationKind::SetBlockTextStyle {
                    block_id,
                    style: BlockTextStyle::ListItem {
                        list_id,
                        level,
                        kind,
                    },
                },
            )
        })
        .collect()
}
