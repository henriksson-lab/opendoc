//! Document-level commands: identity, locale, page setup and furniture.

use super::*;

impl OpenDocApp {
    /// Creates or retargets a durable named bookmark at an existing block.
    /// The command addresses an observable block rather than permitting a
    /// caller to manufacture an orphaned target; a later delete may still
    /// leave it dangling so a collaborative undo can restore the anchor.
    pub fn set_bookmark(
        &mut self,
        bookmark_id: Option<&str>,
        name: impl Into<String>,
        block_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        if find_block_in_blocks(&self.document.blocks, &block_id).is_none() {
            return Err(AppApiError::NotFound(format!(
                "bookmark target block {block_id} was not found"
            )));
        }
        let id = match bookmark_id {
            Some(id) => parse_id(id)?,
            None => StableId::new("bookmark"),
        };
        let revision = self
            .document
            .bookmarks
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.revision + 1)
            .unwrap_or(1);
        let bookmark = opendoc_core::Bookmark {
            id,
            name: name.into(),
            block_id,
            revision,
            deleted: false,
        };
        bookmark
            .validate()
            .map_err(|error| AppApiError::Format(error.to_string()))?;
        self.apply(
            "upsert-bookmark",
            "set bookmark",
            OperationKind::UpsertBookmark { bookmark },
        )
    }

    /// Tombstones a bookmark so deletion converges and is undoable.
    pub fn delete_bookmark(
        &mut self,
        bookmark_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let id = parse_id(bookmark_id.as_ref())?;
        let Some(previous) = self.document.bookmarks.iter().find(|item| item.id == id) else {
            return Err(AppApiError::NotFound(format!(
                "bookmark {id} was not found"
            )));
        };
        let mut bookmark = previous.clone();
        bookmark.revision += 1;
        bookmark.deleted = true;
        self.apply(
            "upsert-bookmark",
            "delete bookmark",
            OperationKind::UpsertBookmark { bookmark },
        )
    }

    pub fn set_document_doi(&mut self, doi: impl Into<String>) -> Result<AppDocument, AppApiError> {
        let doi = doi.into();
        let doi = if doi.trim().is_empty() {
            None
        } else {
            Some(doi.trim().to_string())
        };
        self.apply(
            "set-document-doi",
            "set document DOI",
            OperationKind::SetDocumentDoi { doi },
        )
    }

    pub fn set_document_title(
        &mut self,
        title: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(AppApiError::Format("document title is empty".to_string()));
        }
        self.apply(
            "set-document-title",
            "set document title",
            OperationKind::SetDocumentTitle {
                title: title.trim().to_string(),
            },
        )
    }

    pub fn set_document_locale(
        &mut self,
        locale: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let locale = locale.into();
        if locale.trim().is_empty() {
            return Err(AppApiError::Format("document locale is empty".to_string()));
        }
        self.apply(
            "set-document-locale",
            "set document locale",
            OperationKind::SetDocumentLocale {
                locale: locale.trim().to_string(),
            },
        )
    }

    // ---- Page setup and page furniture (PLAN77 B7) ----------------------
    //
    // Page geometry is written as a whole `PageSetup`, never one dimension at
    // a time: "switch to A4" is one intent, and a merge that combined one
    // actor's A4 width with another's Letter height would produce a page
    // neither actor asked for. The header/footer margins are not part of the
    // page-setup dialog, so they are carried through rather than reset.

    pub fn set_page_setup(
        &mut self,
        width_twips: i32,
        height_twips: i32,
        margin_top_twips: i32,
        margin_bottom_twips: i32,
        margin_start_twips: i32,
        margin_end_twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let page_setup = self
            .document
            .page_setup
            .with_size(parse_length(width_twips)?, parse_length(height_twips)?)
            .and_then(|setup| {
                setup.with_margins(
                    opendoc_core::Length::from_twips(margin_top_twips)?,
                    opendoc_core::Length::from_twips(margin_bottom_twips)?,
                    opendoc_core::Length::from_twips(margin_start_twips)?,
                    opendoc_core::Length::from_twips(margin_end_twips)?,
                )
            })
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        self.apply(
            "set-page-setup",
            "page setup",
            OperationKind::SetPageSetup { page_setup },
        )
    }

    /// Rotates the sheet. Idempotent: asking for the orientation the page
    /// already has is a no-op rather than another 90 degrees.
    pub fn set_page_orientation(
        &mut self,
        orientation: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let orientation = opendoc_core::PageOrientation::parse(orientation.as_ref().trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let page_setup = self.document.page_setup.with_orientation(orientation);
        page_setup
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        self.apply(
            "set-page-setup",
            "page orientation",
            OperationKind::SetPageSetup { page_setup },
        )
    }

    /// Replaces a header or footer with one paragraph for every line in
    /// `text`, and, optionally, a page-number field on the final paragraph.
    ///
    /// The model holds arbitrary blocks in a slot — that is what an importer
    /// needs — but the editor cannot yet put a caret inside page furniture.
    /// Newline-delimited paragraphs deliberately give that non-body editor a
    /// structural, rather than flattened, authoring surface.  This does not
    /// add section-specific or first-page variants: the slot remains one
    /// document-wide header or footer.
    /// `field` is `"none"`, `"page-number"` or `"page-count"`; the field is
    /// never given a value here, because its value depends on pagination.
    pub fn set_page_furniture(
        &mut self,
        slot: impl AsRef<str>,
        text: impl Into<String>,
        field: impl AsRef<str>,
        alignment: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let slot = parse_header_footer_slot(slot.as_ref())?;
        let text = text.into();
        let field = parse_optional_page_number_field(field.as_ref())?;
        let alignment = parse_alignment(alignment.as_ref())?;
        if text.trim().is_empty() && field.is_none() {
            return Err(AppApiError::Format(format!(
                "{} needs text, a page number field, or both",
                slot.as_str()
            )));
        }
        // Keep intentional blank lines between paragraphs.  A field-only
        // header/footer still needs one block to carry the field.
        let lines: Vec<&str> = if text.trim().is_empty() {
            vec![""]
        } else {
            // Unlike `str::lines`, `split` retains a final empty paragraph:
            // pressing Enter after the final paragraph is authoring state,
            // not insignificant whitespace.
            text.split('\n').collect()
        };
        let final_index = lines.len() - 1;
        let mut blocks = Vec::with_capacity(lines.len());
        for (index, line) in lines.into_iter().enumerate() {
            let mut content = Vec::new();
            if !line.is_empty() {
                content.push(Inline::text(if field.is_some() && index == final_index {
                    format!("{line} ")
                } else {
                    line.to_string()
                }));
            }
            if index == final_index {
                if let Some(field) = field {
                    content.push(Inline::PageNumber {
                        id: StableId::new("field"),
                        field,
                    });
                }
            }
            let mut block = Block::paragraph("");
            block.content = content;
            block.properties.set(BlockProperty::Alignment(alignment));
            blocks.push(block);
        }
        self.apply(
            "set-page-furniture",
            &format!("set {}", slot.as_str()),
            OperationKind::SetPageFurniture { slot, blocks },
        )
    }

    /// Replaces one furniture slot from a safe rich-HTML fragment.
    ///
    /// This is the deliberately structural route for a header or footer that
    /// the legacy textarea cannot describe.  It shares the clipboard
    /// allowlist, not a browser HTML sink: tags never become model markup,
    /// and only text, links, marks, headings, lists, tables, and byte-owned
    /// raster data images get a representation.  A fragment that would need
    /// degradation is rejected before an operation is emitted, rather than
    /// silently flattening a person's furniture.
    pub fn set_page_furniture_html(
        &mut self,
        slot: impl AsRef<str>,
        html: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        use crate::editor::clipboard_html::{self, PastedBlock, PastedBlockKind};

        let slot = parse_header_footer_slot(slot.as_ref())?;
        let mut parsed = clipboard_html::parse(html.as_ref());
        if let Some(warning) = parsed.warning {
            return Err(AppApiError::Format(format!(
                "rich {} HTML was not applied: {}",
                slot.as_str(),
                warning.message
            )));
        }
        if parsed.blocks.is_empty() {
            return Err(AppApiError::Format(format!(
                "rich {} HTML contains no supported content",
                slot.as_str()
            )));
        }
        // The shared clipboard parser deliberately recognises a table as a
        // native grid only when it is the fragment's structural payload;
        // mixed prose/table clipboard fragments follow its text path.  That
        // is acceptable for a paste caret, but not for a whole-slot editor:
        // silently turning a table into words here would violate the route's
        // non-flattening promise.  Reject until mixed-fragment table parsing
        // gains a lossless model path.
        if html.as_ref().to_ascii_lowercase().contains("<table")
            && !parsed
                .blocks
                .iter()
                .any(|block| matches!(block.kind, PastedBlockKind::Table { .. }))
        {
            return Err(AppApiError::Format(format!(
                "rich {} HTML mixes a table with other content; submit the table as its own replacement so it remains a native grid",
                slot.as_str()
            )));
        }
        for block in &mut parsed.blocks {
            let PastedBlockKind::Image(image) = &mut block.kind else {
                continue;
            };
            let hash = digest_bytes("sha256", &image.bytes)
                .map_err(|error| AppApiError::Model(error.to_string()))?
                .to_string();
            self.add_binary_blob(
                image.name.clone(),
                image.media_type.clone(),
                image.bytes.clone(),
            )?;
            image.blob_hash = Some(hash);
        }

        fn blocks_from_paste(blocks: Vec<PastedBlock>) -> Vec<Block> {
            use crate::editor::{block_kind_from_style, pasted_block_style, pasted_block_to_block};
            use std::collections::BTreeMap;

            let mut lists = BTreeMap::new();
            blocks
                .into_iter()
                .map(|block| match &block.kind {
                    PastedBlockKind::Table { rows } => {
                        let rows = rows
                            .iter()
                            .map(|row| opendoc_core::TableRow {
                                id: StableId::new("row"),
                                height: None,
                                header: row.header,
                                cells: row
                                    .cells
                                    .iter()
                                    .map(|cell_blocks| {
                                        let blocks = blocks_from_paste(cell_blocks.clone());
                                        opendoc_core::TableCell::new(if blocks.is_empty() {
                                            vec![Block::paragraph("")]
                                        } else {
                                            blocks
                                        })
                                    })
                                    .collect(),
                            })
                            .collect();
                        Block {
                            id: StableId::new("block"),
                            kind: BlockKind::table(rows),
                            content: Vec::new(),
                            properties: BlockProperties::default(),
                        }
                    }
                    _ => {
                        let mut result = pasted_block_to_block(&block);
                        if let Some(style) = pasted_block_style(&block, &mut lists) {
                            result.kind = block_kind_from_style(style);
                        }
                        result
                    }
                })
                .collect()
        }

        let blocks = blocks_from_paste(parsed.blocks);
        self.apply(
            "set-page-furniture-html",
            &format!("set rich {}", slot.as_str()),
            OperationKind::SetPageFurniture { slot, blocks },
        )
    }

    /// Empties a header or footer. Distinct from setting it to an empty
    /// paragraph: an empty slot draws nothing at all.
    pub fn clear_page_furniture(
        &mut self,
        slot: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let slot = parse_header_footer_slot(slot.as_ref())?;
        self.apply(
            "set-page-furniture",
            &format!("clear {}", slot.as_str()),
            OperationKind::SetPageFurniture {
                slot,
                blocks: Vec::new(),
            },
        )
    }

    /// Restores an optional first/even-page slot to ordinary-furniture
    /// inheritance. This is not the same as [`Self::clear_page_furniture`],
    /// whose empty fragment deliberately suppresses inherited content.
    pub fn clear_page_furniture_override(
        &mut self,
        slot: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let slot = parse_header_footer_slot(slot.as_ref())?;
        if !slot.is_override() {
            return Err(AppApiError::Format(format!(
                "{} is ordinary furniture and cannot inherit from itself",
                slot.as_str()
            )));
        }
        self.apply(
            "clear-page-furniture-override",
            &format!("inherit {}", slot.as_str()),
            OperationKind::ClearPageFurnitureOverride { slot },
        )
    }
}
