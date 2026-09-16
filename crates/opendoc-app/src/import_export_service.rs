use crate::*;

pub(crate) struct ImportExportService<'a> {
    app: &'a mut OpenDocApp,
}

impl<'a> ImportExportService<'a> {
    pub(crate) fn new(app: &'a mut OpenDocApp) -> Self {
        Self { app }
    }

    pub(crate) fn import_google_docs_json_text(
        &mut self,
        title: String,
        json_text: &str,
    ) -> Result<AppDocument, AppApiError> {
        let imported_blobs = import_opendoc_blob_refs_from_google_docs_json(json_text)?;
        let report = import_google_docs_json(title, json_text.as_bytes())
            .map_err(|err| AppApiError::Import(err.to_string()))?;
        self.app.document = report.document;
        self.reset_after_document_import();

        let imported_blob_templates = imported_blobs
            .iter()
            .map(|imported| imported.blob.clone())
            .collect::<Vec<_>>();
        for imported in imported_blobs {
            let mut blob = imported.blob;
            if !blob.signatures.is_empty() {
                self.app.document.warnings.push(ModelWarning {
                    code: "opendoc-blob-exact-signature-metadata-only".to_string(),
                    message: format!(
                        "OpenDoc blob {} included exact-byte signature metadata without sidecar bytes",
                        blob.hash
                    ),
                });
            }
            if let Some(tombstone) = blob.archive_tombstone.take() {
                self.app
                    .blob_tombstones
                    .insert(blob.hash.clone(), tombstone);
            }
            if let Some(tombstone_record) = imported.tombstone_record {
                self.app
                    .blob_tombstone_records
                    .insert(blob.hash.clone(), tombstone_record);
            }
            blob.signatures.clear();
            blob.signature_state = "unsigned".to_string();
            blob.available = false;
            self.app.blobs.push(blob);
        }
        self.restore_referenced_image_blobs(&[imported_blob_templates])?;
        self.app
            .push_app_operation("import-google-docs-json", "import Google Docs JSON");
        Ok(self.app.document())
    }

    pub(crate) fn import_doc_or_docx_path(
        &mut self,
        path: PathBuf,
    ) -> Result<AppDocument, AppApiError> {
        let report =
            import_doc_or_docx(&path).map_err(|err| AppApiError::Import(err.to_string()))?;
        self.adopt_import_report(report)
    }

    pub(crate) fn adopt_import_report(
        &mut self,
        report: opendoc_import::ImportReport,
    ) -> Result<AppDocument, AppApiError> {
        // Reject the complete import before replacing the open document. This
        // is the same asset-size boundary as interactive insertion, and keeps
        // a rejected package from leaving a half-replaced document behind.
        for blob in &report.blobs {
            crate::blob_service::validate_binary_blob_size(blob.bytes.len())?;
        }
        self.app.document = report.document;
        self.reset_after_document_import();
        self.app.restore_imported_blobs(report.blobs)?;
        self.restore_referenced_image_blobs(&[])?;
        self.app
            .push_app_operation("import-doc-or-docx", "import Word document");
        Ok(self.app.document())
    }

    pub(crate) fn import_google_sheets_json_text(
        &mut self,
        json_text: &str,
    ) -> Result<AppDocument, AppApiError> {
        let imported = import_google_sheets_workbook(json_text)?;
        self.app
            .mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
                *workbook = imported.workbook;
                Ok(())
            })?;
        for warning in imported.warnings {
            self.app.push_model_warning(&warning.code, warning.message);
        }
        self.app
            .push_app_operation("import-google-sheets-json", "import Google Sheets JSON");
        Ok(self.app.document())
    }

    fn reset_after_document_import(&mut self) {
        // FS-19: an imported document must not carry demo spreadsheet content.
        self.app.workbook = OpenDocApp::blank_workbook("Untitled");
        self.app.is_open = true;
        self.app.invalidate_source_state();
        self.app.clear_blob_state();
        self.app.clear_repository_binding();
        self.app.clear_edit_history();
    }

    fn restore_referenced_image_blobs(
        &mut self,
        templates: &[Vec<AppBlobRef>],
    ) -> Result<(), AppApiError> {
        let blocks = AppDocument::from_core(&self.app.document).blocks;
        let mut warnings = self
            .app
            .document
            .warnings
            .iter()
            .map(AppWarning::from_core)
            .collect::<Vec<_>>();
        restore_referenced_image_blobs(&mut self.app.blobs, &blocks, templates, &mut warnings)?;
        self.app.document.warnings = warnings.iter().map(AppWarning::to_core).collect();
        Ok(())
    }
}

