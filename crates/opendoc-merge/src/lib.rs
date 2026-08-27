use opendoc_core::{
    Anchor, BibliographyReference, Block, BlockKind, CitationGroup, CommentThread, Document,
    Inline, Mark, ModelError, ModelWarning, StableId, Suggestion, SuggestionState,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ActorId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct OperationId {
    pub actor: ActorId,
    pub seq: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Operation {
    pub id: OperationId,
    pub kind: OperationKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationKind {
    InsertBlock {
        after: Option<StableId>,
        block: Block,
    },
    InsertInline {
        block_id: StableId,
        after: Option<StableId>,
        inline: Inline,
    },
    AddMark {
        text_id: StableId,
        mark: Mark,
    },
    AddSuggestion {
        suggestion: Suggestion,
    },
    AddCommentThread {
        thread: CommentThread,
    },
    UpsertBibliographyReference {
        reference: BibliographyReference,
    },
    DeleteBibliographyReference {
        reference_id: StableId,
        revision: u64,
    },
    UpsertCitationGroup {
        citation: CitationGroup,
    },
    DeleteCitationGroup {
        citation_id: StableId,
        revision: u64,
    },
    DeleteCommentThread {
        thread_id: StableId,
    },
    UpdateInlineText {
        inline_id: StableId,
        text: String,
    },
    UpdateBlockEquationSource {
        block_id: StableId,
        source: String,
    },
    AcceptSuggestion {
        suggestion_id: StableId,
        accepted_by: String,
    },
    RejectSuggestion {
        suggestion_id: StableId,
        rejected_by: String,
    },
    DeleteInline {
        inline_id: StableId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeResult {
    pub document: Document,
    pub warnings: Vec<ModelWarning>,
}

pub fn merge_operations(
    base: &Document,
    streams: &[Vec<Operation>],
) -> Result<MergeResult, ModelError> {
    let mut ordered = BTreeMap::new();
    for stream in streams {
        for op in stream {
            ordered.insert(op.id.clone(), op.kind.clone());
        }
    }

    let mut document = base.clone();
    let mut warnings = Vec::new();
    for (_, kind) in ordered {
        apply(&mut document, &mut warnings, kind);
    }
    document.warnings.extend(warnings.clone());
    document.validate()?;
    Ok(MergeResult { document, warnings })
}

fn apply(document: &mut Document, warnings: &mut Vec<ModelWarning>, kind: OperationKind) {
    match kind {
        OperationKind::InsertBlock { after, block } => insert_block(document, after, block),
        OperationKind::InsertInline {
            block_id,
            after,
            inline,
        } => {
            if let Some(block) = document
                .blocks
                .iter_mut()
                .find(|block| block.id == block_id)
            {
                insert_inline(&mut block.content, after, inline);
            } else {
                warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("inline insert target block {block_id} was missing"),
                });
            }
        }
        OperationKind::AddMark { text_id, mark } => {
            if !add_mark(document, &text_id, mark) {
                warnings.push(ModelWarning {
                    code: "missing-text".to_string(),
                    message: format!("mark target {text_id} was missing"),
                });
            }
        }
        OperationKind::AddSuggestion { suggestion } => document.suggestions.push(suggestion),
        OperationKind::AddCommentThread { thread } => {
            if anchor_resolves(document, &thread.anchor) {
                document.comments.push(thread);
            } else {
                let mut thread = thread;
                thread.anchor =
                    nearest_block_anchor(document, "comment anchor could not be resolved");
                document.comments.push(thread);
                warnings.push(ModelWarning {
                    code: "comment-anchor-degraded".to_string(),
                    message: "comment anchor moved to nearest surviving block".to_string(),
                });
            }
        }
        OperationKind::UpsertBibliographyReference { reference } => {
            document.citation_database.upsert_reference(reference);
        }
        OperationKind::DeleteBibliographyReference {
            reference_id,
            revision,
        } => {
            if !document
                .citation_database
                .delete_reference(&reference_id, revision)
            {
                warnings.push(ModelWarning {
                    code: "missing-bibliography-reference".to_string(),
                    message: format!("bibliography reference {reference_id} was missing"),
                });
            }
        }
        OperationKind::UpsertCitationGroup { citation } => {
            document.citation_database.upsert_citation(citation);
        }
        OperationKind::DeleteCitationGroup {
            citation_id,
            revision,
        } => {
            if !document
                .citation_database
                .delete_citation(&citation_id, revision)
            {
                warnings.push(ModelWarning {
                    code: "missing-citation-group".to_string(),
                    message: format!("citation group {citation_id} was missing"),
                });
            }
        }
        OperationKind::DeleteCommentThread { thread_id } => {
            if let Some(thread) = document
                .comments
                .iter_mut()
                .find(|thread| thread.id == thread_id)
            {
                thread.deleted = true;
            } else {
                warnings.push(ModelWarning {
                    code: "missing-comment-thread".to_string(),
                    message: format!("comment thread {thread_id} was missing"),
                });
            }
        }
        OperationKind::UpdateInlineText { inline_id, text } => {
            match update_inline_text(document, &inline_id, &text) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-editable-inline".to_string(),
                    message: format!("inline {inline_id} is derived from structured state"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-inline".to_string(),
                    message: format!("inline {inline_id} was missing"),
                }),
            }
        }
        OperationKind::UpdateBlockEquationSource { block_id, source } => {
            match update_block_equation_source(&mut document.blocks, &block_id, &source) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-equation-block".to_string(),
                    message: format!("block {block_id} is not a block equation"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("block equation target {block_id} was missing"),
                }),
            }
        }
        OperationKind::AcceptSuggestion {
            suggestion_id,
            accepted_by,
        } => {
            if let Some(suggestion) = document
                .suggestions
                .iter_mut()
                .find(|item| item.id == suggestion_id)
            {
                suggestion.state = SuggestionState::Accepted;
                suggestion
                    .provenance
                    .push(format!("accepted-by:{accepted_by}"));
            } else {
                warnings.push(ModelWarning {
                    code: "missing-suggestion".to_string(),
                    message: format!("suggestion {suggestion_id} was missing"),
                });
            }
        }
        OperationKind::RejectSuggestion {
            suggestion_id,
            rejected_by,
        } => {
            if let Some(suggestion) = document
                .suggestions
                .iter_mut()
                .find(|item| item.id == suggestion_id)
            {
                suggestion.state = SuggestionState::Rejected;
                suggestion
                    .provenance
                    .push(format!("rejected-by:{rejected_by}"));
            } else {
                warnings.push(ModelWarning {
                    code: "missing-suggestion".to_string(),
                    message: format!("suggestion {suggestion_id} was missing"),
                });
            }
        }
        OperationKind::DeleteInline { inline_id } => {
            if !delete_inline(document, &inline_id) {
                warnings.push(ModelWarning {
                    code: "missing-inline".to_string(),
                    message: format!("inline {inline_id} was already absent"),
                });
            }
        }
    }
}

