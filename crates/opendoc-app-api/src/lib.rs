use opendoc_core::{
    digest_bytes, Anchor, BibliographyReference, Block, BlockKind, CitationGroup, CitationItem,
    CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary, Comment,
    CommentThread, Document, Equation, EquationSourceFormat, Inline, Mark, MarkExpand, MarkKind,
    ModelWarning, StableId, Suggestion, SuggestionKind, SuggestionState, TextRange,
};
use opendoc_format::{decode_cbor, encode_canonical_cbor, encode_record, ManifestRecord};
use opendoc_merge::{merge_operations, ActorId, Operation, OperationId, OperationKind};
use opendoc_sign::{sign_target, verify_record, OpenSshSigner, SignatureState, Signer};
use opendoc_store::{LocalObjectStore, ObjectStore, Repository};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const SNAPSHOT_BRANCH: &str = "main";
const SNAPSHOT_KIND: &str = "opendoc.app-snapshot.v0";
const OPERATION_SEGMENT_KIND: &str = "opendoc.app-operation-segment.v0";

#[derive(Clone, Debug)]
pub struct OpenDocApp {
    document: Document,
    workbook: AppSpreadsheetWorkbook,
    signatures: Vec<opendoc_format::SignatureRecord>,
    operation_journal: Vec<AppOperationRecord>,
    saved_projection: Option<AppDocument>,
    repository_root: Option<PathBuf>,
    last_manifest: Option<String>,
    next_seq: u64,
}

impl OpenDocApp {
    pub fn new_sample() -> Self {
        let mut app = Self {
            document: Document::new("OpenDoc Prototype"),
            workbook: AppSpreadsheetWorkbook::sample(),
            signatures: Vec::new(),
            operation_journal: Vec::new(),
            saved_projection: None,
            repository_root: None,
            last_manifest: None,
            next_seq: 1,
        };
        app.document
            .blocks
            .push(Block::paragraph("OpenDoc editing surface"));
        app.add_heading("Schema coverage", 2);
        app.add_paragraph(
            "This prototype renders the current docs-like model through a TypeScript UI.",
        );
        app.add_link("Project note", "https://example.invalid/opendoc");
        app.add_equation_inline("E=mc^2");
        app.add_table();
        app.add_sample_citation();
        app.add_comment(
            "Alice",
            "Comment threads are part of signed document state.",
        );
        app.add_suggestion("Bob", "Suggested replacement text");
        app
    }

    pub fn new_document(&mut self, title: impl Into<String>) -> AppDocument {
        let title = title.into();
        let title = if title.trim().is_empty() {
            "Untitled OpenDoc".to_string()
        } else {
            title
        };
        self.document = Document::new(title);
        self.workbook = AppSpreadsheetWorkbook::sample();
        self.signatures.clear();
        self.operation_journal.clear();
        self.saved_projection = None;
        self.repository_root = None;
        self.last_manifest = None;
        self.next_seq = 1;
        self.document()
    }