pub(crate) struct ImportExportReadService<'a> {
    app: &'a OpenDocApp,
}

impl<'a> ImportExportReadService<'a> {
    pub(crate) fn new(app: &'a OpenDocApp) -> Self {
        Self { app }
    }

    /// Google Docs-shaped JSON, plus everything Google's schema could not
    /// carry.
    ///
    /// Nothing here takes `&mut self`: an export reads the document and the
    /// warnings travel out in the result, so exporting cannot dirty source
    /// state or move the bytes a signature covers (ADR 0010).
    pub(crate) fn export_google_docs_json(&self) -> Result<AppExport, AppApiError> {
        let (bytes, warnings) = export_google_docs_json_with_warnings(&self.app.document)
            .map_err(|err| AppApiError::Import(err.to_string()))?;
        let text = export_google_docs_json_with_opendoc_blobs(bytes, self.app.projected_blobs())?;
        Ok(AppExport::text(
            text,
            "application/json",
            "json",
            model_warnings(warnings),
        ))
    }

    /// The workbook as Google Sheets-shaped JSON, plus what that shape could
    /// not carry.
    ///
    /// This export used to hand back `Vec::new()` — not "nothing was lost",
    /// but "nobody asked". Google's schema *can* state a hidden sheet, a tab
    /// colour and a row height, and OpenDoc's writer states none of them, so
    /// a workbook carrying any of those crossed in silence.
    pub(crate) fn export_google_sheets_json(&self) -> Result<AppExport, AppApiError> {
        let workbook = self.app.workbook.evaluated();
        let warnings = google_sheets_export_warnings(&workbook);
        Ok(AppExport::text(
            export_google_sheets_workbook(&workbook)?,
            "application/json",
            "json",
            model_warnings(warnings),
        ))
    }

    /// The document as a PDF (FS-21), plus everything paper could not carry.
    ///
    /// The pages come from `opendoc-layout` — the same pass the editor
    /// paginates with — so the PDF and the screen break in the same places by
    /// construction rather than by two engines agreeing (ADR 0014). ADR 0003
    /// keeps signatures off rendered output, so a PDF is never signed.
    pub(crate) fn export_pdf(&self) -> Result<AppExport, AppApiError> {
        // The source model carries hashes, not bytes. Give PDF the same
        // reachable, content-addressed asset set DOCX and ODT receive; it can
        // then render PNG/JPEG pixels rather than treating every picture as a
        // missing frame.
        let images = self
            .docx_images()
            .into_iter()
            .map(|(hash, image)| {
                (
                    hash,
                    opendoc_pdf::PdfImage {
                        media_type: image.media_type,
                        bytes: image.bytes,
                    },
                )
            })
            .collect();
        let export = opendoc_pdf::export_pdf_with_images(&self.app.document, &images);
        Ok(AppExport::binary(
            &export.bytes,
            "application/pdf",
            "pdf",
            model_warnings(collapse_by_code(export.warnings)),
        ))
    }

    /// The document as one standalone HTML file.
    ///
    /// Built here rather than in the frontend: the markup is the renderer's,
    /// the stylesheet is projected from the type scale the layout measures
    /// with, and the media type and extension are facts about the format
    /// stated next to the writer (ADR 0010). The equation warnings the
    /// renderer produces reach the caller instead of being discarded, which
    /// the TypeScript version could not do at all.
    pub(crate) fn export_html(&self) -> Result<AppExport, AppApiError> {
        let rendering = self.app.render_service().render_standalone_html();
        Ok(AppExport::text(
            rendering.html,
            "text/html;charset=utf-8",
            "html",
            model_warnings(rendering.warnings),
        ))
    }