fn anchor_resolves(document: &Document, anchor: &Anchor) -> bool {
    match anchor {
        Anchor::Document => true,
        Anchor::NearestBlock { block_id, .. } => {
            document.blocks.iter().any(|block| &block.id == block_id)
        }
        Anchor::TextRange(range) => {
            let mut found_start = false;
            let mut found_end = false;
            for block in &document.blocks {
                for inline in &block.content {
                    let id = inline_id(inline);
                    found_start |= id == &range.start;
                    found_end |= id == &range.end;
                }
            }
            found_start && found_end
        }
    }
}

fn nearest_block_anchor(document: &Document, warning: &str) -> Anchor {
    if let Some(block) = document.blocks.first() {
        Anchor::NearestBlock {
            block_id: block.id.clone(),
            warning: warning.to_string(),
        }
    } else {
        Anchor::Document
    }
}

fn insert_block(document: &mut Document, after: Option<StableId>, block: Block) {
    let insert_at = after
        .and_then(|target| document.blocks.iter().position(|item| item.id == target))
        .map(|index| index + 1)
        .unwrap_or(document.blocks.len());
    if !document.blocks.iter().any(|item| item.id == block.id) {
        document.blocks.insert(insert_at, block);
    }
}

fn insert_inline(content: &mut Vec<Inline>, after: Option<StableId>, inline: Inline) {
    let new_inline_id = inline_id(&inline).clone();
    if content.iter().any(|item| inline_id(item) == &new_inline_id) {
        return;
    }
    let insert_at = after
        .and_then(|target| content.iter().position(|item| inline_id(item) == &target))
        .map(|index| index + 1)
        .unwrap_or(content.len());
    content.insert(insert_at, inline);
}

