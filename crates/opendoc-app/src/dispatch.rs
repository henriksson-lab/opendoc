use super::*;

impl OpenDocApp {
    pub fn dispatch_command(
        &mut self,
        command: &str,
        args: Value,
    ) -> Result<AppCommandResult, AppApiError> {
        let command = parse_json_command(command, &args)?
            .ok_or_else(|| AppApiError::NotFound(format!("unsupported app command {command}")))?;
        self.dispatch_typed_command(command, args)
    }

    fn dispatch_typed_command(
        &mut self,
        command: AppCommand,
        args: Value,
    ) -> Result<AppCommandResult, AppApiError> {
        let spec = command.spec();
        let command_name = spec.name;
        if !self.is_open && !spec.allowed_without_open_document {
            return Err(AppApiError::Conflict("no document is open".to_string()));
        }
        match command {
            AppCommand::CloseDocument => {
                return Ok(AppCommandResult::Document(self.close_document()));
            }
            AppCommand::UndoCurrentEdit => {
                self.undo_coalesce = None;
                return Ok(AppCommandResult::Document(self.undo_current_edit()?));
            }
            AppCommand::RedoCurrentEdit => {
                self.undo_coalesce = None;
                return Ok(AppCommandResult::Document(self.redo_current_edit()?));
            }
            _ => {}
        }
        if spec.undoable {
            let coalesce_key = undo_coalesce_key(command_name, &args);
            let now = now_ms();
            let coalesced = match (&coalesce_key, &self.undo_coalesce) {
                (Some(key), Some((last_key, last_at))) => {
                    key == last_key && now.saturating_sub(*last_at) <= UNDO_COALESCE_WINDOW_MS
                }
                _ => false,
            };
            let checkpoint = if coalesced {
                None
            } else {
                Some(self.checkpoint())
            };
            match self.dispatch_typed_command_inner(command) {
                Ok(result) => {
                    if let Some(checkpoint) = checkpoint {
                        self.undo_stack.push(checkpoint);
                        if self.undo_stack.len() > UNDO_STACK_LIMIT {
                            let excess = self.undo_stack.len() - UNDO_STACK_LIMIT;
                            self.undo_stack.drain(..excess);
                        }
                    }
                    self.redo_stack.clear();
                    self.undo_coalesce = coalesce_key.map(|key| (key, now));
                    Ok(result)
                }
                Err(err) => Err(err),
            }
        } else {
            self.dispatch_typed_command_inner(command)
        }
    }