    /// The document as plain text, plus what plain text cannot carry.
    pub(crate) fn export_text(&self) -> Result<AppExport, AppApiError> {
        let (text, warnings) = opendoc_render::render_plain_text(&self.app.document);
        Ok(AppExport::text(
            text,
            "text/plain;charset=utf-8",
            "txt",
            model_warnings(warnings),
        ))
    }

    /// Export an image exactly as OpenDoc stored it.  Conversion belongs to a
    /// separate explicit command: callers must never receive a JPEG while
    /// being told they saved the original PNG.
    pub(crate) fn export_image_blob(&self, blob_hash: &str) -> Result<AppExport, AppApiError> {
        let blob = self
            .app
            .blobs
            .iter()
            .find(|blob| blob.hash == blob_hash)
            .ok_or_else(|| AppApiError::NotFound("image blob was not found".to_string()))?;
        if !image_media_type_essence(&blob.media_type).starts_with("image/") {
            return Err(AppApiError::Format(format!(
                "blob {} is {}, not an image",
                blob.hash, blob.media_type
            )));
        }
        let bytes = self.app.blob_bytes.get(blob_hash).ok_or_else(|| {
            AppApiError::NotFound("image bytes are not available locally".to_string())
        })?;
        let extension = image_file_extension(&blob.media_type, &blob.name);
        Ok(AppExport::binary(
            bytes,
            &blob.media_type,
            &extension,
            Vec::new(),
        ))
    }

    /// The document as a `.docx` package (FS-22), plus the 24-odd things
    /// WordprocessingML cannot carry exactly.
    ///
    /// Image blocks store only a content hash, so the bytes behind every
    /// reachable image are handed to the writer here; a block whose blob is
    /// not held locally is written as its alt text and named in the export
    /// warnings rather than vanishing.
    pub(crate) fn export_docx(&self) -> Result<AppExport, AppApiError> {
        let images = self.docx_images();
        let (bytes, warnings) = export_docx_with_warnings(&self.app.document, &images)
            .map_err(|err| AppApiError::Import(err.to_string()))?;
        Ok(AppExport::binary(
            &bytes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
            model_warnings(warnings),
        ))
    }

    /// The document as an `.odt` (OpenDocument Text) package, plus what
    /// OpenDocument could not carry exactly.
    ///
    /// The image bytes come from the same place the DOCX export takes them:
    /// a package has to embed them and the model stores only a hash.
    ///
    /// There is no ODF *reader* here, so this direction is one-way. What
    /// stands in for the DOCX export's round trip is recorded in
    /// `opendoc_import::odt_write`: the parts validate against the
    /// OpenDocument 1.3 RelaxNG schema, and LibreOffice and pandoc both read
    /// the package back.
    pub(crate) fn export_odt(&self) -> Result<AppExport, AppApiError> {
        let images = self.docx_images();
        let (bytes, warnings) =
            opendoc_import::export_odt_with_warnings(&self.app.document, &images)
                .map_err(|err| AppApiError::Import(err.to_string()))?;
        Ok(AppExport::binary(
            &bytes,
            opendoc_import::ODT_MEDIA_TYPE,
            "odt",
            model_warnings(warnings),
        ))
    }

    /// Every blob the document's image blocks point at, with its media type.
    fn docx_images(&self) -> BTreeMap<String, DocxImage> {
        let media_types: BTreeMap<&str, &str> = self
            .app
            .blobs
            .iter()
            .map(|blob| (blob.hash.as_str(), blob.media_type.as_str()))
            .collect();
        let mut images = BTreeMap::new();
        collect_image_hashes(
            &self.app.document.blocks,
            &mut images,
            &media_types,
            self.app,
        );
        for slot in HeaderFooterSlot::ALL {
            collect_image_hashes(
                self.app.document.furniture(slot),
                &mut images,
                &media_types,
                self.app,
            );
        }
        images
    }
}