    pub fn document(&self) -> AppDocument {
        let mut document = self
            .saved_projection
            .clone()
            .unwrap_or_else(|| AppDocument::from_core(&self.document));
        document.repository_root = self
            .repository_root
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());
        document.last_manifest = self.last_manifest.clone();
        document.signature_state = self.signature_state_label().to_string();
        document.signatures = self
            .signatures
            .iter()
            .map(AppSignature::from_record)
            .collect();
        document.signature = document.signatures.first().cloned();
        document.workbook = self.workbook.evaluated();
        document.operation_count = self.operation_journal.len();
        document.operations = self.operation_journal.clone();
        document
    }

    pub fn save_to_local_repository(
        &mut self,
        root: impl Into<PathBuf>,
    ) -> Result<AppDocument, AppApiError> {
        self.saved_projection = None;
        let root = root.into();
        let repo = Repository::new(LocalObjectStore::new(&root));
        let snapshot = SnapshotRecord {
            kind: SNAPSHOT_KIND.to_string(),
            document: self.snapshot_document(),
        };
        let snapshot_bytes =
            encode_canonical_cbor(&snapshot).map_err(|err| AppApiError::Format(err.to_string()))?;
        let snapshot_hash = digest_bytes("sha256", &snapshot_bytes)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        repo.store()
            .put_if_absent(&snapshot_hash, &snapshot_bytes)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let operation_segment = OperationSegmentRecord {
            kind: OPERATION_SEGMENT_KIND.to_string(),
            document_uuid: self.document.uuid.to_string(),
            branch: SNAPSHOT_BRANCH.to_string(),
            operations: self.operation_journal.clone(),
        };
        let operation_segment_bytes = encode_canonical_cbor(&operation_segment)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let operation_segment_hash = digest_bytes("sha256", &operation_segment_bytes)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        repo.store()
            .put_if_absent(&operation_segment_hash, &operation_segment_bytes)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let mut signature_hashes = Vec::new();
        for signature in &self.signatures {
            let signature_bytes = encode_record(signature);
            let signature_hash = digest_bytes("sha256", &signature_bytes)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            repo.store()
                .put_if_absent(&signature_hash, &signature_bytes)
                .map_err(|err| AppApiError::Store(err.to_string()))?;
            signature_hashes.push(signature_hash);
        }
        let expected = repo
            .store()
            .read_head(self.document.uuid.as_str(), SNAPSHOT_BRANCH)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let manifest = ManifestRecord {
            document_uuid: self.document.uuid.to_string(),
            branch: SNAPSHOT_BRANCH.to_string(),
            parent: expected.clone(),
            snapshot: snapshot_hash,
            operation_segments: vec![operation_segment_hash],
            signatures: signature_hashes,
            blobs: Vec::new(),
            created_at_ms: now_ms(),
        };
        let Some(manifest_hash) = repo
            .commit_manifest(&manifest, expected.as_ref())
            .map_err(|err| AppApiError::Store(err.to_string()))?
        else {
            return Err(AppApiError::Conflict("branch head changed".to_string()));
        };
        self.repository_root = Some(root);
        self.last_manifest = Some(manifest_hash.to_string());
        Ok(self.document())
    }

    pub fn open_saved_projection(
        &mut self,
        root: impl Into<PathBuf>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let repo = Repository::new(LocalObjectStore::new(&root));
        let head = repo
            .store()
            .read_head(document_uuid.as_ref(), SNAPSHOT_BRANCH)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("document head was not found".to_string()))?;
        let manifest = repo
            .read_manifest(&head)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("manifest object was not found".to_string()))?;
        let snapshot_bytes = repo
            .store()
            .get(&manifest.snapshot)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("snapshot object was not found".to_string()))?;
        let actual = digest_bytes(manifest.snapshot.algorithm(), &snapshot_bytes)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        if actual != manifest.snapshot {
            return Err(AppApiError::Store("snapshot hash mismatch".to_string()));
        }
        let snapshot: SnapshotRecord =
            decode_cbor(&snapshot_bytes).map_err(|err| AppApiError::Format(err.to_string()))?;
        if snapshot.kind != SNAPSHOT_KIND {
            return Err(AppApiError::Format("unsupported snapshot kind".to_string()));
        }
        self.operation_journal = self.read_operation_journal(&repo, &manifest)?;
        self.document = snapshot.document.to_core()?;
        self.workbook = snapshot.document.workbook.evaluated();
        self.signatures = self.read_signatures(&repo, &manifest)?;
        self.saved_projection = None;
        self.repository_root = Some(root);
        self.last_manifest = Some(head.to_string());
        Ok(self.document())
    }

    pub fn sign_with_openssh_private_key(
        &mut self,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.saved_projection = None;
        let backend = OpenSshSigner::from_private_key_pem(private_key_pem)
            .map_err(|err| AppApiError::Sign(err.to_string()))?;
        let public_key = backend
            .public_key_openssh()
            .map_err(|err| AppApiError::Sign(err.to_string()))?;
        let payload = self.snapshot_payload()?;
        let target =
            digest_bytes("sha256", &payload).map_err(|err| AppApiError::Model(err.to_string()))?;
        self.signatures.push(
            sign_target(
                &backend,
                target,
                self.document.title.clone(),
                Signer {
                    key_identity: public_key,
                    display_name: signer_display.into(),
                },
                &payload,
            )
            .map_err(|err| AppApiError::Sign(err.to_string()))?,
        );
        Ok(self.document())
    }

    pub fn verify_current_signature(
        &self,
        private_key_pem: impl AsRef<[u8]>,
    ) -> Result<String, AppApiError> {
        if self.signatures.is_empty() {
            return Ok("unsigned".to_string());
        };
        let backend = OpenSshSigner::from_private_key_pem(private_key_pem)
            .map_err(|err| AppApiError::Sign(err.to_string()))?;
        let payload = self.snapshot_payload()?;
        let mut saw_signed = false;
        for signature in &self.signatures {
            let state = verify_record(&backend, signature, &payload)
                .map_err(|err| AppApiError::Sign(err.to_string()))?;
            if state == SignatureState::Signed {
                saw_signed = true;
            }
        }
        if saw_signed {
            Ok("signed".to_string())
        } else {
            Ok("broken".to_string())
        }
    }

    pub fn dispatch_command(
        &mut self,
        command: &str,
        args: Value,
    ) -> Result<AppCommandResult, AppApiError> {
        match command {
            "create_document" => Ok(AppCommandResult::Document(
                self.new_document(arg_string(&args, "title")?),
            )),
            "get_document" => Ok(AppCommandResult::Document(self.document())),
            "add_paragraph" => Ok(AppCommandResult::Document(
                self.add_paragraph(arg_string(&args, "text")?),
            )),
            "add_heading" => Ok(AppCommandResult::Document(
                self.add_heading(arg_string(&args, "text")?, arg_u8(&args, "level")?),
            )),
            "add_link" => Ok(AppCommandResult::Document(
                self.add_link(arg_string(&args, "text")?, arg_string(&args, "href")?),
            )),
            "add_mention" => Ok(AppCommandResult::Document(
                self.add_mention(arg_string(&args, "label")?),
            )),
            "add_footnote_ref" => Ok(AppCommandResult::Document(self.add_footnote_ref())),
            "add_equation" => Ok(AppCommandResult::Document(
                self.add_equation_inline(arg_string(&args, "source")?),
            )),
            "add_equation_block" => Ok(AppCommandResult::Document(
                self.add_equation_block(arg_string(&args, "source")?),
            )),
            "add_list_item" => Ok(AppCommandResult::Document(self.add_list_item(
                arg_string(&args, "text")?,
                arg_u8(&args, "level")?,
                arg_bool(&args, "ordered")?,
            ))),
            "add_page_break" => Ok(AppCommandResult::Document(self.add_page_break())),
            "add_table" => Ok(AppCommandResult::Document(self.add_table())),
            "add_citation" => Ok(AppCommandResult::Document(self.add_sample_citation())),
            "add_comment" => Ok(AppCommandResult::Document(
                self.add_comment(arg_string(&args, "author")?, arg_string(&args, "body")?),
            )),
            "add_suggestion" => Ok(AppCommandResult::Document(
                self.add_suggestion(arg_string(&args, "author")?, arg_string(&args, "text")?),
            )),
            "update_inline_text" => {
                Ok(AppCommandResult::Document(self.update_inline_text(
                    arg_string(&args, "inlineId")?,
                    arg_string(&args, "text")?,
                )?))
            }
            "delete_comment_thread" => Ok(AppCommandResult::Document(
                self.delete_comment_thread(arg_string(&args, "threadId")?)?,
            )),
            "accept_suggestion" => Ok(AppCommandResult::Document(self.accept_suggestion(
                arg_string(&args, "suggestionId")?,
                arg_string(&args, "acceptedBy")?,
            )?)),
            "reject_suggestion" => Ok(AppCommandResult::Document(self.reject_suggestion(
                arg_string(&args, "suggestionId")?,
                arg_string(&args, "rejectedBy")?,
            )?)),
            "add_text_mark" => Ok(AppCommandResult::Document(self.add_text_mark(
                arg_string(&args, "inlineId")?,
                arg_string(&args, "markKind")?,
            )?)),
            "update_block_equation_source" => Ok(AppCommandResult::Document(
                self.update_block_equation_source(
                    arg_string(&args, "blockId")?,
                    arg_string(&args, "source")?,
                )?,
            )),
            "set_spreadsheet_cell" => Ok(AppCommandResult::Document(self.set_spreadsheet_cell(
                arg_string(&args, "address")?,
                arg_string(&args, "value")?,
            )?)),
            "update_bibliography_reference" => Ok(AppCommandResult::Document(
                self.update_bibliography_reference(
                    arg_string(&args, "referenceId")?,
                    arg_string(&args, "title")?,
                    arg_optional_string(&args, "issued")?,
                )?,
            )),
            "save_local_repository" => Ok(AppCommandResult::Document(
                self.save_to_local_repository(arg_string(&args, "path")?)?,
            )),
            "open_local_repository" => Ok(AppCommandResult::Document(self.open_saved_projection(
                arg_string(&args, "path")?,
                arg_string(&args, "documentUuid")?,
            )?)),
            "sign_with_openssh_private_key" => Ok(AppCommandResult::Document(
                self.sign_with_openssh_private_key(
                    arg_string(&args, "privateKeyPem")?,
                    arg_string(&args, "signerDisplay")?,
                )?,
            )),
            "verify_current_signature" => Ok(AppCommandResult::Text(
                self.verify_current_signature(arg_string(&args, "privateKeyPem")?)?,
            )),
            other => Err(AppApiError::NotFound(format!(
                "unsupported app command {other}"
            ))),
        }
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

    pub fn add_heading(&mut self, text: impl Into<String>, level: u8) -> AppDocument {
        self.apply(
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
        )
    }

    pub fn add_link(&mut self, text: impl Into<String>, href: impl Into<String>) -> AppDocument {
        self.apply(
            "insert-block",
            "link paragraph",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Paragraph,
                    content: vec![Inline::Link {
                        id: StableId::new("link"),
                        text: text.into(),
                        href: href.into(),
                        marks: Vec::new(),
                    }],
                    properties: Vec::new(),
                },
            },
        )
    }

    pub fn add_mention(&mut self, label: impl Into<String>) -> AppDocument {
        self.apply(
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
                            label: label.into(),
                        },
                    ],
                    properties: Vec::new(),
                },
            },
        )
    }

    pub fn add_footnote_ref(&mut self) -> AppDocument {
        let footnote_id = StableId::new("footnote");
        self.apply(
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
        )
    }

    pub fn add_equation_inline(&mut self, source: impl Into<String>) -> AppDocument {
        self.apply(
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
                                source: source.into(),
                            },
                        },
                    ],
                    properties: Vec::new(),
                },
            },
        )
    }

    pub fn add_equation_block(&mut self, source: impl Into<String>) -> AppDocument {
        let source = source.into();
        self.apply(
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
        )
    }

    pub fn add_list_item(
        &mut self,
        text: impl Into<String>,
        level: u8,
        ordered: bool,
    ) -> AppDocument {
        self.apply(
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
        )
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

    pub fn add_table(&mut self) -> AppDocument {
        let cell = |text: &str| opendoc_core::TableCell {
            id: StableId::new("cell"),
            blocks: vec![Block::paragraph(text)],
            properties: Vec::new(),
        };
        self.apply(
            "insert-block",
            "table",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Table {
                        rows: vec![
                            opendoc_core::TableRow {
                                id: StableId::new("row"),
                                cells: vec![cell("A1"), cell("B1")],
                            },
                            opendoc_core::TableRow {
                                id: StableId::new("row"),
                                cells: vec![cell("A2"), cell("B2")],
                            },
                        ],
                    },
                    content: Vec::new(),
                    properties: Vec::new(),
                },
            },
        )
    }

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
                    rendered_cache: Some("(see Doe 2020, 42)".to_string()),
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
                    properties: Vec::new(),
                },
            },
        )
    }

    pub fn add_comment(
        &mut self,
        author: impl Into<String>,
        body: impl Into<String>,
    ) -> AppDocument {
        let anchor = self
            .document
            .blocks
            .first()
            .map(|block| Anchor::NearestBlock {
                block_id: block.id.clone(),
                warning: String::new(),
            })
            .unwrap_or(Anchor::Document);
        self.apply(
            "add-comment-thread",
            "comment thread",
            OperationKind::AddCommentThread {
                thread: CommentThread {
                    id: StableId::new("comment-thread"),
                    anchor,
                    comments: vec![Comment {
                        id: StableId::new("comment"),
                        author: author.into(),
                        body: vec![Inline::text(body)],
                        created_at_ms: self.next_seq,
                        deleted: false,
                    }],
                    deleted: false,
                },
            },
        )
    }

    pub fn add_suggestion(
        &mut self,
        author: impl Into<String>,
        text: impl Into<String>,
    ) -> AppDocument {
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
                    author: author.into(),
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

    pub fn accept_suggestion(
        &mut self,
        suggestion_id: impl AsRef<str>,
        accepted_by: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "accept-suggestion",
            "accept suggestion",
            OperationKind::AcceptSuggestion {
                suggestion_id: parse_id(suggestion_id.as_ref())?,
                accepted_by: accepted_by.into(),
            },
        ))
    }

    pub fn reject_suggestion(
        &mut self,
        suggestion_id: impl AsRef<str>,
        rejected_by: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "reject-suggestion",
            "reject suggestion",
            OperationKind::RejectSuggestion {
                suggestion_id: parse_id(suggestion_id.as_ref())?,
                rejected_by: rejected_by.into(),
            },
        ))
    }

    pub fn add_text_mark(
        &mut self,
        inline_id: impl AsRef<str>,
        mark_kind: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let kind = parse_mark_kind(mark_kind.as_ref())?;
        Ok(self.apply(
            "add-mark",
            "format inline",
            OperationKind::AddMark {
                text_id: parse_id(inline_id.as_ref())?,
                mark: Mark {
                    kind,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
        ))
    }

    pub fn update_block_equation_source(
        &mut self,
        block_id: impl AsRef<str>,
        source: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "update-block-equation-source",
            "block equation edit",
            OperationKind::UpdateBlockEquationSource {
                block_id: parse_id(block_id.as_ref())?,
                source: source.into(),
            },
        ))
    }

    pub fn set_spreadsheet_cell(
        &mut self,
        address: impl AsRef<str>,
        value: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.signatures.clear();
        self.saved_projection = None;
        let address = normalize_cell_address(address.as_ref())?;
        let value = value.into();
        self.workbook.set_cell(&address, value);
        self.workbook = self.workbook.evaluated();
        self.push_app_operation("set-spreadsheet-cell", &format!("set {address}"));
        Ok(self.document())
    }

    pub fn update_bibliography_reference(
        &mut self,
        reference_id: impl AsRef<str>,
        title: impl Into<String>,
        issued: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let reference_id = parse_id(reference_id.as_ref())?;
        let title = title.into();
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
        let mut reference = existing;
        reference.revision = self.next_seq;
        reference.summary.title = title;
        reference.summary.issued = issued;
        reference.source.bytes = citation_source_bytes(&reference);
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
            citation.revision = self.next_seq;
            citation.rendered_cache = Some(render_citation_group(
                &self.document.citation_database,
                &citation,
            ));
            self.apply(
                "upsert-citation-group",
                "rerender citation group",
                OperationKind::UpsertCitationGroup { citation },
            );
        }
        Ok(self.document())
    }

    fn apply(&mut self, operation_kind: &str, summary: &str, kind: OperationKind) -> AppDocument {
        self.signatures.clear();
        self.saved_projection = None;
        let seq = self.next_seq;
        let op = Operation {
            id: OperationId {
                actor: ActorId("local".to_string()),
                seq,
            },
            kind,
        };
        self.next_seq += 1;
        if let Ok(result) = merge_operations(&self.document, &[vec![op]]) {
            self.document = result.document;
            self.operation_journal.push(AppOperationRecord {
                actor: "local".to_string(),
                seq,
                kind: operation_kind.to_string(),
                summary: summary.to_string(),
                created_at_ms: now_ms(),
            });
        }
        self.document()
    }

    fn push_app_operation(&mut self, operation_kind: &str, summary: &str) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.operation_journal.push(AppOperationRecord {
            actor: "local".to_string(),
            seq,
            kind: operation_kind.to_string(),
            summary: summary.to_string(),
            created_at_ms: now_ms(),
        });
    }

    fn read_operation_journal(
        &self,
        repo: &Repository<LocalObjectStore>,
        manifest: &ManifestRecord,
    ) -> Result<Vec<AppOperationRecord>, AppApiError> {
        let mut operations = Vec::new();
        for segment_hash in &manifest.operation_segments {
            let Some(bytes) = repo
                .store()
                .get(segment_hash)
                .map_err(|err| AppApiError::Store(err.to_string()))?
            else {
                continue;
            };
            let actual = digest_bytes(segment_hash.algorithm(), &bytes)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            if actual != *segment_hash {
                return Err(AppApiError::Store(
                    "operation segment hash mismatch".to_string(),
                ));
            }
            let segment: OperationSegmentRecord =
                decode_cbor(&bytes).map_err(|err| AppApiError::Format(err.to_string()))?;
            if segment.kind == OPERATION_SEGMENT_KIND {
                operations.extend(segment.operations);
            }
        }
        Ok(operations)
    }

    fn read_signatures(
        &self,
        repo: &Repository<LocalObjectStore>,
        manifest: &ManifestRecord,
    ) -> Result<Vec<opendoc_format::SignatureRecord>, AppApiError> {
        let mut signatures = Vec::new();
        for signature_hash in &manifest.signatures {
            let bytes = repo
                .store()
                .get(signature_hash)
                .map_err(|err| AppApiError::Store(err.to_string()))?
                .ok_or_else(|| {
                    AppApiError::NotFound("signature object was not found".to_string())
                })?;
            let actual = digest_bytes(signature_hash.algorithm(), &bytes)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            if actual != *signature_hash {
                return Err(AppApiError::Store("signature hash mismatch".to_string()));
            }
            signatures.push(
                opendoc_format::decode_record(&bytes)
                    .map_err(|err| AppApiError::Format(err.to_string()))?,
            );
        }
        Ok(signatures)
    }

    fn snapshot_payload(&self) -> Result<Vec<u8>, AppApiError> {
        let snapshot = SnapshotRecord {
            kind: SNAPSHOT_KIND.to_string(),
            document: self.snapshot_document(),
        };
        encode_canonical_cbor(&snapshot).map_err(|err| AppApiError::Format(err.to_string()))
    }

    fn snapshot_document(&self) -> AppDocument {
        let mut document = AppDocument::from_core(&self.document);
        document.workbook = self.workbook.evaluated();
        document
    }

    fn signature_state_label(&self) -> &'static str {
        if !self.signatures.is_empty() {
            "signed"
        } else {
            "unsigned"
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct SnapshotRecord {
    kind: String,
    document: AppDocument,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct OperationSegmentRecord {
    kind: String,
    document_uuid: String,
    branch: String,
    operations: Vec<AppOperationRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppOperationRecord {
    pub actor: String,
    pub seq: u64,
    pub kind: String,
    pub summary: String,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum AppCommandResult {
    Document(AppDocument),
    Text(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppSignature {
    pub target: String,
    pub signer: String,
    pub signer_display: String,
    pub title: String,
    pub signed_at_ms: u64,
}

impl AppSignature {
    fn from_record(record: &opendoc_format::SignatureRecord) -> Self {
        Self {
            target: record.target.to_string(),
            signer: record.signer.clone(),
            signer_display: record.signer_display.clone(),
            title: record.title.clone(),
            signed_at_ms: record.signed_at_ms,
        }
    }
}

#[derive(Debug)]
pub enum AppApiError {
    Conflict(String),
    Format(String),
    Model(String),
    NotFound(String),
    Sign(String),
    Store(String),
}

impl std::fmt::Display for AppApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AppApiError {}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn normalize_cell_address(value: &str) -> Result<String, AppApiError> {
    let value = value.trim().to_ascii_uppercase();
    let (column, row) = split_cell_address(&value);
    if column.is_empty() || row.is_empty() || row.parse::<u32>().unwrap_or(0) == 0 {
        return Err(AppApiError::Format(format!("invalid cell address {value}")));
    }
    Ok(format!("{column}{row}"))
}

fn split_cell_address(value: &str) -> (String, String) {
    let column: String = value
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect();
    let row: String = value
        .chars()
        .skip_while(|ch| ch.is_ascii_alphabetic())
        .collect();
    (column, row)
}

fn classify_cell_value(value: &str) -> &'static str {
    if value.trim().starts_with('=') {
        "formula"
    } else if value.trim().parse::<f64>().is_ok() {
        "number"
    } else if matches!(value.trim(), "TRUE" | "FALSE" | "true" | "false") {
        "bool"
    } else if value.trim().is_empty() {
        "empty"
    } else {
        "string"
    }
}

fn evaluate_formula(
    formula: &str,
    values: &BTreeMap<String, AppCell>,
) -> Result<(f64, Vec<String>), String> {
    let formula = formula.trim();
    let Some(body) = formula.strip_prefix('=') else {
        return Err("#N/A".to_string());
    };
    if let Some(range) = body
        .strip_prefix("SUM(")
        .and_then(|body| body.strip_suffix(')'))
    {
        let addresses = expand_range(range)?;
        let mut sum = 0.0;
        for address in &addresses {
            sum += numeric_cell_value(address, values)?;
        }
        return Ok((sum, addresses));
    }
    if let Ok(value) = body.parse::<f64>() {
        return Ok((value, Vec::new()));
    }
    numeric_cell_value(body, values).map(|value| (value, vec![body.to_string()]))
}

fn expand_range(range: &str) -> Result<Vec<String>, String> {
    let (start, end) = range.split_once(':').ok_or_else(|| "#RANGE".to_string())?;
    let start = normalize_cell_address(start).map_err(|_| "#RANGE".to_string())?;
    let end = normalize_cell_address(end).map_err(|_| "#RANGE".to_string())?;
    let (start_column, start_row) = split_cell_address(&start);
    let (end_column, end_row) = split_cell_address(&end);
    if start_column != end_column {
        return Err("#RANGE".to_string());
    }
    let start_row = start_row.parse::<u32>().map_err(|_| "#RANGE".to_string())?;
    let end_row = end_row.parse::<u32>().map_err(|_| "#RANGE".to_string())?;
    let (first, last) = if start_row <= end_row {
        (start_row, end_row)
    } else {
        (end_row, start_row)
    };
    Ok((first..=last)
        .map(|row| format!("{start_column}{row}"))
        .collect())
}

fn numeric_cell_value(address: &str, values: &BTreeMap<String, AppCell>) -> Result<f64, String> {
    let address = normalize_cell_address(address).map_err(|_| "#REF".to_string())?;
    let Some(cell) = values.get(&address) else {
        return Err("#REF".to_string());
    };
    cell.user_value
        .parse::<f64>()
        .map_err(|_| "#VALUE".to_string())
}

fn trim_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn arg_string(args: &Value, name: &str) -> Result<String, AppApiError> {
    args.get(name)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| AppApiError::Format(format!("missing string argument {name}")))
}

fn arg_optional_string(args: &Value, name: &str) -> Result<Option<String>, AppApiError> {
    match args.get(name) {
        Some(Value::Null) | None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(AppApiError::Format(format!(
            "argument {name} must be a string or null"
        ))),
    }
}

fn arg_u8(args: &Value, name: &str) -> Result<u8, AppApiError> {
    let Some(value) = args.get(name).and_then(Value::as_u64) else {
        return Err(AppApiError::Format(format!(
            "missing integer argument {name}"
        )));
    };
    u8::try_from(value)
        .map_err(|_| AppApiError::Format(format!("integer argument {name} is out of range")))
}

fn arg_bool(args: &Value, name: &str) -> Result<bool, AppApiError> {
    args.get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| AppApiError::Format(format!("missing boolean argument {name}")))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDocument {
    pub uuid: String,
    pub title: String,
    pub locale: String,
    pub visible_text: String,
    pub blocks: Vec<AppBlock>,
    pub comments: Vec<AppCommentThread>,
    pub suggestions: Vec<AppSuggestion>,
    pub citations: AppCitationDatabase,
    pub workbook: AppSpreadsheetWorkbook,
    pub warnings: Vec<AppWarning>,
    pub signature_state: String,
    pub signature: Option<AppSignature>,
    pub signatures: Vec<AppSignature>,
    pub repository_root: Option<String>,
    pub last_manifest: Option<String>,
    pub operation_count: usize,
    pub operations: Vec<AppOperationRecord>,
}

impl AppDocument {
    fn from_core(document: &Document) -> Self {
        Self {
            uuid: document.uuid.to_string(),
            title: document.title.clone(),
            locale: document.locale.clone(),
            visible_text: document.visible_text(),
            blocks: document
                .blocks
                .iter()
                .map(|block| AppBlock::from_core(block, &document.citation_database))
                .collect(),
            comments: document
                .comments
                .iter()
                .map(AppCommentThread::from_core)
                .collect(),
            suggestions: document
                .suggestions
                .iter()
                .map(AppSuggestion::from_core)
                .collect(),
            citations: AppCitationDatabase::from_core(&document.citation_database),
            workbook: AppSpreadsheetWorkbook::sample(),
            warnings: document
                .warnings
                .iter()
                .map(AppWarning::from_core)
                .collect(),
            signature_state: "unsigned".to_string(),
            signature: None,
            signatures: Vec::new(),
            repository_root: None,
            last_manifest: None,
            operation_count: 0,
            operations: Vec::new(),
        }
    }

    fn to_core(&self) -> Result<Document, AppApiError> {
        Ok(Document {
            uuid: opendoc_core::DocumentUuid::parse(self.uuid.clone())
                .map_err(|err| AppApiError::Model(err.to_string()))?,
            title: self.title.clone(),
            locale: self.locale.clone(),
            blocks: self
                .blocks
                .iter()
                .map(AppBlock::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            comments: self
                .comments
                .iter()
                .map(AppCommentThread::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            suggestions: self
                .suggestions
                .iter()
                .map(AppSuggestion::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            citation_database: self.citations.to_core()?,
            warnings: self.warnings.iter().map(AppWarning::to_core).collect(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppSpreadsheetWorkbook {
    pub title: String,
    pub locale: String,
    pub timezone: String,
    pub sheets: Vec<AppSheet>,
}

impl AppSpreadsheetWorkbook {
    fn sample() -> Self {
        let mut workbook = Self {
            title: "Prototype Sheet".to_string(),
            locale: "en-US".to_string(),
            timezone: "UTC".to_string(),
            sheets: vec![AppSheet {
                id: "sheet-1".to_string(),
                title: "Sheet1".to_string(),
                rows: vec!["1".to_string(), "2".to_string(), "3".to_string()],
                columns: vec!["A".to_string(), "B".to_string()],
                cells: vec![
                    AppCell::new("A1", "string", "Item"),
                    AppCell::new("B1", "string", "Count"),
                    AppCell::new("A2", "string", "Apples"),
                    AppCell::new("B2", "number", "5"),
                    AppCell::new("A3", "string", "Total"),
                    AppCell::new("B3", "formula", "=SUM(B2:B2)"),
                ],
            }],
        };
        workbook = workbook.evaluated();
        workbook
    }

    fn evaluated(&self) -> Self {
        let mut workbook = self.clone();
        for sheet in &mut workbook.sheets {
            sheet.evaluate();
        }
        workbook
    }

    fn set_cell(&mut self, address: &str, value: String) {
        let Some(sheet) = self.sheets.first_mut() else {
            return;
        };
        sheet.ensure_address(address);
        let kind = classify_cell_value(&value);
        if let Some(cell) = sheet.cells.iter_mut().find(|cell| cell.address == address) {
            cell.user_kind = kind.to_string();
            cell.user_value = value;
        } else {
            sheet.cells.push(AppCell::new(address, kind, &value));
            sheet
                .cells
                .sort_by(|left, right| left.address.cmp(&right.address));
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppSheet {
    pub id: String,
    pub title: String,
    pub rows: Vec<String>,
    pub columns: Vec<String>,
    pub cells: Vec<AppCell>,
}

impl AppSheet {
    fn evaluate(&mut self) {
        let values: BTreeMap<String, AppCell> = self
            .cells
            .iter()
            .map(|cell| (cell.address.clone(), cell.clone()))
            .collect();
        for cell in &mut self.cells {
            match cell.user_kind.as_str() {
                "formula" => match evaluate_formula(&cell.user_value, &values) {
                    Ok((value, dependencies)) => {
                        cell.computed_kind = "number".to_string();
                        cell.computed_value = trim_number(value);
                        cell.dependencies = dependencies;
                    }
                    Err(message) => {
                        cell.computed_kind = "error".to_string();
                        cell.computed_value = message;
                        cell.dependencies = Vec::new();
                    }
                },
                "number" => {
                    cell.computed_kind = "number".to_string();
                    cell.computed_value = cell.user_value.clone();
                    cell.dependencies = Vec::new();
                }
                "bool" => {
                    cell.computed_kind = "bool".to_string();
                    cell.computed_value = cell.user_value.clone();
                    cell.dependencies = Vec::new();
                }
                _ => {
                    cell.computed_kind = cell.user_kind.clone();
                    cell.computed_value = cell.user_value.clone();
                    cell.dependencies = Vec::new();
                }
            }
        }
    }

    fn ensure_address(&mut self, address: &str) {
        let (column, row) = split_cell_address(address);
        if !self.columns.iter().any(|item| item == &column) {
            self.columns.push(column);
            self.columns.sort();
        }
        if !self.rows.iter().any(|item| item == &row) {
            self.rows.push(row);
            self.rows
                .sort_by_key(|value| value.parse::<u32>().unwrap_or(0));
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCell {
    pub address: String,
    pub user_kind: String,
    pub user_value: String,
    pub computed_kind: String,
    pub computed_value: String,
    pub dependencies: Vec<String>,
}

impl AppCell {
    fn new(address: &str, user_kind: &str, user_value: &str) -> Self {
        Self {
            address: address.to_string(),
            user_kind: user_kind.to_string(),
            user_value: user_value.to_string(),
            computed_kind: user_kind.to_string(),
            computed_value: user_value.to_string(),
            dependencies: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBlock {
    pub id: String,
    pub kind: String,
    pub level: Option<u8>,
    pub ordered: Option<bool>,
    pub equation_source: Option<String>,
    pub content: Vec<AppInline>,
    pub rows: Vec<Vec<Vec<AppBlock>>>,
}

impl AppBlock {
    fn from_core(block: &Block, citations: &opendoc_core::CitationDatabase) -> Self {
        let (kind, level, ordered, equation_source, rows) = match &block.kind {
            BlockKind::Paragraph => ("paragraph".to_string(), None, None, None, Vec::new()),
            BlockKind::Heading { level } => {
                ("heading".to_string(), Some(*level), None, None, Vec::new())
            }
            BlockKind::ListItem { level, ordered, .. } => (
                "list-item".to_string(),
                Some(*level),
                Some(*ordered),
                None,
                Vec::new(),
            ),
            BlockKind::Table { rows } => (
                "table".to_string(),
                None,
                None,
                None,
                rows.iter()
                    .map(|row| {
                        row.cells
                            .iter()
                            .map(|cell| {
                                cell.blocks
                                    .iter()
                                    .map(|block| AppBlock::from_core(block, citations))
                                    .collect()
                            })
                            .collect()
                    })
                    .collect(),
            ),
            BlockKind::EquationBlock { equation } => (
                "equation-block".to_string(),
                None,
                None,
                Some(equation.source.clone()),
                Vec::new(),
            ),
            BlockKind::PageBreak => ("page-break".to_string(), None, None, None, Vec::new()),
        };
        Self {
            id: block.id.to_string(),
            kind,
            level,
            ordered,
            equation_source,
            content: block
                .content
                .iter()
                .map(|inline| AppInline::from_core(inline, citations))
                .collect(),
            rows,
        }
    }

    fn to_core(&self) -> Result<Block, AppApiError> {
        Ok(Block {
            id: parse_id(&self.id)?,
            kind: match self.kind.as_str() {
                "heading" => BlockKind::Heading {
                    level: self.level.unwrap_or(2),
                },
                "table" => BlockKind::Table {
                    rows: self
                        .rows
                        .iter()
                        .map(|row| {
                            Ok(opendoc_core::TableRow {
                                id: StableId::new("row"),
                                cells: row
                                    .iter()
                                    .map(|cell| {
                                        Ok(opendoc_core::TableCell {
                                            id: StableId::new("cell"),
                                            blocks: cell
                                                .iter()
                                                .map(AppBlock::to_core)
                                                .collect::<Result<Vec<_>, AppApiError>>()?,
                                            properties: Vec::new(),
                                        })
                                    })
                                    .collect::<Result<Vec<_>, AppApiError>>()?,
                            })
                        })
                        .collect::<Result<Vec<_>, AppApiError>>()?,
                },
                "page-break" => BlockKind::PageBreak,
                "list-item" => BlockKind::ListItem {
                    list_id: StableId::parse("list-main").expect("static list id"),
                    level: self.level.unwrap_or(0),
                    ordered: self.ordered.unwrap_or(false),
                },
                "equation-block" => BlockKind::EquationBlock {
                    equation: Equation {
                        id: StableId::new("eq"),
                        source_format: EquationSourceFormat::LatexLike,
                        source: self.equation_source.clone().unwrap_or_default(),
                    },
                },
                _ => BlockKind::Paragraph,
            },
            content: self
                .content
                .iter()
                .map(AppInline::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            properties: Vec::new(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppInline {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub href: Option<String>,
    pub target_id: Option<String>,
    pub marks: Vec<String>,
}

impl AppInline {
    fn from_core(inline: &Inline, citations: &opendoc_core::CitationDatabase) -> Self {
        match inline {
            Inline::Text { id, text, marks } => Self::textual(id, "text", text, None, None, marks),
            Inline::Link {
                id,
                text,
                href,
                marks,
            } => Self::textual(id, "link", text, Some(href.clone()), None, marks),
            Inline::Citation {
                id,
                citation_id,
                rendered_cache,
            } => Self {
                id: id.to_string(),
                kind: "citation".to_string(),
                text: rendered_cache
                    .clone()
                    .or_else(|| citations.rendered_citation(citation_id).cloned())
                    .unwrap_or_else(|| format!("[{citation_id}]")),
                href: None,
                target_id: Some(citation_id.to_string()),
                marks: Vec::new(),
            },
            Inline::FootnoteRef { id, footnote_id } => Self {
                id: id.to_string(),
                kind: "footnote-ref".to_string(),
                text: format!("[{footnote_id}]"),
                href: None,
                target_id: Some(footnote_id.to_string()),
                marks: Vec::new(),
            },
            Inline::Mention { id, label } => Self {
                id: id.to_string(),
                kind: "mention".to_string(),
                text: label.clone(),
                href: None,
                target_id: None,
                marks: Vec::new(),
            },
            Inline::Equation { id, equation } => Self {
                id: id.to_string(),
                kind: "equation".to_string(),
                text: equation.source.clone(),
                href: None,
                target_id: Some(equation.id.to_string()),
                marks: Vec::new(),
            },
        }
    }

    fn textual(
        id: &StableId,
        kind: &str,
        text: &str,
        href: Option<String>,
        target_id: Option<String>,
        marks: &[Mark],
    ) -> Self {
        Self {
            id: id.to_string(),
            kind: kind.to_string(),
            text: text.to_string(),
            href,
            target_id,
            marks: marks.iter().map(mark_label).collect(),
        }
    }

    fn to_core(&self) -> Result<Inline, AppApiError> {
        Ok(match self.kind.as_str() {
            "link" => Inline::Link {
                id: parse_id(&self.id)?,
                text: self.text.clone(),
                href: self.href.clone().unwrap_or_default(),
                marks: parse_marks(&self.marks)?,
            },
            "citation" => {
                Inline::Citation {
                    id: parse_id(&self.id)?,
                    citation_id: parse_id(self.target_id.as_deref().ok_or_else(|| {
                        AppApiError::Format("citation target missing".to_string())
                    })?)?,
                    rendered_cache: Some(self.text.clone()),
                }
            }
            "footnote-ref" => {
                Inline::FootnoteRef {
                    id: parse_id(&self.id)?,
                    footnote_id: parse_id(self.target_id.as_deref().ok_or_else(|| {
                        AppApiError::Format("footnote target missing".to_string())
                    })?)?,
                }
            }
            "mention" => Inline::Mention {
                id: parse_id(&self.id)?,
                label: self.text.clone(),
            },
            "equation" => Inline::Equation {
                id: parse_id(&self.id)?,
                equation: Equation {
                    id: self
                        .target_id
                        .as_deref()
                        .map(parse_id)
                        .transpose()?
                        .unwrap_or_else(|| StableId::new("eq")),
                    source_format: EquationSourceFormat::LatexLike,
                    source: self.text.clone(),
                },
            },
            _ => Inline::Text {
                id: parse_id(&self.id)?,
                text: self.text.clone(),
                marks: parse_marks(&self.marks)?,
            },
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCitationDatabase {
    pub style: String,
    pub locale: String,
    pub references: Vec<AppBibliographyReference>,
    pub citations: Vec<AppCitationGroup>,
}

impl AppCitationDatabase {
    fn from_core(database: &opendoc_core::CitationDatabase) -> Self {
        Self {
            style: database.style.clone(),
            locale: database.locale.clone(),
            references: database
                .references
                .iter()
                .map(AppBibliographyReference::from_core)
                .collect(),
            citations: database
                .citations
                .iter()
                .map(AppCitationGroup::from_core)
                .collect(),
        }
    }

    fn to_core(&self) -> Result<opendoc_core::CitationDatabase, AppApiError> {
        Ok(opendoc_core::CitationDatabase {
            style: self.style.clone(),
            locale: self.locale.clone(),
            references: self
                .references
                .iter()
                .map(AppBibliographyReference::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            citations: self
                .citations
                .iter()
                .map(AppCitationGroup::to_core)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBibliographyReference {
    pub id: String,
    pub revision: u64,
    pub format: String,
    pub source: String,
    pub title: String,
    pub authors: Vec<String>,
    pub issued: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub deleted: bool,
}

impl AppBibliographyReference {
    fn from_core(reference: &BibliographyReference) -> Self {
        Self {
            id: reference.id.to_string(),
            revision: reference.revision,
            format: citation_source_format(&reference.source.format),
            source: String::from_utf8_lossy(&reference.source.bytes).to_string(),
            title: reference.summary.title.clone(),
            authors: reference.summary.authors.clone(),
            issued: reference.summary.issued.clone(),
            doi: reference.summary.doi.clone(),
            url: reference.summary.url.clone(),
            deleted: reference.deleted,
        }
    }

    fn to_core(&self) -> Result<BibliographyReference, AppApiError> {
        Ok(BibliographyReference {
            id: parse_id(&self.id)?,
            revision: self.revision,
            source: CitationSource {
                format: citation_source_format_from_label(&self.format),
                bytes: self.source.as_bytes().to_vec(),
            },
            summary: CitationSummary {
                title: self.title.clone(),
                authors: self.authors.clone(),
                issued: self.issued.clone(),
                doi: self.doi.clone(),
                url: self.url.clone(),
            },
            deleted: self.deleted,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCitationGroup {
    pub id: String,
    pub revision: u64,
    pub items: Vec<AppCitationItem>,
    pub placement: String,
    pub rendered_cache: Option<String>,
    pub deleted: bool,
}

impl AppCitationGroup {
    fn from_core(citation: &CitationGroup) -> Self {
        Self {
            id: citation.id.to_string(),
            revision: citation.revision,
            items: citation
                .items
                .iter()
                .map(AppCitationItem::from_core)
                .collect(),
            placement: match citation.placement {
                CitationPlacement::Inline => "inline".to_string(),
                CitationPlacement::Footnote { .. } => "footnote".to_string(),
            },
            rendered_cache: citation.rendered_cache.clone(),
            deleted: citation.deleted,
        }
    }

    fn to_core(&self) -> Result<CitationGroup, AppApiError> {
        Ok(CitationGroup {
            id: parse_id(&self.id)?,
            revision: self.revision,
            items: self
                .items
                .iter()
                .map(AppCitationItem::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            placement: CitationPlacement::Inline,
            rendered_cache: self.rendered_cache.clone(),
            deleted: self.deleted,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCitationItem {
    pub reference_id: String,
    pub locator: Option<String>,
    pub label: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub suppress_author: bool,
}

impl AppCitationItem {
    fn from_core(item: &CitationItem) -> Self {
        Self {
            reference_id: item.reference_id.to_string(),
            locator: item.locator.clone(),
            label: item.label.clone(),
            prefix: item.prefix.clone(),
            suffix: item.suffix.clone(),
            suppress_author: item.suppress_author,
        }
    }

    fn to_core(&self) -> Result<CitationItem, AppApiError> {
        Ok(CitationItem {
            reference_id: parse_id(&self.reference_id)?,
            locator: self.locator.clone(),
            label: self.label.clone(),
            prefix: self.prefix.clone(),
            suffix: self.suffix.clone(),
            suppress_author: self.suppress_author,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCommentThread {
    pub id: String,
    pub anchor: String,
    pub comments: Vec<AppComment>,
    pub deleted: bool,
}

impl AppCommentThread {
    fn from_core(thread: &CommentThread) -> Self {
        Self {
            id: thread.id.to_string(),
            anchor: anchor_label(&thread.anchor),
            comments: thread.comments.iter().map(AppComment::from_core).collect(),
            deleted: thread.deleted,
        }
    }

    fn to_core(&self) -> Result<CommentThread, AppApiError> {
        Ok(CommentThread {
            id: parse_id(&self.id)?,
            anchor: parse_anchor(&self.anchor),
            comments: self
                .comments
                .iter()
                .map(AppComment::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            deleted: self.deleted,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub deleted: bool,
}

impl AppComment {
    fn from_core(comment: &Comment) -> Self {
        Self {
            id: comment.id.to_string(),
            author: comment.author.clone(),
            body: comment
                .body
                .iter()
                .map(|inline| match inline {
                    Inline::Text { text, .. } => text.clone(),
                    Inline::Link { text, .. } => text.clone(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join(""),
            deleted: comment.deleted,
        }
    }

    fn to_core(&self) -> Result<Comment, AppApiError> {
        Ok(Comment {
            id: parse_id(&self.id)?,
            author: self.author.clone(),
            body: vec![Inline::text(self.body.clone())],
            created_at_ms: 0,
            deleted: self.deleted,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppSuggestion {
    pub id: String,
    pub author: String,
    pub kind: String,
    pub state: String,
}

impl AppSuggestion {
    fn from_core(suggestion: &Suggestion) -> Self {
        Self {
            id: suggestion.id.to_string(),
            author: suggestion.author.clone(),
            kind: match suggestion.kind {
                SuggestionKind::Insert { .. } => "insert".to_string(),
                SuggestionKind::Delete { .. } => "delete".to_string(),
                SuggestionKind::Format { .. } => "format".to_string(),
            },
            state: match suggestion.state {
                SuggestionState::Proposed => "proposed".to_string(),
                SuggestionState::Accepted => "accepted".to_string(),
                SuggestionState::Rejected => "rejected".to_string(),
            },
        }
    }

    fn to_core(&self) -> Result<Suggestion, AppApiError> {
        Ok(Suggestion {
            id: parse_id(&self.id)?,
            author: self.author.clone(),
            kind: match self.kind.as_str() {
                "delete" => SuggestionKind::Delete {
                    range: TextRange {
                        start: StableId::new("missing"),
                        end: StableId::new("missing"),
                    },
                },
                "format" => SuggestionKind::Format {
                    range: TextRange {
                        start: StableId::new("missing"),
                        end: StableId::new("missing"),
                    },
                    marks: Vec::new(),
                },
                _ => SuggestionKind::Insert {
                    anchor: Anchor::Document,
                    content: Vec::new(),
                },
            },
            state: match self.state.as_str() {
                "accepted" => SuggestionState::Accepted,
                "rejected" => SuggestionState::Rejected,
                _ => SuggestionState::Proposed,
            },
            provenance: Vec::new(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppWarning {
    pub code: String,
    pub message: String,
}

impl AppWarning {
    fn from_core(warning: &ModelWarning) -> Self {
        Self {
            code: warning.code.clone(),
            message: warning.message.clone(),
        }
    }

    fn to_core(&self) -> ModelWarning {
        ModelWarning {
            code: self.code.clone(),
            message: self.message.clone(),
        }
    }
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

fn mark_label(mark: &Mark) -> String {
    let expand = match mark.expand {
        MarkExpand::None => "none",
        MarkExpand::Start => "start",
        MarkExpand::End => "end",
        MarkExpand::Both => "both",
    };
    let kind = match mark.kind {
        MarkKind::Bold => "bold",
        MarkKind::Italic => "italic",
        MarkKind::Underline => "underline",
        MarkKind::Strike => "strike",
        MarkKind::Code => "code",
        MarkKind::Superscript => "superscript",
        MarkKind::Subscript => "subscript",
        MarkKind::Color => "color",
        MarkKind::Background => "background",
        MarkKind::Font => "font",
        MarkKind::Size => "size",
        MarkKind::Link => "link",
        MarkKind::Citation => "citation",
    };
    match &mark.value {
        Some(value) => format!("{kind}:{value}:{expand}"),
        None => format!("{kind}:{expand}"),
    }
}

fn parse_marks(labels: &[String]) -> Result<Vec<Mark>, AppApiError> {
    let mut marks = Vec::new();
    for label in labels {
        if let Some(mark) = parse_mark_label(label)? {
            marks.push(mark);
        }
    }
    Ok(marks)
}

fn parse_mark_label(label: &str) -> Result<Option<Mark>, AppApiError> {
    let mut parts = label.split(':');
    let kind = parts.next().unwrap_or_default();
    let second = parts.next();
    let third = parts.next();
    if parts.next().is_some() {
        return Ok(None);
    }
    let (value, expand) = match (second, third) {
        (Some(expand), None) => (None, expand),
        (Some(value), Some(expand)) => (Some(value.to_string()), expand),
        _ => return Ok(None),
    };
    Ok(Some(Mark {
        kind: parse_mark_kind(kind)?,
        value,
        expand: match expand {
            "none" => MarkExpand::None,
            "start" => MarkExpand::Start,
            "end" => MarkExpand::End,
            "both" => MarkExpand::Both,
            _ => return Ok(None),
        },
    }))
}

fn parse_mark_kind(kind: &str) -> Result<MarkKind, AppApiError> {
    match kind {
        "bold" => Ok(MarkKind::Bold),
        "italic" => Ok(MarkKind::Italic),
        "underline" => Ok(MarkKind::Underline),
        "strike" => Ok(MarkKind::Strike),
        "code" => Ok(MarkKind::Code),
        "superscript" => Ok(MarkKind::Superscript),
        "subscript" => Ok(MarkKind::Subscript),
        "color" => Ok(MarkKind::Color),
        "background" => Ok(MarkKind::Background),
        "font" => Ok(MarkKind::Font),
        "size" => Ok(MarkKind::Size),
        "link" => Ok(MarkKind::Link),
        "citation" => Ok(MarkKind::Citation),
        _ => Err(AppApiError::Format(format!("unsupported mark kind {kind}"))),
    }
}

fn anchor_label(anchor: &Anchor) -> String {
    match anchor {
        Anchor::TextRange(range) => format!("{}..{}", range.start, range.end),
        Anchor::NearestBlock { block_id, .. } => format!("nearest:{block_id}"),
        Anchor::Document => "document".to_string(),
    }
}

fn citation_source_format(format: &CitationSourceFormat) -> String {
    match format {
        CitationSourceFormat::CitumNative => "citum-native".to_string(),
        CitationSourceFormat::CslJson => "csl-json".to_string(),
        CitationSourceFormat::Bibtex => "bibtex".to_string(),
        CitationSourceFormat::Ris => "ris".to_string(),
        CitationSourceFormat::Unknown(value) => value.clone(),
    }
}

fn citation_source_format_from_label(label: &str) -> CitationSourceFormat {
    match label {
        "citum-native" => CitationSourceFormat::CitumNative,
        "csl-json" => CitationSourceFormat::CslJson,
        "bibtex" => CitationSourceFormat::Bibtex,
        "ris" => CitationSourceFormat::Ris,
        other => CitationSourceFormat::Unknown(other.to_string()),
    }
}

fn citation_source_bytes(reference: &BibliographyReference) -> Vec<u8> {
    let mut out = format!("title: {}\n", reference.summary.title);
    if !reference.summary.authors.is_empty() {
        out.push_str(&format!(
            "author: {}\n",
            reference.summary.authors.join("; ")
        ));
    }
    if let Some(issued) = &reference.summary.issued {
        out.push_str(&format!("year: {issued}\n"));
    }
    if let Some(doi) = &reference.summary.doi {
        out.push_str(&format!("doi: {doi}\n"));
    }
    if let Some(url) = &reference.summary.url {
        out.push_str(&format!("url: {url}\n"));
    }
    out.into_bytes()
}

fn render_citation_group(
    database: &opendoc_core::CitationDatabase,
    citation: &CitationGroup,
) -> String {
    let items = citation
        .items
        .iter()
        .map(|item| {
            let reference = database
                .references
                .iter()
                .find(|reference| reference.id == item.reference_id && !reference.deleted);
            let mut label = reference
                .and_then(|reference| reference.summary.authors.first().cloned())
                .unwrap_or_else(|| item.reference_id.to_string());
            if let Some(reference) = reference {
                if let Some(issued) = &reference.summary.issued {
                    label.push(' ');
                    label.push_str(issued);
                }
            }
            if let Some(locator) = &item.locator {
                label.push_str(", ");
                label.push_str(locator);
            }
            if let Some(prefix) = &item.prefix {
                label = format!("{prefix} {label}");
            }
            if let Some(suffix) = &item.suffix {
                label.push(' ');
                label.push_str(suffix);
            }
            label
        })
        .collect::<Vec<_>>();
    format!("({})", items.join("; "))
}

fn parse_id(value: &str) -> Result<StableId, AppApiError> {
    StableId::parse(value.to_string()).map_err(|err| AppApiError::Model(err.to_string()))
}

fn parse_anchor(value: &str) -> Anchor {
    if value == "document" {
        return Anchor::Document;
    }
    if let Some(block_id) = value.strip_prefix("nearest:") {
        return StableId::parse(block_id.to_string())
            .map(|block_id| Anchor::NearestBlock {
                block_id,
                warning: String::new(),
            })
            .unwrap_or(Anchor::Document);
    }
    if let Some((start, end)) = value.split_once("..") {
        return match (
            StableId::parse(start.to_string()),
            StableId::parse(end.to_string()),
        ) {
            (Ok(start), Ok(end)) => Anchor::TextRange(TextRange { start, end }),
            _ => Anchor::Document,
        };
    }
    Anchor::Document
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_document_projects_full_schema_surface() {
        let mut app = OpenDocApp::new_sample();
        app.add_mention("@alice");
        app.add_footnote_ref();
        let doc = app.document();
        assert!(doc.blocks.iter().any(|block| block.kind == "table"));
        assert!(doc.blocks.iter().any(|block| block
            .content
            .iter()
            .any(|inline| inline.kind == "mention" && inline.text == "@alice")));
        assert!(doc.blocks.iter().any(|block| block
            .content
            .iter()
            .any(|inline| inline.kind == "footnote-ref")));
        assert!(!doc.comments.is_empty());
        assert!(!doc.suggestions.is_empty());
        assert_eq!(doc.citations.references.len(), 1);
        assert!(doc.visible_text.contains("(see Doe 2020, 42)"));
        assert_eq!(doc.signature_state, "unsigned");
    }

    #[test]
    fn creates_blank_document_with_fresh_state() {
        let mut app = OpenDocApp::new_sample();
        app.sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Alice")
            .unwrap();

        let created = app.new_document("Fresh document");
        assert_eq!(created.title, "Fresh document");
        assert!(created.blocks.is_empty());
        assert!(created.comments.is_empty());
        assert!(created.suggestions.is_empty());
        assert!(created.citations.references.is_empty());
        assert_eq!(created.operation_count, 0);
        assert_eq!(created.signature_state, "unsigned");
        assert!(created.signature.is_none());
        assert!(created.signatures.is_empty());
        assert!(created.repository_root.is_none());
        assert!(created.last_manifest.is_none());
        assert!(!created.workbook.sheets.is_empty());

        let added = app.add_paragraph("after create");
        assert!(added.visible_text.contains("after create"));
        assert_eq!(added.operation_count, 1);
    }

    #[test]
    fn saves_and_opens_projection_through_local_object_repository() {
        let root = std::env::temp_dir().join(format!("opendoc-app-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = OpenDocApp::new_sample();
        let inline_id = first_editable_inline_id(&app.document());
        app.update_inline_text(&inline_id, "Edited opening line")
            .unwrap();
        let saved = app.save_to_local_repository(&root).unwrap();
        assert_eq!(
            saved.repository_root,
            Some(root.to_string_lossy().to_string())
        );
        assert!(saved
            .last_manifest
            .as_ref()
            .is_some_and(|hash| hash.starts_with("sha256:")));
        assert!(saved.operation_count > 0);
        assert!(saved
            .operations
            .iter()
            .any(|operation| operation.kind == "upsert-citation-group"));

        let mut reopened = OpenDocApp::new_sample();
        let opened = reopened.open_saved_projection(&root, &saved.uuid).unwrap();
        assert_eq!(opened.uuid, saved.uuid);
        assert_eq!(opened.visible_text, saved.visible_text);
        assert!(opened.visible_text.contains("Edited opening line"));
        assert_eq!(opened.last_manifest, saved.last_manifest);
        assert_eq!(opened.citations.references.len(), 1);
        assert_eq!(opened.operation_count, saved.operation_count);
        assert_eq!(opened.operations, saved.operations);
        assert!(opened
            .operations
            .iter()
            .any(|operation| operation.kind == "update-inline-text"));
        let edited = reopened.add_paragraph("after reopen");
        assert_eq!(edited.uuid, saved.uuid);
        assert!(edited.visible_text.contains("after reopen"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn openssh_signing_hook_signs_current_snapshot_and_clears_on_edit() {
        let mut app = OpenDocApp::new_sample();
        let signed = app
            .sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Alice")
            .unwrap();
        assert_eq!(signed.signature_state, "signed");
        assert_eq!(
            app.verify_current_signature(TEST_ED25519_PRIVATE_KEY)
                .unwrap(),
            "signed"
        );
        let inline_id = first_editable_inline_id(&app.document());
        let edited = app
            .update_inline_text(&inline_id, "mutation clears signature")
            .unwrap();
        assert_eq!(edited.signature_state, "unsigned");
        assert!(edited.visible_text.contains("mutation clears signature"));
    }

    #[test]
    fn signed_snapshot_persists_signature_sidecar() {
        let root =
            std::env::temp_dir().join(format!("opendoc-app-api-signature-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = OpenDocApp::new_sample();
        let signed = app
            .sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Alice")
            .unwrap();
        assert_eq!(signed.signature_state, "signed");
        let signed = app
            .sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Second signer")
            .unwrap();
        assert_eq!(signed.signatures.len(), 2);
        let saved = app.save_to_local_repository(&root).unwrap();

        let mut reopened = OpenDocApp::new_sample();
        let opened = reopened.open_saved_projection(&root, &saved.uuid).unwrap();
        assert_eq!(opened.signature_state, "signed");
        assert_eq!(opened.signatures.len(), 2);
        assert_eq!(
            opened
                .signature
                .as_ref()
                .map(|signature| signature.signer_display.as_str()),
            Some("Alice")
        );
        assert_eq!(
            reopened
                .verify_current_signature(TEST_ED25519_PRIVATE_KEY)
                .unwrap(),
            "signed"
        );
        let edited = reopened.add_paragraph("after signed open");
        assert_eq!(edited.signature_state, "unsigned");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn spreadsheet_cells_evaluate_persist_and_clear_signatures() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-app-api-spreadsheet-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = OpenDocApp::new_sample();
        app.sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Alice")
            .unwrap();
        let edited = app.set_spreadsheet_cell("B2", "7").unwrap();
        assert_eq!(edited.signature_state, "unsigned");
        let total = edited.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "B3")
            .unwrap();
        assert_eq!(total.user_value, "=SUM(B2:B2)");
        assert_eq!(total.computed_value, "7");
        assert_eq!(total.dependencies, vec!["B2"]);

        let saved = app.save_to_local_repository(&root).unwrap();
        let mut reopened = OpenDocApp::new_sample();
        let opened = reopened.open_saved_projection(&root, &saved.uuid).unwrap();
        let reopened_total = opened.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "B3")
            .unwrap();
        assert_eq!(reopened_total.computed_value, "7");
        assert!(opened
            .operations
            .iter()
            .any(|operation| operation.kind == "set-spreadsheet-cell"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn citation_reference_updates_rerender_dependent_labels() {
        let root =
            std::env::temp_dir().join(format!("opendoc-app-api-citations-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = OpenDocApp::new_sample();
        app.sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Alice")
            .unwrap();
        let edited = app
            .update_bibliography_reference(
                "ref-doe-2020",
                "Updated Article",
                Some("2024".to_string()),
            )
            .unwrap();
        assert_eq!(edited.signature_state, "unsigned");
        assert!(edited.visible_text.contains("(see Doe 2024, 42)"));
        assert!(edited
            .operations
            .iter()
            .any(|operation| operation.kind == "upsert-citation-group"));

        let saved = app.save_to_local_repository(&root).unwrap();
        let mut reopened = OpenDocApp::new_sample();
        let opened = reopened.open_saved_projection(&root, &saved.uuid).unwrap();
        assert!(opened.visible_text.contains("(see Doe 2024, 42)"));
        assert_eq!(opened.citations.references[0].title, "Updated Article");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn full_gui_equivalent_workflow_survives_save_open_and_verification() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-app-api-full-workflow-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = OpenDocApp::new_sample();

        app.add_heading("Workflow Heading", 2);
        app.add_paragraph("Workflow paragraph");
        app.add_link("Workflow link", "https://example.invalid/workflow");
        app.add_mention("@workflow-user");
        app.add_footnote_ref();
        app.add_equation_inline("a=b");
        app.add_equation_block("x=1");
        app.add_list_item("Workflow list item", 1, false);
        app.add_page_break();
        app.add_table();
        app.add_sample_citation();
        app.add_comment("Reviewer", "Workflow comment");
        app.add_suggestion("Editor", "Workflow suggestion");

        let doc = app.document();
        let text_id = first_editable_inline_id(&doc);
        let link_id = first_inline_id(&doc, "link");
        let mention_id = first_inline_id(&doc, "mention");
        let inline_equation_id = first_inline_id(&doc, "equation");
        let equation_block_id = first_block_id(&doc, "equation-block");
        let comment_id = doc.comments.last().unwrap().id.clone();
        let accepted_suggestion_id = doc.suggestions.first().unwrap().id.clone();
        let rejected_suggestion_id = doc.suggestions.last().unwrap().id.clone();

        app.update_inline_text(&text_id, "Workflow edited opening")
            .unwrap();
        app.update_inline_text(&link_id, "Workflow link text")
            .unwrap();
        app.update_inline_text(&mention_id, "@edited-user").unwrap();
        app.update_inline_text(&inline_equation_id, "a=c").unwrap();
        app.update_block_equation_source(&equation_block_id, "x=2")
            .unwrap();
        app.add_text_mark(&text_id, "bold").unwrap();
        app.delete_comment_thread(&comment_id).unwrap();
        app.accept_suggestion(&accepted_suggestion_id, "Approver")
            .unwrap();
        app.reject_suggestion(&rejected_suggestion_id, "Approver")
            .unwrap();
        app.set_spreadsheet_cell("B2", "11").unwrap();
        app.update_bibliography_reference(
            "ref-doe-2020",
            "Workflow Article",
            Some("2026".to_string()),
        )
        .unwrap();

        let signed = app
            .sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Workflow Signer")
            .unwrap();
        assert_eq!(signed.signature_state, "signed");
        assert_eq!(signed.signatures.len(), 1);
        let saved = app.save_to_local_repository(&root).unwrap();

        let mut reopened = OpenDocApp::new_sample();
        let opened = reopened.open_saved_projection(&root, &saved.uuid).unwrap();
        assert_eq!(opened.uuid, saved.uuid);
        assert_eq!(opened.signature_state, "signed");
        assert_eq!(
            reopened
                .verify_current_signature(TEST_ED25519_PRIVATE_KEY)
                .unwrap(),
            "signed"
        );
        assert!(opened.visible_text.contains("Workflow edited opening"));
        assert!(opened.visible_text.contains("Workflow link text"));
        assert!(opened.visible_text.contains("@edited-user"));
        assert!(opened.visible_text.contains("a=c"));
        assert!(opened.visible_text.contains("x=2"));
        assert!(opened.visible_text.contains("(see Doe 2026, 42)"));
        assert!(opened.blocks.iter().any(|block| block.kind == "list-item"));
        assert!(opened.blocks.iter().any(|block| block.kind == "page-break"));
        assert!(opened.blocks.iter().any(|block| block.kind == "table"));
        assert!(opened
            .comments
            .iter()
            .any(|thread| thread.id == comment_id && thread.deleted));
        assert!(opened.suggestions.iter().any(|suggestion| {
            suggestion.id == accepted_suggestion_id && suggestion.state == "accepted"
        }));
        assert!(opened.suggestions.iter().any(|suggestion| {
            suggestion.id == rejected_suggestion_id && suggestion.state == "rejected"
        }));
        let total = opened.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "B3")
            .unwrap();
        assert_eq!(total.computed_value, "11");
        for expected in [
            "update-inline-text",
            "add-mark",
            "update-block-equation-source",
            "delete-comment-thread",
            "accept-suggestion",
            "reject-suggestion",
            "set-spreadsheet-cell",
            "upsert-bibliography-reference",
            "upsert-citation-group",
        ] {
            assert!(
                opened
                    .operations
                    .iter()
                    .any(|operation| operation.kind == expected),
                "missing operation {expected}"
            );
        }
        assert_app_document_contract(&opened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn comment_and_suggestion_lifecycle_is_journaled() {
        let mut app = OpenDocApp::new_sample();
        let doc = app.document();
        let comment_id = doc.comments[0].id.clone();
        let suggestion_id = doc.suggestions[0].id.clone();

        let after_delete = app.delete_comment_thread(&comment_id).unwrap();
        assert!(after_delete
            .comments
            .iter()
            .find(|thread| thread.id == comment_id)
            .is_some_and(|thread| thread.deleted));
        assert!(after_delete
            .operations
            .iter()
            .any(|operation| operation.kind == "delete-comment-thread"));

        let after_accept = app.accept_suggestion(&suggestion_id, "Alice").unwrap();
        assert!(after_accept
            .suggestions
            .iter()
            .find(|suggestion| suggestion.id == suggestion_id)
            .is_some_and(|suggestion| suggestion.state == "accepted"));

        let rejected_id = app.add_suggestion("Carol", "alternative").suggestions[1]
            .id
            .clone();
        let after_reject = app.reject_suggestion(&rejected_id, "Alice").unwrap();
        assert!(after_reject
            .suggestions
            .iter()
            .find(|suggestion| suggestion.id == rejected_id)
            .is_some_and(|suggestion| suggestion.state == "rejected"));
        assert!(after_reject
            .operations
            .iter()
            .any(|operation| operation.kind == "reject-suggestion"));
    }

    #[test]
    fn rich_text_marks_are_projected_and_persisted() {
        let root =
            std::env::temp_dir().join(format!("opendoc-app-api-marks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = OpenDocApp::new_sample();
        let inline_id = first_editable_inline_id(&app.document());

        let marked = app.add_text_mark(&inline_id, "bold").unwrap();
        let marked_inline = marked
            .blocks
            .iter()
            .flat_map(|block| block.content.iter())
            .find(|inline| inline.id == inline_id)
            .unwrap();
        assert_eq!(marked_inline.marks, vec!["bold:both"]);
        assert!(marked
            .operations
            .iter()
            .any(|operation| operation.kind == "add-mark"));

        let saved = app.save_to_local_repository(&root).unwrap();
        let mut reopened = OpenDocApp::new_sample();
        let opened = reopened.open_saved_projection(&root, &saved.uuid).unwrap();
        let reopened_inline = opened
            .blocks
            .iter()
            .flat_map(|block| block.content.iter())
            .find(|inline| inline.id == inline_id)
            .unwrap();
        assert_eq!(reopened_inline.marks, vec!["bold:both"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn block_level_schema_nodes_are_projected_and_persisted() {
        let root =
            std::env::temp_dir().join(format!("opendoc-app-api-blocks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = OpenDocApp::new_sample();
        app.add_list_item("Checklist item", 1, false);
        app.add_equation_block("\\alpha + \\beta");
        app.add_page_break();
        let equation_block_id = app
            .document()
            .blocks
            .iter()
            .find(|block| block.kind == "equation-block")
            .map(|block| block.id.clone())
            .unwrap();
        app.update_block_equation_source(&equation_block_id, "\\gamma")
            .unwrap();

        let saved = app.save_to_local_repository(&root).unwrap();
        assert!(saved.blocks.iter().any(|block| {
            block.kind == "list-item" && block.level == Some(1) && block.ordered == Some(false)
        }));
        assert!(saved.blocks.iter().any(|block| {
            block.kind == "equation-block" && block.equation_source.as_deref() == Some("\\gamma")
        }));
        assert!(saved.blocks.iter().any(|block| block.kind == "page-break"));

        let mut reopened = OpenDocApp::new_sample();
        let opened = reopened.open_saved_projection(&root, &saved.uuid).unwrap();
        assert!(opened.blocks.iter().any(|block| {
            block.kind == "list-item" && block.level == Some(1) && block.ordered == Some(false)
        }));
        assert!(opened.blocks.iter().any(|block| {
            block.kind == "equation-block" && block.equation_source.as_deref() == Some("\\gamma")
        }));
        assert!(opened.blocks.iter().any(|block| block.kind == "page-break"));
        assert!(opened.visible_text.contains("\\gamma"));
        assert!(opened
            .operations
            .iter()
            .any(|operation| operation.kind == "update-block-equation-source"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dispatches_desktop_command_contract() {
        let root =
            std::env::temp_dir().join(format!("opendoc-app-api-dispatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = OpenDocApp::new_sample();
        let mut exercised = Vec::new();

        let doc = dispatch_document(
            &mut app,
            &mut exercised,
            "get_document",
            serde_json::json!({}),
        );
        assert_eq!(doc.title, "OpenDoc Prototype");
        let doc = dispatch_document(
            &mut app,
            &mut exercised,
            "create_document",
            serde_json::json!({ "title": "Dispatch Created" }),
        );
        assert_eq!(doc.title, "Dispatch Created");
        assert!(doc.blocks.is_empty());

        dispatch_document(
            &mut app,
            &mut exercised,
            "add_paragraph",
            serde_json::json!({ "text": "Dispatch paragraph" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_heading",
            serde_json::json!({ "text": "Dispatch heading", "level": 2 }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_link",
            serde_json::json!({ "text": "Dispatch link", "href": "https://example.invalid/dispatch" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_mention",
            serde_json::json!({ "label": "@dispatch-user" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_footnote_ref",
            serde_json::json!({}),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_equation",
            serde_json::json!({ "source": "a=b" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_equation_block",
            serde_json::json!({ "source": "x=1" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_list_item",
            serde_json::json!({ "text": "Dispatch list", "level": 1, "ordered": false }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_page_break",
            serde_json::json!({}),
        );
        dispatch_document(&mut app, &mut exercised, "add_table", serde_json::json!({}));
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_citation",
            serde_json::json!({}),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_comment",
            serde_json::json!({ "author": "Reviewer", "body": "Dispatch comment" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_suggestion",
            serde_json::json!({ "author": "Editor", "text": "Dispatch suggestion" }),
        );

        let doc = app.document();
        let inline_id = first_editable_inline_id(&doc);
        let equation_block_id = first_block_id(&doc, "equation-block");
        let comment_id = doc.comments.last().unwrap().id.clone();
        let accepted_suggestion_id = doc.suggestions.first().unwrap().id.clone();
        let rejected_suggestion_id = doc.suggestions.last().unwrap().id.clone();

        dispatch_document(
            &mut app,
            &mut exercised,
            "update_inline_text",
            serde_json::json!({ "inlineId": inline_id, "text": "Dispatch edited inline" }),
        );
        let mark_inline_id = first_editable_inline_id(&app.document());
        dispatch_document(
            &mut app,
            &mut exercised,
            "add_text_mark",
            serde_json::json!({ "inlineId": mark_inline_id, "markKind": "bold" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "update_block_equation_source",
            serde_json::json!({ "blockId": equation_block_id, "source": "x=2" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "delete_comment_thread",
            serde_json::json!({ "threadId": comment_id }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "accept_suggestion",
            serde_json::json!({ "suggestionId": accepted_suggestion_id, "acceptedBy": "Approver" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "reject_suggestion",
            serde_json::json!({ "suggestionId": rejected_suggestion_id, "rejectedBy": "Approver" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "set_spreadsheet_cell",
            serde_json::json!({ "address": "B2", "value": "17" }),
        );
        dispatch_document(
            &mut app,
            &mut exercised,
            "update_bibliography_reference",
            serde_json::json!({ "referenceId": "ref-doe-2020", "title": "Dispatch Article", "issued": "2028" }),
        );

        let signed = dispatch_document(
            &mut app,
            &mut exercised,
            "sign_with_openssh_private_key",
            serde_json::json!({
                "privateKeyPem": TEST_ED25519_PRIVATE_KEY,
                "signerDisplay": "Dispatch Signer"
            }),
        );
        assert_eq!(signed.signature_state, "signed");
        assert_eq!(
            dispatch_text(
                &mut app,
                &mut exercised,
                "verify_current_signature",
                serde_json::json!({ "privateKeyPem": TEST_ED25519_PRIVATE_KEY }),
            ),
            "signed"
        );

        let saved = dispatch_document(
            &mut app,
            &mut exercised,
            "save_local_repository",
            serde_json::json!({ "path": root.to_string_lossy() }),
        );
        assert!(saved.last_manifest.is_some());
        let opened = dispatch_document(
            &mut app,
            &mut exercised,
            "open_local_repository",
            serde_json::json!({ "path": root.to_string_lossy(), "documentUuid": saved.uuid }),
        );
        assert!(opened.visible_text.contains("Dispatch edited inline"));
        assert!(opened.visible_text.contains("(see Doe 2028, 42)"));
        assert_eq!(opened.signature_state, "signed");
        assert_eq!(desktop_contract_command_names(), sorted_unique(exercised));
        assert!(app
            .dispatch_command("missing_command", serde_json::json!({}))
            .is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    fn first_editable_inline_id(document: &AppDocument) -> String {
        document
            .blocks
            .iter()
            .flat_map(|block| block.content.iter())
            .find(|inline| inline.kind == "text")
            .map(|inline| inline.id.clone())
            .expect("sample document has editable text")
    }

    fn first_inline_id(document: &AppDocument, kind: &str) -> String {
        document
            .blocks
            .iter()
            .flat_map(|block| block.content.iter())
            .find(|inline| inline.kind == kind)
            .map(|inline| inline.id.clone())
            .unwrap_or_else(|| panic!("sample document has inline kind {kind}"))
    }

    fn first_block_id(document: &AppDocument, kind: &str) -> String {
        document
            .blocks
            .iter()
            .find(|block| block.kind == kind)
            .map(|block| block.id.clone())
            .unwrap_or_else(|| panic!("sample document has block kind {kind}"))
    }

    fn expect_document(result: Result<AppCommandResult, AppApiError>) -> AppDocument {
        match result.unwrap() {
            AppCommandResult::Document(document) => document,
            AppCommandResult::Text(value) => panic!("expected document result, got {value}"),
        }
    }

    fn expect_text(result: Result<AppCommandResult, AppApiError>) -> String {
        match result.unwrap() {
            AppCommandResult::Text(value) => value,
            AppCommandResult::Document(_) => panic!("expected text result"),
        }
    }

    fn dispatch_document(
        app: &mut OpenDocApp,
        exercised: &mut Vec<String>,
        command: &str,
        args: serde_json::Value,
    ) -> AppDocument {
        exercised.push(command.to_string());
        expect_document(app.dispatch_command(command, args))
    }

    fn dispatch_text(
        app: &mut OpenDocApp,
        exercised: &mut Vec<String>,
        command: &str,
        args: serde_json::Value,
    ) -> String {
        exercised.push(command.to_string());
        expect_text(app.dispatch_command(command, args))
    }

    fn desktop_contract_command_names() -> Vec<String> {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../../../apps/desktop/commands.v0.json"))
                .expect("desktop command contract is valid JSON");
        let mut names = value["commands"]
            .as_array()
            .expect("desktop command contract has commands")
            .iter()
            .map(|command| {
                command["name"]
                    .as_str()
                    .expect("desktop command has name")
                    .to_string()
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn sorted_unique(mut values: Vec<String>) -> Vec<String> {
        values.sort();
        values.dedup();
        values
    }

    fn assert_app_document_contract(document: &AppDocument) {
        let value = serde_json::to_value(document).expect("AppDocument serializes to JSON");
        for field in [
            "uuid",
            "title",
            "locale",
            "visible_text",
            "blocks",
            "comments",
            "suggestions",
            "citations",
            "workbook",
            "warnings",
            "signature_state",
            "signature",
            "signatures",
            "repository_root",
            "last_manifest",
            "operation_count",
            "operations",
        ] {
            assert!(
                value.get(field).is_some(),
                "missing AppDocument field {field}"
            );
        }

        let blocks = value["blocks"].as_array().expect("blocks is an array");
        assert!(blocks.iter().any(|block| block["kind"] == "paragraph"));
        let table = blocks
            .iter()
            .find(|block| block["kind"] == "table")
            .expect("full workflow exposes table blocks");
        assert!(table["rows"][0][0][0]["content"].is_array());

        let equation_block = blocks
            .iter()
            .find(|block| block["kind"] == "equation-block")
            .expect("full workflow exposes equation blocks");
        assert_eq!(equation_block["equation_source"], "x=2");

        let inlines = blocks
            .iter()
            .flat_map(|block| block["content"].as_array().into_iter().flatten());
        let inline_kinds = inlines
            .map(|inline| inline["kind"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        for kind in [
            "text",
            "link",
            "citation",
            "mention",
            "equation",
            "footnote-ref",
        ] {
            assert!(
                inline_kinds.contains(&kind),
                "missing projected inline kind {kind}"
            );
        }

        assert_eq!(
            value["citations"]["references"][0]["format"],
            "citum-native"
        );
        assert_eq!(
            value["citations"]["references"][0]["title"],
            "Workflow Article"
        );
        assert_eq!(
            value["workbook"]["sheets"][0]["cells"][5]["user_value"],
            "=SUM(B2:B2)"
        );
        assert_eq!(
            value["workbook"]["sheets"][0]["cells"][5]["computed_value"],
            "11"
        );
        assert_eq!(
            value["workbook"]["sheets"][0]["cells"][5]["dependencies"][0],
            "B2"
        );
        assert_eq!(value["signature_state"], "signed");
        assert_eq!(value["signatures"][0]["signer_display"], "Workflow Signer");
        assert!(
            value["operations"]
                .as_array()
                .expect("operations is an array")
                .len()
                >= 10
        );
    }

    const TEST_ED25519_PRIVATE_KEY: &str = r#"
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYgAAAJgAIAxdACAM
XQAAAAtzc2gtZWQyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYg
AAAEC2BsIi0QwW2uFscKTUUXNHLsYX4FxlaSDSblbAj7WR7bM+rvN+ot98qgEN796jTiQf
ZfG1KaT0PtFDJ/XFSqtiAAAAEHVzZXJAZXhhbXBsZS5jb20BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----
"#;
}
