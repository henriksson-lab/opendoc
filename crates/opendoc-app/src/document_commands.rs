//! Document-level commands: identity, locale, page setup and furniture.

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
        Ok(self.apply(
            "set-page-setup",
            "page setup",
            OperationKind::SetPageSetup { page_setup },
        ))
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
        Ok(self.apply(
            "set-page-setup",
            "page orientation",
            OperationKind::SetPageSetup { page_setup },
        ))
    }

    /// Replaces a header or footer with a single paragraph holding `text` and,
    /// optionally, a page-number field.
    ///
    /// The model holds arbitrary blocks in a slot — that is what an importer
    /// needs — but the editor cannot yet put a caret inside page furniture,
    /// so the command surface offers the shape a user can actually build.
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
        let text = text.trim().to_string();
        let field = parse_optional_page_number_field(field.as_ref())?;
        let alignment = parse_alignment(alignment.as_ref())?;
        if text.is_empty() && field.is_none() {
            return Err(AppApiError::Format(format!(
                "{} needs text, a page number field, or both",
                slot.as_str()
            )));
        }
        let mut content = Vec::new();
        if !text.is_empty() {
            content.push(Inline::text(if field.is_some() {
                format!("{text} ")
            } else {
                text
            }));
        }
        if let Some(field) = field {
            content.push(Inline::PageNumber {
                id: StableId::new("field"),
                field,
            });
        }
        let mut block = Block::paragraph("");
        block.content = content;
        block.properties.set(BlockProperty::Alignment(alignment));
        Ok(self.apply(
            "set-page-furniture",
            &format!("set {}", slot.as_str()),
            OperationKind::SetPageFurniture {
                slot,
                blocks: vec![block],
            },
        ))
    }

    /// Empties a header or footer. Distinct from setting it to an empty
    /// paragraph: an empty slot draws nothing at all.
    pub fn clear_page_furniture(
        &mut self,
        slot: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let slot = parse_header_footer_slot(slot.as_ref())?;
        Ok(self.apply(
            "set-page-furniture",
            &format!("clear {}", slot.as_str()),
            OperationKind::SetPageFurniture {
                slot,
                blocks: Vec::new(),
            },
        ))
    }
}