/// A save dialog needs an extension even when a source was imported without a
/// useful filename. Prefer the media type, which names the bytes we return;
/// retain a conservative source suffix only for an image subtype OpenDoc does
/// not yet know by name.
fn image_file_extension(media_type: &str, name: &str) -> String {
    match image_media_type_essence(media_type).as_str() {
        "image/png" => "png".to_string(),
        "image/jpeg" | "image/jpg" => "jpg".to_string(),
        "image/gif" => "gif".to_string(),
        "image/webp" => "webp".to_string(),
        "image/svg+xml" => "svg".to_string(),
        "image/bmp" => "bmp".to_string(),
        "image/tiff" => "tiff".to_string(),
        "image/apng" => "apng".to_string(),
        "image/avif" => "avif".to_string(),
        "image/heic" => "heic".to_string(),
        "image/heif" => "heif".to_string(),
        "image/jxl" => "jxl".to_string(),
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico".to_string(),
        _ => name
            .rsplit_once('.')
            .map(|(_, suffix)| suffix)
            .filter(|suffix| {
                !suffix.is_empty()
                    && suffix.len() <= 10
                    && suffix.chars().all(|ch| ch.is_ascii_alphanumeric())
                    && !is_unsafe_download_extension(suffix)
            })
            .unwrap_or("img")
            .to_string(),
    }
}

/// A raw image export must retain unknown image bytes, but an unrecognised
/// `image/*` declaration is not evidence that an arbitrary source filename is
/// safe to offer back to the operating system.  In particular, blob metadata
/// can arrive from an imported extension without byte validation.  Keep useful
/// opaque image suffixes (for example `.qoi`), but do not project executable,
/// script, shortcut, or HTML suffixes into a save dialog.
fn is_unsafe_download_extension(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "ade"
            | "adp"
            | "app"
            | "bat"
            | "cab"
            | "cmd"
            | "com"
            | "cpl"
            | "dll"
            | "exe"
            | "hta"
            | "htm"
            | "html"
            | "inf"
            | "ins"
            | "isp"
            | "jar"
            | "js"
            | "jse"
            | "lib"
            | "lnk"
            | "msc"
            | "msi"
            | "msp"
            | "mst"
            | "pif"
            | "ps1"
            | "reg"
            | "scr"
            | "sct"
            | "sh"
            | "shb"
            | "sys"
            | "vb"
            | "vbe"
            | "vbs"
            | "vxd"
            | "wsc"
            | "wsf"
            | "wsh"
    )
}

/// Media type tokens are ASCII case-insensitive and parameters do not change
/// the kind of bytes a raw download contains. Keep the stored declaration
/// untouched for signatures and metadata; this is only the conservative save
/// dialog projection used to choose an extension and image eligibility.
fn image_media_type_essence(media_type: &str) -> String {
    media_type
        .split_once(';')
        .map_or(media_type, |(essence, _)| essence)
        .trim()
        .to_ascii_lowercase()
}

/// What a Google Sheets-shaped export cannot say about this workbook.
///
/// The list is short and each entry names a field the OpenDoc model holds and
/// `export_google_sheets_workbook` does not write. It lives here rather than
/// beside the writer only because the writer returns a bare `String`:
/// converting it to carry its own warnings reaches `opendoc-spreadsheet` and
/// `opendoc-api`, which is the right end state (ADR 0010) and the right home
/// for this function. Until then the check is here, where the command result
/// that has somewhere to put a warning is built.
fn google_sheets_export_warnings(
    workbook: &opendoc_spreadsheet::SpreadsheetWorkbook,
) -> Vec<ModelWarning> {
    let mut images = Vec::new();
    for sheet in &workbook.sheets {
        if !sheet.images.is_empty() {
            images.push(sheet.title.clone());
        }
    }
    let mut warnings = Vec::new();
    for (code, what, sheets) in [(
        "google-sheets-export-dropped-floating-images",
        "floating images were dropped because Google Sheets JSON has no blob package",
        images,
    )] {
        if sheets.is_empty() {
            continue;
        }
        warnings.push(ModelWarning {
            code: code.to_string(),
            message: format!(
                "{what}: the Google Sheets export does not write that property ({})",
                sheets.join(", ")
            ),
        });
    }
    warnings
}

