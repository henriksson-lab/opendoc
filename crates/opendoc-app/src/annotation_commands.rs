use super::*;

impl OpenDocApp {
    pub fn add_sample_citation(&mut self) -> Result<AppDocument, AppApiError> {
        let reference_id = StableId::parse("ref-doe-2020").expect("static reference id");
        let citation_id = StableId::parse("cite-intro").expect("static citation id");
        self.apply(
            "upsert-bibliography-reference",
            "sample citation reference",
            OperationKind::UpsertBibliographyReference {
                reference: BibliographyReference {
                    id: reference_id.clone(),
                    revision: self.next_envelope_seq,
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
        )?;
        self.apply(
            "upsert-citation-group",
            "sample citation group",
            OperationKind::UpsertCitationGroup {
                citation: CitationGroup {
                    id: citation_id.clone(),
                    revision: self.next_envelope_seq,
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
        )?;
        self.apply(
            "insert-block",
            "citation label paragraph",
            OperationKind::InsertBlock {
                position: InsertPosition::after_or_last(
                    self.document.blocks.last().map(|block| block.id.clone()),
                ),
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
            revision: self.next_envelope_seq,
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
        )?;

        self.apply(
            "insert-inline",
            "citation label",
            OperationKind::InsertInline {
                block_id,
                position: InsertPosition::after_or_last(after),
                inline: Inline::Citation {
                    id: StableId::new("citation-label"),
                    citation_id,
                    rendered_cache: None,
                },
            },
        )
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
            revision: self.next_envelope_seq,
            items,
            placement: CitationPlacement::Footnote { footnote_id },
            rendered_cache: None,
            deleted: false,
        };
        let rendered = render_citation_cache(&self.document.citation_database, &citation);
        self.apply(
            "upsert-citation-group",
            "footnote citation group",
            OperationKind::UpsertCitationGroup {
                citation: CitationGroup {
                    rendered_cache: rendered,
                    ..citation
                },
            },
        )
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
            revision: self.next_envelope_seq,
            items,
            placement: CitationPlacement::Footnote {
                footnote_id: footnote_id.clone(),
            },
            rendered_cache: None,
            deleted: false,
        };
        let rendered = render_citation_cache(&self.document.citation_database, &citation);
        self.apply_batch(vec![
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
                "insert-inline",
                "insert footnote reference",
                OperationKind::InsertInline {
                    block_id,
                    position: InsertPosition::after_or_last(after),
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
        ])
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
        )?;
        self.rerender_all_citation_groups()?;
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
        citation.revision = self.next_envelope_seq;
        citation.items = items;
        citation.rendered_cache =
            render_citation_cache(&self.document.citation_database, &citation);
        self.apply(
            "upsert-citation-group",
            "update citation group items",
            OperationKind::UpsertCitationGroup {
                citation: citation.clone(),
            },
        )?;
        refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        Ok(self.document())
    }

    /// The author a new annotation is written under.
    ///
    /// In a joined collaboration session there is exactly one answer: the
    /// subject the service attested in its welcome frame. The command argument
    /// is not an input to it. The service refuses any batch whose comment or
    /// suggestion author is not the session subject — it cannot rewrite the
    /// field, because that would make its copy of the operation differ from the
    /// client's under one id — so a name a caller supplied could only ever
    /// produce an annotation the service throws away and a session that pays
    /// three resynchronisation attempts for it.
    ///
    /// With no session there is no attested identity at all, so the caller's
    /// name is the only one there is. That is a deliberate choice, not an
    /// oversight: a local document has nothing to bind to, and ADR 0004 already
    /// says a local runtime's permissions are advisory. It does mean a local
    /// document still carries a caller-asserted author into signed state, and
    /// that is irreducible without an identity to assert against.
    fn annotation_author(
        &self,
        requested: impl Into<String>,
        what: &str,
    ) -> Result<String, AppApiError> {
        match self.service_session() {
            Some(session) => normalize_source_author(session.subject.clone(), what),
            None => normalize_source_author(requested.into(), what),
        }
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
        let author = self.annotation_author(author, "comment author")?;
        let body = normalize_source_text(body.into(), "comment body")?;
        self.apply(
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
                        created_at_ms: self.next_envelope_seq,
                        deleted: false,
                    }],
                    state: opendoc_core::CommentThreadState::Open,
                    resolved_by: None,
                    resolved_at_ms: None,
                    action_assignee: None,
                    action_due_at_ms: None,
                    action_completed_by: None,
                    action_completed_at_ms: None,
                    reactions: Vec::new(),
                    deleted: false,
                },
            },
        )
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
        let author = self.annotation_author(author, "comment author")?;
        let body = normalize_source_text(body.into(), "comment body")?;
        self.apply(
            "add-comment-reply",
            "comment reply",
            OperationKind::AddCommentReply {
                thread_id,
                comment: Comment {
                    id: StableId::new("comment"),
                    author,
                    body: vec![Inline::text(body)],
                    created_at_ms: self.next_envelope_seq,
                    deleted: false,
                },
            },
        )
    }

    pub fn add_suggestion(
        &mut self,
        author: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = self.annotation_author(author, "suggestion author")?;
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
        self.apply(
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
        )
    }

    pub fn add_text_range_suggestion(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        author: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = self.annotation_author(author, "suggestion author")?;
        let text = normalize_source_text(text.into(), "insert suggestion content")?;
        let anchor =
            Anchor::TextRange(self.parse_existing_text_range(start_inline_id, end_inline_id)?);
        self.apply(
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
        )
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
        let author = self.annotation_author(author, "suggestion author")?;
        let text = normalize_source_text(text.into(), "insert suggestion content")?;
        self.apply(
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
        )
    }

    /// Propose deletion of one exact block.  The proposal stores the stable
    /// identity rather than its current index, so it cannot drift to a block
    /// another collaborator inserted beside it.
    pub fn add_block_delete_suggestion(
        &mut self,
        block_id: impl AsRef<str>,
        author: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        if !block_tree_contains_id(&self.document.blocks, &block_id) {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        }
        let author = self.annotation_author(author, "suggestion author")?;
        self.apply(
            "add-suggestion",
            "block delete suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::BlockDelete { block_id },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        )
    }

    /// Propose a new plain paragraph immediately after this exact sibling.
    /// The target is retained as an identity; a concurrent deletion rejects
    /// the proposal rather than moving it to the end of another container.
    pub fn add_block_insert_suggestion(
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
        let author = self.annotation_author(author, "suggestion author")?;
        // Enter at the end of a paragraph proposes exactly this empty
        // paragraph. Whitespace-only content remains invalid authored prose;
        // an empty payload is the one structural gesture with no text yet.
        let text = text.into();
        let text = if text.is_empty() {
            text
        } else {
            normalize_source_text(text, "structural suggestion paragraph")?
        };
        self.apply(
            "add-suggestion",
            "block insert suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::BlockInsert {
                        position: opendoc_core::InsertPosition::After(block_id),
                        block: Block::paragraph(text),
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        )
    }

    /// Propose replacing precisely this paragraph.  Rich/table/image payloads
    /// intentionally require their own review semantics and are not accepted
    /// through this text-only command.
    pub fn add_block_replace_suggestion(
        &mut self,
        block_id: impl AsRef<str>,
        author: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        let Some(target) = find_block_in_blocks(&self.document.blocks, &block_id) else {
            return Err(AppApiError::NotFound(format!(
                "block {block_id} was not found"
            )));
        };
        if !matches!(target.kind, BlockKind::Paragraph)
            || target
                .content
                .iter()
                .any(|item| !matches!(item, Inline::Text { .. }))
        {
            return Err(AppApiError::Format(
                "structural replacement suggestions currently support plain paragraphs only"
                    .to_string(),
            ));
        }
        let author = self.annotation_author(author, "suggestion author")?;
        let text = normalize_source_text(text.into(), "structural suggestion paragraph")?;
        let expected = target.clone();
        let mut replacement = Block::paragraph(text);
        replacement.id = block_id.clone();
        replacement.properties = target.properties.clone();
        self.apply(
            "add-suggestion",
            "block replacement suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::BlockReplace {
                        block_id,
                        expected: Box::new(expected),
                        replacement: Box::new(replacement),
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        )
    }

    pub fn add_delete_suggestion(
        &mut self,
        author: impl Into<String>,
        inline_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let author = self.annotation_author(author, "suggestion author")?;
        let inline_id = parse_id(inline_id.as_ref())?;
        if find_inline_in_blocks(&self.document.blocks, &inline_id).is_none() {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        }
        self.apply(
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
        )
    }

    pub fn add_text_range_delete_suggestion(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        author: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = self.annotation_author(author, "suggestion author")?;
        let range = self.parse_existing_text_range(start_inline_id, end_inline_id)?;
        self.apply(
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
        )
    }

    pub fn add_format_suggestion(
        &mut self,
        author: impl Into<String>,
        inline_id: impl AsRef<str>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = self.annotation_author(author, "suggestion author")?;
        let inline_id = parse_id(inline_id.as_ref())?;
        if find_inline_in_blocks(&self.document.blocks, &inline_id).is_none() {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        }
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_payload(&kind, value.as_deref())?;
        self.apply(
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
        )
    }

    pub fn add_text_range_format_suggestion(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        author: impl Into<String>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = self.annotation_author(author, "suggestion author")?;
        let range = self.parse_existing_text_range(start_inline_id, end_inline_id)?;
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_payload(&kind, value.as_deref())?;
        self.apply(
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
        )
    }

    pub fn add_text_range_format_removal_suggestion(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        author: impl Into<String>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = self.annotation_author(author, "suggestion author")?;
        let range = self.parse_existing_text_range(start_inline_id, end_inline_id)?;
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_removal_payload(&kind, value.as_deref())?;
        self.apply(
            "add-suggestion",
            "text range format removal suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::FormatRemove { range, kind, value },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        )
    }

    /// Propose replacing one value-bearing mark across complete source runs.
    /// The retained old value is a compare-and-set precondition at review.
    pub fn add_text_range_format_replacement_suggestion(
        &mut self,
        start_inline_id: impl AsRef<str>,
        end_inline_id: impl AsRef<str>,
        author: impl Into<String>,
        mark_kind: impl AsRef<str>,
        expected_value: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = self.annotation_author(author, "suggestion author")?;
        let range = self.parse_existing_text_range(start_inline_id, end_inline_id)?;
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        let expected_value = expected_value.into();
        let value = value.into();
        validate_format_replacement_payload(&kind, &expected_value, &value)?;
        let ids = app_editable_inline_ids(&AppDocument::from_core(&self.document).blocks);
        let start = ids
            .iter()
            .position(|id| id == &range.start.to_string())
            .ok_or_else(|| {
                AppApiError::NotFound(format!("inline {} was not found", range.start))
            })?;
        let end = ids
            .iter()
            .position(|id| id == &range.end.to_string())
            .ok_or_else(|| AppApiError::NotFound(format!("inline {} was not found", range.end)))?;
        for id in &ids[start.min(end)..=start.max(end)] {
            let id = parse_id(id)?;
            let Some(inline) = find_inline_in_blocks(&self.document.blocks, &id) else {
                return Err(AppApiError::NotFound(format!("inline {id} was not found")));
            };
            let marks = match inline {
                Inline::Text { marks, .. } | Inline::Link { marks, .. } => marks,
                _ => {
                    return Err(AppApiError::Format(format!(
                        "inline {id} is not editable text"
                    )))
                }
            };
            if !matches!(
                marks.iter().filter(|mark| mark.kind == kind).collect::<Vec<_>>().as_slice(),
                [mark] if mark.value.as_deref() == Some(expected_value.as_str())
            ) {
                return Err(AppApiError::Conflict(format!(
                    "inline {id} no longer has the expected format value"
                )));
            }
        }
        self.apply(
            "add-suggestion",
            "text range format replacement suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::FormatReplace {
                        range,
                        kind,
                        expected_value,
                        value,
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        )
    }

    /// Propose an atomic link change on one entire text run.  This never
    /// mutates the source inline: the source href is retained as a precondition
    /// for a later reviewer decision.
    pub fn add_link_change_suggestion(
        &mut self,
        inline_id: impl AsRef<str>,
        author: impl Into<String>,
        href: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let author = self.annotation_author(author, "suggestion author")?;
        let inline_id = parse_id(inline_id.as_ref())?;
        let source = find_inline_in_blocks(&self.document.blocks, &inline_id)
            .ok_or_else(|| AppApiError::NotFound(format!("inline {inline_id} was not found")))?;
        let expected_href = match source {
            Inline::Text { .. } => None,
            Inline::Link { href, .. } => Some(href.clone()),
            _ => {
                return Err(AppApiError::Format(format!(
                    "inline {inline_id} is not text or a link and cannot receive a link proposal"
                )))
            }
        };
        if href
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.trim() != value)
        {
            return Err(AppApiError::Format(
                "link suggestion href is empty or has surrounding whitespace".to_string(),
            ));
        }
        if expected_href == href {
            return Err(AppApiError::Conflict(
                "link suggestion does not change the current href".to_string(),
            ));
        }
        self.apply(
            "add-suggestion",
            "link change suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::LinkChange {
                        inline_id,
                        expected_href,
                        href,
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        )
    }

    /// Propose one non-list paragraph-style transition.  The current kind is
    /// recorded as an acceptance precondition; source text and block
    /// properties are deliberately never copied into a replacement payload.
    pub fn add_paragraph_style_suggestion(
        &mut self,
        block_id: impl AsRef<str>,
        author: impl Into<String>,
        proposed: ParagraphStyle,
    ) -> Result<AppDocument, AppApiError> {
        proposed
            .validate()
            .map_err(|error| AppApiError::Format(error.to_string()))?;
        let block_id = parse_id(block_id.as_ref())?;
        let source = find_block_in_blocks(&self.document.blocks, &block_id)
            .ok_or_else(|| AppApiError::NotFound(format!("block {block_id} was not found")))?;
        let expected = match source.kind {
            BlockKind::Paragraph => ParagraphStyle::Paragraph,
            BlockKind::Title => ParagraphStyle::Title,
            BlockKind::Subtitle => ParagraphStyle::Subtitle,
            BlockKind::Heading { level } => ParagraphStyle::Heading { level },
            BlockKind::ListItem { .. } => {
                return Err(AppApiError::Format(format!(
                "block {block_id} is a list item; list conversions need list-run review semantics"
            )))
            }
            _ => {
                return Err(AppApiError::Format(format!(
                "block {block_id} is not an eligible text block for a paragraph style suggestion"
            )))
            }
        };
        if expected == proposed {
            return Err(AppApiError::Conflict(
                "paragraph style suggestion does not change the current style".to_string(),
            ));
        }
        let author = self.annotation_author(author, "suggestion author")?;
        self.apply(
            "add-suggestion",
            "paragraph style suggestion",
            OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::new("suggestion"),
                    author,
                    kind: SuggestionKind::ParagraphStyleChange {
                        block_id,
                        expected,
                        proposed,
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
        )
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
        self.apply(
            "update-suggestion",
            "update suggestion",
            OperationKind::UpdateSuggestionInsertContent {
                suggestion_id,
                content: vec![Inline::text(text)],
            },
        )
    }

    pub fn update_inline_text(
        &mut self,
        inline_id: impl AsRef<str>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "update-inline-text",
            "inline text edit",
            OperationKind::UpdateInlineText {
                inline_id: parse_id(inline_id.as_ref())?,
                text: text.into(),
            },
        )
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
        self.apply(
            "update-mention-label",
            "mention label edit",
            OperationKind::UpdateMentionLabel { inline_id, label },
        )
    }

    pub fn select_dropdown_option(
        &mut self,
        inline_id: impl AsRef<str>,
        option_id: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let inline_id = parse_id(inline_id.as_ref())?;
        let option_id = option_id.into();
        let Some(inline) = find_inline_in_blocks(&self.document.blocks, &inline_id) else {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        };
        let Inline::Dropdown { options, .. } = inline else {
            return Err(AppApiError::Format(format!(
                "inline {inline_id} is not a dropdown"
            )));
        };
        if !options.iter().any(|option| option.id == option_id) {
            return Err(AppApiError::Format(format!(
                "dropdown {inline_id} has no option {option_id}"
            )));
        }
        self.apply(
            "select-dropdown-option",
            "dropdown selection",
            OperationKind::SelectDropdownOption {
                inline_id,
                option_id,
            },
        )
    }

    /// Change a date chip as one atomic calendar value; it is not a text-run
    /// edit because partial dates have no portable meaning.
    pub fn update_date_chip(
        &mut self,
        inline_id: impl AsRef<str>,
        date: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let inline_id = parse_id(inline_id.as_ref())?;
        let date = date.into();
        Inline::DateChip {
            id: inline_id.clone(),
            date: date.clone(),
        }
        .validate()
        .map_err(|err| AppApiError::Format(err.to_string()))?;
        let Some(inline) = find_inline_in_blocks(&self.document.blocks, &inline_id) else {
            return Err(AppApiError::NotFound(format!(
                "inline {inline_id} was not found"
            )));
        };
        if !matches!(inline, Inline::DateChip { .. }) {
            return Err(AppApiError::Format(format!(
                "inline {inline_id} is not a date chip"
            )));
        }
        self.apply(
            "update-date-chip",
            "date chip edit",
            OperationKind::UpdateDateChip { inline_id, date },
        )
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
        self.apply(
            "update-link-href",
            "update link target",
            OperationKind::UpdateLinkHref { inline_id, href },
        )
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
        self.apply(
            "update-inline-equation-source",
            "inline equation edit",
            OperationKind::UpdateInlineEquationSource { inline_id, source },
        )
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
        self.apply(
            "insert-inline",
            "insert inline text",
            OperationKind::InsertInline {
                block_id,
                position: InsertPosition::after_or_last(after),
                inline: Inline::text(text),
            },
        )
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
        self.apply(
            "insert-inline",
            "insert link",
            OperationKind::InsertInline {
                block_id,
                position: InsertPosition::after_or_last(after),
                inline: Inline::Link {
                    id: StableId::new("link"),
                    text,
                    href,
                    marks: Vec::new(),
                },
            },
        )
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
        self.apply(
            "delete-inline",
            "delete inline",
            OperationKind::DeleteInline { inline_id },
        )
    }

    pub fn delete_comment_thread(
        &mut self,
        thread_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "delete-comment-thread",
            "delete comment thread",
            OperationKind::DeleteCommentThread {
                thread_id: parse_id(thread_id.as_ref())?,
            },
        )
    }

    pub fn resolve_comment_thread(
        &mut self,
        thread_id: impl AsRef<str>,
        resolved_by: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let thread_id = parse_id(thread_id.as_ref())?;
        let resolved_by = self.annotation_author(resolved_by, "comment resolver")?;
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
        self.apply(
            "resolve-comment-thread",
            "resolve comment thread",
            OperationKind::ResolveCommentThread {
                thread_id,
                resolved_by,
                resolved_at_ms: self.next_envelope_seq,
            },
        )
    }

    pub fn reopen_comment_thread(
        &mut self,
        thread_id: impl AsRef<str>,
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
        self.apply(
            "reopen-comment-thread",
            "reopen comment thread",
            OperationKind::ReopenCommentThread { thread_id },
        )
    }

    /// Set (or clear) a document-local action-item assignment.  The supplied
    /// names are display labels, not claims about an authenticated account.
    pub fn set_comment_thread_action(
        &mut self,
        thread_id: impl AsRef<str>,
        assignee: Option<String>,
        due_at_ms: Option<u64>,
        completed: bool,
        completed_by: Option<String>,
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
        let assignee = assignee
            .map(|name| self.annotation_author(name, "comment action assignee"))
            .transpose()?;
        if due_at_ms.is_some() && assignee.is_none() {
            return Err(AppApiError::Format(
                "a comment action due date requires an assignee".to_string(),
            ));
        }
        if completed && assignee.is_none() {
            return Err(AppApiError::Format(
                "a completed comment action requires an assignee".to_string(),
            ));
        }
        let completed_by = if completed {
            Some(self.annotation_author(
                completed_by.unwrap_or_else(|| "Unknown".to_string()),
                "comment action completer",
            )?)
        } else {
            None
        };
        self.apply(
            "set-comment-thread-action",
            "comment action item",
            OperationKind::SetCommentThreadAction {
                thread_id,
                assignee,
                due_at_ms,
                completed_by,
                completed_at_ms: completed.then_some(self.next_envelope_seq),
            },
        )
    }

    /// Records whether this author has one document-local emoji reaction on a
    /// discussion.  In a joined session `annotation_author` deliberately
    /// substitutes the authenticated subject, matching comment authorship.
    pub fn set_comment_thread_reaction(
        &mut self,
        thread_id: impl AsRef<str>,
        emoji: impl Into<String>,
        actor: impl Into<String>,
        present: bool,
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
        let emoji = emoji.into();
        opendoc_core::validate_comment_reaction_emoji(&emoji)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let actor = self.annotation_author(actor, "comment reaction actor")?;
        self.apply(
            "set-comment-thread-reaction",
            "comment reaction",
            OperationKind::SetCommentThreadReaction {
                thread_id,
                emoji,
                actor,
                present,
            },
        )
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
        self.apply(
            "restore-comment-thread",
            "restore comment thread",
            OperationKind::RestoreCommentThread { thread_id },
        )
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
        self.apply(
            "delete-comment",
            "delete comment",
            OperationKind::DeleteComment {
                thread_id,
                comment_id,
            },
        )
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
        self.apply(
            "restore-comment",
            "restore comment",
            OperationKind::RestoreComment {
                thread_id,
                comment_id,
            },
        )
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
        self.apply(
            "update-comment",
            "update comment",
            OperationKind::UpdateCommentBody {
                thread_id,
                comment_id,
                body: vec![Inline::text(body)],
            },
        )
    }

    pub fn accept_suggestion(
        &mut self,
        suggestion_id: impl AsRef<str>,
        accepted_by: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let accepted_by = normalize_source_author(accepted_by.into(), "suggestion accepted by")?;
        self.apply(
            "accept-suggestion",
            "accept suggestion",
            OperationKind::AcceptSuggestion {
                suggestion_id: parse_id(suggestion_id.as_ref())?,
                accepted_by,
            },
        )
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
        self.apply_batch(
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
        )
    }

    pub fn reject_suggestion(
        &mut self,
        suggestion_id: impl AsRef<str>,
        rejected_by: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let rejected_by = normalize_source_author(rejected_by.into(), "suggestion rejected by")?;
        self.apply(
            "reject-suggestion",
            "reject suggestion",
            OperationKind::RejectSuggestion {
                suggestion_id: parse_id(suggestion_id.as_ref())?,
                rejected_by,
            },
        )
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
        self.apply_batch(
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
        )
    }

    pub fn add_text_mark(
        &mut self,
        inline_id: impl AsRef<str>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_payload(&kind, value.as_deref())?;
        self.apply(
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
        )
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
        self.apply(
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
        )
    }

    pub fn remove_text_mark(
        &mut self,
        inline_id: impl AsRef<str>,
        mark_kind: impl AsRef<str>,
        value: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        validate_mark_removal_payload(&kind, value.as_deref())?;
        self.apply(
            "remove-mark",
            "remove inline format",
            OperationKind::RemoveMark {
                text_id: parse_id(inline_id.as_ref())?,
                kind,
                value,
            },
        )
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
        self.apply_batch(operations)
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
        self.apply(
            "update-block-equation-source",
            "block equation edit",
            OperationKind::UpdateBlockEquationSource {
                block_id: parse_id(block_id.as_ref())?,
                source,
            },
        )
    }

    pub fn update_image_alt_text(
        &mut self,
        block_id: impl AsRef<str>,
        alt_text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "update-image-alt-text",
            "image alt text edit",
            OperationKind::UpdateImageAltText {
                block_id: parse_id(block_id.as_ref())?,
                alt_text: alt_text.into(),
            },
        )
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
        self.apply(
            "update-image-blob-hash",
            "image blob replacement",
            OperationKind::UpdateImageBlobHash {
                block_id: parse_id(block_id.as_ref())?,
                blob_hash: hash,
            },
        )
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
        reference.revision = self.next_envelope_seq;
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
        )?;

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
            citation.revision = self.next_envelope_seq;
            citation.rendered_cache =
                render_citation_cache(&self.document.citation_database, &citation);
            self.apply(
                "upsert-citation-group",
                "rerender citation group",
                OperationKind::UpsertCitationGroup { citation },
            )?;
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
            revision: self.next_envelope_seq,
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
        self.apply(
            "upsert-bibliography-reference",
            "add bibliography reference",
            OperationKind::UpsertBibliographyReference { reference },
        )
    }

    /// Imports a local BibTeX/BibLaTeX library without collapsing it into the
    /// desktop's former five-field preview.  Every stored reference keeps the
    /// exact uploaded UTF-8 source and uses the parser's citation key as its
    /// stable identity, so the CSL renderer can recover container, editor,
    /// pages, publisher and other source metadata later.
    pub fn import_bibtex(&mut self, source: impl Into<String>) -> Result<AppDocument, AppApiError> {
        const MAX_BIBTEX_BYTES: usize = 8 * 1024 * 1024;
        const MAX_BIBTEX_ENTRIES: usize = 10_000;

        let source = source.into();
        if source.is_empty() {
            return Err(AppApiError::Format("BibTeX source is empty".to_string()));
        }
        if source.len() > MAX_BIBTEX_BYTES {
            return Err(AppApiError::Format(format!(
                "BibTeX source exceeds the {} MiB local import limit",
                MAX_BIBTEX_BYTES / (1024 * 1024)
            )));
        }
        let entries = opendoc_citations::parse_bibtex(&source)
            .map_err(|error| AppApiError::Format(error.to_string()))?;
        if entries.len() > MAX_BIBTEX_ENTRIES {
            return Err(AppApiError::Format(format!(
                "BibTeX source has more than {MAX_BIBTEX_ENTRIES} entries"
            )));
        }

        let sources = bibtex_entry_sources(&source)?;
        let mut ids = BTreeSet::new();
        let mut operations = Vec::with_capacity(entries.len());
        for (offset, entry) in entries.iter().enumerate() {
            let id = StableId::parse(entry.key()).map_err(|error| {
                AppApiError::Format(format!(
                    "BibTeX citation key {} is invalid: {error}",
                    entry.key()
                ))
            })?;
            if !ids.insert(id.clone()) {
                return Err(AppApiError::Format(format!(
                    "BibTeX source has duplicate citation key {id}"
                )));
            }
            let entry_source = sources.get(entry.key()).ok_or_else(|| {
                AppApiError::Format(format!(
                    "BibTeX parser accepted citation key {} but its source entry could not be retained",
                    entry.key()
                ))
            })?;
            let summary = opendoc_citations::details_from_entry(entry).to_summary();
            let reference = BibliographyReference {
                id,
                // The batch mints its envelopes in gesture order.  Assigning
                // the matching revision keeps a same-key reimport a real
                // newer write instead of an accidental tie.
                revision: self.next_envelope_seq + offset as u64,
                source: CitationSource {
                    format: CitationSourceFormat::Bibtex,
                    // Keep this entry's exact source (and any library-wide
                    // string/preamble declarations it needs), not the whole
                    // library once for every reference.
                    bytes: entry_source.as_bytes().to_vec(),
                },
                summary,
                deleted: false,
            };
            reference
                .validate()
                .map_err(|error| AppApiError::Format(error.to_string()))?;
            operations.push((
                "upsert-bibliography-reference",
                "import BibTeX bibliography reference",
                OperationKind::UpsertBibliographyReference { reference },
            ));
        }
        self.apply_batch(operations)
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
                revision: self.next_envelope_seq,
            },
        )?;

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
            citation.revision = self.next_envelope_seq;
            citation.rendered_cache =
                render_citation_cache(&self.document.citation_database, &citation);
            self.apply(
                "upsert-citation-group",
                "rerender citation group",
                OperationKind::UpsertCitationGroup { citation },
            )?;
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
        reference.revision = self.next_envelope_seq;
        reference
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        self.apply(
            "upsert-bibliography-reference",
            "restore citation reference",
            OperationKind::UpsertBibliographyReference {
                reference: reference.clone(),
            },
        )?;

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
            citation.revision = self.next_envelope_seq;
            citation.rendered_cache =
                render_citation_cache(&self.document.citation_database, &citation);
            self.apply(
                "upsert-citation-group",
                "rerender citation group",
                OperationKind::UpsertCitationGroup { citation },
            )?;
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
                revision: self.next_envelope_seq,
            },
        )?;
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
        citation.revision = self.next_envelope_seq;
        citation.rendered_cache =
            render_citation_cache(&self.document.citation_database, &citation);
        self.apply(
            "upsert-citation-group",
            "restore citation group",
            OperationKind::UpsertCitationGroup {
                citation: citation.clone(),
            },
        )?;
        refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        Ok(self.document())
    }

    fn rerender_all_citation_groups(&mut self) -> Result<(), AppApiError> {
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
            citation.revision = self.next_envelope_seq;
            citation.rendered_cache =
                render_citation_cache(&self.document.citation_database, &citation);
            self.apply(
                "upsert-citation-group",
                "rerender citation group",
                OperationKind::UpsertCitationGroup { citation },
            )?;
            refresh_inline_citation_cache(&mut self.document.blocks, &citation_id);
        }
        Ok(())
    }
}

