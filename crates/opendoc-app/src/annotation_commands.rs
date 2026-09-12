use super::*;

impl OpenDocApp {
    pub fn add_sample_citation(&mut self) -> AppDocument {
        let reference_id = StableId::parse("ref-doe-2020").expect("static reference id");
        let citation_id = StableId::parse("cite-intro").expect("static citation id");
        self.apply(
            "upsert-bibliography-reference",
            "sample citation reference",
            OperationKind::UpsertBibliographyReference {
                reference: BibliographyReference {
                    id: reference_id.clone(),
                    revision: self.next_seq,
                    source: CitationSource {
                        format: CitationSourceFormat::CitumNative,
                        bytes: b"id: doe-2020\ntitle: Example Article\nauthor: Doe\nyear: 2020"
                            .to_vec(),
                    },
                    summary: CitationSummary {
                        title: "Example Article".to_string(),
                        authors: vec!["Doe".to_string()],
                        issued: Some("2020".to_string()),
                        doi: Some("10.0000/example".to_string()),
                        url: None,
                    },
                    deleted: false,
                },
            },
        );
        self.apply(
            "upsert-citation-group",
            "sample citation group",
            OperationKind::UpsertCitationGroup {
                citation: CitationGroup {
                    id: citation_id.clone(),
                    revision: self.next_seq,
                    items: vec![CitationItem {
                        reference_id,
                        locator: Some("42".to_string()),
                        label: Some("page".to_string()),
                        prefix: Some("see".to_string()),
                        suffix: None,
                        suppress_author: false,
                    }],
                    placement: CitationPlacement::Inline,
                    rendered_cache: Some("(see Doe 2020, page 42)".to_string()),
                    deleted: false,
                },
            },
        );
        self.apply(
            "insert-block",
            "citation label paragraph",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Paragraph,
                    content: vec![
                        Inline::text("Citation label: "),
                        Inline::Citation {
                            id: StableId::new("citation-label"),
                            citation_id,
                            rendered_cache: None,
                        },
                    ],
                    properties: BlockProperties::default(),
                },
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_citation(
        &mut self,
        reference_id: impl AsRef<str>,
        after_inline_id: Option<String>,
        locator: Option<String>,
        label: Option<String>,
        prefix: Option<String>,
        suffix: Option<String>,
        suppress_author: bool,
    ) -> Result<AppDocument, AppApiError> {
        self.insert_citation_group(
            vec![AppCitationItem {
                reference_id: reference_id.as_ref().to_string(),
                locator,
                label,
                prefix,
                suffix,
                suppress_author,
            }],
            after_inline_id,
        )
    }

    pub fn insert_citation_group(
        &mut self,
        items: Vec<AppCitationItem>,
        after_inline_id: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        if items.is_empty() {
            return Err(AppApiError::Format(
                "citation group requires at least one item".to_string(),
            ));
        }
        let items = items
            .into_iter()
            .map(|item| app_citation_item_to_core(&item))
            .collect::<Result<Vec<_>, _>>()?;
        for item in &items {
            if self
                .document
                .citation_database
                .references
                .iter()
                .all(|reference| reference.id != item.reference_id || reference.deleted)
            {
                return Err(AppApiError::NotFound(format!(
                    "bibliography reference {} was not found",
                    item.reference_id
                )));
            }
        }
        let after = after_inline_id.map(|id| parse_id(&id)).transpose()?;
        let block_id = match &after {
            Some(after) => find_block_id_containing_inline(&self.document.blocks, after)
                .ok_or_else(|| AppApiError::NotFound(format!("inline {after} was not found")))?,
            None => self
                .document
                .blocks
                .last()
                .map(|block| block.id.clone())
                .ok_or_else(|| AppApiError::NotFound("no block is available".to_string()))?,
        };

        let citation_id = StableId::new("citation");
        let citation = CitationGroup {
            id: citation_id.clone(),
            revision: self.next_seq,
            items,
            placement: CitationPlacement::Inline,
            rendered_cache: None,
            deleted: false,
        };
        let rendered = render_citation_cache(&self.document.citation_database, &citation);
        self.apply(
            "upsert-citation-group",
            "citation group",
            OperationKind::UpsertCitationGroup {
                citation: CitationGroup {
                    rendered_cache: rendered,
                    ..citation
                },
            },
        );

        Ok(self.apply(
            "insert-inline",
            "citation label",
            OperationKind::InsertInline {
                block_id,
                after,
                inline: Inline::Citation {
                    id: StableId::new("citation-label"),
                    citation_id,
                    rendered_cache: None,
                },
            },
        ))
    }

    pub fn insert_footnote_citation_group(
        &mut self,
        footnote_id: impl AsRef<str>,
        items: Vec<AppCitationItem>,
    ) -> Result<AppDocument, AppApiError> {
        if items.is_empty() {
            return Err(AppApiError::Format(
                "citation group requires at least one item".to_string(),
            ));
        }
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
        let items = items
            .into_iter()
            .map(|item| app_citation_item_to_core(&item))
            .collect::<Result<Vec<_>, _>>()?;
        for item in &items {
            if self
                .document
                .citation_database
                .references
                .iter()
                .all(|reference| reference.id != item.reference_id || reference.deleted)
            {
                return Err(AppApiError::NotFound(format!(
                    "bibliography reference {} was not found",
                    item.reference_id
                )));
            }
        }

        let citation = CitationGroup {
            id: StableId::new("citation"),
            revision: self.next_seq,
            items,
            placement: CitationPlacement::Footnote { footnote_id },
            rendered_cache: None,
            deleted: false,
        };
        let rendered = render_citation_cache(&self.document.citation_database, &citation);
        Ok(self.apply(
            "upsert-citation-group",
            "footnote citation group",
            OperationKind::UpsertCitationGroup {
                citation: CitationGroup {
                    rendered_cache: rendered,
                    ..citation
                },
            },
        ))
    }

    pub fn insert_footnote_citation_after(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
        items: Vec<AppCitationItem>,
    ) -> Result<AppDocument, AppApiError> {
        if items.is_empty() {
            return Err(AppApiError::Format(
                "citation group requires at least one item".to_string(),
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
        let items = items
            .into_iter()
            .map(|item| app_citation_item_to_core(&item))
            .collect::<Result<Vec<_>, _>>()?;
        for item in &items {
            if self
                .document
                .citation_database
                .references
                .iter()
                .all(|reference| reference.id != item.reference_id || reference.deleted)
            {
                return Err(AppApiError::NotFound(format!(
                    "bibliography reference {} was not found",
                    item.reference_id
                )));
            }
        }
        let footnote_id = StableId::new("footnote");
        let citation = CitationGroup {
            id: StableId::new("citation"),
            revision: self.next_seq,
            items,
            placement: CitationPlacement::Footnote {
                footnote_id: footnote_id.clone(),
            },
            rendered_cache: None,
            deleted: false,
        };
        let rendered = render_citation_cache(&self.document.citation_database, &citation);
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
                        footnote_id: footnote_id.clone(),
                    },
                },
            ),
            (
                "upsert-citation-group",
                "footnote citation group",
                OperationKind::UpsertCitationGroup {
                    citation: CitationGroup {
                        rendered_cache: rendered,
                        ..citation
                    },
                },
            ),
        ]))
    }

    pub fn set_citation_style(
        &mut self,
        style: impl Into<String>,
        locale: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let style = normalize_citation_style(style.into())?;
        let locale = normalize_citation_locale(locale.into())?;
        self.apply(
            "set-citation-style",
            &format!("citation style {style}/{locale}"),
            OperationKind::UpdateCitationStyle {
                style: style.clone(),
                locale: locale.clone(),
            },
        );
        self.rerender_all_citation_groups();
        Ok(self.document())
    }

    pub fn update_citation_group_items(
        &mut self,
        citation_id: impl AsRef<str>,
        items: Vec<AppCitationItem>,
    ) -> Result<AppDocument, AppApiError> {
        if items.is_empty() {
            return Err(AppApiError::Format(
                "citation group requires at least one item".to_string(),
            ));
        }
        let citation_id = parse_id(citation_id.as_ref())?;
        let Some(existing) = self
            .document
            .citation_database
            .citations
            .iter()
            .find(|citation| citation.id == citation_id && !citation.deleted)
            .cloned()
        else {
            return Err(AppApiError::NotFound(format!(
                "citation group {citation_id} was not found"
            )));
        };
        let items = items
            .into_iter()
            .map(|item| app_citation_item_to_core(&item))
            .collect::<Result<Vec<_>, _>>()?;
        for item in &items {
            if self
                .document
                .citation_database
                .references
                .iter()
                .all(|reference| reference.id != item.reference_id || reference.deleted)
            {
                return Err(AppApiError::NotFound(format!(
                    "bibliography reference {} was not found",
                    item.reference_id
                )));
            }
        }

        let mut citation = existing;
        citation.revision = self.next_seq;
        citation.items = items;
        citation.rendered_cache =
            render_citation_cache(&self.document.citation_database, &citation);
        self.apply(
            "upsert-citation-group",
            "update citation group items",
            OperationKind::UpsertCitationGroup {
                citation: citation.clone(),
            },
        );
        refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        Ok(self.document())
    }

    pub fn add_comment(
        &mut self,
        author: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.add_comment_with_anchor(Anchor::Document, author, body)
    }

    pub fn add_text_range_comment(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        author: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let range = self.parse_existing_text_range(start_inline_id, end_inline_id)?;
        self.add_comment_with_anchor(Anchor::TextRange(range), author, body)
    }

    pub fn add_block_comment(
        &mut self,
        block_id: impl AsRef<str>,
        author: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        if !block_tree_contains_id(&self.document.blocks, &block_id) {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        }
        self.add_comment_with_anchor(
            Anchor::NearestBlock {
                block_id,
                warning: "anchor restored to nearest block".to_string(),
            },
            author,
            body,
        )
    }

    fn parse_existing_text_range(
        &self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
    ) -> Result<TextRange, AppApiError> {
        let start = parse_id(start_inline_id.as_ref())?;
        let end = parse_id(end_inline_id.as_ref())?;
        if find_inline_in_blocks(&self.document.blocks, &start).is_none() {
            return Err(AppApiError::NotFound(format!(
                "inline {start} was not found"
            )));
        }
        if find_inline_in_blocks(&self.document.blocks, &end).is_none() {
            return Err(AppApiError::NotFound(format!("inline {end} was not found")));
        }
        Ok(TextRange { start, end })
    }

    fn add_comment_with_anchor(
        &mut self,
        anchor: Anchor,
        author: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = normalize_source_author(author.into(), "comment author")?;
        let body = normalize_source_text(body.into(), "comment body")?;
        Ok(self.apply(
            "add-comment-thread",
            "comment thread",
            OperationKind::AddCommentThread {
                thread: CommentThread {
                    id: StableId::new("comment-thread"),
                    anchor,
                    comments: vec![Comment {
                        id: StableId::new("comment"),
                        author,
                        body: vec![Inline::text(body)],
                        created_at_ms: self.next_seq,
                        deleted: false,
                    }],
                    deleted: false,
                },
            },
        ))
    }

    pub fn add_comment_reply(
        &mut self,
        thread_id: impl AsRef<str>,
        author: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let thread_id = parse_id(thread_id.as_ref())?;
        if self
            .document
            .comments
            .iter()
            .all(|thread| thread.id != thread_id || thread.deleted)
        {
            return Err(AppApiError::NotFound(format!(
                "comment thread {thread_id} was not found"
            )));
        }
        let author = normalize_source_author(author.into(), "comment author")?;
        let body = normalize_source_text(body.into(), "comment body")?;
        Ok(self.apply(
            "add-comment-reply",
            "comment reply",
            OperationKind::AddCommentReply {
                thread_id,
                comment: Comment {
                    id: StableId::new("comment"),
                    author,
                    body: vec![Inline::text(body)],
                    created_at_ms: self.next_seq,
                    deleted: false,
                },
            },
        ))
    }

    pub fn add_suggestion(
        &mut self,
        author: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = normalize_source_author(author.into(), "suggestion author")?;
        let text = normalize_source_text(text.into(), "insert suggestion content")?;
        let anchor = self
            .document
            .blocks
            .first()
            .and_then(|block| block.content.first())
            .map(|inline| {
                let id = inline_id(inline).clone();
                Anchor::TextRange(TextRange {
                    start: id.clone(),
                    end: id,
                })
            })
            .unwrap_or(Anchor::Document);
        Ok(self.apply(
            "add-suggestion",
            "insert suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::Insert {
                        anchor,
                        content: vec![Inline::text(text)],
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        ))
    }

    pub fn add_text_range_suggestion(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        author: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = normalize_source_author(author.into(), "suggestion author")?;
        let text = normalize_source_text(text.into(), "insert suggestion content")?;
        let anchor =
            Anchor::TextRange(self.parse_existing_text_range(start_inline_id, end_inline_id)?);
        Ok(self.apply(
            "add-suggestion",
            "text range insert suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::Insert {
                        anchor,
                        content: vec![Inline::text(text)],
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        ))
    }

    pub fn add_block_suggestion(
        &mut self,
        block_id: impl AsRef<str>,
        author: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        if !block_tree_contains_id(&self.document.blocks, &block_id) {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        }
        let author = normalize_source_author(author.into(), "suggestion author")?;
        let text = normalize_source_text(text.into(), "insert suggestion content")?;
        Ok(self.apply(
            "add-suggestion",
            "block insert suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::Insert {
                        anchor: Anchor::NearestBlock {
                            block_id,
                            warning: "anchor restored to nearest block".to_string(),
                        },
                        content: vec![Inline::text(text)],
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        ))
    }

    pub fn add_delete_suggestion(
        &mut self,
        author: impl Into<String>,
        inline_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let author = normalize_source_author(author.into(), "suggestion author")?;
        let inline_id = parse_id(inline_id.as_ref())?;
        if find_inline_in_blocks(&self.document.blocks, &inline_id).is_none() {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        }
        Ok(self.apply(
            "add-suggestion",
            "delete suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::Delete {
                        range: TextRange {
                            start: inline_id.clone(),
                            end: inline_id,
                        },
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        ))
    }

    pub fn add_text_range_delete_suggestion(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        author: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = normalize_source_author(author.into(), "suggestion author")?;
        let range = self.parse_existing_text_range(start_inline_id, end_inline_id)?;
        Ok(self.apply(
            "add-suggestion",
            "text range delete suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::Delete { range },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        ))
    }

    pub fn add_format_suggestion(
        &mut self,
        author: impl Into<String>,
        inline_id: impl AsRef<str>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = normalize_source_author(author.into(), "suggestion author")?;
        let inline_id = parse_id(inline_id.as_ref())?;
        if find_inline_in_blocks(&self.document.blocks, &inline_id).is_none() {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        }
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_payload(&kind, value.as_deref())?;
        Ok(self.apply(
            "add-suggestion",
            "format suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::Format {
                        range: TextRange {
                            start: inline_id.clone(),
                            end: inline_id,
                        },
                        marks: vec![Mark {
                            kind,
                            value,
                            expand: MarkExpand::Both,
                        }],
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        ))
    }

    pub fn add_text_range_format_suggestion(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        author: impl Into<String>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = normalize_source_author(author.into(), "suggestion author")?;
        let range = self.parse_existing_text_range(start_inline_id, end_inline_id)?;
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_payload(&kind, value.as_deref())?;
        Ok(self.apply(
            "add-suggestion",
            "text range format suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::Format {
                        range,
                        marks: vec![Mark {
                            kind,
                            value,
                            expand: MarkExpand::Both,
                        }],
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        ))
    }

    pub fn update_suggestion(
        &mut self,
        suggestion_id: impl AsRef<str>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let text = normalize_source_text(text.into(), "insert suggestion content")?;
        let suggestion_id = parse_id(suggestion_id.as_ref())?;
        let Some(suggestion) = self.document.suggestions.iter().find(|suggestion| {
            suggestion.id == suggestion_id && suggestion.state == SuggestionState::Proposed
        }) else {
            return Err(AppApiError::NotFound(format!(
                "suggestion {suggestion_id} was not found"
            )));
        };
        if !matches!(suggestion.kind, SuggestionKind::Insert { .. }) {
            return Err(AppApiError::Format(format!(
                "suggestion {suggestion_id} is not an editable insert suggestion"
            )));
        }
        Ok(self.apply(
            "update-suggestion",
            "update suggestion",
            OperationKind::UpdateSuggestionInsertContent {
                suggestion_id,
                content: vec![Inline::text(text)],
            },
        ))
    }

    pub fn update_inline_text(
        &mut self,
        inline_id: impl AsRef<str>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "update-inline-text",
            "inline text edit",
            OperationKind::UpdateInlineText {
                inline_id: parse_id(inline_id.as_ref())?,
                text: text.into(),
            },
        ))
    }

    pub fn update_mention_label(
        &mut self,
        inline_id: impl AsRef<str>,
        label: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let label = label.into().trim().to_string();
        if label.is_empty() {
            return Err(AppApiError::Format("mention label is empty".to_string()));
        }
        let inline_id = parse_id(inline_id.as_ref())?;
        let Some(inline) = find_inline_in_blocks(&self.document.blocks, &inline_id) else {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        };
        if !matches!(inline, Inline::Mention { .. }) {
            return Err(AppApiError::Format(format!(
                "inline {inline_id} is not a mention"
            )));
        }
        Ok(self.apply(
            "update-mention-label",
            "mention label edit",
            OperationKind::UpdateMentionLabel { inline_id, label },
        ))
    }

    pub fn update_link_href(
        &mut self,
        inline_id: impl AsRef<str>,
        href: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let href = href.into().trim().to_string();
        if href.is_empty() {
            return Err(AppApiError::Format("link href is empty".to_string()));
        }
        let inline_id = parse_id(inline_id.as_ref())?;
        let Some(inline) = find_inline_in_blocks(&self.document.blocks, &inline_id) else {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        };
        if !matches!(inline, Inline::Link { .. }) {
            return Err(AppApiError::Format(format!(
                "inline {inline_id} is not a link"
            )));
        }
        Ok(self.apply(
            "update-link-href",
            "update link target",
            OperationKind::UpdateLinkHref { inline_id, href },
        ))
    }

    pub fn update_inline_equation_source(
        &mut self,
        inline_id: impl AsRef<str>,
        source: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let source = source.into().trim().to_string();
        if source.is_empty() {
            return Err(AppApiError::Format(
                "inline equation source is empty".to_string(),
            ));
        }
        let inline_id = parse_id(inline_id.as_ref())?;
        let Some(inline) = find_inline_in_blocks(&self.document.blocks, &inline_id) else {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        };
        if !matches!(inline, Inline::Equation { .. }) {
            return Err(AppApiError::Format(format!(
                "inline {inline_id} is not an equation"
            )));
        }
        Ok(self.apply(
            "update-inline-equation-source",
            "inline equation edit",
            OperationKind::UpdateInlineEquationSource { inline_id, source },
        ))
    }

    pub fn insert_inline_text(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
        text: impl Into<String>,
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
        Ok(self.apply(
            "insert-inline",
            "insert inline text",
            OperationKind::InsertInline {
                block_id,
                after,
                inline: Inline::text(text),
            },
        ))
    }

    pub fn insert_link_after(
        &mut self,
        block_id: impl AsRef<str>,
        after_inline_id: Option<String>,
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
            "insert link",
            OperationKind::InsertInline {
                block_id,
                after,
                inline: Inline::Link {
                    id: StableId::new("link"),
                    text,
                    href,
                    marks: Vec::new(),
                },
            },
        ))
    }

    pub fn delete_inline(
        &mut self,
        inline_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let inline_id = parse_id(inline_id.as_ref())?;
        if find_inline_in_blocks(&self.document.blocks, &inline_id).is_none() {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        }
        Ok(self.apply(
            "delete-inline",
            "delete inline",
            OperationKind::DeleteInline { inline_id },
        ))
    }

    pub fn delete_comment_thread(
        &mut self,
        thread_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "delete-comment-thread",
            "delete comment thread",
            OperationKind::DeleteCommentThread {
                thread_id: parse_id(thread_id.as_ref())?,
            },
        ))
    }

    pub fn restore_comment_thread(
        &mut self,
        thread_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let thread_id = parse_id(thread_id.as_ref())?;
        if self
            .document
            .comments
            .iter()
            .all(|thread| thread.id != thread_id || !thread.deleted)
        {
            return Err(AppApiError::NotFound(format!(
                "deleted comment thread {thread_id} was not found"
            )));
        }
        Ok(self.apply(
            "restore-comment-thread",
            "restore comment thread",
            OperationKind::RestoreCommentThread { thread_id },
        ))
    }

    pub fn delete_comment(
        &mut self,
        thread_id: impl AsRef<str>,
        comment_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let thread_id = parse_id(thread_id.as_ref())?;
        let comment_id = parse_id(comment_id.as_ref())?;
        let Some(thread) = self
            .document
            .comments
            .iter()
            .find(|thread| thread.id == thread_id && !thread.deleted)
        else {
            return Err(AppApiError::NotFound(format!(
                "comment thread {thread_id} was not found"
            )));
        };
        if thread
            .comments
            .iter()
            .all(|comment| comment.id != comment_id || comment.deleted)
        {
            return Err(AppApiError::NotFound(format!(
                "comment {comment_id} was not found"
            )));
        }
        Ok(self.apply(
            "delete-comment",
            "delete comment",
            OperationKind::DeleteComment {
                thread_id,
                comment_id,
            },
        ))
    }

    pub fn restore_comment(
        &mut self,
        thread_id: impl AsRef<str>,
        comment_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let thread_id = parse_id(thread_id.as_ref())?;
        let comment_id = parse_id(comment_id.as_ref())?;
        let Some(thread) = self
            .document
            .comments
            .iter()
            .find(|thread| thread.id == thread_id)
        else {
            return Err(AppApiError::NotFound(format!(
                "comment thread {thread_id} was not found"
            )));
        };
        if thread
            .comments
            .iter()
            .all(|comment| comment.id != comment_id || !comment.deleted)
        {
            return Err(AppApiError::NotFound(format!(
                "deleted comment {comment_id} was not found"
            )));
        }
        Ok(self.apply(
            "restore-comment",
            "restore comment",
            OperationKind::RestoreComment {
                thread_id,
                comment_id,
            },
        ))
    }

    pub fn update_comment(
        &mut self,
        thread_id: impl AsRef<str>,
        comment_id: impl AsRef<str>,
        body: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let body = normalize_source_text(body.into(), "comment body")?;
        let thread_id = parse_id(thread_id.as_ref())?;
        let comment_id = parse_id(comment_id.as_ref())?;
        let Some(thread) = self
            .document
            .comments
            .iter()
            .find(|thread| thread.id == thread_id && !thread.deleted)
        else {
            return Err(AppApiError::NotFound(format!(
                "comment thread {thread_id} was not found"
            )));
        };
        if thread
            .comments
            .iter()
            .all(|comment| comment.id != comment_id || comment.deleted)
        {
            return Err(AppApiError::NotFound(format!(
                "comment {comment_id} was not found"
            )));
        }
        Ok(self.apply(
            "update-comment",
            "update comment",
            OperationKind::UpdateCommentBody {
                thread_id,
                comment_id,
                body: vec![Inline::text(body)],
            },
        ))
    }

    pub fn accept_suggestion(
        &mut self,
        suggestion_id: impl AsRef<str>,
        accepted_by: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let accepted_by = normalize_source_author(accepted_by.into(), "suggestion accepted by")?;
        Ok(self.apply(
            "accept-suggestion",
            "accept suggestion",
            OperationKind::AcceptSuggestion {
                suggestion_id: parse_id(suggestion_id.as_ref())?,
                accepted_by,
            },
        ))
    }

    pub fn accept_all_suggestions(
        &mut self,
        accepted_by: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let accepted_by = normalize_source_author(accepted_by.into(), "suggestion accepted by")?;
        let ids = self
            .document
            .suggestions
            .iter()
            .filter(|suggestion| suggestion.state == SuggestionState::Proposed)
            .map(|suggestion| suggestion.id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(self.document());
        }
        Ok(self.apply_batch(
            ids.into_iter()
                .map(|suggestion_id| {
                    (
                        "accept-suggestion",
                        "accept suggestion",
                        OperationKind::AcceptSuggestion {
                            suggestion_id,
                            accepted_by: accepted_by.clone(),
                        },
                    )
                })
                .collect(),
        ))
    }

    pub fn reject_suggestion(
        &mut self,
        suggestion_id: impl AsRef<str>,
        rejected_by: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let rejected_by = normalize_source_author(rejected_by.into(), "suggestion rejected by")?;
        Ok(self.apply(
            "reject-suggestion",
            "reject suggestion",
            OperationKind::RejectSuggestion {
                suggestion_id: parse_id(suggestion_id.as_ref())?,
                rejected_by,
            },
        ))
    }

    pub fn reject_all_suggestions(
        &mut self,
        rejected_by: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let rejected_by = normalize_source_author(rejected_by.into(), "suggestion rejected by")?;
        let ids = self
            .document
            .suggestions
            .iter()
            .filter(|suggestion| suggestion.state == SuggestionState::Proposed)
            .map(|suggestion| suggestion.id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(self.document());
        }
        Ok(self.apply_batch(
            ids.into_iter()
                .map(|suggestion_id| {
                    (
                        "reject-suggestion",
                        "reject suggestion",
                        OperationKind::RejectSuggestion {
                            suggestion_id,
                            rejected_by: rejected_by.clone(),
                        },
                    )
                })
                .collect(),
        ))
    }

    pub fn add_text_mark(
        &mut self,
        inline_id: impl AsRef<str>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_payload(&kind, value.as_deref())?;
        Ok(self.apply(
            "add-mark",
            "format inline",
            OperationKind::AddMark {
                text_id: parse_id(inline_id.as_ref())?,
                mark: Mark {
                    kind: kind.clone(),
                    value,
                    expand: MarkExpand::Both,
                },
            },
        ))
    }

    pub fn add_text_mark_range(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let start = parse_id(start_inline_id.as_ref())?;
        let end = parse_id(end_inline_id.as_ref())?;
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_payload(&kind, value.as_deref())?;
        Ok(self.apply(
            "add-mark-range",
            "format inline range",
            OperationKind::AddMarkRange {
                range: TextRange { start, end },
                mark: Mark {
                    kind,
                    value,
                    expand: MarkExpand::Both,
                },
            },
        ))
    }

    pub fn remove_text_mark(
        &mut self,
        inline_id: impl AsRef<str>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_removal_payload(&kind, value.as_deref())?;
        Ok(self.apply(
            "remove-mark",
            "remove inline format",
            OperationKind::RemoveMark {
                text_id: parse_id(inline_id.as_ref())?,
                kind,
                value,
            },
        ))
    }

    pub fn remove_text_mark_range(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let start = parse_id(start_inline_id.as_ref())?;
        let end = parse_id(end_inline_id.as_ref())?;
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_removal_payload(&kind, value.as_deref())?;
        let ids = app_editable_inline_ids(&AppDocument::from_core(&self.document).blocks);
        let Some(start_index) = ids.iter().position(|id| id == &start.to_string()) else {
            return Err(AppApiError::NotFound(format!(
                "inline {start} was not found"
            )));
        };
        let Some(end_index) = ids.iter().position(|id| id == &end.to_string()) else {
            return Err(AppApiError::NotFound(format!("inline {end} was not found")));
        };
        let (first, last) = if start_index <= end_index {
            (start_index, end_index)
        } else {
            (end_index, start_index)
        };
        let operations = ids[first..=last]
            .iter()
            .map(|inline_id| {
                (
                    "remove-mark",
                    "remove inline range format",
                    OperationKind::RemoveMark {
                        text_id: parse_id(inline_id).expect("app inline IDs are canonical"),
                        kind: kind.clone(),
                        value: value.clone(),
                    },
                )
            })
            .collect();
        Ok(self.apply_batch(operations))
    }

    pub fn update_block_equation_source(
        &mut self,
        block_id: impl AsRef<str>,
        source: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let source = source.into().trim().to_string();
        if source.is_empty() {
            return Err(AppApiError::Format(
                "block equation source is empty".to_string(),
            ));
        }
        Ok(self.apply(
            "update-block-equation-source",
            "block equation edit",
            OperationKind::UpdateBlockEquationSource {
                block_id: parse_id(block_id.as_ref())?,
                source,
            },
        ))
    }

    pub fn update_image_alt_text(
        &mut self,
        block_id: impl AsRef<str>,
        alt_text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "update-image-alt-text",
            "image alt text edit",
            OperationKind::UpdateImageAltText {
                block_id: parse_id(block_id.as_ref())?,
                alt_text: alt_text.into(),
            },
        ))
    }

    pub fn update_image_blob_hash(
        &mut self,
        block_id: impl AsRef<str>,
        blob_hash: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let hash = opendoc_core::HashRef::parse(blob_hash.as_ref().trim())
            .map_err(|err| AppApiError::Model(err.to_string()))?
            .to_string();
        if !self.blobs.iter().any(|blob| blob.hash == hash) {
            return Err(AppApiError::NotFound("blob was not found".to_string()));
        }
        Ok(self.apply(
            "update-image-blob-hash",
            "image blob replacement",
            OperationKind::UpdateImageBlobHash {
                block_id: parse_id(block_id.as_ref())?,
                blob_hash: hash,
            },
        ))
    }
    pub fn update_bibliography_reference(
        &mut self,
        reference_id: impl AsRef<str>,
        title: impl Into<String>,
        issued: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let reference_id = parse_id(reference_id.as_ref())?;
        let Some(existing) = self
            .document
            .citation_database
            .references
            .iter()
            .find(|reference| reference.id == reference_id)
            .cloned()
        else {
            return Err(AppApiError::NotFound(format!(
                "bibliography reference {reference_id} was not found"
            )));
        };
        self.update_bibliography_reference_metadata(
            reference_id.as_str(),
            title,
            existing.summary.authors,
            issued,
            existing.summary.doi,
            existing.summary.url,
        )
    }

    pub fn update_bibliography_reference_metadata(
        &mut self,
        reference_id: impl AsRef<str>,
        title: impl Into<String>,
        authors: Vec<String>,
        issued: Option<String>,
        doi: Option<String>,
        url: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let reference_id = parse_id(reference_id.as_ref())?;
        let Some(existing) = self
            .document
            .citation_database
            .references
            .iter()
            .find(|reference| reference.id == reference_id)
            .cloned()
        else {
            return Err(AppApiError::NotFound(format!(
                "bibliography reference {reference_id} was not found"
            )));
        };
        let title = title.into().trim().to_string();
        if title.is_empty() {
            return Err(AppApiError::Format(
                "bibliography reference title is empty".to_string(),
            ));
        }
        let authors = normalize_bibliography_authors(authors)?;
        let mut reference = existing;
        reference.revision = self.next_seq;
        reference.summary.title = title;
        reference.summary.authors = authors;
        reference.summary.issued = normalize_optional_source_string(issued);
        reference.summary.doi = normalize_optional_source_string(doi);
        reference.summary.url = normalize_optional_source_string(url);
        reference.source.format = CitationSourceFormat::CitumNative;
        reference.source.bytes = citation_source_bytes(&reference);
        reference
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        self.apply(
            "upsert-bibliography-reference",
            "update citation reference",
            OperationKind::UpsertBibliographyReference {
                reference: reference.clone(),
            },
        );

        let affected: Vec<_> = self
            .document
            .citation_database
            .citations
            .iter()
            .filter(|citation| {
                citation
                    .items
                    .iter()
                    .any(|item| item.reference_id == reference.id)
            })
            .cloned()
            .collect();
        for mut citation in affected {
            let citation_id = citation.id.clone();
            citation.revision = self.next_seq;
            citation.rendered_cache =
                render_citation_cache(&self.document.citation_database, &citation);
            self.apply(
                "upsert-citation-group",
                "rerender citation group",
                OperationKind::UpsertCitationGroup { citation },
            );
            refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        }
        Ok(self.document())
    }

    pub fn add_bibliography_reference(
        &mut self,
        title: impl Into<String>,
        authors: Vec<String>,
        issued: Option<String>,
        doi: Option<String>,
        url: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let title = title.into().trim().to_string();
        if title.is_empty() {
            return Err(AppApiError::Format(
                "bibliography reference title is empty".to_string(),
            ));
        }
        let authors = normalize_bibliography_authors(authors)?;
        let reference = BibliographyReference {
            id: StableId::new("ref"),
            revision: self.next_seq,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: Vec::new(),
            },
            summary: CitationSummary {
                title,
                authors,
                issued: normalize_optional_source_string(issued),
                doi: normalize_optional_source_string(doi),
                url: normalize_optional_source_string(url),
            },
            deleted: false,
        };
        let mut reference = reference;
        reference.source.bytes = citation_source_bytes(&reference);
        reference
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Ok(self.apply(
            "upsert-bibliography-reference",
            "add bibliography reference",
            OperationKind::UpsertBibliographyReference { reference },
        ))
    }

    pub fn delete_bibliography_reference(
        &mut self,
        reference_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let reference_id = parse_id(reference_id.as_ref())?;
        let Some(reference) = self
            .document
            .citation_database
            .references
            .iter()
            .find(|reference| reference.id == reference_id && !reference.deleted)
            .cloned()
        else {
            return Err(AppApiError::NotFound(format!(
                "bibliography reference {reference_id} was not found"
            )));
        };
        self.apply(
            "delete-bibliography-reference",
            "delete citation reference",
            OperationKind::DeleteBibliographyReference {
                reference_id: reference.id.clone(),
                revision: self.next_seq,
            },
        );

        let affected: Vec<_> = self
            .document
            .citation_database
            .citations
            .iter()
            .filter(|citation| {
                !citation.deleted
                    && citation
                        .items
                        .iter()
                        .any(|item| item.reference_id == reference.id)
            })
            .cloned()
            .collect();
        for mut citation in affected {
            let citation_id = citation.id.clone();
            citation.revision = self.next_seq;
            citation.rendered_cache =
                render_citation_cache(&self.document.citation_database, &citation);
            self.apply(
                "upsert-citation-group",
                "rerender citation group",
                OperationKind::UpsertCitationGroup { citation },
            );
            refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        }
        Ok(self.document())
    }

    pub fn restore_bibliography_reference(
        &mut self,
        reference_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let reference_id = parse_id(reference_id.as_ref())?;
        let Some(mut reference) = self
            .document
            .citation_database
            .references
            .iter()
            .find(|reference| reference.id == reference_id && reference.deleted)
            .cloned()
        else {
            return Err(AppApiError::NotFound(format!(
                "deleted bibliography reference {reference_id} was not found"
            )));
        };
        reference.deleted = false;
        reference.revision = self.next_seq;
        reference
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        self.apply(
            "upsert-bibliography-reference",
            "restore citation reference",
            OperationKind::UpsertBibliographyReference {
                reference: reference.clone(),
            },
        );

        let affected: Vec<_> = self
            .document
            .citation_database
            .citations
            .iter()
            .filter(|citation| {
                !citation.deleted
                    && citation
                        .items
                        .iter()
                        .any(|item| item.reference_id == reference.id)
            })
            .cloned()
            .collect();
        for mut citation in affected {
            let citation_id = citation.id.clone();
            citation.revision = self.next_seq;
            citation.rendered_cache =
                render_citation_cache(&self.document.citation_database, &citation);
            self.apply(
                "upsert-citation-group",
                "rerender citation group",
                OperationKind::UpsertCitationGroup { citation },
            );
            refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        }
        Ok(self.document())
    }

    pub fn delete_citation_group(
        &mut self,
        citation_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let citation_id = parse_id(citation_id.as_ref())?;
        if self
            .document
            .citation_database
            .citations
            .iter()
            .all(|citation| citation.id != citation_id || citation.deleted)
        {
            return Err(AppApiError::NotFound(format!(
                "citation group {citation_id} was not found"
            )));
        }
        self.apply(
            "delete-citation-group",
            "delete citation group",
            OperationKind::DeleteCitationGroup {
                citation_id: citation_id.clone(),
                revision: self.next_seq,
            },
        );
        refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        Ok(self.document())
    }

    pub fn restore_citation_group(
        &mut self,
        citation_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let citation_id = parse_id(citation_id.as_ref())?;
        let Some(mut citation) = self
            .document
            .citation_database
            .citations
            .iter()
            .find(|citation| citation.id == citation_id && citation.deleted)
            .cloned()
        else {
            return Err(AppApiError::NotFound(format!(
                "deleted citation group {citation_id} was not found"
            )));
        };
        citation.deleted = false;
        citation.revision = self.next_seq;
        citation.rendered_cache =
            render_citation_cache(&self.document.citation_database, &citation);
        self.apply(
            "upsert-citation-group",
            "restore citation group",
            OperationKind::UpsertCitationGroup {
                citation: citation.clone(),
            },
        );
        refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        Ok(self.document())
    }

    fn rerender_all_citation_groups(&mut self) {
        let affected = self
            .document
            .citation_database
            .citations
            .iter()
            .filter(|citation| !citation.deleted)
            .cloned()
            .collect::<Vec<_>>();
        for mut citation in affected {
            let citation_id = citation.id.clone();
            citation.revision = self.next_seq;
            citation.rendered_cache =
                render_citation_cache(&self.document.citation_database, &citation);
            self.apply(
                "upsert-citation-group",
                "rerender citation group",
                OperationKind::UpsertCitationGroup { citation },
            );
            refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_footnote_citation_after_is_one_rust_owned_workflow() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Citations");
        app.document.blocks.clear();
        app.document.blocks.push(Block::paragraph("Body"));
        app.add_bibliography_reference("Source", vec!["Author".to_string()], None, None, None)
            .unwrap();
        let block_id = app.document.blocks[0].id.to_string();
        let inline_id = inline_id(&app.document.blocks[0].content[0]).to_string();
        let reference_id = app.document.citation_database.references[0].id.to_string();

        app.insert_footnote_citation_after(
            block_id,
            Some(inline_id),
            vec![AppCitationItem {
                reference_id,
                locator: Some("12".to_string()),
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
        )
        .unwrap();

        let footnote_id = app.document.footnotes[0].id.clone();
        assert!(app.document.blocks[0]
            .content
            .iter()
            .any(|inline| matches!(inline, Inline::FootnoteRef { footnote_id: id, .. } if id == &footnote_id)));
        assert!(app
            .document
            .citation_database
            .citations
            .iter()
            .any(|citation| {
                matches!(
                    &citation.placement,
                    CitationPlacement::Footnote { footnote_id: id } if id == &footnote_id
                )
            }));
    }

    #[test]
    fn accept_and_reject_all_suggestions_are_rust_owned_batches() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Suggestions");
        app.add_suggestion("Ada", "first").unwrap();
        app.add_suggestion("Ada", "second").unwrap();

        app.accept_all_suggestions("Reviewer").unwrap();

        assert!(app
            .document
            .suggestions
            .iter()
            .all(|suggestion| suggestion.state == SuggestionState::Accepted));

        app.add_suggestion("Ada", "third").unwrap();
        app.reject_all_suggestions("Reviewer").unwrap();

        assert!(app.document.suggestions.iter().any(|suggestion| {
            suggestion.state == SuggestionState::Accepted
                && suggestion
                    .provenance
                    .contains(&"accepted-by:Reviewer".to_string())
        }));
        assert!(app.document.suggestions.iter().any(|suggestion| {
            suggestion.state == SuggestionState::Rejected
                && suggestion
                    .provenance
                    .contains(&"rejected-by:Reviewer".to_string())
        }));
    }
}
