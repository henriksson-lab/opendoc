//! Parsing a named JSON command into the typed command enum.

use crate::command_arg_structs::*;
use crate::command_arg_values::*;
use crate::command_args::*;
use crate::{OpenDocCommand, OpenDocRuntimeMode, OpenDocRuntimeProfile};
use serde_json::Value;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CommandParseError {
    Format(String),
}

impl std::fmt::Display for CommandParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for CommandParseError {}

fn parse_runtime_mode(mode: &str) -> Result<OpenDocRuntimeMode, CommandParseError> {
    match mode {
        "tauri-local" => Ok(OpenDocRuntimeMode::TauriLocal),
        "browser-local" => Ok(OpenDocRuntimeMode::BrowserLocal),
        "hpc-single-user" => Ok(OpenDocRuntimeMode::HpcSingleUser),
        "multi-user-service" => Ok(OpenDocRuntimeMode::MultiUserService),
        other => Err(CommandParseError::Format(format!(
            "unsupported runtime mode {other}"
        ))),
    }
}

pub fn runtime_profile_from_args(args: &Value) -> Result<OpenDocRuntimeProfile, CommandParseError> {
    Ok(OpenDocRuntimeProfile::for_mode_with_capabilities(
        parse_runtime_mode(&arg_string(args, "mode")?)?,
        arg_runtime_storage_backends(args, "storageBackends")?,
        arg_optional_bool(args, "signingEnabled")?,
    ))
}

/// Gestures that extend the previous undo step when repeated quickly on the
/// same block: plain typing and single-character deletes.
pub fn undo_coalesce_key(command: &str, args: &Value) -> Option<String> {
    if command != "apply_editor_input" {
        return None;
    }
    let input_type = args.get("input_type")?.as_str()?;
    if !matches!(
        input_type,
        "insertText" | "insertCompositionText" | "deleteContentBackward" | "deleteContentForward"
    ) {
        return None;
    }
    let block_id = args
        .get("selection")?
        .get("focus")?
        .get("block_id")?
        .as_str()?;
    Some(format!("{input_type}:{block_id}"))
}

