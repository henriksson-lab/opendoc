//! Block commands: paragraphs, block properties, styles and page breaks.

use super::*;

impl OpenDocApp {
    pub fn add_paragraph(&mut self, text: impl Into<String>) -> AppDocument {
        self.apply(
            "insert-block",
            "paragraph",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block::paragraph(text),
            },
        )
    }

    pub fn insert_paragraph_after(
        &mut self,
        after_block_id: Option<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let after = after_block_id.map(|id| parse_id(&id)).transpose()?;
        if let Some(after) = &after {
            if !self.document.blocks.iter().any(|block| &block.id == after) {
                return Err(AppApiError::NotFound(format!(
                    "top-level block {after} was not found"
                )));
            }
        }
        Ok(self.apply(
            "insert-block",
            "paragraph after block",
            OperationKind::InsertBlock {
                after,
                block: Block::paragraph(text),
            },
        ))
    }

    pub fn split_paragraph_at_inline(
        &mut self,
        inline_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let inline_id = parse_id(inline_id.as_ref())?;
        let source_block_id = find_block_id_containing_inline(&self.document.blocks, &inline_id)
            .ok_or_else(|| AppApiError::NotFound(format!("inline {inline_id} was not found")))?;
        let split_block_id = StableId::new("block");
        Ok(self.apply_batch(vec![
            (
                "insert-block",
                "split paragraph target",
                OperationKind::InsertBlock {
                    after: Some(source_block_id),
                    block: Block {
                        id: split_block_id.clone(),
                        kind: BlockKind::Paragraph,
                        content: Vec::new(),
                        properties: BlockProperties::default(),
                    },
                },
            ),
            (
                "move-inline-to-block",
                "split paragraph inline move",
                OperationKind::MoveInlineToBlock {
                    inline_id,
                    target_block_id: split_block_id,
                    after: None,
                },
            ),
        ]))
    }

    pub fn split_paragraph_at_text_offset(
        &mut self,
        block_id: impl AsRef<str>,
        inline_id_input: impl AsRef<str>,
        offset: usize,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        let split_inline_id = parse_id(inline_id_input.as_ref())?;
        let block = self
            .document
            .blocks
            .iter()
            .find(|block| block.id == block_id)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("top-level block {block_id} was not found"))
            })?;
        if !matches!(
            block.kind,
            BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::ListItem { .. }
        ) {
            return Err(AppApiError::Format(format!(
                "block {block_id} cannot be split at a text offset"
            )));
        }
        let inline = block
            .content
            .iter()
            .find(|inline| inline_id(inline) == &split_inline_id)
            .ok_or_else(|| {
                AppApiError::NotFound(format!(
                    "inline {split_inline_id} was not found in block {block_id}"
                ))
            })?;
        let (text, marks) = match inline {
            Inline::Text { text, marks, .. } => (text, marks),
            _ => {
                return Err(AppApiError::Format(format!(
                    "inline {split_inline_id} cannot be split at a text offset"
                )))
            }
        };
        let split_byte = byte_offset_for_char_offset(text, offset)?;
        let before = text[..split_byte].to_string();
        let after = text[split_byte..].to_string();
        let after_inline = Inline::Text {
            id: StableId::new("text"),
            text: after,
            marks: marks.clone(),
        };
        let split_block_id = StableId::new("block");
        Ok(self.apply_batch(vec![
            (
                "update-inline-text",
                "split paragraph leading text",
                OperationKind::UpdateInlineText {
                    inline_id: split_inline_id,
                    text: before,
                },
            ),
            (
                "insert-block",
                "split paragraph trailing text",
                OperationKind::InsertBlock {
                    after: Some(block_id),
                    block: Block {
                        id: split_block_id,
                        kind: BlockKind::Paragraph,
                        content: vec![after_inline],
                        properties: BlockProperties::default(),
                    },
                },
            ),
        ]))
    }

    pub fn join_paragraph_with_previous(
        &mut self,
        block_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        let Some(index) = self
            .document
            .blocks
            .iter()
            .position(|block| block.id == block_id)
        else {
            return Err(AppApiError::NotFound(format!(
                "top-level block {block_id} was not found"
            )));
        };
        if index == 0 {
            return Err(AppApiError::Format(format!(
                "paragraph {block_id} has no previous paragraph"
            )));
        }
        if !matches!(self.document.blocks[index].kind, BlockKind::Paragraph) {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not a paragraph"
            )));
        }
        let previous = &self.document.blocks[index - 1];
        if !matches!(previous.kind, BlockKind::Paragraph) {
            return Err(AppApiError::Format(format!(
                "previous block {} is not a paragraph",
                previous.id
            )));
        }

        let target_block_id = previous.id.clone();
        let inline_ids = self.document.blocks[index]
            .content
            .iter()
            .map(|inline| inline_id(inline).clone())
            .collect::<Vec<_>>();
        let mut after = previous.content.last().map(inline_id).cloned();
        let mut operations = Vec::new();
        for inline_id_to_move in inline_ids {
            operations.push((
                "move-inline-to-block",
                "join paragraph inline move",
                OperationKind::MoveInlineToBlock {
                    inline_id: inline_id_to_move.clone(),
                    target_block_id: target_block_id.clone(),
                    after: after.clone(),
                },
            ));
            after = Some(inline_id_to_move);
        }
        operations.push((
            "delete-block",
            "join paragraph source",
            OperationKind::DeleteBlock { block_id },
        ));
        Ok(self.apply_batch(operations))
    }

    pub fn delete_block(
        &mut self,
        block_id: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id =
            StableId::parse(block_id.into()).map_err(|err| AppApiError::Model(err.to_string()))?;
        if !block_exists(&self.document.blocks, &block_id) {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        }
        Ok(self.apply(
            "delete-block",
            "delete block",
            OperationKind::DeleteBlock { block_id },
        ))
    }

    pub fn set_block_text_style(
        &mut self,
        block_id: impl AsRef<str>,
        style: impl AsRef<str>,
        level: u8,
        list_kind_name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        let operations = self.block_style_operations(
            vec![block_id],
            style.as_ref(),
            level,
            list_kind_name.as_ref(),
        )?;
        Ok(self.apply_batch(operations))
    }

    pub fn set_editor_selection_block_style(
        &mut self,
        selection: EditorSelection,
        style: impl AsRef<str>,
        level: u8,
        list_kind_name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let block_ids = self
            .describe_editor_selection(selection)?
            .selected_block_ids
            .into_iter()
            .map(|id| parse_id(&id))
            .collect::<Result<Vec<_>, _>>()?;
        let operations =
            self.block_style_operations(block_ids, style.as_ref(), level, list_kind_name.as_ref())?;
        Ok(self.apply_batch(operations))
    }

    /// Writes one typed block property. Concurrent writes to the same
    /// property of the same block converge last-writer-wins; see
    /// `docs/adr/0006-block-property-merge.md`.
    pub fn set_block_property(
        &mut self,
        block_id: impl AsRef<str>,
        property: BlockProperty,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = self.formattable_block_id(block_id.as_ref())?;
        property
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Ok(self.apply(
            "set-block-property",
            "set block property",
            OperationKind::SetBlockProperty { block_id, property },
        ))
    }

    /// Returns one block property to inheriting its default.
    pub fn clear_block_property(
        &mut self,
        block_id: impl AsRef<str>,
        key: BlockPropertyKey,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = self.formattable_block_id(block_id.as_ref())?;
        Ok(self.apply(
            "clear-block-property",
            "clear block property",
            OperationKind::ClearBlockProperty { block_id, key },
        ))
    }

    pub fn set_editor_selection_block_property(
        &mut self,
        selection: EditorSelection,
        property: BlockProperty,
    ) -> Result<AppDocument, AppApiError> {
        property
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let operations = self
            .selected_formattable_block_ids(selection)?
            .into_iter()
            .map(|block_id| {
                (
                    "set-block-property",
                    "set block property",
                    OperationKind::SetBlockProperty { block_id, property },
                )
            })
            .collect();
        Ok(self.apply_batch(operations))
    }

    pub fn clear_editor_selection_block_property(
        &mut self,
        selection: EditorSelection,
        key: BlockPropertyKey,
    ) -> Result<AppDocument, AppApiError> {
        let operations = self
            .selected_formattable_block_ids(selection)?
            .into_iter()
            .map(|block_id| {
                (
                    "clear-block-property",
                    "clear block property",
                    OperationKind::ClearBlockProperty { block_id, key },
                )
            })
            .collect();
        Ok(self.apply_batch(operations))
    }

    // ---- Named block-property commands (PLAN77 B3) ----------------------
    //
    // The command surface carries names and twips because that is all a JSON
    // payload can carry. These turn them into typed `BlockProperty` values
    // through the model's own smart constructors, so an unknown alignment or
    // an out-of-range length is refused at the boundary instead of reaching
    // the document. Which property is written is the method, never an
    // argument, so a caller cannot pair an indent key with a spacing value.

    pub fn set_block_alignment(
        &mut self,
        block_id: impl AsRef<str>,
        alignment: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::Alignment(parse_alignment(alignment.as_ref())?);
        self.set_block_property(block_id, property)
    }

    pub fn set_editor_selection_block_alignment(
        &mut self,
        selection: EditorSelection,
        alignment: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::Alignment(parse_alignment(alignment.as_ref())?);
        self.set_editor_selection_block_property(selection, property)
    }

    pub fn set_block_direction(
        &mut self,
        block_id: impl AsRef<str>,
        direction: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::Direction(parse_direction(direction.as_ref())?);
        self.set_block_property(block_id, property)
    }

    pub fn set_editor_selection_block_direction(
        &mut self,
        selection: EditorSelection,
        direction: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::Direction(parse_direction(direction.as_ref())?);
        self.set_editor_selection_block_property(selection, property)
    }

    pub fn set_block_indent_start(
        &mut self,
        block_id: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::IndentStart(parse_length(twips)?);
        self.set_block_property(block_id, property)
    }

    pub fn set_editor_selection_block_indent_start(
        &mut self,
        selection: EditorSelection,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::IndentStart(parse_length(twips)?);
        self.set_editor_selection_block_property(selection, property)
    }

    pub fn set_block_indent_end(
        &mut self,
        block_id: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::IndentEnd(parse_length(twips)?);
        self.set_block_property(block_id, property)
    }

    pub fn set_editor_selection_block_indent_end(
        &mut self,
        selection: EditorSelection,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::IndentEnd(parse_length(twips)?);
        self.set_editor_selection_block_property(selection, property)
    }

    /// A negative value here is a hanging indent; the model has no second
    /// representation for one.
    pub fn set_block_indent_first_line(
        &mut self,
        block_id: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::IndentFirstLine(parse_length(twips)?);
        self.set_block_property(block_id, property)
    }

    pub fn set_editor_selection_block_indent_first_line(
        &mut self,
        selection: EditorSelection,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::IndentFirstLine(parse_length(twips)?);
        self.set_editor_selection_block_property(selection, property)
    }

    pub fn set_block_space_before(
        &mut self,
        block_id: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::SpaceBefore(parse_length(twips)?);
        self.set_block_property(block_id, property)
    }

    pub fn set_editor_selection_block_space_before(
        &mut self,
        selection: EditorSelection,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::SpaceBefore(parse_length(twips)?);
        self.set_editor_selection_block_property(selection, property)
    }

    pub fn set_block_space_after(
        &mut self,
        block_id: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::SpaceAfter(parse_length(twips)?);
        self.set_block_property(block_id, property)
    }

    pub fn set_editor_selection_block_space_after(
        &mut self,
        selection: EditorSelection,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::SpaceAfter(parse_length(twips)?);
        self.set_editor_selection_block_property(selection, property)
    }

    /// `mode` is `"multiple"`, `"exact"` or `"at-least"`; `value` is
    /// thousandths of a line for `"multiple"` and twips for the other two.
    pub fn set_block_line_spacing(
        &mut self,
        block_id: impl AsRef<str>,
        mode: impl AsRef<str>,
        value: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::LineSpacing(parse_line_spacing(mode.as_ref(), value)?);
        self.set_block_property(block_id, property)
    }

    pub fn set_editor_selection_block_line_spacing(
        &mut self,
        selection: EditorSelection,
        mode: impl AsRef<str>,
        value: i32,
    ) -> Result<AppDocument, AppApiError> {
        let property = BlockProperty::LineSpacing(parse_line_spacing(mode.as_ref(), value)?);
        self.set_editor_selection_block_property(selection, property)
    }

    /// Clearing takes only a key, so one command covers every property: there
    /// is no value that could be paired with the wrong key.
    pub fn clear_named_block_property(
        &mut self,
        block_id: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let key = parse_block_property_key(key.as_ref())?;
        self.clear_block_property(block_id, key)
    }

    pub fn clear_editor_selection_named_block_property(
        &mut self,
        selection: EditorSelection,
        key: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let key = parse_block_property_key(key.as_ref())?;
        self.clear_editor_selection_block_property(selection, key)
    }

    /// Ticks or unticks a checklist item.
    ///
    /// Only a checklist item has a checkbox — asking a bullet or a numbered
    /// item to be checked is a mistake, not a no-op, so it is refused.
    pub fn set_list_item_checked(
        &mut self,
        block_id: impl AsRef<str>,
        checked: bool,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        let Some(block) = find_block_in_blocks(&self.document.blocks, &block_id) else {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        };
        let BlockKind::ListItem { level, kind, .. } = block.kind else {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not a list item"
            )));
        };
        if kind.checked().is_none() {
            return Err(AppApiError::Format(format!(
                "list item {block_id} is a {} item and has no checkbox",
                kind.as_str()
            )));
        }
        Ok(self.apply(
            "update-list-item",
            "set checklist item state",
            OperationKind::UpdateListItem {
                block_id,
                level,
                kind: kind.with_checked(checked),
            },
        ))
    }

    /// Toolbar indent / outdent over a selection.
    ///
    /// A list item moves a nesting level, because that is what indenting a
    /// list means; every other block shifts its start indent by one tab stop.
    /// Deciding which is a document question, so it is decided here and not
    /// by the caller. Outdenting stops at zero indent and clears the property
    /// rather than storing an explicit zero — a deliberate negative indent is
    /// still reachable through `set_block_indent_start`.
    pub fn adjust_editor_selection_indent(
        &mut self,
        selection: EditorSelection,
        delta: i8,
    ) -> Result<AppDocument, AppApiError> {
        let block_ids = self.selected_formattable_block_ids(selection)?;
        let mut operations = Vec::new();
        for block_id in block_ids {
            let Some(block) = find_block_in_blocks(&self.document.blocks, &block_id) else {
                continue;
            };
            if let BlockKind::ListItem { level, kind, .. } = block.kind {
                let level = (i16::from(level) + i16::from(delta)).clamp(0, 8) as u8;
                operations.push((
                    "update-list-item",
                    "update list item",
                    OperationKind::UpdateListItem {
                        block_id,
                        level,
                        kind,
                    },
                ));
                continue;
            }
            let current = block
                .properties
                .indent_start
                .map(opendoc_core::Length::twips)
                .unwrap_or(0);
            let next = (current + i32::from(delta) * INDENT_STEP_TWIPS)
                .clamp(0, opendoc_core::Length::MAX_TWIPS);
            if next == 0 {
                operations.push((
                    "clear-block-property",
                    "clear block property",
                    OperationKind::ClearBlockProperty {
                        block_id,
                        key: BlockPropertyKey::IndentStart,
                    },
                ));
            } else {
                operations.push((
                    "set-block-property",
                    "set block property",
                    OperationKind::SetBlockProperty {
                        block_id,
                        property: BlockProperty::IndentStart(parse_length(next)?),
                    },
                ));
            }
        }
        Ok(self.apply_batch(operations))
    }

    fn formattable_block_id(&self, block_id: &str) -> Result<StableId, AppApiError> {
        let block_id = parse_id(block_id)?;
        if !block_exists(&self.document.blocks, &block_id) {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        }
        Ok(block_id)
    }

    fn selected_formattable_block_ids(
        &mut self,
        selection: EditorSelection,
    ) -> Result<Vec<StableId>, AppApiError> {
        self.describe_editor_selection(selection)?
            .selected_block_ids
            .into_iter()
            .map(|id| self.formattable_block_id(&id))
            .collect()
    }

    /// Restyles a set of blocks in one gesture, keeping list identity honest.
    ///
    /// Blocks becoming list items get their run ids assigned together, so a
    /// multi-block selection becomes one list rather than several. Blocks
    /// leaving a list split the run they were in: the items after them are
    /// moved to a fresh run so numbering does not keep counting across the
    /// paragraph that now separates the two halves.
    fn block_style_operations(
        &self,
        block_ids: Vec<StableId>,
        style: &str,
        level: u8,
        list_kind_name: &str,
    ) -> Result<Vec<(&'static str, &'static str, OperationKind)>, AppApiError> {
        for block_id in &block_ids {
            let Some(block) = find_block_in_blocks(&self.document.blocks, block_id) else {
                return Err(AppApiError::NotFound(format!(
                    "block {block_id} was not found"
                )));
            };
            if !matches!(
                block.kind,
                BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::ListItem { .. }
            ) {
                return Err(AppApiError::Format(format!(
                    "block {block_id} is not a paragraph, heading, or list item"
                )));
            }
        }
        let style = style.trim();
        match style {
            "heading" if !(1..=6).contains(&level) => {
                return Err(AppApiError::Format(format!(
                    "heading level {level} is outside 1..=6"
                )));
            }
            "list-item" if level > 8 => {
                return Err(AppApiError::Format(format!(
                    "list item level {level} is outside 0..=8"
                )));
            }
            "paragraph" | "heading" | "list-item" => {}
            other => {
                return Err(AppApiError::Format(format!(
                    "unsupported block text style {other}"
                )));
            }
        }

        let kind = if style == "list-item" {
            list_kind(list_kind_name)?
        } else {
            ListKind::Bullet
        };
        let targets = block_ids.iter().cloned().collect::<BTreeSet<_>>();
        let list_ids = if style == "list-item" {
            list_ids_for_converted_blocks(&self.document.blocks, &targets)
        } else {
            BTreeMap::new()
        };
        let mut operations = block_ids
            .into_iter()
            .map(|block_id| {
                let style = match style {
                    "paragraph" => BlockTextStyle::Paragraph,
                    "heading" => BlockTextStyle::Heading { level },
                    _ => BlockTextStyle::ListItem {
                        list_id: list_ids.get(&block_id).cloned().unwrap_or_else(new_list_id),
                        level,
                        kind,
                    },
                };
                (
                    "set-block-text-style",
                    "set block text style",
                    OperationKind::SetBlockTextStyle { block_id, style },
                )
            })
            .collect::<Vec<_>>();
        if style != "list-item" {
            operations.extend(list_run_split_operations(&self.document.blocks, &targets));
        }
        Ok(operations)
    }

    pub fn add_page_break(&mut self) -> AppDocument {
        self.apply(
            "insert-block",
            "page break",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::PageBreak,
                    content: Vec::new(),
                    properties: BlockProperties::default(),
                },
            },
        )
    }

    pub fn insert_page_break_after(
        &mut self,
        after_block_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let after = parse_id(after_block_id.as_ref())?;
        if !self.document.blocks.iter().any(|block| block.id == after) {
            return Err(AppApiError::NotFound(format!(
                "top-level block {after} was not found"
            )));
        }
        Ok(self.apply(
            "insert-block",
            "page break after block",
            OperationKind::InsertBlock {
                after: Some(after),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::PageBreak,
                    content: Vec::new(),
                    properties: BlockProperties::default(),
                },
            },
        ))
    }
}