/// Extracts complete source entries after the authoritative parser has
/// accepted the library. This is retention, not a second parser: it only
/// finds balanced top-level `@type{...}` / `@type(...)` records so a document
/// does not multiply the complete selected library by its number of entries.
/// Declarations are prepended to each entry because BibTeX values may refer to
/// a preceding `@string` macro or `@preamble`.
fn bibtex_entry_sources(source: &str) -> Result<BTreeMap<String, String>, AppApiError> {
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut declarations = String::new();
    let mut entries = BTreeMap::new();
    while let Some(relative_at) = source[cursor..].find('@') {
        let at = cursor + relative_at;
        let mut kind_end = at + 1;
        while bytes
            .get(kind_end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        {
            kind_end += 1;
        }
        if kind_end == at + 1 {
            cursor = at + 1;
            continue;
        }
        let mut open = kind_end;
        while bytes.get(open).is_some_and(u8::is_ascii_whitespace) {
            open += 1;
        }
        let Some(&delimiter) = bytes.get(open) else {
            break;
        };
        let close_delimiter = match delimiter {
            b'{' => b'}',
            b'(' => b')',
            _ => {
                cursor = kind_end;
                continue;
            }
        };
        let close =
            bibtex_balanced_end(bytes, open, delimiter, close_delimiter).ok_or_else(|| {
                AppApiError::Format("BibTeX source has an unclosed delimiter".to_string())
            })?;
        let raw = &source[at..=close];
        cursor = close + 1;
        let kind = source[at + 1..kind_end].to_ascii_lowercase();
        if matches!(kind.as_str(), "string" | "preamble") {
            declarations.push_str(raw);
            declarations.push('\n');
            continue;
        }
        if kind == "comment" {
            continue;
        }
        let body = &source[open + 1..close];
        let key = body
            .split_once(',')
            .map(|(key, _)| key.trim())
            .unwrap_or("");
        if key.is_empty() {
            continue;
        }
        if entries
            .insert(key.to_string(), format!("{declarations}{raw}"))
            .is_some()
        {
            return Err(AppApiError::Format(format!(
                "BibTeX source has duplicate citation key {key}"
            )));
        }
    }
    Ok(entries)
}

fn bibtex_balanced_end(bytes: &[u8], open: usize, opening: u8, closing: u8) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = false;
    let mut index = open;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'\\' {
            index += 2;
            continue;
        }
        if byte == b'"' {
            quote = !quote;
        } else if !quote && byte == opening {
            depth += 1;
        } else if !quote && byte == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bibtex_import_is_one_local_journalled_gesture_with_raw_rich_source() {
        let source = r#"@incollection{muller2025,
  author = {Müller, Jürgen},
  title = {A rich source},
  booktitle = {Collected Works},
  editor = {Editor, Eve},
  publisher = {Open Press},
  pages = {12--18},
  year = {2025},
  doi = {10.1000/rich}
}"#;
        let mut app = OpenDocApp::new_sample();
        app.new_document("Citations");
        app.import_bibtex(source).expect("local BibTeX import");

        assert_eq!(app.document.citation_database.references.len(), 1);
        let reference = &app.document.citation_database.references[0];
        assert_eq!(reference.id.as_str(), "muller2025");
        assert_eq!(reference.source.format, CitationSourceFormat::Bibtex);
        assert_eq!(reference.source.bytes, source.as_bytes());
        assert_eq!(reference.summary.title, "A rich source");
        let details = opendoc_citations::reference_details(reference);
        assert_eq!(details.container_title.as_deref(), Some("Collected Works"));
        assert_eq!(details.publisher.as_deref(), Some("Open Press"));
        assert_eq!(details.pages.as_deref(), Some("12-18"));
        assert_eq!(
            app.operation_journal.last().unwrap().summary,
            "import BibTeX bibliography reference"
        );
        assert_eq!(
            app.operation_journal.len(),
            1,
            "the library is one undo gesture"
        );
    }

    #[test]
    fn bibtex_import_rejects_a_library_before_any_operation_is_journalled() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Citations");
        let error = app
            .import_bibtex("@book{broken, title = {unfinished")
            .unwrap_err();
        assert!(error.to_string().contains("invalid BibTeX"), "{error}");
        assert!(app.document.citation_database.references.is_empty());
        assert!(app.operation_journal.is_empty());
    }

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

    #[test]
    fn format_removal_suggestion_is_journalled_and_accepted_without_a_preview_mutation() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Suggestions");
        let inline_id = inline_id(&app.document.blocks[0].content[0]).to_string();
        app.add_text_mark(&inline_id, "bold", None).unwrap();

        app.add_text_range_format_removal_suggestion(&inline_id, &inline_id, "Ada", "bold", None)
            .unwrap();
        assert!(matches!(
            &app.document.blocks[0].content[0],
            Inline::Text { marks, .. } if marks.iter().any(|mark| mark.kind == MarkKind::Bold)
        ));
        let suggestion_id = app.document.suggestions[0].id.to_string();

        app.accept_suggestion(&suggestion_id, "Reviewer").unwrap();
        assert!(matches!(
            &app.document.blocks[0].content[0],
            Inline::Text { marks, .. } if marks.iter().all(|mark| mark.kind != MarkKind::Bold)
        ));
        assert_eq!(app.document.suggestions[0].state, SuggestionState::Accepted);
    }

    #[test]
    fn whole_inline_colour_suggestion_is_journalled_and_applies_only_when_accepted() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Suggestions");
        let inline_id = inline_id(&app.document.blocks[0].content[0]).to_string();

        app.add_text_range_format_suggestion(
            &inline_id,
            &inline_id,
            "Ada",
            "color",
            Some("#123456".to_string()),
        )
        .unwrap();
        assert!(matches!(
            &app.document.blocks[0].content[0],
            Inline::Text { marks, .. } if marks.iter().all(|mark| mark.kind != MarkKind::Color)
        ));
        let suggestion_id = app.document.suggestions[0].id.to_string();
        assert!(matches!(
            &app.document.suggestions[0].kind,
            SuggestionKind::Format { marks, .. }
                if matches!(marks.as_slice(), [Mark { kind: MarkKind::Color, value: Some(value), .. }]
                    if value == "#123456")
        ));

        app.accept_suggestion(&suggestion_id, "Reviewer").unwrap();
        assert!(matches!(
            &app.document.blocks[0].content[0],
            Inline::Text { marks, .. }
                if marks.iter().any(|mark| mark.kind == MarkKind::Color && mark.value.as_deref() == Some("#123456"))
        ));
        assert_eq!(app.document.suggestions[0].state, SuggestionState::Accepted);
    }

    #[test]
    fn whole_inline_initial_font_and_size_suggestions_are_journalled_then_applied() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Suggestions");
        let inline_id = inline_id(&app.document.blocks[0].content[0]).to_string();

        app.add_text_range_format_suggestion(
            &inline_id,
            &inline_id,
            "Ada",
            "font",
            Some("Georgia".to_string()),
        )
        .unwrap();
        let font_suggestion = app.document.suggestions[0].id.to_string();
        app.accept_suggestion(&font_suggestion, "Reviewer").unwrap();

        app.add_text_range_format_suggestion(
            &inline_id,
            &inline_id,
            "Ada",
            "size",
            Some("14".to_string()),
        )
        .unwrap();
        let size_suggestion = app.document.suggestions[1].id.to_string();
        app.accept_suggestion(&size_suggestion, "Reviewer").unwrap();

        assert!(matches!(
            &app.document.blocks[0].content[0],
            Inline::Text { marks, .. }
                if marks.iter().any(|mark| mark.kind == MarkKind::Font && mark.value.as_deref() == Some("Georgia"))
                    && marks.iter().any(|mark| mark.kind == MarkKind::Size && mark.value.as_deref() == Some("14"))
        ));
        assert!(app
            .document
            .suggestions
            .iter()
            .all(|suggestion| suggestion.state == SuggestionState::Accepted));
    }

    #[test]
    fn block_delete_suggestion_is_journalled_then_applies_only_when_accepted() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Suggestions");
        let block_id = app.document.blocks[0].id.to_string();
        app.add_block_delete_suggestion(&block_id, "Ada").unwrap();
        assert_eq!(app.document.blocks.len(), 1);
        assert!(matches!(
            app.document.suggestions[0].kind,
            SuggestionKind::BlockDelete { .. }
        ));

        let suggestion_id = app.document.suggestions[0].id.to_string();
        app.accept_suggestion(&suggestion_id, "Reviewer").unwrap();
        assert!(app.document.blocks.is_empty());
        assert_eq!(app.document.suggestions[0].state, SuggestionState::Accepted);
        assert!(app
            .operation_journal
            .iter()
            .any(|entry| entry.kind == "add-suggestion"));
    }

    #[test]
    fn block_replace_suggestion_captures_source_and_retains_paragraph_formatting() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Suggestions");
        let block_id = app.document.blocks[0].id.to_string();
        app.document.blocks[0].properties.alignment = Some(opendoc_core::Alignment::Center);
        let expected = app.document.blocks[0].clone();

        app.add_block_replace_suggestion(&block_id, "Ada", "Proposed")
            .unwrap();
        let SuggestionKind::BlockReplace {
            expected: captured,
            replacement,
            ..
        } = &app.document.suggestions[0].kind
        else {
            panic!("expected block replacement proposal")
        };
        assert_eq!(captured.as_ref(), &expected);
        assert_eq!(replacement.properties, expected.properties);
        assert_eq!(
            app.document.blocks[0], expected,
            "proposal must not edit source"
        );
    }

    #[test]
    fn paragraph_style_suggestion_is_preconditioned_and_preserves_block_contents() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Suggestions");
        let block_id = app.document.blocks[0].id.to_string();
        let original_inline_id = inline_id(&app.document.blocks[0].content[0]).to_string();
        app.add_paragraph_style_suggestion(&block_id, "Ada", ParagraphStyle::Heading { level: 2 })
            .unwrap();

        assert!(matches!(
            &app.document.suggestions[0].kind,
            SuggestionKind::ParagraphStyleChange { block_id: target, expected: ParagraphStyle::Paragraph, proposed: ParagraphStyle::Heading { level: 2 } }
                if target.to_string() == block_id
        ));
        assert!(matches!(app.document.blocks[0].kind, BlockKind::Paragraph));
        let suggestion_id = app.document.suggestions[0].id.to_string();
        app.accept_suggestion(&suggestion_id, "Reviewer").unwrap();
        assert!(matches!(
            app.document.blocks[0].kind,
            BlockKind::Heading { level: 2 }
        ));
        assert_eq!(
            inline_id(&app.document.blocks[0].content[0]).to_string(),
            original_inline_id
        );
    }

    #[test]
    fn paragraph_style_suggestion_rejects_list_targets_and_noops() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Suggestions");
        let block_id = app.document.blocks[0].id.to_string();
        assert!(matches!(
            app.add_paragraph_style_suggestion(&block_id, "Ada", ParagraphStyle::Paragraph),
            Err(AppApiError::Conflict(_))
        ));
        app.document.blocks[0].kind = BlockKind::ListItem {
            list_id: new_list_id(),
            level: 0,
            kind: ListKind::Bullet,
        };
        assert!(matches!(
            app.add_paragraph_style_suggestion(&block_id, "Ada", ParagraphStyle::Title),
            Err(AppApiError::Format(_))
        ));
    }

    #[test]
    fn empty_block_insert_suggestion_is_a_durable_enter_at_paragraph_end() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Suggestions");
        let block_id = app.document.blocks[0].id.to_string();

        app.add_block_insert_suggestion(&block_id, "Ada", "")
            .unwrap();
        assert_eq!(
            app.document.blocks.len(),
            1,
            "proposal must not edit source"
        );
        let suggestion_id = app.document.suggestions[0].id.to_string();
        assert!(matches!(
            &app.document.suggestions[0].kind,
            SuggestionKind::BlockInsert { block, .. }
                if matches!(block.content.as_slice(), [Inline::Text { text, .. }] if text.is_empty())
        ));

        app.accept_suggestion(&suggestion_id, "Reviewer").unwrap();
        assert_eq!(app.document.blocks.len(), 2);
        assert!(matches!(
            &app.document.blocks[1].content[..], [Inline::Text { text, .. }] if text.is_empty()
        ));
        assert_eq!(app.document.suggestions[0].state, SuggestionState::Accepted);
    }

    #[test]
    fn comment_action_assignment_and_completion_are_journalled() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Actions");
        let block_id = app.document.blocks[0].id.to_string();
        app.add_block_comment(block_id, "Reviewer", "Please update this")
            .unwrap();
        let thread_id = app.document.comments[0].id.to_string();

        app.set_comment_thread_action(
            &thread_id,
            Some("Ada".to_string()),
            Some(1_700_000_000_000),
            true,
            Some("Reviewer".to_string()),
        )
        .unwrap();

        let thread = &app.document.comments[0];
        assert_eq!(thread.action_assignee.as_deref(), Some("Ada"));
        assert_eq!(thread.action_due_at_ms, Some(1_700_000_000_000));
        assert_eq!(thread.action_completed_by.as_deref(), Some("Reviewer"));
        assert!(thread.action_completed_at_ms.is_some());
        assert_eq!(
            app.document().comments[0].action_completed_by.as_deref(),
            Some("Reviewer")
        );
        assert!(app.operation_journal.iter().any(|operation| {
            operation.kind == "set-comment-thread-action"
                && operation.summary == "comment action item"
        }));
    }

    #[test]
    fn completing_an_unassigned_action_is_rejected() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Actions");
        let block_id = app.document.blocks[0].id.to_string();
        app.add_block_comment(block_id, "Reviewer", "Please update this")
            .unwrap();
        let thread_id = app.document.comments[0].id.to_string();

        assert!(matches!(
            app.set_comment_thread_action(
                &thread_id,
                None,
                None,
                true,
                Some("Reviewer".to_string()),
            ),
            Err(AppApiError::Format(message))
                if message == "a completed comment action requires an assignee"
        ));
        assert!(app.document.comments[0].action_completed_by.is_none());
    }
}