/// What a CSV file cannot carry out of one sheet.
///
/// CSV is a grid of display text and nothing else: `export_csv` writes
/// `display_value` (or the number format applied to the computed value) and
/// reads no other field of the model. So this reports what the *model* holds
/// that the file will not — counted from the workbook, never from the writer,
/// which is why it can say anything at all about a function that returns one
/// flat string.
pub(crate) fn csv_export_warnings(
    workbook: &opendoc_spreadsheet::SpreadsheetWorkbook,
    sheet_id: &str,
) -> Vec<ModelWarning> {
    let Some(sheet) = workbook.sheets.iter().find(|sheet| sheet.id == sheet_id) else {
        return Vec::new();
    };
    let mut warnings = Vec::new();
    let other_sheets = workbook
        .sheets
        .iter()
        .filter(|other| other.id != sheet_id)
        .map(|other| other.title.clone())
        .collect::<Vec<_>>();
    if !other_sheets.is_empty() {
        warnings.push(ModelWarning {
            code: "csv-export-single-sheet".to_string(),
            message: format!(
                "a CSV file holds one sheet: {} was written, {} other sheet(s) were not ({})",
                sheet.title,
                other_sheets.len(),
                other_sheets.join(", ")
            ),
        });
    }
    let formulas = sheet
        .cells
        .iter()
        .filter(|cell| cell.user_kind == "formula")
        .count();
    if formulas > 0 {
        warnings.push(ModelWarning {
            code: "csv-export-dropped-formulas".to_string(),
            message: format!(
                "{formulas} formula cell(s) were written as their last computed text; a CSV file cannot carry a formula"
            ),
        });
    }
    let formatted = sheet
        .cells
        .iter()
        .filter(|cell| {
            let format = &cell.format;
            format.bold
                || format.italic
                || format.text_color.is_some()
                || format.background_color.is_some()
                || format.horizontal_align.is_some()
        })
        .count();
    if formatted > 0 {
        warnings.push(ModelWarning {
            code: "csv-export-dropped-cell-formatting".to_string(),
            message: format!(
                "bold, italic, colour or alignment on {formatted} cell(s) was dropped; CSV carries text only"
            ),
        });
    }
    if !sheet.merges.is_empty() {
        warnings.push(ModelWarning {
            code: "csv-export-dropped-merged-cells".to_string(),
            message: format!(
                "{} merged range(s) were written as separate cells",
                sheet.merges.len()
            ),
        });
    }
    let commented = sheet
        .cells
        .iter()
        .filter(|cell| !cell.comments.is_empty())
        .count();
    if commented > 0 {
        warnings.push(ModelWarning {
            code: "csv-export-dropped-cell-comments".to_string(),
            message: format!("comments on {commented} cell(s) were dropped"),
        });
    }
    let validated = sheet
        .cells
        .iter()
        .filter(|cell| cell.validation.is_some())
        .count();
    if validated > 0 {
        warnings.push(ModelWarning {
            code: "csv-export-dropped-data-validation".to_string(),
            message: format!("data validation on {validated} cell(s) was dropped"),
        });
    }
    warnings
}