fn add_mark(document: &mut Document, text_id: &StableId, mark: Mark) -> bool {
    add_mark_in_blocks(&mut document.blocks, text_id, mark)
}

fn add_mark_in_blocks(blocks: &mut [Block], text_id: &StableId, mark: Mark) -> bool {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text { id, marks, .. } | Inline::Link { id, marks, .. }
                    if id == text_id =>
                {
                    if !marks.contains(&mark) {
                        marks.push(mark);
                    }
                    return true;
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if add_mark_in_blocks(&mut cell.blocks, text_id, mark.clone()) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn delete_inline(document: &mut Document, inline_id_to_delete: &StableId) -> bool {
    delete_inline_in_blocks(&mut document.blocks, inline_id_to_delete)
}

fn delete_inline_in_blocks(blocks: &mut [Block], inline_id_to_delete: &StableId) -> bool {
    for block in blocks {
        let before = block.content.len();
        block
            .content
            .retain(|item| inline_id(item) != inline_id_to_delete);
        if block.content.len() != before {
            return true;
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if delete_inline_in_blocks(&mut cell.blocks, inline_id_to_delete) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn update_inline_text(
    document: &mut Document,
    inline_id_to_update: &StableId,
    text: &str,
) -> Option<bool> {
    update_inline_text_in_blocks(&mut document.blocks, inline_id_to_update, text)
}

fn update_inline_text_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    text: &str,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text {
                    id, text: value, ..
                } if id == inline_id_to_update => {
                    *value = text.to_string();
                    return Some(true);
                }
                Inline::Link {
                    id, text: value, ..
                } if id == inline_id_to_update => {
                    *value = text.to_string();
                    return Some(true);
                }
                Inline::Mention { id, label, .. } if id == inline_id_to_update => {
                    *label = text.to_string();
                    return Some(true);
                }
                Inline::Equation { id, equation } if id == inline_id_to_update => {
                    equation.source = text.to_string();
                    return Some(true);
                }
                Inline::Citation { id, .. } | Inline::FootnoteRef { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_inline_text_in_blocks(&mut cell.blocks, inline_id_to_update, text)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn update_block_equation_source(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    source: &str,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::EquationBlock { equation } => {
                    equation.source = source.to_string();
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_block_equation_source(&mut cell.blocks, block_id_to_update, source)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn inline_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::Equation { id, .. } => id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_core::{
        BibliographyReference, CitationItem, CitationPlacement, CitationSource,
        CitationSourceFormat, CitationSummary, Comment, Equation, EquationSourceFormat, MarkExpand,
        MarkKind, SuggestionKind, TextRange,
    };

    #[test]
    fn concurrent_operations_converge_independent_of_stream_order() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let block_id = block.id.clone();
        let text_id = match &block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        base.blocks.push(block);

        let op_a = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id: block_id.clone(),
                after: Some(text_id.clone()),
                inline: Inline::text(" world"),
            },
        };
        let op_b = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMark {
                text_id,
                mark: Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
        };
        let merged_ab = merge_operations(&base, &[vec![op_a.clone()], vec![op_b.clone()]]).unwrap();
        let merged_ba = merge_operations(&base, &[vec![op_b], vec![op_a]]).unwrap();
        assert_eq!(
            merged_ab.document.visible_text(),
            merged_ba.document.visible_text()
        );
        assert!(merged_ab.document.validate().is_ok());
    }

    #[test]
    fn comment_anchor_degrades_to_nearest_block_when_text_is_missing() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread = CommentThread {
            id: StableId::new("comment-thread"),
            anchor: Anchor::TextRange(TextRange {
                start: StableId::parse("missing-start").unwrap(),
                end: StableId::parse("missing-end").unwrap(),
            }),
            comments: vec![Comment {
                id: StableId::new("comment"),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        };
        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddCommentThread { thread },
            }]],
        )
        .unwrap();
        assert_eq!(result.warnings[0].code, "comment-anchor-degraded");
        assert!(matches!(
            result.document.comments[0].anchor,
            Anchor::NearestBlock { .. }
        ));
    }

    #[test]
    fn suggestions_and_atomic_equations_converge() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let block_id = block.id.clone();
        base.blocks.push(block);
        let equation = Inline::Equation {
            id: StableId::new("eq-inline"),
            equation: Equation {
                id: StableId::new("eq"),
                source_format: EquationSourceFormat::LatexLike,
                source: "E=mc^2".to_string(),
            },
        };
        let suggestion_id = StableId::new("suggestion");
        let suggestion = Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Format {
                range: TextRange {
                    start: StableId::parse("x").unwrap(),
                    end: StableId::parse("y").unwrap(),
                },
                marks: vec![Mark {
                    kind: MarkKind::Italic,
                    value: None,
                    expand: MarkExpand::Both,
                }],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        };
        let ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id,
                    after: None,
                    inline: equation,
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion { suggestion },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("c".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AcceptSuggestion {
                    suggestion_id,
                    accepted_by: "Carol".to_string(),
                },
            },
        ];
        let result = merge_operations(&base, &[ops]).unwrap();
        assert!(result.document.visible_text().contains("E=mc^2"));
        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Accepted
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["accepted-by:Carol"]
        );
    }

    #[test]
    fn suggestion_rejection_is_recorded_as_provenance() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::new("suggestion");
        let suggestion = Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text("nope")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        };

        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::AddSuggestion { suggestion },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::RejectSuggestion {
                        suggestion_id,
                        rejected_by: "Carol".to_string(),
                    },
                },
            ]],
        )
        .unwrap();

        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Rejected
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["rejected-by:Carol"]
        );
    }

    #[test]
    fn citation_labels_reference_document_local_database() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("cited ");
        let block_id = block.id.clone();
        let after = match &block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        base.blocks.push(block);
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();

        let reference = BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"id: doe-2020\ntitle: Example".to_vec(),
            },
            summary: CitationSummary {
                title: "Example".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        };
        let citation = CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: Some("42".to_string()),
                label: Some("page".to_string()),
                prefix: Some("see".to_string()),
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(see Doe 2020, 42)".to_string()),
            deleted: false,
        };
        let label = Inline::Citation {
            id: StableId::new("citation-label"),
            citation_id,
            rendered_cache: None,
        };
        let ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertBibliographyReference { reference },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertCitationGroup { citation },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("c".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id,
                    after: Some(after),
                    inline: label,
                },
            },
        ];
        let result = merge_operations(&base, &[ops]).unwrap();
        assert_eq!(result.document.visible_text(), "cited (see Doe 2020, 42)\n");
        assert_eq!(result.document.citation_database.references.len(), 1);
        assert_eq!(result.document.citation_database.citations.len(), 1);
    }

    #[test]
    fn citation_reference_updates_are_revision_ordered() {
        let base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let old = BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Old".to_vec(),
            },
            summary: CitationSummary {
                title: "Old".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        };
        let new = BibliographyReference {
            id: reference_id,
            revision: 2,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: New".to_vec(),
            },
            summary: CitationSummary {
                title: "New".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2021".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        };
        let result = merge_operations(
            &base,
            &[
                vec![Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpsertBibliographyReference { reference: new },
                }],
                vec![Operation {
                    id: OperationId {
                        actor: ActorId("b".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::UpsertBibliographyReference { reference: old },
                }],
            ],
        )
        .unwrap();
        assert_eq!(
            result.document.citation_database.references[0]
                .summary
                .title,
            "New"
        );
    }

    #[test]
    fn inline_text_updates_preserve_marks_and_reach_table_cells() {
        let mut base = Document::new("Doc");
        let paragraph = Block::paragraph("before");
        let paragraph_text_id = match &paragraph.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        let table_text = Inline::text("cell");
        let table_text_id = inline_id(&table_text).clone();
        base.blocks.push(paragraph);
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Table {
                rows: vec![opendoc_core::TableRow {
                    id: StableId::new("row"),
                    cells: vec![opendoc_core::TableCell {
                        id: StableId::new("cell"),
                        blocks: vec![Block {
                            id: StableId::new("block"),
                            kind: BlockKind::Paragraph,
                            content: vec![table_text],
                            properties: Vec::new(),
                        }],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::AddMark {
                        text_id: paragraph_text_id.clone(),
                        mark: Mark {
                            kind: MarkKind::Bold,
                            value: None,
                            expand: MarkExpand::Both,
                        },
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpdateInlineText {
                        inline_id: paragraph_text_id,
                        text: "after".to_string(),
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 3,
                    },
                    kind: OperationKind::UpdateInlineText {
                        inline_id: table_text_id,
                        text: "edited cell".to_string(),
                    },
                },
            ]],
        )
        .unwrap();

        assert!(result.document.visible_text().contains("after"));
        assert!(result.document.visible_text().contains("edited cell"));
        match &result.document.blocks[0].content[0] {
            Inline::Text { marks, .. } => assert_eq!(marks.len(), 1),
            _ => unreachable!(),
        }
    }

    #[test]
    fn citation_label_text_is_not_directly_editable() {
        let mut base = Document::new("Doc");
        let citation_id = StableId::parse("cite-intro").unwrap();
        let citation_inline = Inline::Citation {
            id: StableId::new("citation-label"),
            citation_id,
            rendered_cache: Some("(Doe 2020)".to_string()),
        };
        let citation_inline_id = inline_id(&citation_inline).clone();
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![citation_inline],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateInlineText {
                    inline_id: citation_inline_id,
                    text: "manual edit".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "non-editable-inline");
        assert_eq!(result.document.visible_text(), "(Doe 2020)\n");
    }

    #[test]
    fn block_equation_source_updates_as_atomic_block_state() {
        let mut base = Document::new("Doc");
        let block_id = StableId::new("block");
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::new("eq"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "x=1".to_string(),
                },
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateBlockEquationSource {
                    block_id,
                    source: "x=2".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "x=2\n");
    }

    #[test]
    fn deterministic_fuzz_like_replay_keeps_document_valid() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("seed"));
        let block_id = base.blocks[0].id.clone();
        let mut streams = Vec::new();
        for actor in 0..3 {
            let mut stream = Vec::new();
            for seq in 0..8 {
                stream.push(Operation {
                    id: OperationId {
                        actor: ActorId(format!("actor-{actor}")),
                        seq,
                    },
                    kind: OperationKind::InsertInline {
                        block_id: block_id.clone(),
                        after: None,
                        inline: Inline::text(format!("{actor}-{seq};")),
                    },
                });
            }
            streams.push(stream);
        }
        let result = merge_operations(&base, &streams).unwrap();
        result.document.validate().unwrap();
        assert!(result.document.visible_text().contains("2-7;"));
    }
}