    fn dispatch_typed_command_inner(
        &mut self,
        command: AppCommand,
    ) -> Result<AppCommandResult, AppApiError> {
        match command {
            AppCommand::CreateDocument(args) => {
                Ok(AppCommandResult::Document(self.new_document(args.title)))
            }
            AppCommand::CloseDocument => Ok(AppCommandResult::Document(self.close_document())),
            AppCommand::UndoCurrentEdit => {
                Ok(AppCommandResult::Document(self.undo_current_edit()?))
            }
            AppCommand::RedoCurrentEdit => {
                Ok(AppCommandResult::Document(self.redo_current_edit()?))
            }
            AppCommand::GetDocument => Ok(AppCommandResult::Document(self.document())),
            AppCommand::GetAuditView => Ok(AppCommandResult::AuditView(self.audit_view())),
            AppCommand::SetDocumentTitle(args) => Ok(AppCommandResult::Document(
                self.set_document_title(args.title)?,
            )),
            AppCommand::SetDocumentLocale(args) => Ok(AppCommandResult::Document(
                self.set_document_locale(args.locale)?,
            )),
            AppCommand::AddParagraph(args) => {
                Ok(AppCommandResult::Document(self.add_paragraph(args.text)))
            }
            AppCommand::RenderDocumentHtml => {
                Ok(AppCommandResult::Text(self.render_document_html()))
            }
            AppCommand::GetRuntimeProfile(profile) => Ok(AppCommandResult::RuntimeProfile(profile)),
            AppCommand::GetRuntimeSession(args) => Ok(AppCommandResult::RuntimeSession(
                OpenDocRuntimeSession::for_profile_with_permissions(
                    args.profile,
                    args.subject,
                    args.document_uuid,
                    args.presence,
                    args.permissions,
                ),
            )),
            AppCommand::AuthorizeRuntimeCommand(args) => {
                Ok(AppCommandResult::AuthorizationDecision(
                    OpenDocAuthorizationDecision::for_profile_command(
                        args.profile,
                        args.subject,
                        args.document_uuid,
                        args.command_name,
                        args.permissions,
                    ),
                ))
            }
            AppCommand::CreateRuntimeShareInvite(args) => Ok(AppCommandResult::ShareInvite(
                OpenDocShareInvite::for_profile(
                    args.profile,
                    args.subject,
                    args.document_uuid,
                    args.target_subject,
                    args.actions,
                    args.permissions,
                    now_ms(),
                ),
            )),
            AppCommand::RelayRuntimeSync(args) => Ok(AppCommandResult::SyncRelay(
                OpenDocSyncRelayResult::for_profile(
                    args.profile,
                    args.subject,
                    args.document_uuid,
                    args.base_manifest,
                    args.operations,
                    args.permissions,
                    args.presence,
                ),
            )),
            AppCommand::ResolveRuntimeDocumentLookup(args) => Ok(AppCommandResult::RuntimeLookup(
                OpenDocRuntimeLookupResult::for_profile(
                    args.profile,
                    args.subject,
                    args.document_uuid,
                    args.doi,
                    args.permissions,
                    args.service_index,
                    args.scanned_documents,
                ),
            )),
            AppCommand::ImportGoogleDocsJson(args) => Ok(AppCommandResult::Document(
                self.import_google_docs_json_text(args.title, args.json_text)?,
            )),
            AppCommand::ImportDocOrDocxPath(args) => Ok(AppCommandResult::Document(
                self.import_doc_or_docx_path(args.path)?,
            )),
            AppCommand::ExportGoogleDocsJson => {
                Ok(AppCommandResult::Text(self.export_google_docs_json_text()?))
            }
            AppCommand::ImportGoogleSheetsJson(args) => Ok(AppCommandResult::Document(
                self.import_google_sheets_json_text(args.json_text)?,
            )),
            AppCommand::ExportGoogleSheetsJson => Ok(AppCommandResult::Text(
                self.export_google_sheets_json_text()?,
            )),
            AppCommand::RenderWorkbookHtml(args) => Ok(AppCommandResult::Text(
                self.render_workbook_html(&args.sheet_id)?,
            )),
            AppCommand::ImportDocxBase64(args) => Ok(AppCommandResult::Document(
                self.import_docx_base64(args.name, args.base64)?,
            )),
            AppCommand::AddBinaryBlob(args) => Ok(AppCommandResult::Document(
                self.add_binary_blob(args.name, args.media_type, args.bytes)?,
            )),
            AppCommand::SimulateShallowClone => {
                Ok(AppCommandResult::Document(self.simulate_shallow_clone()))
            }
            AppCommand::UpdateBinaryBlobMetadata(args) => Ok(AppCommandResult::Document(
                self.update_binary_blob_metadata(args.blob_hash, args.name, args.media_type)?,
            )),
            AppCommand::DeleteBinaryBlob(args) => Ok(AppCommandResult::Document(
                self.delete_binary_blob(args.blob_hash)?,
            )),
            AppCommand::RestoreBinaryBlob(args) => Ok(AppCommandResult::Document(
                self.restore_binary_blob(args.blob_hash)?,
            )),
            AppCommand::RecordBlobArchiveTombstone(args) => Ok(AppCommandResult::Document(
                self.record_blob_archive_tombstone(
                    args.blob_hash,
                    args.archive_locator,
                    args.restore_hint,
                    args.signer,
                    args.signature,
                )?,
            )),
            AppCommand::AddImageBlock(args) => Ok(AppCommandResult::Document(
                self.add_image_block(args.blob_hash, args.alt_text)?,
            )),
            AppCommand::InsertImageBlockAfter(args) => Ok(AppCommandResult::Document(
                self.insert_image_block_after(args.after_block_id, args.blob_hash, args.alt_text)?,
            )),
            AppCommand::SignBlobWithOpenSshPrivateKey(args) => Ok(AppCommandResult::Document(
                self.sign_blob_with_openssh_private_key(
                    args.blob_hash,
                    args.private_key_pem,
                    args.signer_display,
                )?,
            )),
            AppCommand::SignFastqBlobWithOpenSshPrivateKey(args) => Ok(AppCommandResult::Document(
                self.sign_fastq_blob_with_openssh_private_key(
                    args.blob_hash,
                    args.profile,
                    args.private_key_pem,
                    args.signer_display,
                )?,
            )),
            AppCommand::SignImagePixelsBlobWithOpenSshPrivateKey(args) => Ok(
                AppCommandResult::Document(self.sign_image_pixels_blob_with_openssh_private_key(
                    args.blob_hash,
                    args.width,
                    args.height,
                    args.pixels,
                    args.private_key_pem,
                    args.signer_display,
                )?),
            ),
            AppCommand::SignWithOpenSshPrivateKey(args) => Ok(AppCommandResult::Document(
                self.sign_with_openssh_private_key(args.private_key_pem, args.signer_display)?,
            )),
            AppCommand::VerifyCurrentSignature(args) => Ok(AppCommandResult::Text(
                self.verify_current_signature(args.private_key_pem)?,
            )),
            AppCommand::VerifyCurrentSignatures => {
                Ok(AppCommandResult::Text(self.verify_current_signatures()?))
            }
            AppCommand::SaveLocalRepository(args) => Ok(AppCommandResult::Document(
                self.save_to_local_repository(args.path)?,
            )),
            AppCommand::SaveLocalRepositoryOrCandidate(args) => Ok(AppCommandResult::Document(
                self.save_to_local_repository_or_candidate(args.path)?,
            )),
            AppCommand::SaveFlatRepository(args) => Ok(AppCommandResult::Document(
                self.save_to_flat_repository(args.path, args.namespace)?,
            )),
            AppCommand::SaveFlatRepositoryOrCandidate(args) => Ok(AppCommandResult::Document(
                self.save_to_flat_repository_or_candidate(args.path, args.namespace)?,
            )),
            AppCommand::SaveOpenDalFsRepository(args) => Ok(AppCommandResult::Document(
                self.save_to_opendal_fs_repository(args.path, args.namespace)?,
            )),
            AppCommand::SaveOpenDalFsRepositoryOrCandidate(args) => Ok(AppCommandResult::Document(
                self.save_to_opendal_fs_repository_or_candidate(args.path, args.namespace)?,
            )),
            AppCommand::AutosaveCurrentRepository => Ok(AppCommandResult::Document(
                self.autosave_current_repository()?,
            )),
            AppCommand::CompactLocalRepository(args) => Ok(AppCommandResult::Document(
                self.compact_local_repository(args.path, args.pack_name)?,
            )),
            AppCommand::OpenLocalRepository(args) => Ok(AppCommandResult::Document(
                self.open_saved_projection(args.path, args.document_uuid)?,
            )),
            AppCommand::ScanLocalRepository(args) => Ok(AppCommandResult::Document(
                self.scan_local_repository(args.path)?,
            )),
            AppCommand::OpenFlatRepository(args) => Ok(AppCommandResult::Document(
                self.open_flat_projection(args.path, args.namespace, args.document_uuid)?,
            )),
            AppCommand::OpenOpenDalFsRepository(args) => Ok(AppCommandResult::Document(
                self.open_opendal_fs_projection(args.path, args.namespace, args.document_uuid)?,
            )),
            AppCommand::MergeLocalRepositoryCandidates(args) => Ok(AppCommandResult::Document(
                self.merge_local_repository_candidates(args.path, args.document_uuid)?,
            )),
            AppCommand::MergeFlatRepositoryCandidates(args) => Ok(AppCommandResult::Document(
                self.merge_flat_repository_candidates(
                    args.path,
                    args.namespace,
                    args.document_uuid,
                )?,
            )),
            AppCommand::MergeOpenDalFsRepositoryCandidates(args) => Ok(AppCommandResult::Document(
                self.merge_opendal_fs_repository_candidates(
                    args.path,
                    args.namespace,
                    args.document_uuid,
                )?,
            )),
            AppCommand::OpenLocalRepositoryByDoi(args) => Ok(AppCommandResult::Document(
                self.open_saved_projection_by_doi(args.path, args.doi)?,
            )),
            AppCommand::OpenFlatRepositoryByDoi(args) => Ok(AppCommandResult::Document(
                self.open_flat_projection_by_doi(args.path, args.namespace, args.doi)?,
            )),
            AppCommand::OpenOpenDalFsRepositoryByDoi(args) => Ok(AppCommandResult::Document(
                self.open_opendal_fs_projection_by_doi(args.path, args.namespace, args.doi)?,
            )),
            AppCommand::SetDocumentDoi(args) => {
                Ok(AppCommandResult::Document(self.set_document_doi(args.doi)?))
            }
            AppCommand::InsertParagraphAfter(args) => Ok(AppCommandResult::Document(
                self.insert_paragraph_after(args.after_block_id, args.text)?,
            )),
            AppCommand::SplitParagraphAtInline(args) => Ok(AppCommandResult::Document(
                self.split_paragraph_at_inline(args.inline_id)?,
            )),
            AppCommand::SplitParagraphAtTextOffset(args) => Ok(AppCommandResult::Document(
                self.split_paragraph_at_text_offset(args.block_id, args.inline_id, args.offset)?,
            )),
            AppCommand::JoinParagraphWithPrevious(args) => Ok(AppCommandResult::Document(
                self.join_paragraph_with_previous(args.block_id)?,
            )),
            AppCommand::DeleteBlock(args) => Ok(AppCommandResult::Document(
                self.delete_block(args.block_id)?,
            )),
            AppCommand::SetBlockTextStyle(args) => Ok(AppCommandResult::Document(
                self.set_block_text_style(args.block_id, args.style, args.level, args.ordered)?,
            )),
            AppCommand::SetEditorSelectionBlockStyle(args) => Ok(AppCommandResult::Document(
                self.set_editor_selection_block_style(
                    args.selection,
                    args.style,
                    args.level,
                    args.ordered,
                )?,
            )),
            AppCommand::AddHeading(args) => Ok(AppCommandResult::Document(
                self.add_heading(args.text, args.level)?,
            )),
            AppCommand::UpdateHeadingLevel(args) => Ok(AppCommandResult::Document(
                self.update_heading_level(args.block_id, args.level)?,
            )),
            AppCommand::AddLink(args) => Ok(AppCommandResult::Document(
                self.add_link(args.text, args.href)?,
            )),
            AppCommand::InsertLinkAfter(args) => Ok(AppCommandResult::Document(
                self.insert_link_after(args.block_id, args.after_inline_id, args.text, args.href)?,
            )),
            AppCommand::AddMention(args) => {
                Ok(AppCommandResult::Document(self.add_mention(args.label)?))
            }
            AppCommand::InsertMentionAfter(args) => Ok(AppCommandResult::Document(
                self.insert_mention_after(args.block_id, args.after_inline_id, args.label)?,
            )),
            AppCommand::AddFootnoteRef => Ok(AppCommandResult::Document(self.add_footnote_ref())),
            AppCommand::InsertFootnoteRefAfter(args) => Ok(AppCommandResult::Document(
                self.insert_footnote_ref_after(args.block_id, args.after_inline_id)?,
            )),
            AppCommand::UpdateFootnoteBody(args) => Ok(AppCommandResult::Document(
                self.update_footnote_body(args.footnote_id, args.body)?,
            )),
            AppCommand::AddEquation(args) => Ok(AppCommandResult::Document(
                self.add_equation_inline(args.source)?,
            )),
            AppCommand::InsertEquationAfter(args) => Ok(AppCommandResult::Document(
                self.insert_equation_after(args.block_id, args.after_inline_id, args.source)?,
            )),
            AppCommand::AddEquationBlock(args) => Ok(AppCommandResult::Document(
                self.add_equation_block(args.source)?,
            )),
            AppCommand::InsertEquationBlockAfter(args) => Ok(AppCommandResult::Document(
                self.insert_equation_block_after(args.after_block_id, args.source)?,
            )),
            AppCommand::AddListItem(args) => Ok(AppCommandResult::Document(self.add_list_item(
                args.text,
                args.level,
                args.ordered,
            )?)),
            AppCommand::InsertListItemAfter(args) => {
                Ok(AppCommandResult::Document(self.insert_list_item_after(
                    args.after_block_id,
                    args.text,
                    args.level,
                    args.ordered,
                )?))
            }
            AppCommand::UpdateListItem(args) => Ok(AppCommandResult::Document(
                self.update_list_item(args.block_id, args.level, args.ordered)?,
            )),
            AppCommand::AdjustEditorSelectionListIndent(args) => Ok(AppCommandResult::Document(
                self.adjust_editor_selection_list_indent(args.selection, args.delta)?,
            )),
            AppCommand::InsertPageBreakAfter(args) => Ok(AppCommandResult::Document(
                self.insert_page_break_after(args.after_block_id)?,
            )),
            AppCommand::AddPageBreak => Ok(AppCommandResult::Document(self.add_page_break())),
            AppCommand::InsertTableAfter(args) => match args {
                InsertTableAfterArgs::Default { after_block_id } => Ok(AppCommandResult::Document(
                    self.insert_table_after(after_block_id)?,
                )),
                InsertTableAfterArgs::Sized {
                    after_block_id,
                    rows,
                    columns,
                } => Ok(AppCommandResult::Document(self.insert_table_after_sized(
                    after_block_id,
                    rows,
                    columns,
                )?)),
            },
            AppCommand::AddTable => Ok(AppCommandResult::Document(self.add_table())),
            AppCommand::AddTableRow(args) => Ok(AppCommandResult::Document(self.add_table_row(
                args.table_block_id,
                args.after_row,
                args.text,
            )?)),
            AppCommand::DeleteTableRow(args) => Ok(AppCommandResult::Document(
                self.delete_table_row(args.table_block_id, args.row_id)?,
            )),
            AppCommand::AddTableCell(args) => Ok(AppCommandResult::Document(self.add_table_cell(
                args.table_block_id,
                args.row_id,
                args.after_cell,
                args.text,
            )?)),
            AppCommand::DeleteTableCell(args) => Ok(AppCommandResult::Document(
                self.delete_table_cell(args.table_block_id, args.row_id, args.cell_id)?,
            )),
            AppCommand::AddCitation => Ok(AppCommandResult::Document(self.add_sample_citation())),
            AppCommand::InsertCitation(args) => {
                Ok(AppCommandResult::Document(self.insert_citation(
                    args.reference_id,
                    args.after_inline_id,
                    args.locator,
                    args.label,
                    args.prefix,
                    args.suffix,
                    args.suppress_author,
                )?))
            }
            AppCommand::InsertCitationGroup(args) => Ok(AppCommandResult::Document(
                self.insert_citation_group(args.items, args.after_inline_id)?,
            )),
            AppCommand::InsertFootnoteCitationGroup(args) => Ok(AppCommandResult::Document(
                self.insert_footnote_citation_group(args.footnote_id, args.items)?,
            )),
            AppCommand::InsertFootnoteCitationAfter(args) => Ok(AppCommandResult::Document(
                self.insert_footnote_citation_after(
                    args.block_id,
                    args.after_inline_id,
                    args.items,
                )?,
            )),
            AppCommand::UpdateCitationGroupItems(args) => Ok(AppCommandResult::Document(
                self.update_citation_group_items(args.citation_id, args.items)?,
            )),
            AppCommand::SetCitationStyle(args) => Ok(AppCommandResult::Document(
                self.set_citation_style(args.style, args.locale)?,
            )),
            AppCommand::AddComment(args) => Ok(AppCommandResult::Document(
                self.add_comment(args.author, args.body)?,
            )),
            AppCommand::AddTextRangeComment(args) => {
                Ok(AppCommandResult::Document(self.add_text_range_comment(
                    args.start_inline_id,
                    args.end_inline_id,
                    args.author,
                    args.body,
                )?))
            }
            AppCommand::AddBlockComment(args) => Ok(AppCommandResult::Document(
                self.add_block_comment(args.block_id, args.author, args.body)?,
            )),
            AppCommand::AddCommentReply(args) => Ok(AppCommandResult::Document(
                self.add_comment_reply(args.thread_id, args.author, args.body)?,
            )),
            AppCommand::DeleteCommentThread(args) => Ok(AppCommandResult::Document(
                self.delete_comment_thread(args.thread_id)?,
            )),
            AppCommand::RestoreCommentThread(args) => Ok(AppCommandResult::Document(
                self.restore_comment_thread(args.thread_id)?,
            )),
            AppCommand::DeleteComment(args) => Ok(AppCommandResult::Document(
                self.delete_comment(args.thread_id, args.comment_id)?,
            )),
            AppCommand::RestoreComment(args) => Ok(AppCommandResult::Document(
                self.restore_comment(args.thread_id, args.comment_id)?,
            )),
            AppCommand::UpdateComment(args) => Ok(AppCommandResult::Document(
                self.update_comment(args.thread_id, args.comment_id, args.body)?,
            )),
            AppCommand::AddSuggestion(args) => Ok(AppCommandResult::Document(
                self.add_suggestion(args.author, args.text)?,
            )),
            AppCommand::AddTextRangeSuggestion(args) => {
                Ok(AppCommandResult::Document(self.add_text_range_suggestion(
                    args.start_inline_id,
                    args.end_inline_id,
                    args.author,
                    args.text,
                )?))
            }
            AppCommand::AddBlockSuggestion(args) => Ok(AppCommandResult::Document(
                self.add_block_suggestion(args.block_id, args.author, args.text)?,
            )),
            AppCommand::AddDeleteSuggestion(args) => Ok(AppCommandResult::Document(
                self.add_delete_suggestion(args.author, args.inline_id)?,
            )),
            AppCommand::AddTextRangeDeleteSuggestion(args) => Ok(AppCommandResult::Document(
                self.add_text_range_delete_suggestion(
                    args.start_inline_id,
                    args.end_inline_id,
                    args.author,
                )?,
            )),
            AppCommand::AddFormatSuggestion(args) => {
                Ok(AppCommandResult::Document(self.add_format_suggestion(
                    args.author,
                    args.inline_id,
                    args.mark_kind,
                    args.value,
                )?))
            }
            AppCommand::AddTextRangeFormatSuggestion(args) => Ok(AppCommandResult::Document(
                self.add_text_range_format_suggestion(
                    args.start_inline_id,
                    args.end_inline_id,
                    args.author,
                    args.mark_kind,
                    args.value,
                )?,
            )),
            AppCommand::UpdateSuggestion(args) => Ok(AppCommandResult::Document(
                self.update_suggestion(args.suggestion_id, args.text)?,
            )),
            AppCommand::AcceptSuggestion(args) => Ok(AppCommandResult::Document(
                self.accept_suggestion(args.suggestion_id, args.accepted_by)?,
            )),
            AppCommand::AcceptAllSuggestions(args) => Ok(AppCommandResult::Document(
                self.accept_all_suggestions(args.accepted_by)?,
            )),
            AppCommand::RejectSuggestion(args) => Ok(AppCommandResult::Document(
                self.reject_suggestion(args.suggestion_id, args.rejected_by)?,
            )),
            AppCommand::RejectAllSuggestions(args) => Ok(AppCommandResult::Document(
                self.reject_all_suggestions(args.rejected_by)?,
            )),
            AppCommand::DescribeEditorSelection(selection) => Ok(
                AppCommandResult::EditorSelection(self.describe_editor_selection(selection)?),
            ),
            AppCommand::SelectAllEditorContent => {
                Ok(AppCommandResult::Editor(self.select_all_editor_content()?))
            }
            AppCommand::ApplyEditorInput(input) => {
                Ok(AppCommandResult::Editor(self.apply_editor_input(input)?))
            }
            AppCommand::ApplyEditorMark(input) => {
                Ok(AppCommandResult::Editor(self.apply_editor_mark(input)?))
            }
            AppCommand::AddBibliographyReference(args) => Ok(AppCommandResult::Document(
                self.add_bibliography_reference(
                    args.title,
                    args.authors,
                    args.issued,
                    args.doi,
                    args.url,
                )?,
            )),
            AppCommand::UpdateBibliographyReference(args) => Ok(AppCommandResult::Document(
                self.update_bibliography_reference(args.reference_id, args.title, args.issued)?,
            )),
            AppCommand::UpdateBibliographyReferenceMetadata(args) => Ok(
                AppCommandResult::Document(self.update_bibliography_reference_metadata(
                    args.reference_id,
                    args.metadata.title,
                    args.metadata.authors,
                    args.metadata.issued,
                    args.metadata.doi,
                    args.metadata.url,
                )?),
            ),
            AppCommand::DeleteBibliographyReference(args) => Ok(AppCommandResult::Document(
                self.delete_bibliography_reference(args.reference_id)?,
            )),
            AppCommand::RestoreBibliographyReference(args) => Ok(AppCommandResult::Document(
                self.restore_bibliography_reference(args.reference_id)?,
            )),
            AppCommand::DeleteCitationGroup(args) => Ok(AppCommandResult::Document(
                self.delete_citation_group(args.citation_id)?,
            )),
            AppCommand::RestoreCitationGroup(args) => Ok(AppCommandResult::Document(
                self.restore_citation_group(args.citation_id)?,
            )),
            AppCommand::UpdateInlineText(args) => Ok(AppCommandResult::Document(
                self.update_inline_text(args.inline_id, args.text)?,
            )),
            AppCommand::UpdateInlineEquationSource(args) => Ok(AppCommandResult::Document(
                self.update_inline_equation_source(args.inline_id, args.source)?,
            )),
            AppCommand::UpdateMentionLabel(args) => Ok(AppCommandResult::Document(
                self.update_mention_label(args.inline_id, args.label)?,
            )),
            AppCommand::UpdateLinkHref(args) => Ok(AppCommandResult::Document(
                self.update_link_href(args.inline_id, args.href)?,
            )),
            AppCommand::InsertInlineText(args) => Ok(AppCommandResult::Document(
                self.insert_inline_text(args.block_id, args.after_inline_id, args.text)?,
            )),
            AppCommand::DeleteInline(args) => Ok(AppCommandResult::Document(
                self.delete_inline(args.inline_id)?,
            )),
            AppCommand::AddTextMark(args) => Ok(AppCommandResult::Document(self.add_text_mark(
                args.inline_id,
                args.mark_kind,
                args.value,
            )?)),
            AppCommand::AddTextMarkRange(args) => {
                Ok(AppCommandResult::Document(self.add_text_mark_range(
                    args.start_inline_id,
                    args.end_inline_id,
                    args.mark_kind,
                    args.value,
                )?))
            }
            AppCommand::RemoveTextMark(args) => Ok(AppCommandResult::Document(
                self.remove_text_mark(args.inline_id, args.mark_kind, args.value)?,
            )),
            AppCommand::RemoveTextMarkRange(args) => {
                Ok(AppCommandResult::Document(self.remove_text_mark_range(
                    args.start_inline_id,
                    args.end_inline_id,
                    args.mark_kind,
                    args.value,
                )?))
            }
            AppCommand::UpdateBlockEquationSource(args) => Ok(AppCommandResult::Document(
                self.update_block_equation_source(args.block_id, args.source)?,
            )),
            AppCommand::UpdateImageAltText(args) => Ok(AppCommandResult::Document(
                self.update_image_alt_text(args.block_id, args.alt_text)?,
            )),
            AppCommand::UpdateImageBlobHash(args) => Ok(AppCommandResult::Document(
                self.update_image_blob_hash(args.block_id, args.blob_hash)?,
            )),
            AppCommand::DescribeSpreadsheetSelection(args) => {
                Ok(AppCommandResult::SpreadsheetSelection(
                    self.describe_spreadsheet_selection(args.sheet_id, args.anchor, args.focus)?,
                ))
            }
            AppCommand::ReduceSpreadsheetSelection(args) => Ok(
                AppCommandResult::SpreadsheetSelection(self.reduce_spreadsheet_selection(
                    args.sheet_id,
                    args.anchor,
                    args.focus,
                    args.action,
                    args.value,
                    args.extend,
                )?),
            ),
            AppCommand::CopySpreadsheetSelectionTsv(args) => Ok(AppCommandResult::Text(
                self.copy_spreadsheet_selection_tsv(args.sheet_id, args.anchor, args.focus)?,
            )),
            AppCommand::PasteSpreadsheetTsv(args) => Ok(AppCommandResult::Document(
                self.paste_spreadsheet_tsv(args.sheet_id, args.origin, args.text)?,
            )),
            AppCommand::ClearSpreadsheetSelection(args) => Ok(AppCommandResult::Document(
                self.clear_spreadsheet_selection(args.sheet_id, args.anchor, args.focus)?,
            )),
            AppCommand::SetSpreadsheetSelectionFormat(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_selection_format(
                    args.sheet_id,
                    args.anchor,
                    args.focus,
                    args.property,
                    args.value,
                )?,
            )),
            AppCommand::AddSpreadsheetRowAfterSelection(args) => Ok(AppCommandResult::Document(
                self.add_spreadsheet_row_after_selection(args.sheet_id, args.focus)?,
            )),
            AppCommand::AddSpreadsheetColumnAfterSelection(args) => Ok(AppCommandResult::Document(
                self.add_spreadsheet_column_after_selection(args.sheet_id, args.focus)?,
            )),
            AppCommand::DeleteSpreadsheetSelectionRow(args) => Ok(AppCommandResult::Document(
                self.delete_spreadsheet_selection_row(args.sheet_id, args.focus)?,
            )),
            AppCommand::DeleteSpreadsheetSelectionColumn(args) => Ok(AppCommandResult::Document(
                self.delete_spreadsheet_selection_column(args.sheet_id, args.focus)?,
            )),
            AppCommand::MergeSpreadsheetSelection(args) => Ok(AppCommandResult::Document(
                self.merge_spreadsheet_selection(args.sheet_id, args.anchor, args.focus)?,
            )),
            AppCommand::FreezeSpreadsheetSelection(args) => Ok(AppCommandResult::Document(
                self.freeze_spreadsheet_selection(args.sheet_id, args.focus)?,
            )),
            AppCommand::SetSpreadsheetSelectionFilter(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_selection_filter(args.sheet_id, args.anchor, args.focus)?,
            )),
            AppCommand::SetSpreadsheetWorkbookMetadata(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_workbook_metadata(args.title, args.locale, args.timezone)?,
            )),
            AppCommand::SetSpreadsheetCell(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_cell(args.address, args.value)?,
            )),
            AppCommand::SetSpreadsheetCells(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_cells(args.cells)?,
            )),
            AppCommand::AddSpreadsheetSheet(args) => Ok(AppCommandResult::Document(
                self.add_spreadsheet_sheet(args.title)?,
            )),
            AppCommand::RenameSpreadsheetSheet(args) => Ok(AppCommandResult::Document(
                self.rename_spreadsheet_sheet(args.sheet_id, args.title)?,
            )),
            AppCommand::DeleteSpreadsheetSheet(args) => Ok(AppCommandResult::Document(
                self.delete_spreadsheet_sheet(args.sheet_id)?,
            )),
            AppCommand::RestoreSpreadsheetSheet(args) => Ok(AppCommandResult::Document(
                self.restore_spreadsheet_sheet(args.sheet_id)?,
            )),
            AppCommand::AddSpreadsheetRow(args) => Ok(AppCommandResult::Document(
                self.add_spreadsheet_row(args.sheet_id, args.row)?,
            )),
            AppCommand::DeleteSpreadsheetRow(args) => Ok(AppCommandResult::Document(
                self.delete_spreadsheet_row(args.sheet_id, args.row)?,
            )),
            AppCommand::RestoreSpreadsheetRow(args) => Ok(AppCommandResult::Document(
                self.restore_spreadsheet_row(args.sheet_id, args.row)?,
            )),
            AppCommand::AddSpreadsheetColumn(args) => Ok(AppCommandResult::Document(
                self.add_spreadsheet_column(args.sheet_id, args.column)?,
            )),
            AppCommand::DeleteSpreadsheetColumn(args) => Ok(AppCommandResult::Document(
                self.delete_spreadsheet_column(args.sheet_id, args.column)?,
            )),
            AppCommand::RestoreSpreadsheetColumn(args) => Ok(AppCommandResult::Document(
                self.restore_spreadsheet_column(args.sheet_id, args.column)?,
            )),
            AppCommand::AddSpreadsheetCellComment(args) => Ok(AppCommandResult::Document(
                self.add_spreadsheet_cell_comment(
                    args.sheet_id,
                    args.address,
                    args.author,
                    args.body,
                )?,
            )),
            AppCommand::UpdateSpreadsheetCellComment(args) => Ok(AppCommandResult::Document(
                self.update_spreadsheet_cell_comment(args.comment_id, args.body)?,
            )),
            AppCommand::DeleteSpreadsheetCellComment(args) => Ok(AppCommandResult::Document(
                self.delete_spreadsheet_cell_comment(args.comment_id)?,
            )),
            AppCommand::RestoreSpreadsheetCellComment(args) => Ok(AppCommandResult::Document(
                self.restore_spreadsheet_cell_comment(args.comment_id)?,
            )),
            AppCommand::SetSpreadsheetFrozenAxes(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_frozen_axes(
                    args.sheet_id,
                    args.frozen_rows,
                    args.frozen_columns,
                )?,
            )),
            AppCommand::SetSpreadsheetCellValidation(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_cell_validation(
                    args.sheet_id,
                    args.address,
                    args.kind,
                    args.values,
                    args.strict,
                )?,
            )),
            AppCommand::ClearSpreadsheetCellValidation(args) => Ok(AppCommandResult::Document(
                self.clear_spreadsheet_cell_validation(args.sheet_id, args.address)?,
            )),
            AppCommand::RestoreSpreadsheetCellValidation(args) => Ok(AppCommandResult::Document(
                self.restore_spreadsheet_cell_validation(args.sheet_id, args.address)?,
            )),
            AppCommand::MergeSpreadsheetCells(args) => Ok(AppCommandResult::Document(
                self.merge_spreadsheet_cells(args.sheet_id, args.range)?,
            )),
            AppCommand::UnmergeSpreadsheetCells(args) => Ok(AppCommandResult::Document(
                self.unmerge_spreadsheet_cells(args.sheet_id, args.range)?,
            )),
            AppCommand::RestoreSpreadsheetMerge(args) => Ok(AppCommandResult::Document(
                self.restore_spreadsheet_merge(args.sheet_id, args.range)?,
            )),
            AppCommand::SetSpreadsheetBasicFilter(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_basic_filter(args.sheet_id, args.range)?,
            )),
            AppCommand::SetSpreadsheetBasicFilterOptions(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_basic_filter_options(
                    args.sheet_id,
                    args.criteria,
                    args.sort_specs,
                )?,
            )),
            AppCommand::ClearSpreadsheetBasicFilter(args) => Ok(AppCommandResult::Document(
                self.clear_spreadsheet_basic_filter(args.sheet_id)?,
            )),
            AppCommand::RestoreSpreadsheetBasicFilter(args) => Ok(AppCommandResult::Document(
                self.restore_spreadsheet_basic_filter(args.sheet_id)?,
            )),
            AppCommand::AddSpreadsheetProtectedRange(args) => Ok(AppCommandResult::Document(
                self.add_spreadsheet_protected_range(
                    args.sheet_id,
                    args.range,
                    args.description,
                    args.warning_only,
                )?,
            )),
            AppCommand::UpdateSpreadsheetProtectedRange(args) => Ok(AppCommandResult::Document(
                self.update_spreadsheet_protected_range(
                    args.sheet_id,
                    args.range,
                    args.description,
                    args.warning_only,
                )?,
            )),
            AppCommand::DeleteSpreadsheetProtectedRange(args) => Ok(AppCommandResult::Document(
                self.delete_spreadsheet_protected_range(args.sheet_id, args.range)?,
            )),
            AppCommand::RestoreSpreadsheetProtectedRange(args) => Ok(AppCommandResult::Document(
                self.restore_spreadsheet_protected_range(args.sheet_id, args.range)?,
            )),
            AppCommand::SetSpreadsheetCellInSheet(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_cell_in_sheet(args.sheet_id, args.address, args.value)?,
            )),
            AppCommand::SetSpreadsheetCellsInSheet(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_cells_in_sheet(args.sheet_id, args.cells)?,
            )),
            AppCommand::SetSpreadsheetCellFormat(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_cell_format(
                    args.sheet_id,
                    args.address,
                    args.property,
                    args.value,
                )?,
            )),
            AppCommand::SetSpreadsheetRowHeight(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_row_height(args.sheet_id, args.row, args.height)?,
            )),
            AppCommand::SetSpreadsheetColumnWidth(args) => Ok(AppCommandResult::Document(
                self.set_spreadsheet_column_width(args.sheet_id, args.column, args.width)?,
            )),
            AppCommand::CopySpreadsheetRange(args) => Ok(AppCommandResult::Document(
                self.copy_spreadsheet_range(args.sheet_id, args.source_range, args.target_address)?,
            )),
            AppCommand::AddSpreadsheetNamedRange(args) => Ok(AppCommandResult::Document(
                self.add_spreadsheet_named_range(args.sheet_id, args.name, args.range)?,
            )),
            AppCommand::UpdateSpreadsheetNamedRange(args) => Ok(AppCommandResult::Document(
                self.update_spreadsheet_named_range(args.sheet_id, args.name, args.range)?,
            )),
            AppCommand::DeleteSpreadsheetNamedRange(args) => Ok(AppCommandResult::Document(
                self.delete_spreadsheet_named_range(args.name)?,
            )),
            AppCommand::RestoreSpreadsheetNamedRange(args) => Ok(AppCommandResult::Document(
                self.restore_spreadsheet_named_range(args.name)?,
            )),
        }
    }
}
