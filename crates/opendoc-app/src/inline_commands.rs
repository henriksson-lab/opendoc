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
        self.apply(
            "insert-block",
            "heading",
            OperationKind::InsertBlock {
                position: InsertPosition::after_or_last(
                    self.document.blocks.last().map(|block| block.id.clone()),
                ),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Heading { level },
                    content: vec![Inline::text(text)],
                    properties: BlockProperties::default(),
                },
            },
        )
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
        self.apply(
            "update-heading-level",
            "update heading level",
            OperationKind::UpdateHeadingLevel { block_id, level },
        )
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
        self.apply(
            "insert-block",
            "link paragraph",
            OperationKind::InsertBlock {
                position: InsertPosition::after_or_last(
                    self.document.blocks.last().map(|block| block.id.clone()),
                ),
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
        )
    }

    pub fn add_mention(&mut self, label: impl Into<String>) -> Result<AppDocument, AppApiError> {
        let label = label.into().trim().to_string();
        if label.is_empty() {
            return Err(AppApiError::Format("mention label is empty".to_string()));
        }
        self.apply(
            "insert-block",
            "mention paragraph",
            OperationKind::InsertBlock {
                position: InsertPosition::after_or_last(
                    self.document.blocks.last().map(|block| block.id.clone()),
                ),
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
        )
    }

    pub fn add_footnote_ref(&mut self) -> Result<AppDocument, AppApiError> {
        let footnote_id = StableId::new("footnote");
        let operations = vec![
            (
                "upsert-footnote",
                "footnote body",
                OperationKind::UpsertFootnote {
                    footnote: Footnote {
                        id: footnote_id.clone(),
                        revision: self.next_envelope_seq,
                        body: vec![Inline::text("New footnote")],
                        deleted: false,
                    },
                },
            ),
            (
                "insert-block",
                "footnote reference paragraph",
                OperationKind::InsertBlock {
                    position: InsertPosition::after_or_last(
                        self.document.blocks.last().map(|block| block.id.clone()),
                    ),
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

    /// Create a document-end note and its atomic reference.  The placement is
    /// journalled separately from the body so replicas never infer it from an
    /// id prefix or a renderer-only convention.
    pub fn add_endnote_ref(&mut self) -> Result<AppDocument, AppApiError> {
        let footnote_id = StableId::new("endnote");
        let revision = self.next_envelope_seq;
        self.apply_batch(vec![
            (
                "upsert-footnote",
                "endnote body",
                OperationKind::UpsertFootnote {
                    footnote: Footnote {
                        id: footnote_id.clone(),
                        revision,
                        body: vec![Inline::text("New endnote")],
                        deleted: false,
                    },
                },
            ),
            (
                "set-endnote-placement",
                "endnote placement",
                OperationKind::SetEndnotePlacement {
                    footnote_id: footnote_id.clone(),
                    revision,
                    endnote: true,
                },
            ),
            (
                "insert-block",
                "endnote reference paragraph",
                OperationKind::InsertBlock {
                    position: InsertPosition::after_or_last(
                        self.document.blocks.last().map(|block| block.id.clone()),
                    ),
                    block: Block {
                        id: StableId::new("block"),
                        kind: BlockKind::Paragraph,
                        content: vec![
                            Inline::text("Endnote reference: "),
                            Inline::FootnoteRef {
                                id: StableId::new("endnote-ref"),
                                footnote_id,
                            },
                        ],
                        properties: BlockProperties::default(),
                    },
                },
            ),
        ])
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
        self.apply(
            "insert-inline",
            "insert mention",
            OperationKind::InsertInline {
                block_id,
                position: InsertPosition::after_or_last(after),
                inline: Inline::Mention {
                    id: StableId::new("mention"),
                    label,
                },
            },
        )
    }

    /// Insert an atomic date chip at the editor's stable inline boundary.
    ///
    /// This deliberately takes a canonical calendar value rather than a
    /// locale-formatted label: the projection can format it for a reader, but
    /// the durable model must have one representation on every replica.
    pub fn insert_date_chip_after(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
        date: impl Into<String>,
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
        let inline = Inline::DateChip {
            id: StableId::new("date-chip"),
            date: date.into(),
        };
        inline
            .validate()
            .map_err(|error| AppApiError::Format(error.to_string()))?;
        self.apply(
            "insert-inline",
            "insert date chip",
            OperationKind::InsertInline {
                block_id,
                position: InsertPosition::after_or_last(after),
                inline,
            },
        )
    }

    pub fn insert_footnote_ref_after(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.insert_note_ref_after(block_id, after_inline_id, false)
    }

    /// Insert a document-end note reference at an exact inline position.
    /// Unlike `add_endnote_ref`, this is the caret-oriented command used by
    /// the editor and therefore must not manufacture a separate paragraph.
    pub fn insert_endnote_ref_after(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.insert_note_ref_after(block_id, after_inline_id, true)
    }

    fn insert_note_ref_after(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
        endnote: bool,
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
        let footnote_id = StableId::new(if endnote { "endnote" } else { "footnote" });
        let revision = self.next_envelope_seq;
        let mut operations = vec![(
            "upsert-footnote",
            if endnote {
                "endnote body"
            } else {
                "footnote body"
            },
            OperationKind::UpsertFootnote {
                footnote: Footnote {
                    id: footnote_id.clone(),
                    revision,
                    body: vec![Inline::text(if endnote {
                        "New endnote"
                    } else {
                        "New footnote"
                    })],
                    deleted: false,
                },
            },
        )];
        if endnote {
            operations.push((
                "set-endnote-placement",
                "endnote placement",
                OperationKind::SetEndnotePlacement {
                    footnote_id: footnote_id.clone(),
                    revision,
                    endnote: true,
                },
            ));
        }
        operations.push((
            "insert-inline",
            if endnote {
                "insert endnote reference"
            } else {
                "insert footnote reference"
            },
            OperationKind::InsertInline {
                block_id,
                position: InsertPosition::after_or_last(after),
                inline: Inline::FootnoteRef {
                    id: StableId::new("footnote-ref"),
                    footnote_id,
                },
            },
        ));
        self.apply_batch(operations)
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
        self.apply(
            "insert-inline",
            "insert inline equation",
            OperationKind::InsertInline {
                block_id,
                position: InsertPosition::after_or_last(after),
                inline: Inline::Equation {
                    id: StableId::new("eq-inline"),
                    equation: Equation {
                        id: StableId::new("eq"),
                        source_format: EquationSourceFormat::LatexLike,
                        source,
                    },
                },
            },
        )
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
        self.apply(
            "upsert-footnote",
            "update footnote body",
            OperationKind::UpsertFootnote {
                footnote: Footnote {
                    id: footnote_id,
                    revision: self.next_envelope_seq,
                    body: vec![Inline::text(body)],
                    deleted: false,
                },
            },
        )
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
        self.apply(
            "insert-block",
            "inline equation",
            OperationKind::InsertBlock {
                position: InsertPosition::after_or_last(
                    self.document.blocks.last().map(|block| block.id.clone()),
                ),
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
        )
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
        self.apply(
            "insert-block",
            "block equation",
            OperationKind::InsertBlock {
                position: InsertPosition::after_or_last(
                    self.document.blocks.last().map(|block| block.id.clone()),
                ),
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
        )
    }

    pub fn insert_equation_block_after(
        &mut self,
        after_block_id: impl AsRef<str>,
        source: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let after = parse_id(after_block_id.as_ref())?;
        if find_block_in_blocks(&self.document.blocks, &after).is_none() {
            return Err(AppApiError::NotFound(format!(
                "block {after} was not found"
            )));
        }
        let source = source.into().trim().to_string();
        if source.is_empty() {
            return Err(AppApiError::Format(
                "block equation source is empty".to_string(),
            ));
        }
        self.apply(
            "insert-block",
            "block equation after block",
            OperationKind::InsertBlock {
                position: InsertPosition::After(after),
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
        )
    }
}