pub fn parse_json_command(
    command: &str,
    args: &Value,
) -> Result<Option<OpenDocCommand>, CommandParseError> {
    match command {
        "create_document" => Ok(Some(OpenDocCommand::CreateDocument(CreateDocumentArgs {
            title: arg_string(args, "title")?,
        }))),
        "close_document" => Ok(Some(OpenDocCommand::CloseDocument)),
        "undo_current_edit" => Ok(Some(OpenDocCommand::UndoCurrentEdit)),
        "redo_current_edit" => Ok(Some(OpenDocCommand::RedoCurrentEdit)),
        "get_document" => Ok(Some(OpenDocCommand::GetDocument)),
        "get_audit_view" => Ok(Some(OpenDocCommand::GetAuditView)),
        "set_document_title" => Ok(Some(OpenDocCommand::SetDocumentTitle(
            SetDocumentTitleArgs {
                title: arg_string(args, "title")?,
            },
        ))),
        "set_document_locale" => Ok(Some(OpenDocCommand::SetDocumentLocale(
            SetDocumentLocaleArgs {
                locale: arg_string(args, "locale")?,
            },
        ))),
        "set_bookmark" => Ok(Some(OpenDocCommand::SetBookmark(SetBookmarkArgs {
            bookmark_id: arg_optional_string(args, "bookmarkId")?,
            name: arg_string(args, "name")?,
            block_id: arg_string(args, "blockId")?,
        }))),
        "delete_bookmark" => Ok(Some(OpenDocCommand::DeleteBookmark(BookmarkIdArgs {
            bookmark_id: arg_string(args, "bookmarkId")?,
        }))),
        "add_paragraph" => Ok(Some(OpenDocCommand::AddParagraph(AddParagraphArgs {
            text: arg_string(args, "text")?,
        }))),
        "render_document_html" => Ok(Some(OpenDocCommand::RenderDocumentHtml)),
        "render_suggestion_preview_html" => Ok(Some(OpenDocCommand::RenderSuggestionPreviewHtml(
            SuggestionPreviewArgs {
                suggestion_id: arg_string(args, "suggestionId")?,
                resolution: arg_string(args, "resolution")?,
            },
        ))),
        "get_runtime_profile" => Ok(Some(OpenDocCommand::GetRuntimeProfile(
            runtime_profile_from_args(args)?,
        ))),
        "get_runtime_session" => Ok(Some(OpenDocCommand::GetRuntimeSession(
            GetRuntimeSessionArgs {
                profile: runtime_profile_from_args(args)?,
            },
        ))),
        "authorize_runtime_command" => Ok(Some(OpenDocCommand::AuthorizeRuntimeCommand(
            AuthorizeRuntimeCommandArgs {
                profile: runtime_profile_from_args(args)?,
                command_name: arg_string(args, "commandName")?,
            },
        ))),
        "create_runtime_share_invite" => Ok(Some(OpenDocCommand::CreateRuntimeShareInvite(
            CreateRuntimeShareInviteArgs {
                profile: runtime_profile_from_args(args)?,
                target_subject: arg_optional_string(args, "targetSubject")?,
                requested_role: arg_optional_string(args, "role")?,
            },
        ))),
        "relay_runtime_sync" => Ok(Some(OpenDocCommand::RelayRuntimeSync(
            RelayRuntimeSyncArgs {
                profile: runtime_profile_from_args(args)?,
                operations: arg_relay_operations(args, "operations")?,
            },
        ))),
        "resolve_runtime_document_lookup" => Ok(Some(
            OpenDocCommand::ResolveRuntimeDocumentLookup(ResolveRuntimeDocumentLookupArgs {
                profile: runtime_profile_from_args(args)?,
                document_uuid: arg_optional_string(args, "documentUuid")?,
                doi: arg_optional_string(args, "doi")?,
                service_index: arg_runtime_lookup_entries(args, "serviceIndex")?,
                scanned_documents: arg_runtime_lookup_entries(args, "scannedDocuments")?,
            }),
        )),
        "import_google_docs_json" => Ok(Some(OpenDocCommand::ImportGoogleDocsJson(
            ImportGoogleDocsJsonArgs {
                title: arg_string(args, "title")?,
                json_text: arg_string(args, "jsonText")?,
            },
        ))),
        "import_doc_or_docx_path" => Ok(Some(OpenDocCommand::ImportDocOrDocxPath(
            ImportDocOrDocxPathArgs {
                path: arg_string(args, "path")?,
            },
        ))),
        "export_google_docs_json" => Ok(Some(OpenDocCommand::ExportGoogleDocsJson)),
        "export_docx" => Ok(Some(OpenDocCommand::ExportDocx)),
        "export_odt" => Ok(Some(OpenDocCommand::ExportOdt)),
        "export_pdf" => Ok(Some(OpenDocCommand::ExportPdf)),
        "export_html" => Ok(Some(OpenDocCommand::ExportHtml)),
        "export_text" => Ok(Some(OpenDocCommand::ExportText)),
        "export_image_blob" => Ok(Some(OpenDocCommand::ExportImageBlob(ExportImageBlobArgs {
            blob_hash: arg_string(args, "blobHash")?,
        }))),
        "import_google_sheets_json" => Ok(Some(OpenDocCommand::ImportGoogleSheetsJson(
            ImportGoogleSheetsJsonArgs {
                json_text: arg_string(args, "jsonText")?,
            },
        ))),
        "export_google_sheets_json" => Ok(Some(OpenDocCommand::ExportGoogleSheetsJson)),
        "render_workbook_html" => Ok(Some(OpenDocCommand::RenderWorkbookHtml(
            RenderWorkbookHtmlArgs {
                sheet_id: arg_string(args, "sheetId")?,
            },
        ))),
        "import_docx_base64" => Ok(Some(OpenDocCommand::ImportDocxBase64(
            ImportDocxBase64Args {
                name: arg_string(args, "name")?,
                base64: arg_string(args, "base64")?,
            },
        ))),
        "add_binary_blob" => Ok(Some(OpenDocCommand::AddBinaryBlob(AddBinaryBlobArgs {
            name: arg_string(args, "name")?,
            media_type: arg_string(args, "mediaType")?,
            bytes: arg_u8_vec(args, "bytes")?,
        }))),
        "simulate_shallow_clone" => Ok(Some(OpenDocCommand::SimulateShallowClone)),
        "update_binary_blob_metadata" => Ok(Some(OpenDocCommand::UpdateBinaryBlobMetadata(
            UpdateBinaryBlobMetadataArgs {
                blob_hash: arg_string(args, "blobHash")?,
                name: arg_string(args, "name")?,
                media_type: arg_string(args, "mediaType")?,
            },
        ))),
        "delete_binary_blob" => Ok(Some(OpenDocCommand::DeleteBinaryBlob(
            DeleteBinaryBlobArgs {
                blob_hash: arg_string(args, "blobHash")?,
            },
        ))),
        "restore_binary_blob" => Ok(Some(OpenDocCommand::RestoreBinaryBlob(
            RestoreBinaryBlobArgs {
                blob_hash: arg_string(args, "blobHash")?,
            },
        ))),
        "record_blob_archive_tombstone" => Ok(Some(OpenDocCommand::RecordBlobArchiveTombstone(
            RecordBlobArchiveTombstoneArgs {
                blob_hash: arg_string(args, "blobHash")?,
                archive_locator: arg_string(args, "archiveLocator")?,
                restore_hint: arg_string(args, "restoreHint")?,
                signer: arg_string(args, "signer")?,
                signature: arg_u8_vec(args, "signature")?,
            },
        ))),
        "add_image_block" => Ok(Some(OpenDocCommand::AddImageBlock(AddImageBlockArgs {
            blob_hash: arg_string(args, "blobHash")?,
            alt_text: arg_string(args, "altText")?,
        }))),
        "insert_image_block_after" => Ok(Some(OpenDocCommand::InsertImageBlockAfter(
            InsertImageBlockAfterArgs {
                after_block_id: arg_string(args, "afterBlockId")?,
                blob_hash: arg_string(args, "blobHash")?,
                alt_text: arg_string(args, "altText")?,
            },
        ))),
        "sign_blob_with_openssh_private_key" => Ok(Some(
            OpenDocCommand::SignBlobWithOpenSshPrivateKey(SignBlobWithOpenSshPrivateKeyArgs {
                blob_hash: arg_string(args, "blobHash")?,
                private_key_pem: arg_string(args, "privateKeyPem")?,
                signer_display: arg_string(args, "signerDisplay")?,
            }),
        )),
        "sign_fastq_blob_with_openssh_private_key" => {
            Ok(Some(OpenDocCommand::SignFastqBlobWithOpenSshPrivateKey(
                SignFastqBlobWithOpenSshPrivateKeyArgs {
                    blob_hash: arg_string(args, "blobHash")?,
                    profile: arg_string(args, "profile")?,
                    private_key_pem: arg_string(args, "privateKeyPem")?,
                    signer_display: arg_string(args, "signerDisplay")?,
                },
            )))
        }
        "sign_image_pixels_blob_with_openssh_private_key" => Ok(Some(
            OpenDocCommand::SignImagePixelsBlobWithOpenSshPrivateKey(
                SignImagePixelsBlobWithOpenSshPrivateKeyArgs {
                    blob_hash: arg_string(args, "blobHash")?,
                    width: arg_u32(args, "width")?,
                    height: arg_u32(args, "height")?,
                    pixels: arg_u8_vec(args, "pixels")?,
                    private_key_pem: arg_string(args, "privateKeyPem")?,
                    signer_display: arg_string(args, "signerDisplay")?,
                },
            ),
        )),
        "sign_with_openssh_private_key" => Ok(Some(OpenDocCommand::SignWithOpenSshPrivateKey(
            SignWithOpenSshPrivateKeyArgs {
                private_key_pem: arg_string(args, "privateKeyPem")?,
                signer_display: arg_string(args, "signerDisplay")?,
            },
        ))),
        "sign_current_repository_version_with_openssh_private_key" => Ok(Some(
            OpenDocCommand::SignCurrentRepositoryVersionWithOpenSshPrivateKey(
                SignCurrentRepositoryVersionWithOpenSshPrivateKeyArgs {
                    private_key_pem: arg_string(args, "privateKeyPem")?,
                    signer_display: arg_string(args, "signerDisplay")?,
                },
            ),
        )),
        "verify_current_signature" => Ok(Some(OpenDocCommand::VerifyCurrentSignature(
            VerifyCurrentSignatureArgs {
                private_key_pem: arg_string(args, "privateKeyPem")?,
            },
        ))),
        "verify_current_signatures" => Ok(Some(OpenDocCommand::VerifyCurrentSignatures)),
        "save_local_repository" => Ok(Some(OpenDocCommand::SaveLocalRepository(repository_path(
            args,
        )?))),
        "save_local_repository_or_candidate" => Ok(Some(
            OpenDocCommand::SaveLocalRepositoryOrCandidate(repository_path(args)?),
        )),
        "save_flat_repository" => Ok(Some(OpenDocCommand::SaveFlatRepository(
            repository_namespace(args)?,
        ))),
        "save_flat_repository_or_candidate" => Ok(Some(
            OpenDocCommand::SaveFlatRepositoryOrCandidate(repository_namespace(args)?),
        )),
        "save_opendal_fs_repository" => Ok(Some(OpenDocCommand::SaveOpenDalFsRepository(
            repository_namespace(args)?,
        ))),
        "save_opendal_fs_repository_or_candidate" => Ok(Some(
            OpenDocCommand::SaveOpenDalFsRepositoryOrCandidate(repository_namespace(args)?),
        )),
        "autosave_current_repository" => Ok(Some(OpenDocCommand::AutosaveCurrentRepository)),
        "list_document_versions" => Ok(Some(OpenDocCommand::ListDocumentVersions(
            ListDocumentVersionsArgs {
                limit: arg_optional_u32(args, "limit")?,
            },
        ))),
        "open_document_at_version" => Ok(Some(OpenDocCommand::OpenDocumentAtVersion(
            document_version(args)?,
        ))),
        "diff_document_versions" => Ok(Some(OpenDocCommand::DiffDocumentVersions(
            DiffDocumentVersionsArgs {
                from_manifest: arg_string(args, "fromManifest")?,
                to_manifest: arg_string(args, "toManifest")?,
            },
        ))),
        "name_document_version" => Ok(Some(OpenDocCommand::NameDocumentVersion(
            NameDocumentVersionArgs {
                manifest: arg_string(args, "manifest")?,
                label: arg_string(args, "label")?,
                author: arg_string(args, "author")?,
            },
        ))),
        "restore_document_version" => Ok(Some(OpenDocCommand::RestoreDocumentVersion(
            document_version(args)?,
        ))),
        "compact_local_repository" => Ok(Some(OpenDocCommand::CompactLocalRepository(
            CompactLocalRepositoryArgs {
                path: arg_string(args, "path")?,
                pack_name: arg_string(args, "packName")?,
            },
        ))),
        "open_local_repository" => Ok(Some(OpenDocCommand::OpenLocalRepository(
            repository_document(args)?,
        ))),
        "recover_session" => Ok(Some(OpenDocCommand::RecoverSession(RecoverySessionArgs {
            session_id: arg_string(args, "sessionId")?,
        }))),
        "discard_recovery_session" => Ok(Some(OpenDocCommand::DiscardRecoverySession(
            RecoverySessionArgs {
                session_id: arg_string(args, "sessionId")?,
            },
        ))),
        "scan_local_repository" => Ok(Some(OpenDocCommand::ScanLocalRepository(repository_path(
            args,
        )?))),
        "open_flat_repository" => Ok(Some(OpenDocCommand::OpenFlatRepository(
            repository_namespace_document(args)?,
        ))),
        "open_opendal_fs_repository" => Ok(Some(OpenDocCommand::OpenOpenDalFsRepository(
            repository_namespace_document(args)?,
        ))),
        "merge_local_repository_candidates" => Ok(Some(
            OpenDocCommand::MergeLocalRepositoryCandidates(repository_document(args)?),
        )),
        "merge_flat_repository_candidates" => Ok(Some(
            OpenDocCommand::MergeFlatRepositoryCandidates(repository_namespace_document(args)?),
        )),
        "merge_opendal_fs_repository_candidates" => {
            Ok(Some(OpenDocCommand::MergeOpenDalFsRepositoryCandidates(
                repository_namespace_document(args)?,
            )))
        }
        "open_local_repository_by_doi" => Ok(Some(OpenDocCommand::OpenLocalRepositoryByDoi(
            repository_doi(args)?,
        ))),
        "open_flat_repository_by_doi" => Ok(Some(OpenDocCommand::OpenFlatRepositoryByDoi(
            repository_namespace_doi(args)?,
        ))),
        "open_opendal_fs_repository_by_doi" => Ok(Some(
            OpenDocCommand::OpenOpenDalFsRepositoryByDoi(repository_namespace_doi(args)?),
        )),
        "set_document_doi" => Ok(Some(OpenDocCommand::SetDocumentDoi(SetDocumentDoiArgs {
            doi: arg_string(args, "doi")?,
        }))),
        "insert_paragraph_after" => Ok(Some(OpenDocCommand::InsertParagraphAfter(
            InsertParagraphAfterArgs {
                after_block_id: arg_optional_string(args, "afterBlockId")?,
                text: arg_string(args, "text")?,
            },
        ))),
        "split_paragraph_at_inline" => Ok(Some(OpenDocCommand::SplitParagraphAtInline(
            SplitParagraphAtInlineArgs {
                inline_id: arg_string(args, "inlineId")?,
            },
        ))),
        "split_paragraph_at_text_offset" => Ok(Some(OpenDocCommand::SplitParagraphAtTextOffset(
            SplitParagraphAtTextOffsetArgs {
                block_id: arg_string(args, "blockId")?,
                inline_id: arg_string(args, "inlineId")?,
                offset: arg_u32(args, "offset")? as usize,
            },
        ))),
        "join_paragraph_with_previous" => Ok(Some(OpenDocCommand::JoinParagraphWithPrevious(
            block_id_args(args)?,
        ))),
        "delete_block" => Ok(Some(OpenDocCommand::DeleteBlock(block_id_args(args)?))),
        "move_block" => Ok(Some(OpenDocCommand::MoveBlock(MoveBlockArgs {
            block_id: arg_string(args, "blockId")?,
            anchor_block_id: arg_string(args, "anchorBlockId")?,
            placement: arg_string(args, "placement")?,
        }))),
        "set_block_text_style" => Ok(Some(OpenDocCommand::SetBlockTextStyle(
            SetBlockTextStyleArgs {
                block_id: arg_string(args, "blockId")?,
                style: arg_string(args, "style")?,
                level: arg_u8(args, "level")?,
                list_kind: arg_string(args, "listKind")?,
            },
        ))),
        "set_editor_selection_block_style" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockStyle(SetEditorSelectionBlockStyleArgs {
                selection: editor_selection_arg(args)?,
                style: arg_string(args, "style")?,
                level: arg_u8(args, "level")?,
                list_kind: arg_string(args, "listKind")?,
            }),
        )),
        "set_page_setup" => Ok(Some(OpenDocCommand::SetPageSetup(SetPageSetupArgs {
            width_twips: arg_i32(args, "widthTwips")?,
            height_twips: arg_i32(args, "heightTwips")?,
            margin_top_twips: arg_i32(args, "marginTopTwips")?,
            margin_bottom_twips: arg_i32(args, "marginBottomTwips")?,
            margin_start_twips: arg_i32(args, "marginStartTwips")?,
            margin_end_twips: arg_i32(args, "marginEndTwips")?,
        }))),
        "set_page_orientation" => Ok(Some(OpenDocCommand::SetPageOrientation(
            SetPageOrientationArgs {
                orientation: arg_string(args, "orientation")?,
            },
        ))),
        "set_page_furniture" => Ok(Some(OpenDocCommand::SetPageFurniture(
            SetPageFurnitureArgs {
                slot: arg_string(args, "slot")?,
                text: arg_string(args, "text")?,
                field: arg_string(args, "field")?,
                alignment: arg_string(args, "alignment")?,
            },
        ))),
        "set_page_furniture_html" => Ok(Some(OpenDocCommand::SetPageFurnitureHtml(
            SetPageFurnitureHtmlArgs {
                slot: arg_string(args, "slot")?,
                html: arg_string(args, "html")?,
            },
        ))),
        "clear_page_furniture" => Ok(Some(OpenDocCommand::ClearPageFurniture(
            PageFurnitureSlotArgs {
                slot: arg_string(args, "slot")?,
            },
        ))),
        "clear_page_furniture_override" => Ok(Some(OpenDocCommand::ClearPageFurnitureOverride(
            PageFurnitureSlotArgs {
                slot: arg_string(args, "slot")?,
            },
        ))),
        "set_block_alignment" => Ok(Some(OpenDocCommand::SetBlockAlignment(
            SetBlockNamedValueArgs {
                block_id: arg_string(args, "blockId")?,
                value: arg_string(args, "alignment")?,
            },
        ))),
        "set_editor_selection_block_alignment" => {
            Ok(Some(OpenDocCommand::SetEditorSelectionBlockAlignment(
                SetEditorSelectionBlockNamedValueArgs {
                    selection: editor_selection_arg(args)?,
                    value: arg_string(args, "alignment")?,
                },
            )))
        }
        "set_block_indent_start" => Ok(Some(OpenDocCommand::SetBlockIndentStart(
            SetBlockLengthArgs {
                block_id: arg_string(args, "blockId")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "set_editor_selection_block_indent_start" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockIndentStart(SetEditorSelectionBlockLengthArgs {
                selection: editor_selection_arg(args)?,
                twips: arg_i32(args, "twips")?,
            }),
        )),
        "set_block_indent_end" => Ok(Some(OpenDocCommand::SetBlockIndentEnd(
            SetBlockLengthArgs {
                block_id: arg_string(args, "blockId")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "set_editor_selection_block_indent_end" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockIndentEnd(SetEditorSelectionBlockLengthArgs {
                selection: editor_selection_arg(args)?,
                twips: arg_i32(args, "twips")?,
            }),
        )),
        "set_block_indent_first_line" => Ok(Some(OpenDocCommand::SetBlockIndentFirstLine(
            SetBlockLengthArgs {
                block_id: arg_string(args, "blockId")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "set_editor_selection_block_indent_first_line" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockIndentFirstLine(
                SetEditorSelectionBlockLengthArgs {
                    selection: editor_selection_arg(args)?,
                    twips: arg_i32(args, "twips")?,
                },
            ),
        )),
        "set_block_line_spacing" => Ok(Some(OpenDocCommand::SetBlockLineSpacing(
            SetBlockLineSpacingArgs {
                block_id: arg_string(args, "blockId")?,
                mode: arg_string(args, "spacingMode")?,
                value: arg_i32(args, "spacingValue")?,
            },
        ))),
        "set_editor_selection_block_line_spacing" => {
            Ok(Some(OpenDocCommand::SetEditorSelectionBlockLineSpacing(
                SetEditorSelectionBlockLineSpacingArgs {
                    selection: editor_selection_arg(args)?,
                    mode: arg_string(args, "spacingMode")?,
                    value: arg_i32(args, "spacingValue")?,
                },
            )))
        }
        "set_block_space_before" => Ok(Some(OpenDocCommand::SetBlockSpaceBefore(
            SetBlockLengthArgs {
                block_id: arg_string(args, "blockId")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "set_editor_selection_block_space_before" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockSpaceBefore(SetEditorSelectionBlockLengthArgs {
                selection: editor_selection_arg(args)?,
                twips: arg_i32(args, "twips")?,
            }),
        )),
        "set_block_space_after" => Ok(Some(OpenDocCommand::SetBlockSpaceAfter(
            SetBlockLengthArgs {
                block_id: arg_string(args, "blockId")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "set_editor_selection_block_space_after" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockSpaceAfter(SetEditorSelectionBlockLengthArgs {
                selection: editor_selection_arg(args)?,
                twips: arg_i32(args, "twips")?,
            }),
        )),
        "set_block_direction" => Ok(Some(OpenDocCommand::SetBlockDirection(
            SetBlockNamedValueArgs {
                block_id: arg_string(args, "blockId")?,
                value: arg_string(args, "direction")?,
            },
        ))),
        "set_editor_selection_block_direction" => {
            Ok(Some(OpenDocCommand::SetEditorSelectionBlockDirection(
                SetEditorSelectionBlockNamedValueArgs {
                    selection: editor_selection_arg(args)?,
                    value: arg_string(args, "direction")?,
                },
            )))
        }
        "set_block_keep_with_next" => Ok(Some(OpenDocCommand::SetBlockKeepWithNext(
            SetBlockBoolArgs {
                block_id: arg_string(args, "blockId")?,
                value: arg_bool(args, "keepWithNext")?,
            },
        ))),
        "set_editor_selection_block_keep_with_next" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockKeepWithNext(SetEditorSelectionBlockBoolArgs {
                selection: editor_selection_arg(args)?,
                value: arg_bool(args, "keepWithNext")?,
            }),
        )),
        "set_block_background" => Ok(Some(OpenDocCommand::SetBlockBackground(
            SetBlockColorArgs {
                block_id: arg_string(args, "blockId")?,
                color: arg_string(args, "color")?,
            },
        ))),
        "set_editor_selection_block_background" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockBackground(SetEditorSelectionBlockColorArgs {
                selection: editor_selection_arg(args)?,
                color: arg_string(args, "color")?,
            }),
        )),
        "set_block_border" => Ok(Some(OpenDocCommand::SetBlockBorder(SetBlockBorderArgs {
            block_id: arg_string(args, "blockId")?,
            style: arg_string(args, "style")?,
            twips: arg_i32(args, "twips")?,
            color: arg_string(args, "color")?,
        }))),
        "set_editor_selection_block_border" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockBorder(SetEditorSelectionBlockBorderArgs {
                selection: editor_selection_arg(args)?,
                style: arg_string(args, "style")?,
                twips: arg_i32(args, "twips")?,
                color: arg_string(args, "color")?,
            }),
        )),
        "clear_block_property" => Ok(Some(OpenDocCommand::ClearBlockProperty(
            ClearBlockPropertyArgs {
                block_id: arg_string(args, "blockId")?,
                key: arg_string(args, "key")?,
            },
        ))),
        "clear_editor_selection_block_property" => {
            Ok(Some(OpenDocCommand::ClearEditorSelectionBlockProperty(
                ClearEditorSelectionBlockPropertyArgs {
                    selection: editor_selection_arg(args)?,
                    key: arg_string(args, "key")?,
                },
            )))
        }
        "set_list_item_checked" => Ok(Some(OpenDocCommand::SetListItemChecked(
            SetListItemCheckedArgs {
                block_id: arg_string(args, "blockId")?,
                checked: arg_bool(args, "checked")?,
            },
        ))),
        "layout_document" => Ok(Some(OpenDocCommand::LayoutDocument)),
        "find_in_document" => Ok(Some(OpenDocCommand::FindInDocument(FindInDocumentArgs {
            find: find_options_arg(args)?,
        }))),
        "replace_match_in_document" => Ok(Some(OpenDocCommand::ReplaceMatchInDocument(
            ReplaceMatchInDocumentArgs {
                find: find_options_arg(args)?,
                replacement: arg_string(args, "replacement")?,
                match_index: arg_u32(args, "matchIndex")? as usize,
            },
        ))),
        "replace_all_in_document" => Ok(Some(OpenDocCommand::ReplaceAllInDocument(
            ReplaceAllInDocumentArgs {
                find: find_options_arg(args)?,
                replacement: arg_string(args, "replacement")?,
            },
        ))),
        "adjust_editor_selection_indent" => Ok(Some(OpenDocCommand::AdjustEditorSelectionIndent(
            AdjustEditorSelectionListIndentArgs {
                selection: editor_selection_arg(args)?,
                delta: arg_i8(args, "delta")?,
            },
        ))),
        "add_heading" => Ok(Some(OpenDocCommand::AddHeading(AddHeadingArgs {
            text: arg_string(args, "text")?,
            level: arg_u8(args, "level")?,
        }))),
        "update_heading_level" => Ok(Some(OpenDocCommand::UpdateHeadingLevel(
            UpdateHeadingLevelArgs {
                block_id: arg_string(args, "blockId")?,
                level: arg_u8(args, "level")?,
            },
        ))),
        "add_link" => Ok(Some(OpenDocCommand::AddLink(AddLinkArgs {
            text: arg_string(args, "text")?,
            href: arg_string(args, "href")?,
        }))),
        "insert_link_after" => Ok(Some(OpenDocCommand::InsertLinkAfter(InsertLinkAfterArgs {
            block_id: arg_string(args, "blockId")?,
            after_inline_id: arg_optional_string(args, "afterInlineId")?,
            text: arg_string(args, "text")?,
            href: arg_string(args, "href")?,
        }))),
        "add_mention" => Ok(Some(OpenDocCommand::AddMention(AddMentionArgs {
            label: arg_string(args, "label")?,
        }))),
        "insert_mention_after" => Ok(Some(OpenDocCommand::InsertMentionAfter(
            InsertMentionAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
                label: arg_string(args, "label")?,
            },
        ))),
        "insert_date_chip_after" => Ok(Some(OpenDocCommand::InsertDateChipAfter(
            InsertDateChipAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
                date: arg_string(args, "date")?,
            },
        ))),
        "add_footnote_ref" => Ok(Some(OpenDocCommand::AddFootnoteRef)),
        "add_endnote_ref" => Ok(Some(OpenDocCommand::AddEndnoteRef)),
        "insert_footnote_ref_after" => Ok(Some(OpenDocCommand::InsertFootnoteRefAfter(
            InsertFootnoteRefAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
            },
        ))),
        "insert_endnote_ref_after" => Ok(Some(OpenDocCommand::InsertEndnoteRefAfter(
            InsertFootnoteRefAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
            },
        ))),
        "update_footnote_body" => Ok(Some(OpenDocCommand::UpdateFootnoteBody(
            UpdateFootnoteBodyArgs {
                footnote_id: arg_string(args, "footnoteId")?,
                body: arg_string(args, "body")?,
            },
        ))),
        "add_equation" => Ok(Some(OpenDocCommand::AddEquation(AddEquationArgs {
            source: arg_string(args, "source")?,
        }))),
        "insert_equation_after" => Ok(Some(OpenDocCommand::InsertEquationAfter(
            InsertEquationAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
                source: arg_string(args, "source")?,
            },
        ))),
        "add_equation_block" => Ok(Some(OpenDocCommand::AddEquationBlock(AddEquationArgs {
            source: arg_string(args, "source")?,
        }))),
        "insert_equation_block_after" => Ok(Some(OpenDocCommand::InsertEquationBlockAfter(
            InsertEquationBlockAfterArgs {
                after_block_id: arg_string(args, "afterBlockId")?,
                source: arg_string(args, "source")?,
            },
        ))),
        "add_list_item" => Ok(Some(OpenDocCommand::AddListItem(AddListItemArgs {
            text: arg_string(args, "text")?,
            level: arg_u8(args, "level")?,
            list_kind: arg_string(args, "listKind")?,
        }))),
        "insert_list_item_after" => Ok(Some(OpenDocCommand::InsertListItemAfter(
            InsertListItemAfterArgs {
                after_block_id: arg_string(args, "afterBlockId")?,
                text: arg_string(args, "text")?,
                level: arg_u8(args, "level")?,
                list_kind: arg_string(args, "listKind")?,
            },
        ))),
        "update_list_item" => Ok(Some(OpenDocCommand::UpdateListItem(UpdateListItemArgs {
            block_id: arg_string(args, "blockId")?,
            level: arg_u8(args, "level")?,
            list_kind: arg_string(args, "listKind")?,
        }))),
        "set_ordered_list_start" => Ok(Some(OpenDocCommand::SetOrderedListStart(
            SetOrderedListStartArgs {
                block_id: arg_string(args, "blockId")?,
                start: arg_u32(args, "start")?,
            },
        ))),
        "set_ordered_list_format" => Ok(Some(OpenDocCommand::SetOrderedListFormat(
            SetOrderedListFormatArgs {
                block_id: arg_string(args, "blockId")?,
                format: arg_string(args, "format")?,
            },
        ))),
        "set_bullet_list_marker" => Ok(Some(OpenDocCommand::SetBulletListMarker(
            SetBulletListMarkerArgs {
                block_id: arg_string(args, "blockId")?,
                marker: arg_string(args, "marker")?,
            },
        ))),
        "adjust_editor_selection_list_indent" => Ok(Some(
            OpenDocCommand::AdjustEditorSelectionListIndent(AdjustEditorSelectionListIndentArgs {
                selection: editor_selection_arg(args)?,
                delta: arg_i8(args, "delta")?,
            }),
        )),
        "insert_page_break_after" => Ok(Some(OpenDocCommand::InsertPageBreakAfter(
            after_block_args(args)?,
        ))),
        "add_page_break" => Ok(Some(OpenDocCommand::AddPageBreak)),
        "insert_horizontal_rule_after" => Ok(Some(OpenDocCommand::InsertHorizontalRuleAfter(
            after_block_args(args)?,
        ))),
        "add_horizontal_rule" => Ok(Some(OpenDocCommand::AddHorizontalRule)),
        "insert_table_of_contents_after" => Ok(Some(OpenDocCommand::InsertTableOfContentsAfter(
            after_block_args(args)?,
        ))),
        "insert_bibliography_after" => Ok(Some(OpenDocCommand::InsertBibliographyAfter(
            after_block_args(args)?,
        ))),
        "insert_table_after" => Ok(Some(OpenDocCommand::InsertTableAfter(
            insert_table_after_args(args)?,
        ))),
        "add_table" => Ok(Some(OpenDocCommand::AddTable)),
        "add_table_row" => Ok(Some(OpenDocCommand::AddTableRow(AddTableRowArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            after_row: arg_optional_string(args, "afterRow")?,
            text: arg_string(args, "text")?,
        }))),
        "delete_table_row" => Ok(Some(OpenDocCommand::DeleteTableRow(DeleteTableRowArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            row_id: arg_string(args, "rowId")?,
        }))),
        "add_table_cell" => Ok(Some(OpenDocCommand::AddTableCell(AddTableCellArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            row_id: arg_string(args, "rowId")?,
            after_cell: arg_optional_string(args, "afterCell")?,
            text: arg_string(args, "text")?,
        }))),
        "delete_table_cell" => Ok(Some(OpenDocCommand::DeleteTableCell(DeleteTableCellArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            row_id: arg_string(args, "rowId")?,
            cell_id: arg_string(args, "cellId")?,
        }))),
        "insert_table_column" => Ok(Some(OpenDocCommand::InsertTableColumn(
            InsertTableColumnArgs {
                table_block_id: arg_string(args, "tableBlockId")?,
                after_column_id: arg_optional_string(args, "afterColumnId")?,
            },
        ))),
        "delete_table_column" => Ok(Some(OpenDocCommand::DeleteTableColumn(TableColumnArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            column_id: arg_string(args, "columnId")?,
        }))),
        "set_table_column_width" => Ok(Some(OpenDocCommand::SetTableColumnWidth(
            SetTableColumnWidthArgs {
                table_block_id: arg_string(args, "tableBlockId")?,
                column_id: arg_string(args, "columnId")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "clear_table_column_width" => Ok(Some(OpenDocCommand::ClearTableColumnWidth(
            TableColumnArgs {
                table_block_id: arg_string(args, "tableBlockId")?,
                column_id: arg_string(args, "columnId")?,
            },
        ))),
        "set_table_row_height" => Ok(Some(OpenDocCommand::SetTableRowHeight(
            SetTableRowHeightArgs {
                table_block_id: arg_string(args, "tableBlockId")?,
                row_id: arg_string(args, "rowId")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "clear_table_row_height" => Ok(Some(OpenDocCommand::ClearTableRowHeight(TableRowArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            row_id: arg_string(args, "rowId")?,
        }))),
        "set_table_row_header" => Ok(Some(OpenDocCommand::SetTableRowHeader(
            SetTableRowHeaderArgs {
                table_block_id: arg_string(args, "tableBlockId")?,
                row_id: arg_string(args, "rowId")?,
                header: arg_bool(args, "header")?,
            },
        ))),
        "sort_table_rows" => Ok(Some(OpenDocCommand::SortTableRows(SortTableRowsArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            column_id: arg_string(args, "columnId")?,
            descending: arg_bool(args, "descending")?,
        }))),
        "set_table_border" => Ok(Some(OpenDocCommand::SetTableBorder(SetTableBorderArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            style: arg_string(args, "style")?,
            twips: arg_i32(args, "twips")?,
            color: arg_string(args, "color")?,
        }))),
        "clear_table_border" => Ok(Some(OpenDocCommand::ClearTableBorder(TableArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
        }))),
        "set_table_alignment" => Ok(Some(OpenDocCommand::SetTableAlignment(
            SetTableAlignmentArgs {
                table_block_id: arg_string(args, "tableBlockId")?,
                alignment: arg_string(args, "alignment")?,
            },
        ))),
        "clear_table_alignment" => Ok(Some(OpenDocCommand::ClearTableAlignment(TableArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
        }))),
        "merge_table_cells" => Ok(Some(OpenDocCommand::MergeTableCells(MergeTableCellsArgs {
            cell_id: arg_string(args, "cellId")?,
            row_span: arg_u32(args, "rowSpan")?,
            column_span: arg_u32(args, "columnSpan")?,
        }))),
        "split_table_cell" => Ok(Some(OpenDocCommand::SplitTableCell(TableCellArgs {
            cell_id: arg_string(args, "cellId")?,
        }))),
        "set_table_cell_background" => Ok(Some(OpenDocCommand::SetTableCellBackground(
            SetTableCellBackgroundArgs {
                cell_id: arg_string(args, "cellId")?,
                color: arg_string(args, "color")?,
            },
        ))),
        "set_table_cell_border" => Ok(Some(OpenDocCommand::SetTableCellBorder(
            SetTableCellBorderArgs {
                cell_id: arg_string(args, "cellId")?,
                edge: arg_string(args, "edge")?,
                style: arg_string(args, "style")?,
                twips: arg_i32(args, "twips")?,
                color: arg_string(args, "color")?,
            },
        ))),
        "set_table_cell_vertical_alignment" => Ok(Some(
            OpenDocCommand::SetTableCellVerticalAlignment(SetTableCellVerticalAlignmentArgs {
                cell_id: arg_string(args, "cellId")?,
                alignment: arg_string(args, "alignment")?,
            }),
        )),
        "set_table_cell_row_header" => Ok(Some(OpenDocCommand::SetTableCellRowHeader(
            SetTableCellRowHeaderArgs {
                cell_id: arg_string(args, "cellId")?,
                row_header: arg_bool(args, "rowHeader")?,
            },
        ))),
        "set_table_cell_padding" => Ok(Some(OpenDocCommand::SetTableCellPadding(
            SetTableCellPaddingArgs {
                cell_id: arg_string(args, "cellId")?,
                edge: arg_string(args, "edge")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "clear_table_cell_property" => Ok(Some(OpenDocCommand::ClearTableCellProperty(
            ClearTableCellPropertyArgs {
                cell_id: arg_string(args, "cellId")?,
                key: arg_string(args, "key")?,
            },
        ))),
        "add_citation" => Ok(Some(OpenDocCommand::AddCitation)),
        "insert_citation" => Ok(Some(OpenDocCommand::InsertCitation(InsertCitationArgs {
            reference_id: arg_string(args, "referenceId")?,
            after_inline_id: arg_optional_string(args, "afterInlineId")?,
            locator: arg_optional_string(args, "locator")?,
            label: arg_optional_string(args, "label")?,
            prefix: arg_optional_string(args, "prefix")?,
            suffix: arg_optional_string(args, "suffix")?,
            suppress_author: arg_bool(args, "suppressAuthor")?,
        }))),
        "insert_citation_group" => Ok(Some(OpenDocCommand::InsertCitationGroup(
            InsertCitationGroupArgs {
                items: arg_citation_items(args, "items")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
            },
        ))),
        "insert_footnote_citation_group" => Ok(Some(OpenDocCommand::InsertFootnoteCitationGroup(
            InsertFootnoteCitationGroupArgs {
                footnote_id: arg_string(args, "footnoteId")?,
                items: arg_citation_items(args, "items")?,
            },
        ))),
        "insert_footnote_citation_after" => Ok(Some(OpenDocCommand::InsertFootnoteCitationAfter(
            InsertFootnoteCitationAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
                items: arg_citation_items(args, "items")?,
            },
        ))),
        "update_citation_group_items" => Ok(Some(OpenDocCommand::UpdateCitationGroupItems(
            UpdateCitationGroupItemsArgs {
                citation_id: arg_string(args, "citationId")?,
                items: arg_citation_items(args, "items")?,
            },
        ))),
        "set_citation_style" => Ok(Some(OpenDocCommand::SetCitationStyle(
            SetCitationStyleArgs {
                style: arg_string(args, "style")?,
                locale: arg_string(args, "locale")?,
            },
        ))),
        "add_comment" => Ok(Some(OpenDocCommand::AddComment(author_body_args(args)?))),
        "add_text_range_comment" => Ok(Some(OpenDocCommand::AddTextRangeComment(
            text_range_author_body_args(args)?,
        ))),
        "add_block_comment" => Ok(Some(OpenDocCommand::AddBlockComment(
            block_author_body_args(args)?,
        ))),
        "add_comment_reply" => Ok(Some(OpenDocCommand::AddCommentReply(
            thread_author_body_args(args)?,
        ))),
        "resolve_comment_thread" => Ok(Some(OpenDocCommand::ResolveCommentThread(
            ResolveCommentThreadArgs {
                thread_id: arg_string(args, "threadId")?,
                resolved_by: arg_string(args, "resolvedBy")?,
            },
        ))),
        "reopen_comment_thread" => Ok(Some(OpenDocCommand::ReopenCommentThread(thread_id_args(
            args,
        )?))),
        "set_comment_thread_action" => Ok(Some(OpenDocCommand::SetCommentThreadAction(
            SetCommentThreadActionArgs {
                thread_id: arg_string(args, "threadId")?,
                assignee: arg_optional_string(args, "assignee")?,
                due_at_ms: arg_optional_u64(args, "dueAtMs")?,
                completed: arg_bool(args, "completed")?,
                completed_by: arg_optional_string(args, "completedBy")?,
            },
        ))),
        "set_comment_thread_reaction" => Ok(Some(OpenDocCommand::SetCommentThreadReaction(
            SetCommentThreadReactionArgs {
                thread_id: arg_string(args, "threadId")?,
                emoji: arg_string(args, "emoji")?,
                actor: arg_string(args, "actor")?,
                present: arg_bool(args, "present")?,
            },
        ))),
        "delete_comment_thread" => Ok(Some(OpenDocCommand::DeleteCommentThread(thread_id_args(
            args,
        )?))),
        "restore_comment_thread" => Ok(Some(OpenDocCommand::RestoreCommentThread(thread_id_args(
            args,
        )?))),
        "delete_comment" => Ok(Some(OpenDocCommand::DeleteComment(thread_comment_id_args(
            args,
        )?))),
        "restore_comment" => Ok(Some(OpenDocCommand::RestoreComment(
            thread_comment_id_args(args)?,
        ))),
        "update_comment" => Ok(Some(OpenDocCommand::UpdateComment(UpdateCommentArgs {
            thread_id: arg_string(args, "threadId")?,
            comment_id: arg_string(args, "commentId")?,
            body: arg_string(args, "body")?,
        }))),
        "add_suggestion" => Ok(Some(OpenDocCommand::AddSuggestion(author_text_args(args)?))),
        "add_text_range_suggestion" => Ok(Some(OpenDocCommand::AddTextRangeSuggestion(
            text_range_author_text_args(args)?,
        ))),
        "add_block_suggestion" => Ok(Some(OpenDocCommand::AddBlockSuggestion(
            block_author_text_args(args)?,
        ))),
        "add_block_delete_suggestion" => Ok(Some(OpenDocCommand::AddBlockDeleteSuggestion(
            BlockDeleteSuggestionArgs {
                block_id: arg_string(args, "blockId")?,
                author: arg_string(args, "author")?,
            },
        ))),
        "add_block_insert_suggestion" => Ok(Some(OpenDocCommand::AddBlockInsertSuggestion(
            BlockInsertSuggestionArgs {
                block_id: arg_string(args, "blockId")?,
                author: arg_string(args, "author")?,
                text: arg_string(args, "text")?,
            },
        ))),
        "add_block_replace_suggestion" => Ok(Some(OpenDocCommand::AddBlockReplaceSuggestion(
            BlockReplaceSuggestionArgs {
                block_id: arg_string(args, "blockId")?,
                author: arg_string(args, "author")?,
                text: arg_string(args, "text")?,
            },
        ))),
        "add_delete_suggestion" => Ok(Some(OpenDocCommand::AddDeleteSuggestion(
            DeleteSuggestionArgs {
                author: arg_string(args, "author")?,
                inline_id: arg_string(args, "inlineId")?,
            },
        ))),
        "add_text_range_delete_suggestion" => Ok(Some(
            OpenDocCommand::AddTextRangeDeleteSuggestion(text_range_author_args(args)?),
        )),
        "add_format_suggestion" => Ok(Some(OpenDocCommand::AddFormatSuggestion(
            format_suggestion_args(args)?,
        ))),
        "add_text_range_format_suggestion" => Ok(Some(
            OpenDocCommand::AddTextRangeFormatSuggestion(TextRangeFormatSuggestionArgs {
                start_inline_id: arg_string(args, "startInlineId")?,
                end_inline_id: arg_string(args, "endInlineId")?,
                author: arg_string(args, "author")?,
                mark_kind: arg_string(args, "markKind")?,
                value: arg_optional_string(args, "value")?,
            }),
        )),
        "add_text_range_format_removal_suggestion" => {
            Ok(Some(OpenDocCommand::AddTextRangeFormatRemovalSuggestion(
                TextRangeFormatRemovalSuggestionArgs {
                    start_inline_id: arg_string(args, "startInlineId")?,
                    end_inline_id: arg_string(args, "endInlineId")?,
                    author: arg_string(args, "author")?,
                    mark_kind: arg_string(args, "markKind")?,
                    value: arg_optional_string(args, "value")?,
                },
            )))
        }
        "add_text_range_format_replacement_suggestion" => Ok(Some(
            OpenDocCommand::AddTextRangeFormatReplacementSuggestion(
                TextRangeFormatReplacementSuggestionArgs {
                    start_inline_id: arg_string(args, "startInlineId")?,
                    end_inline_id: arg_string(args, "endInlineId")?,
                    author: arg_string(args, "author")?,
                    mark_kind: arg_string(args, "markKind")?,
                    expected_value: arg_string(args, "expectedValue")?,
                    value: arg_string(args, "value")?,
                },
            ),
        )),
        "add_link_change_suggestion" => Ok(Some(OpenDocCommand::AddLinkChangeSuggestion(
            LinkChangeSuggestionArgs {
                inline_id: arg_string(args, "inlineId")?,
                author: arg_string(args, "author")?,
                href: arg_optional_string(args, "href")?,
            },
        ))),
        "add_paragraph_style_suggestion" => Ok(Some(OpenDocCommand::AddParagraphStyleSuggestion(
            ParagraphStyleSuggestionArgs {
                block_id: arg_string(args, "blockId")?,
                author: arg_string(args, "author")?,
                style: arg_string(args, "style")?,
            },
        ))),
        "update_suggestion" => Ok(Some(OpenDocCommand::UpdateSuggestion(
            UpdateSuggestionArgs {
                suggestion_id: arg_string(args, "suggestionId")?,
                text: arg_string(args, "text")?,
            },
        ))),
        "accept_suggestion" => Ok(Some(OpenDocCommand::AcceptSuggestion(
            AcceptSuggestionArgs {
                suggestion_id: arg_string(args, "suggestionId")?,
                accepted_by: arg_string(args, "acceptedBy")?,
            },
        ))),
        "accept_all_suggestions" => Ok(Some(OpenDocCommand::AcceptAllSuggestions(
            AcceptAllSuggestionsArgs {
                accepted_by: arg_string(args, "acceptedBy")?,
            },
        ))),
        "reject_suggestion" => Ok(Some(OpenDocCommand::RejectSuggestion(
            RejectSuggestionArgs {
                suggestion_id: arg_string(args, "suggestionId")?,
                rejected_by: arg_string(args, "rejectedBy")?,
            },
        ))),
        "reject_all_suggestions" => Ok(Some(OpenDocCommand::RejectAllSuggestions(
            RejectAllSuggestionsArgs {
                rejected_by: arg_string(args, "rejectedBy")?,
            },
        ))),
        "describe_editor_selection" => Ok(Some(OpenDocCommand::DescribeEditorSelection(
            editor_selection_arg(args)?,
        ))),
        "select_all_editor_content" => Ok(Some(OpenDocCommand::SelectAllEditorContent)),
        "apply_editor_input" => Ok(Some(OpenDocCommand::ApplyEditorInput(
            serde_json::from_value(args.clone())
                .map_err(|err| CommandParseError::Format(format!("invalid editor input: {err}")))?,
        ))),
        "apply_editor_mark" => Ok(Some(OpenDocCommand::ApplyEditorMark(
            serde_json::from_value(args.clone()).map_err(|err| {
                CommandParseError::Format(format!("invalid editor mark input: {err}"))
            })?,
        ))),
        "import_bibtex" => Ok(Some(OpenDocCommand::ImportBibtex(ImportBibtexArgs {
            source: arg_string(args, "source")?,
        }))),
        "add_bibliography_reference" => Ok(Some(OpenDocCommand::AddBibliographyReference(
            bibliography_reference_metadata_args(args)?,
        ))),
        "update_bibliography_reference" => Ok(Some(OpenDocCommand::UpdateBibliographyReference(
            UpdateBibliographyReferenceArgs {
                reference_id: arg_string(args, "referenceId")?,
                title: arg_string(args, "title")?,
                issued: arg_optional_string(args, "issued")?,
            },
        ))),
        "update_bibliography_reference_metadata" => {
            Ok(Some(OpenDocCommand::UpdateBibliographyReferenceMetadata(
                UpdateBibliographyReferenceMetadataArgs {
                    reference_id: arg_string(args, "referenceId")?,
                    metadata: bibliography_reference_metadata_args(args)?,
                },
            )))
        }
        "delete_bibliography_reference" => Ok(Some(OpenDocCommand::DeleteBibliographyReference(
            reference_id_args(args)?,
        ))),
        "restore_bibliography_reference" => Ok(Some(OpenDocCommand::RestoreBibliographyReference(
            reference_id_args(args)?,
        ))),
        "delete_citation_group" => Ok(Some(OpenDocCommand::DeleteCitationGroup(citation_id_args(
            args,
        )?))),
        "restore_citation_group" => Ok(Some(OpenDocCommand::RestoreCitationGroup(
            citation_id_args(args)?,
        ))),
        "update_inline_text" => Ok(Some(OpenDocCommand::UpdateInlineText(InlineTextArgs {
            inline_id: arg_string(args, "inlineId")?,
            text: arg_string(args, "text")?,
        }))),
        "update_inline_equation_source" => Ok(Some(OpenDocCommand::UpdateInlineEquationSource(
            InlineSourceArgs {
                inline_id: arg_string(args, "inlineId")?,
                source: arg_string(args, "source")?,
            },
        ))),
        "update_mention_label" => Ok(Some(OpenDocCommand::UpdateMentionLabel(InlineLabelArgs {
            inline_id: arg_string(args, "inlineId")?,
            label: arg_string(args, "label")?,
        }))),
        "select_dropdown_option" => Ok(Some(OpenDocCommand::SelectDropdownOption(
            SelectDropdownOptionArgs {
                inline_id: arg_string(args, "inlineId")?,
                option_id: arg_string(args, "optionId")?,
            },
        ))),
        "update_date_chip" => Ok(Some(OpenDocCommand::UpdateDateChip(UpdateDateChipArgs {
            inline_id: arg_string(args, "inlineId")?,
            date: arg_string(args, "date")?,
        }))),
        "update_link_href" => Ok(Some(OpenDocCommand::UpdateLinkHref(InlineHrefArgs {
            inline_id: arg_string(args, "inlineId")?,
            href: arg_string(args, "href")?,
        }))),
        "insert_inline_text" => Ok(Some(OpenDocCommand::InsertInlineText(
            InsertInlineTextArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
                text: arg_string(args, "text")?,
            },
        ))),
        "delete_inline" => Ok(Some(OpenDocCommand::DeleteInline(inline_id_args(args)?))),
        "add_text_mark" => Ok(Some(OpenDocCommand::AddTextMark(text_mark_args(args)?))),
        "add_text_mark_range" => Ok(Some(OpenDocCommand::AddTextMarkRange(
            text_mark_range_args(args)?,
        ))),
        "remove_text_mark" => Ok(Some(OpenDocCommand::RemoveTextMark(text_mark_args(args)?))),
        "remove_text_mark_range" => Ok(Some(OpenDocCommand::RemoveTextMarkRange(
            text_mark_range_args(args)?,
        ))),
        "update_block_equation_source" => Ok(Some(OpenDocCommand::UpdateBlockEquationSource(
            BlockSourceArgs {
                block_id: arg_string(args, "blockId")?,
                source: arg_string(args, "source")?,
            },
        ))),
        "update_image_alt_text" => Ok(Some(OpenDocCommand::UpdateImageAltText(BlockAltTextArgs {
            block_id: arg_string(args, "blockId")?,
            alt_text: arg_string(args, "altText")?,
        }))),
        "update_image_blob_hash" => Ok(Some(OpenDocCommand::UpdateImageBlobHash(
            BlockBlobHashArgs {
                block_id: arg_string(args, "blockId")?,
                blob_hash: arg_string(args, "blobHash")?,
            },
        ))),
        "set_image_block_width" => Ok(Some(OpenDocCommand::SetImageBlockWidth(
            ImageBlockLengthArgs {
                block_id: arg_string(args, "blockId")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "set_image_block_height" => Ok(Some(OpenDocCommand::SetImageBlockHeight(
            ImageBlockLengthArgs {
                block_id: arg_string(args, "blockId")?,
                twips: arg_i32(args, "twips")?,
            },
        ))),
        "set_image_block_size" => Ok(Some(OpenDocCommand::SetImageBlockSize(
            ImageBlockSizeArgs {
                block_id: arg_string(args, "blockId")?,
                width_twips: arg_i32(args, "widthTwips")?,
                height_twips: arg_i32(args, "heightTwips")?,
            },
        ))),
        "clear_image_block_size" => Ok(Some(OpenDocCommand::ClearImageBlockSize(BlockIdArgs {
            block_id: arg_string(args, "blockId")?,
        }))),
        "set_image_block_placement" => Ok(Some(OpenDocCommand::SetImageBlockPlacement(
            ImageBlockPlacementArgs {
                block_id: arg_string(args, "blockId")?,
                placement: arg_string(args, "placement")?,
            },
        ))),
        "set_image_block_wrap_clearance" => Ok(Some(OpenDocCommand::SetImageBlockWrapClearance(
            ImageBlockWrapClearanceArgs {
                block_id: arg_string(args, "blockId")?,
                top_twips: arg_i32(args, "topTwips")?,
                end_twips: arg_i32(args, "endTwips")?,
                bottom_twips: arg_i32(args, "bottomTwips")?,
                start_twips: arg_i32(args, "startTwips")?,
            },
        ))),
        "set_image_block_positioned" => Ok(Some(OpenDocCommand::SetImageBlockPositioned(
            ImageBlockPositionedArgs {
                block_id: arg_string(args, "blockId")?,
                anchor_block_id: arg_optional_string(args, "anchorBlockId")?,
                horizontal_offset_twips: arg_i32(args, "horizontalOffsetTwips")?,
                vertical_offset_twips: arg_i32(args, "verticalOffsetTwips")?,
                layer: arg_string(args, "layer")?,
            },
        ))),
        "clear_image_block_positioned" => Ok(Some(OpenDocCommand::ClearImageBlockPositioned(
            BlockIdArgs {
                block_id: arg_string(args, "blockId")?,
            },
        ))),
        "set_image_block_effects" => Ok(Some(OpenDocCommand::SetImageBlockEffects(
            ImageBlockEffectsArgs {
                block_id: arg_string(args, "blockId")?,
                rotation_degrees: arg_i16(args, "rotationDegrees")?,
                opacity_percent: arg_u8(args, "opacityPercent")?,
            },
        ))),
        "set_image_block_crop" => Ok(Some(OpenDocCommand::SetImageBlockCrop(
            ImageBlockCropArgs {
                block_id: arg_string(args, "blockId")?,
                top_percent: arg_u8(args, "topPercent")?,
                right_percent: arg_u8(args, "rightPercent")?,
                bottom_percent: arg_u8(args, "bottomPercent")?,
                left_percent: arg_u8(args, "leftPercent")?,
            },
        ))),
        "set_image_block_caption" => Ok(Some(OpenDocCommand::SetImageBlockCaption(
            ImageBlockCaptionArgs {
                block_id: arg_string(args, "blockId")?,
                caption: arg_string(args, "caption")?,
            },
        ))),
        "set_image_block_border" => Ok(Some(OpenDocCommand::SetImageBlockBorder(
            ImageBlockBorderArgs {
                block_id: arg_string(args, "blockId")?,
                style: arg_string(args, "style")?,
                twips: arg_i32(args, "twips")?,
                color: arg_string(args, "color")?,
            },
        ))),
        "describe_spreadsheet_selection" => Ok(Some(OpenDocCommand::DescribeSpreadsheetSelection(
            spreadsheet_selection_args(args)?,
        ))),
        "reduce_spreadsheet_selection" => Ok(Some(OpenDocCommand::ReduceSpreadsheetSelection(
            SpreadsheetSelectionActionArgs {
                sheet_id: arg_string(args, "sheetId")?,
                anchor: arg_string(args, "anchor")?,
                focus: arg_string(args, "focus")?,
                action: arg_string(args, "action")?,
                value: arg_string(args, "value")?,
                extend: arg_bool(args, "extend")?,
            },
        ))),
        "copy_spreadsheet_selection_tsv" => Ok(Some(OpenDocCommand::CopySpreadsheetSelectionTsv(
            spreadsheet_selection_args(args)?,
        ))),
        "paste_spreadsheet_tsv" => Ok(Some(OpenDocCommand::PasteSpreadsheetTsv(
            SpreadsheetPasteTsvArgs {
                sheet_id: arg_string(args, "sheetId")?,
                origin: arg_string(args, "origin")?,
                text: arg_string(args, "text")?,
                source_origin: arg_optional_string(args, "sourceOrigin")?,
            },
        ))),
        "clear_spreadsheet_selection" => Ok(Some(OpenDocCommand::ClearSpreadsheetSelection(
            spreadsheet_selection_args(args)?,
        ))),
        "set_spreadsheet_selection_format" => Ok(Some(
            OpenDocCommand::SetSpreadsheetSelectionFormat(SpreadsheetSelectionFormatArgs {
                sheet_id: arg_string(args, "sheetId")?,
                anchor: arg_string(args, "anchor")?,
                focus: arg_string(args, "focus")?,
                property: arg_string(args, "property")?,
                value: arg_string(args, "value")?,
            }),
        )),
        "add_spreadsheet_row_after_selection" => Ok(Some(
            OpenDocCommand::AddSpreadsheetRowAfterSelection(spreadsheet_selection_args(args)?),
        )),
        "add_spreadsheet_column_after_selection" => Ok(Some(
            OpenDocCommand::AddSpreadsheetColumnAfterSelection(spreadsheet_selection_args(args)?),
        )),
        "delete_spreadsheet_selection_row" => Ok(Some(
            OpenDocCommand::DeleteSpreadsheetSelectionRow(spreadsheet_selection_args(args)?),
        )),
        "delete_spreadsheet_selection_column" => Ok(Some(
            OpenDocCommand::DeleteSpreadsheetSelectionColumn(spreadsheet_selection_args(args)?),
        )),
        "merge_spreadsheet_selection" => Ok(Some(OpenDocCommand::MergeSpreadsheetSelection(
            spreadsheet_selection_args(args)?,
        ))),
        "freeze_spreadsheet_selection" => Ok(Some(OpenDocCommand::FreezeSpreadsheetSelection(
            spreadsheet_selection_args(args)?,
        ))),
        "set_spreadsheet_selection_filter" => Ok(Some(
            OpenDocCommand::SetSpreadsheetSelectionFilter(spreadsheet_selection_args(args)?),
        )),
        "set_spreadsheet_workbook_metadata" => Ok(Some(
            OpenDocCommand::SetSpreadsheetWorkbookMetadata(SpreadsheetWorkbookMetadataArgs {
                title: arg_string(args, "title")?,
                locale: arg_string(args, "locale")?,
                timezone: arg_string(args, "timezone")?,
            }),
        )),
        "set_spreadsheet_cell" => Ok(Some(OpenDocCommand::SetSpreadsheetCell(
            SpreadsheetCellArgs {
                address: arg_string(args, "address")?,
                value: arg_string(args, "value")?,
            },
        ))),
        "set_spreadsheet_cells" => Ok(Some(OpenDocCommand::SetSpreadsheetCells(
            SpreadsheetCellEditsArgs {
                cells: arg_spreadsheet_cell_edits(args, "cells")?,
            },
        ))),
        "add_spreadsheet_sheet" => Ok(Some(OpenDocCommand::AddSpreadsheetSheet(SheetTitleArgs {
            title: arg_string(args, "title")?,
        }))),
        "rename_spreadsheet_sheet" => Ok(Some(OpenDocCommand::RenameSpreadsheetSheet(
            sheet_rename_args(args)?,
        ))),
        "delete_spreadsheet_sheet" => Ok(Some(OpenDocCommand::DeleteSpreadsheetSheet(
            sheet_id_args(args)?,
        ))),
        "restore_spreadsheet_sheet" => Ok(Some(OpenDocCommand::RestoreSpreadsheetSheet(
            sheet_id_args(args)?,
        ))),
        "add_spreadsheet_row" => Ok(Some(OpenDocCommand::AddSpreadsheetRow(sheet_row_args(
            args,
        )?))),
        "delete_spreadsheet_row" => Ok(Some(OpenDocCommand::DeleteSpreadsheetRow(sheet_row_args(
            args,
        )?))),
        "restore_spreadsheet_row" => Ok(Some(OpenDocCommand::RestoreSpreadsheetRow(
            sheet_row_args(args)?,
        ))),
        "add_spreadsheet_column" => Ok(Some(OpenDocCommand::AddSpreadsheetColumn(
            sheet_column_args(args)?,
        ))),
        "delete_spreadsheet_column" => Ok(Some(OpenDocCommand::DeleteSpreadsheetColumn(
            sheet_column_args(args)?,
        ))),
        "restore_spreadsheet_column" => Ok(Some(OpenDocCommand::RestoreSpreadsheetColumn(
            sheet_column_args(args)?,
        ))),
        "add_spreadsheet_cell_comment" => Ok(Some(OpenDocCommand::AddSpreadsheetCellComment(
            CellCommentCreateArgs {
                sheet_id: arg_string(args, "sheetId")?,
                address: arg_string(args, "address")?,
                author: arg_string(args, "author")?,
                body: arg_string(args, "body")?,
            },
        ))),
        "update_spreadsheet_cell_comment" => Ok(Some(
            OpenDocCommand::UpdateSpreadsheetCellComment(CellCommentUpdateArgs {
                comment_id: arg_string(args, "commentId")?,
                body: arg_string(args, "body")?,
            }),
        )),
        "delete_spreadsheet_cell_comment" => Ok(Some(
            OpenDocCommand::DeleteSpreadsheetCellComment(cell_comment_id_args(args)?),
        )),
        "restore_spreadsheet_cell_comment" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetCellComment(cell_comment_id_args(args)?),
        )),
        "set_spreadsheet_frozen_axes" => Ok(Some(OpenDocCommand::SetSpreadsheetFrozenAxes(
            FrozenAxesArgs {
                sheet_id: arg_string(args, "sheetId")?,
                frozen_rows: arg_u32(args, "frozenRows")?,
                frozen_columns: arg_u32(args, "frozenColumns")?,
            },
        ))),
        "set_spreadsheet_cell_validation" => Ok(Some(
            OpenDocCommand::SetSpreadsheetCellValidation(CellValidationArgs {
                sheet_id: arg_string(args, "sheetId")?,
                address: arg_string(args, "address")?,
                kind: arg_string(args, "kind")?,
                values: arg_string_array(args, "values")?,
                strict: arg_bool(args, "strict")?,
            }),
        )),
        "clear_spreadsheet_cell_validation" => Ok(Some(
            OpenDocCommand::ClearSpreadsheetCellValidation(sheet_address_args(args)?),
        )),
        "restore_spreadsheet_cell_validation" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetCellValidation(sheet_address_args(args)?),
        )),
        "merge_spreadsheet_cells" => Ok(Some(OpenDocCommand::MergeSpreadsheetCells(
            sheet_range_args(args)?,
        ))),
        "unmerge_spreadsheet_cells" => Ok(Some(OpenDocCommand::UnmergeSpreadsheetCells(
            sheet_range_args(args)?,
        ))),
        "restore_spreadsheet_merge" => Ok(Some(OpenDocCommand::RestoreSpreadsheetMerge(
            sheet_range_args(args)?,
        ))),
        "set_spreadsheet_basic_filter" => Ok(Some(OpenDocCommand::SetSpreadsheetBasicFilter(
            sheet_range_args(args)?,
        ))),
        "set_spreadsheet_print_area" => Ok(Some(OpenDocCommand::SetSpreadsheetPrintArea(
            sheet_range_args(args)?,
        ))),
        "clear_spreadsheet_print_area" => Ok(Some(OpenDocCommand::ClearSpreadsheetPrintArea(
            sheet_id_args(args)?,
        ))),
        "set_spreadsheet_print_orientation" => Ok(Some(
            OpenDocCommand::SetSpreadsheetPrintOrientation(SheetPrintOrientationArgs {
                sheet_id: arg_string(args, "sheetId")?,
                orientation: arg_string(args, "orientation")?,
            }),
        )),
        "set_spreadsheet_basic_filter_options" => Ok(Some(
            OpenDocCommand::SetSpreadsheetBasicFilterOptions(FilterOptionsArgs {
                sheet_id: arg_string(args, "sheetId")?,
                criteria: arg_filter_criteria(args, "criteria")?,
                sort_specs: arg_filter_sort_specs(args, "sortSpecs")?,
            }),
        )),
        "clear_spreadsheet_basic_filter" => Ok(Some(OpenDocCommand::ClearSpreadsheetBasicFilter(
            sheet_id_args(args)?,
        ))),
        "restore_spreadsheet_basic_filter" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetBasicFilter(sheet_id_args(args)?),
        )),
        "add_spreadsheet_protected_range" => Ok(Some(
            OpenDocCommand::AddSpreadsheetProtectedRange(protected_range_args(args)?),
        )),
        "update_spreadsheet_protected_range" => Ok(Some(
            OpenDocCommand::UpdateSpreadsheetProtectedRange(protected_range_args(args)?),
        )),
        "delete_spreadsheet_protected_range" => Ok(Some(
            OpenDocCommand::DeleteSpreadsheetProtectedRange(sheet_range_args(args)?),
        )),
        "restore_spreadsheet_protected_range" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetProtectedRange(sheet_range_args(args)?),
        )),
        "set_spreadsheet_cell_in_sheet" => Ok(Some(OpenDocCommand::SetSpreadsheetCellInSheet(
            SheetCellArgs {
                sheet_id: arg_string(args, "sheetId")?,
                address: arg_string(args, "address")?,
                value: arg_string(args, "value")?,
            },
        ))),
        "set_spreadsheet_cells_in_sheet" => Ok(Some(OpenDocCommand::SetSpreadsheetCellsInSheet(
            SheetCellEditsArgs {
                sheet_id: arg_string(args, "sheetId")?,
                cells: arg_spreadsheet_cell_edits(args, "cells")?,
            },
        ))),
        "set_spreadsheet_cell_format" => Ok(Some(OpenDocCommand::SetSpreadsheetCellFormat(
            CellFormatArgs {
                sheet_id: arg_string(args, "sheetId")?,
                address: arg_string(args, "address")?,
                property: arg_string(args, "property")?,
                value: arg_string(args, "value")?,
            },
        ))),
        "set_spreadsheet_row_height" => Ok(Some(OpenDocCommand::SetSpreadsheetRowHeight(
            SheetRowHeightArgs {
                sheet_id: arg_string(args, "sheetId")?,
                row: arg_string(args, "row")?,
                height: arg_u32(args, "height")?,
            },
        ))),
        "set_spreadsheet_column_width" => Ok(Some(OpenDocCommand::SetSpreadsheetColumnWidth(
            SheetColumnWidthArgs {
                sheet_id: arg_string(args, "sheetId")?,
                column: arg_string(args, "column")?,
                width: arg_u32(args, "width")?,
            },
        ))),
        "set_spreadsheet_selection_rows_hidden" => Ok(Some(
            OpenDocCommand::SetSpreadsheetSelectionRowsHidden(SpreadsheetSelectionHiddenArgs {
                sheet_id: arg_string(args, "sheetId")?,
                anchor: arg_string(args, "anchor")?,
                focus: arg_string(args, "focus")?,
                hidden: arg_bool(args, "hidden")?,
            }),
        )),
        "set_spreadsheet_selection_columns_hidden" => Ok(Some(
            OpenDocCommand::SetSpreadsheetSelectionColumnsHidden(SpreadsheetSelectionHiddenArgs {
                sheet_id: arg_string(args, "sheetId")?,
                anchor: arg_string(args, "anchor")?,
                focus: arg_string(args, "focus")?,
                hidden: arg_bool(args, "hidden")?,
            }),
        )),
        "copy_spreadsheet_range" => Ok(Some(OpenDocCommand::CopySpreadsheetRange(CopyRangeArgs {
            sheet_id: arg_string(args, "sheetId")?,
            source_range: arg_string(args, "sourceRange")?,
            target_address: arg_string(args, "targetAddress")?,
        }))),
        "sort_spreadsheet_range" => Ok(Some(OpenDocCommand::SortSpreadsheetRange(SortRangeArgs {
            sheet_id: arg_string(args, "sheetId")?,
            range: arg_string(args, "range")?,
            column: arg_string(args, "column")?,
            descending: arg_bool(args, "descending")?,
            has_header: arg_bool(args, "hasHeader")?,
        }))),
        "fill_spreadsheet_range" => Ok(Some(OpenDocCommand::FillSpreadsheetRange(FillRangeArgs {
            sheet_id: arg_string(args, "sheetId")?,
            source_range: arg_string(args, "sourceRange")?,
            target_range: arg_string(args, "targetRange")?,
        }))),
        "import_spreadsheet_csv" => Ok(Some(OpenDocCommand::ImportSpreadsheetCsv(
            ImportSpreadsheetCsvArgs {
                sheet_id: arg_string(args, "sheetId")?,
                origin: arg_string(args, "origin")?,
                text: arg_string(args, "text")?,
                delimiter: arg_optional_string(args, "delimiter")?,
            },
        ))),
        "export_spreadsheet_csv" => Ok(Some(OpenDocCommand::ExportSpreadsheetCsv(
            ExportSpreadsheetCsvArgs {
                sheet_id: arg_string(args, "sheetId")?,
                delimiter: arg_optional_string(args, "delimiter")?,
            },
        ))),
        "import_spreadsheet_xlsx" => Ok(Some(OpenDocCommand::ImportSpreadsheetXlsx(
            ImportSpreadsheetXlsxArgs {
                title: arg_string(args, "title")?,
                base64: arg_string(args, "base64")?,
            },
        ))),
        "export_spreadsheet_xlsx" => Ok(Some(OpenDocCommand::ExportSpreadsheetXlsx)),
        "export_spreadsheet_pdf" => Ok(Some(OpenDocCommand::ExportSpreadsheetPdf)),
        "add_spreadsheet_named_range" => Ok(Some(OpenDocCommand::AddSpreadsheetNamedRange(
            named_range_args(args)?,
        ))),
        "update_spreadsheet_named_range" => Ok(Some(OpenDocCommand::UpdateSpreadsheetNamedRange(
            named_range_args(args)?,
        ))),
        "delete_spreadsheet_named_range" => Ok(Some(OpenDocCommand::DeleteSpreadsheetNamedRange(
            named_range_name_args(args)?,
        ))),
        "restore_spreadsheet_named_range" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetNamedRange(named_range_name_args(args)?),
        )),
        _ => Ok(None),
    }
}
