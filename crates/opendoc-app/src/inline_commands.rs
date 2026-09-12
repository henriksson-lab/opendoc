//! Inline commands: headings, links, mentions, footnotes and equations.

use super::*;

impl OpenDocApp {
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
                    properties: BlockProperties::default(),
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
                    properties: BlockProperties::default(),
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
                    properties: BlockProperties::default(),
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
                        properties: BlockProperties::default(),
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
                    properties: BlockProperties::default(),
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
                    properties: BlockProperties::default(),
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
                    properties: BlockProperties::default(),
                },
            },
        ))
    }
}