/// What the XLSX writer has no code for.
///
/// Unlike the CSV list this is short, because `export_xlsx` writes almost
/// everything the model holds — sheet visibility, tab colour, frozen panes,
/// row heights, column widths, hidden rows and columns, cell formats, number
/// formats, merges, formulas with their cached results, and defined names.
/// The remaining unsupported model components are named here. Reported as facts about the writer's coverage, which is
/// why the list is not derived from anything in `opendoc-spreadsheet`: if that
/// writer grows support for one of them, this warning becomes wrong and the
/// test below is what says so.
pub(crate) fn xlsx_export_warnings(
    workbook: &opendoc_spreadsheet::SpreadsheetWorkbook,
) -> Vec<ModelWarning> {
    let mut filters = 0usize;
    let mut protected = 0usize;
    let mut validated = 0usize;
    let mut commented = 0usize;
    let mut images = 0usize;
    for sheet in &workbook.sheets {
        // XLSX owns the bare range plus an exact one-value text selection per
        // column. Other operators, multi-value selections, and sort state have
        // deliberately narrower semantics here, so name only those portions
        // as a loss instead of claiming the whole filter vanished.
        filters += sheet
            .filters
            .iter()
            .filter(|filter| !opendoc_spreadsheet::xlsx_filter_is_representable(filter))
            .count();
        protected += sheet.protected_ranges.len();
        images += sheet.images.len();
        for cell in &sheet.cells {
            // This predicate is the writer's precise coverage contract. Keep
            // the warning coupled to it: a rule name alone cannot say whether
            // its operands (for example numeric bounds) fit OOXML.
            if cell.validation.as_ref().is_some_and(|validation| {
                !opendoc_spreadsheet::xlsx_validation_is_representable(validation)
            }) {
                validated += 1;
            }
            // XLSX's legacy note has one author/body record per cell. Do not
            // claim that a deleted or threaded OpenDoc conversation survived
            // just because its first live entry could be written as a note.
            if !cell.comments.is_empty() && (cell.comments.len() != 1 || cell.comments[0].deleted) {
                commented += 1;
            }
        }
    }
    let mut warnings = Vec::new();
    for (count, code, what) in [
        (
            filters,
            "xlsx-export-dropped-filter-options",
            "basic filter(s) lost criteria or sort state the XLSX writer cannot represent exactly",
        ),
        (
            protected,
            "xlsx-export-dropped-protected-ranges",
            "protected range(s) were dropped",
        ),
        (
            validated,
            "xlsx-export-dropped-data-validation",
            "cell(s) with data validation the XLSX writer cannot represent exactly lost their rule",
        ),
        (
            commented,
            "xlsx-export-dropped-cell-comments",
            "cell(s) with comments that cannot fit one XLSX legacy note lost them",
        ),
        (
            images,
            "xlsx-export-dropped-floating-images",
            "floating image(s) were dropped because the XLSX writer has no blob input",
        ),
    ] {
        if count == 0 {
            continue;
        }
        warnings.push(ModelWarning {
            code: code.to_string(),
            message: format!("{count} {what}: the XLSX writer does not write that property"),
        });
    }
    warnings
}

/// One warning per code, with the occurrences it stands for named.
///
/// A producer that reports per *item* — `opendoc-pdf` reports one estimate
/// per block — otherwise hands the same code back three times over, and a
/// warnings panel showing `pdf-estimated-unmeasurable-mark` twice tells the
/// reader nothing the first one did not. This is the DOCX reader's
/// `DroppedCounter` rule applied at the export boundary: collapse by code,
/// keep the first message, and say how many there were.
pub(crate) fn collapse_by_code(warnings: Vec<ModelWarning>) -> Vec<ModelWarning> {
    let mut order: Vec<String> = Vec::new();
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for warning in warnings {
        let messages = grouped.entry(warning.code.clone()).or_default();
        if messages.is_empty() {
            order.push(warning.code);
        }
        messages.push(warning.message);
    }
    order
        .into_iter()
        .map(|code| {
            let messages = grouped.remove(&code).unwrap_or_default();
            let mut message = messages.first().cloned().unwrap_or_default();
            if messages.len() > 1 {
                message.push_str(&format!(" (and {} more)", messages.len() - 1));
            }
            ModelWarning { code, message }
        })
        .collect()
}

/// Walks blocks (including table cells) collecting the bytes behind every
/// image block that the app still holds.
fn collect_image_hashes(
    blocks: &[opendoc_core::Block],
    images: &mut BTreeMap<String, DocxImage>,
    media_types: &BTreeMap<&str, &str>,
    app: &OpenDocApp,
) {
    for block in blocks {
        match &block.kind {
            opendoc_core::BlockKind::Image { blob_hash, .. } => {
                if images.contains_key(blob_hash) {
                    continue;
                }
                let Some(bytes) = app.blob_bytes.get(blob_hash) else {
                    continue;
                };
                images.insert(
                    blob_hash.clone(),
                    DocxImage {
                        media_type: media_types
                            .get(blob_hash.as_str())
                            .copied()
                            .unwrap_or("application/octet-stream")
                            .to_string(),
                        bytes: bytes.clone(),
                    },
                );
            }
            opendoc_core::BlockKind::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_image_hashes(&cell.blocks, images, media_types, app);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Exporter warnings as the app's own warning DTO. They are the same shape as
/// a `ModelWarning` but they never become one: a `ModelWarning` belongs to a
/// document, and these belong to one export.
pub(crate) fn model_warnings(warnings: Vec<ModelWarning>) -> Vec<AppWarning> {
    warnings.iter().map(AppWarning::from_core).collect()
}
