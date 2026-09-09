use super::*;

impl OpenDocApp {
    pub fn set_document_doi(&mut self, doi: impl Into<String>) -> Result<AppDocument, AppApiError> {
        let doi = doi.into();
        let doi = if doi.trim().is_empty() {
            None
        } else {
            Some(doi.trim().to_string())
        };
        Ok(self.apply(
            "set-document-doi",
            "set document DOI",
            OperationKind::SetDocumentDoi { doi },
        ))
    }

    pub fn set_document_title(
        &mut self,
        title: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(AppApiError::Format("document title is empty".to_string()));
        }
        Ok(self.apply(
            "set-document-title",
            "set document title",
            OperationKind::SetDocumentTitle {
                title: title.trim().to_string(),
            },
        ))
    }

    pub fn set_document_locale(
        &mut self,
        locale: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let locale = locale.into();
        if locale.trim().is_empty() {
            return Err(AppApiError::Format("document locale is empty".to_string()));
        }
        Ok(self.apply(
            "set-document-locale",
            "set document locale",
            OperationKind::SetDocumentLocale {
                locale: locale.trim().to_string(),
            },
        ))
    }

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
                        properties: Vec::new(),
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
                        properties: Vec::new(),
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
        ordered: bool,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        let style = self.block_text_style_for(&block_id, style.as_ref(), level, ordered)?;
        Ok(self.apply(
            "set-block-text-style",
            "set block text style",
            OperationKind::SetBlockTextStyle { block_id, style },
        ))
    }

    pub fn set_editor_selection_block_style(
        &mut self,
        selection: EditorSelection,
        style: impl AsRef<str>,
        level: u8,
        ordered: bool,
    ) -> Result<AppDocument, AppApiError> {
        let block_ids = self
            .describe_editor_selection(selection)?
            .selected_block_ids
            .into_iter()
            .map(|id| parse_id(&id))
            .collect::<Result<Vec<_>, _>>()?;
        let operations = block_ids
            .into_iter()
            .map(|block_id| {
                let style = self.block_text_style_for(&block_id, style.as_ref(), level, ordered)?;
                Ok((
                    "set-block-text-style",
                    "set block text style",
                    OperationKind::SetBlockTextStyle { block_id, style },
                ))
            })
            .collect::<Result<Vec<_>, AppApiError>>()?;
        Ok(self.apply_batch(operations))
    }

    pub fn add_heading(
        &mut self,
        text: impl Into<String>,
        level: u8,
    ) -> Result<AppDocument, AppApiError> {
        if !(1..=6).contains(&level) {
            return Err(AppApiError::Format(format!(
                "heading level {level} is outside 1..=6"
            )));
        }
        Ok(self.apply(
            "insert-block",
            "heading",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Heading { level },
                    content: vec![Inline::text(text)],
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn update_heading_level(
        &mut self,
        block_id: impl AsRef<str>,
        level: u8,
    ) -> Result<AppDocument, AppApiError> {
        if !(1..=6).contains(&level) {
            return Err(AppApiError::Format(format!(
                "heading level {level} is outside 1..=6"
            )));
        }
        let block_id = parse_id(block_id.as_ref())?;
        let Some(block) = find_block_in_blocks(&self.document.blocks, &block_id) else {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        };
        if !matches!(block.kind, BlockKind::Heading { .. }) {
            return Err(AppApiError::Format(format!(
                "block {block_id} is not a heading"
            )));
        }
        Ok(self.apply(
            "update-heading-level",
            "update heading level",
            OperationKind::UpdateHeadingLevel { block_id, level },
        ))
    }

    pub fn add_link(
        &mut self,
        text: impl Into<String>,
        href: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let text = text.into();
        let href = href.into().trim().to_string();
        if text.trim().is_empty() {
            return Err(AppApiError::Format("link text is empty".to_string()));
        }
        if href.is_empty() {
            return Err(AppApiError::Format("link href is empty".to_string()));
        }
        Ok(self.apply(
            "insert-block",
            "link paragraph",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Paragraph,
                    content: vec![Inline::Link {
                        id: StableId::new("link"),
                        text,
                        href,
                        marks: Vec::new(),
                    }],
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn add_mention(&mut self, label: impl Into<String>) -> Result<AppDocument, AppApiError> {
        let label = label.into().trim().to_string();
        if label.is_empty() {
            return Err(AppApiError::Format("mention label is empty".to_string()));
        }
        Ok(self.apply(
            "insert-block",
            "mention paragraph",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Paragraph,
                    content: vec![
                        Inline::text("Mention: "),
                        Inline::Mention {
                            id: StableId::new("mention"),
                            label,
                        },
                    ],
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn add_footnote_ref(&mut self) -> AppDocument {
        let footnote_id = StableId::new("footnote");
        let operations = vec![
            (
                "upsert-footnote",
                "footnote body",
                OperationKind::UpsertFootnote {
                    footnote: Footnote {
                        id: footnote_id.clone(),
                        revision: self.next_seq,
                        body: vec![Inline::text("New footnote")],
                        deleted: false,
                    },
                },
            ),
            (
                "insert-block",
                "footnote reference paragraph",
                OperationKind::InsertBlock {
                    after: self.document.blocks.last().map(|block| block.id.clone()),
                    block: Block {
                        id: StableId::new("block"),
                        kind: BlockKind::Paragraph,
                        content: vec![
                            Inline::text("Footnote reference: "),
                            Inline::FootnoteRef {
                                id: StableId::new("footnote-ref"),
                                footnote_id,
                            },
                        ],
                        properties: Vec::new(),
                    },
                },
            ),
        ];
        self.apply_batch(operations)
    }

    pub fn insert_mention_after(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
        label: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let label = label.into().trim().to_string();
        if label.is_empty() {
            return Err(AppApiError::Format("mention label is empty".to_string()));
        }
        let block_id = parse_id(block_id.as_ref())?;
        if !block_exists(&self.document.blocks, &block_id) {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        }
        let after = after_inline_id.map(|id| parse_id(&id)).transpose()?;
        if let Some(after) = &after {
            if !block_contains_inline(&self.document.blocks, &block_id, after) {
                return Err(AppApiError::NotFound(format!(
                    "inline {after} was not found in block {block_id}"
                )));
            }
        }
        Ok(self.apply(
            "insert-inline",
            "insert mention",
            OperationKind::InsertInline {
                block_id,
                after,
                inline: Inline::Mention {
                    id: StableId::new("mention"),
                    label,
                },
            },
        ))
    }

    pub fn insert_footnote_ref_after(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        if !block_exists(&self.document.blocks, &block_id) {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        }
        let after = after_inline_id.map(|id| parse_id(&id)).transpose()?;
        if let Some(after) = &after {
            if !block_contains_inline(&self.document.blocks, &block_id, after) {
                return Err(AppApiError::NotFound(format!(
                    "inline {after} was not found in block {block_id}"
                )));
            }
        }
        let footnote_id = StableId::new("footnote");
        Ok(self.apply_batch(vec![
            (
                "upsert-footnote",
                "footnote body",
                OperationKind::UpsertFootnote {
                    footnote: Footnote {
                        id: footnote_id.clone(),
                        revision: self.next_seq,
                        body: vec![Inline::text("New footnote")],
                        deleted: false,
                    },
                },
            ),
            (
                "insert-inline",
                "insert footnote reference",
                OperationKind::InsertInline {
                    block_id,
                    after,
                    inline: Inline::FootnoteRef {
                        id: StableId::new("footnote-ref"),
                        footnote_id,
                    },
                },
            ),
        ]))
    }

    pub fn insert_equation_after(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
        source: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let source = source.into().trim().to_string();
        if source.is_empty() {
            return Err(AppApiError::Format(
                "inline equation source is empty".to_string(),
            ));
        }
        let block_id = parse_id(block_id.as_ref())?;
        if !block_exists(&self.document.blocks, &block_id) {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        }
        let after = after_inline_id.map(|id| parse_id(&id)).transpose()?;
        if let Some(after) = &after {
            if !block_contains_inline(&self.document.blocks, &block_id, after) {
                return Err(AppApiError::NotFound(format!(
                    "inline {after} was not found in block {block_id}"
                )));
            }
        }
        Ok(self.apply(
            "insert-inline",
            "insert inline equation",
            OperationKind::InsertInline {
                block_id,
                after,
                inline: Inline::Equation {
                    id: StableId::new("eq-inline"),
                    equation: Equation {
                        id: StableId::new("eq"),
                        source_format: EquationSourceFormat::LatexLike,
                        source,
                    },
                },
            },
        ))
    }

    pub fn update_footnote_body(
        &mut self,
        footnote_id: impl AsRef<str>,
        body: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let footnote_id = parse_id(footnote_id.as_ref())?;
        if self
            .document
            .footnotes
            .iter()
            .all(|footnote| footnote.id != footnote_id || footnote.deleted)
        {
            return Err(AppApiError::NotFound(format!(
                "footnote {footnote_id} was not found"
            )));
        }
        let body = body.into();
        if body.trim().is_empty() {
            return Err(AppApiError::Format("footnote body is empty".to_string()));
        }
        Ok(self.apply(
            "upsert-footnote",
            "update footnote body",
            OperationKind::UpsertFootnote {
                footnote: Footnote {
                    id: footnote_id,
                    revision: self.next_seq,
                    body: vec![Inline::text(body)],
                    deleted: false,
                },
            },
        ))
    }

    pub fn add_equation_inline(
        &mut self,
        source: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let source = source.into().trim().to_string();
        if source.is_empty() {
            return Err(AppApiError::Format(
                "inline equation source is empty".to_string(),
            ));
        }
        Ok(self.apply(
            "insert-block",
            "inline equation",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Paragraph,
                    content: vec![
                        Inline::text("Equation: "),
                        Inline::Equation {
                            id: StableId::new("eq-inline"),
                            equation: Equation {
                                id: StableId::new("eq"),
                                source_format: EquationSourceFormat::LatexLike,
                                source,
                            },
                        },
                    ],
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn add_equation_block(
        &mut self,
        source: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let source = source.into().trim().to_string();
        if source.is_empty() {
            return Err(AppApiError::Format(
                "block equation source is empty".to_string(),
            ));
        }
        Ok(self.apply(
            "insert-block",
            "block equation",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::EquationBlock {
                        equation: Equation {
                            id: StableId::new("eq"),
                            source_format: EquationSourceFormat::LatexLike,
                            source,
                        },
                    },
                    content: Vec::new(),
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn insert_equation_block_after(
        &mut self,
        after_block_id: impl AsRef<str>,
        source: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let after = parse_id(after_block_id.as_ref())?;
        if !self.document.blocks.iter().any(|block| block.id == after) {
            return Err(AppApiError::NotFound(format!(
                "top-level block {after} was not found"
            )));
        }
        let source = source.into().trim().to_string();
        if source.is_empty() {
            return Err(AppApiError::Format(
                "block equation source is empty".to_string(),
            ));
        }
        Ok(self.apply(
            "insert-block",
            "block equation after block",
            OperationKind::InsertBlock {
                after: Some(after),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::EquationBlock {
                        equation: Equation {
                            id: StableId::new("eq"),
                            source_format: EquationSourceFormat::LatexLike,
                            source,
                        },
                    },
                    content: Vec::new(),
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn add_list_item(
        &mut self,
        text: impl Into<String>,
        level: u8,
        ordered: bool,
    ) -> Result<AppDocument, AppApiError> {
        if level > 8 {
            return Err(AppApiError::Format(format!(
                "list item level {level} is outside 0..=8"
            )));
        }
        Ok(self.apply(
            "insert-block",
            "list item",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::ListItem {
                        list_id: StableId::parse("list-main").expect("static list id"),
                        level,
                        ordered,
                    },
                    content: vec![Inline::text(text)],
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn insert_list_item_after(
        &mut self,
        after_block_id: impl AsRef<str>,
        text: impl Into<String>,
        level: u8,
        ordered: bool,
    ) -> Result<AppDocument, AppApiError> {
        if level > 8 {
            return Err(AppApiError::Format(format!(
                "list item level {level} is outside 0..=8"
            )));
        }
        let after = parse_id(after_block_id.as_ref())?;
        let Some(anchor) = find_block_in_blocks(&self.document.blocks, &after) else {
            return Err(AppApiError::NotFound(format!(
                "top-level block {after} was not found"
            )));
        };
        let list_id = match &anchor.kind {
            BlockKind::ListItem { list_id, .. } => list_id.clone(),
            _ => StableId::parse("list-main").expect("static list id"),
        };
        Ok(self.apply(
            "insert-block",
            "list item after block",
            OperationKind::InsertBlock {
                after: Some(after),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::ListItem {
                        list_id,
                        level,
                        ordered,
                    },
                    content: vec![Inline::text(text)],
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn update_list_item(
        &mut self,
        block_id: impl AsRef<str>,
        level: u8,
        ordered: bool,
    ) -> Result<AppDocument, AppApiError> {
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
        Ok(self.apply(
            "update-list-item",
            "update list item",
            OperationKind::UpdateListItem {
                block_id,
                level,
                ordered,
            },
        ))
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
                let BlockKind::ListItem { level, ordered, .. } = block.kind else {
                    return None;
                };
                let level = (i16::from(level) + i16::from(delta)).clamp(0, 8) as u8;
                Some((
                    "update-list-item",
                    "update list item",
                    OperationKind::UpdateListItem {
                        block_id,
                        level,
                        ordered,
                    },
                ))
            })
            .collect();
        Ok(self.apply_batch(operations))
    }

    fn block_text_style_for(
        &self,
        block_id: &StableId,
        style: &str,
        level: u8,
        ordered: bool,
    ) -> Result<BlockTextStyle, AppApiError> {
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
        Ok(match style.trim() {
            "paragraph" => BlockTextStyle::Paragraph,
            "heading" => {
                if !(1..=6).contains(&level) {
                    return Err(AppApiError::Format(format!(
                        "heading level {level} is outside 1..=6"
                    )));
                }
                BlockTextStyle::Heading { level }
            }
            "list-item" => {
                if level > 8 {
                    return Err(AppApiError::Format(format!(
                        "list item level {level} is outside 0..=8"
                    )));
                }
                let list_id = match &block.kind {
                    BlockKind::ListItem { list_id, .. } => list_id.clone(),
                    _ => StableId::parse("list-main").expect("static list id"),
                };
                BlockTextStyle::ListItem {
                    list_id,
                    level,
                    ordered,
                }
            }
            other => {
                return Err(AppApiError::Format(format!(
                    "unsupported block text style {other}"
                )));
            }
        })
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
                    properties: Vec::new(),
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
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn add_table(&mut self) -> AppDocument {
        self.apply(
            "insert-block",
            "table",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: default_table_block(),
            },
        )
    }

    pub fn insert_table_after(
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
            "table after block",
            OperationKind::InsertBlock {
                after: Some(after),
                block: default_table_block(),
            },
        ))
    }

    pub fn add_table_row(
        &mut self,
        table_block_id: impl AsRef<str>,
        after_row: Option<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let table_block_id = parse_id(table_block_id.as_ref())?;
        let after_row = after_row.as_deref().map(parse_id).transpose()?;
        Ok(self.apply(
            "insert-table-row",
            "table row",
            OperationKind::InsertTableRow {
                table_block_id,
                after_row,
                row: opendoc_core::TableRow {
                    id: StableId::new("row"),
                    cells: vec![opendoc_core::TableCell {
                        id: StableId::new("cell"),
                        blocks: vec![Block::paragraph(text)],
                        properties: Vec::new(),
                    }],
                },
            },
        ))
    }

    pub fn delete_table_row(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "delete-table-row",
            "delete table row",
            OperationKind::DeleteTableRow {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_id: parse_id(row_id.as_ref())?,
            },
        ))
    }

    pub fn add_table_cell(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
        after_cell: Option<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let table_block_id = parse_id(table_block_id.as_ref())?;
        let row_id = parse_id(row_id.as_ref())?;
        let after_cell = after_cell.as_deref().map(parse_id).transpose()?;
        Ok(self.apply(
            "insert-table-cell",
            "table cell",
            OperationKind::InsertTableCell {
                table_block_id,
                row_id,
                after_cell,
                cell: opendoc_core::TableCell {
                    id: StableId::new("cell"),
                    blocks: vec![Block::paragraph(text)],
                    properties: Vec::new(),
                },
            },
        ))
    }

    pub fn delete_table_cell(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
        cell_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "delete-table-cell",
            "delete table cell",
            OperationKind::DeleteTableCell {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_id: parse_id(row_id.as_ref())?,
                cell_id: parse_id(cell_id.as_ref())?,
            },
        ))
    }
}
