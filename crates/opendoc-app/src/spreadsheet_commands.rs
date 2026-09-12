use super::*;

impl OpenDocApp {
    pub fn set_spreadsheet_cell(
        &mut self,
        address: impl AsRef<str>,
        value: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let address = normalize_cell_address(address.as_ref())?;
        let value = value.into();
        let sheet_id = self
            .workbook
            .sheets
            .first()
            .map(|sheet| sheet.id.clone())
            .unwrap_or_else(|| "sheet-1".to_string());
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::RespectDeferred, |workbook| {
            workbook.set_cell(&address, value.clone());
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("set {address}"),
            AppSpreadsheetOperation::SetCell {
                sheet_id,
                address,
                value,
            },
        );
        Ok(self.document())
    }

    pub fn set_spreadsheet_workbook_metadata(
        &mut self,
        title: impl Into<String>,
        locale: impl Into<String>,
        timezone: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let (title, locale, timezone) =
            self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
                workbook.set_metadata(title.into(), locale.into(), timezone.into())?;
                Ok((
                    workbook.title.clone(),
                    workbook.locale.clone(),
                    workbook.timezone.clone(),
                ))
            })?;
        self.journal_spreadsheet_operation(
            "set spreadsheet workbook metadata",
            AppSpreadsheetOperation::SetWorkbookMetadata {
                title,
                locale,
                timezone,
            },
        );
        Ok(self.document())
    }

    pub fn set_spreadsheet_cells(
        &mut self,
        entries: Vec<(String, String)>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = self
            .workbook
            .sheets
            .first()
            .map(|sheet| sheet.id.clone())
            .unwrap_or_else(|| "sheet-1".to_string());
        let entries = entries
            .into_iter()
            .map(|(address, value)| {
                normalize_cell_address(&address).map(|address| (address, value))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            for (address, value) in &entries {
                workbook.set_cell(address, value.clone());
            }
            Ok(())
        })?;
        for (address, value) in entries {
            self.journal_spreadsheet_operation(
                &format!("set {address}"),
                AppSpreadsheetOperation::SetCell {
                    sheet_id: sheet_id.clone(),
                    address,
                    value,
                },
            );
        }
        Ok(self.document())
    }

    pub fn add_spreadsheet_sheet(
        &mut self,
        title: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let title = title.into();
        let sheet_id = StableId::new("sheet-id").to_string();
        let title = self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.add_sheet_with_id(&sheet_id, &title);
            workbook
                .sheets
                .iter()
                .find(|sheet| sheet.id == sheet_id)
                .map(|sheet| sheet.title.clone())
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))
        })?;
        self.journal_spreadsheet_operation(
            &format!("add sheet {sheet_id}"),
            AppSpreadsheetOperation::AddSheet { sheet_id, title },
        );
        Ok(self.document())
    }

    pub fn rename_spreadsheet_sheet(
        &mut self,
        sheet_id: impl AsRef<str>,
        title: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let title = normalize_sheet_title(title.into());
        let old_title = self
            .workbook
            .sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)
            .map(|sheet| sheet.title.clone())
            .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .rename_sheet(&sheet_id, &title)
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            workbook.rewrite_formula_sheet_title_references(&old_title, &title);
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("rename sheet {sheet_id}"),
            AppSpreadsheetOperation::RenameSheet { sheet_id, title },
        );
        Ok(self.document())
    }

    pub fn delete_spreadsheet_sheet(
        &mut self,
        sheet_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let Some((sheet, named_ranges)) = self.workbook.sheet_restore_payload(&sheet_id) else {
            return Err(AppApiError::NotFound(format!(
                "sheet {sheet_id} was not found"
            )));
        };
        self.mutate_spreadsheet(
            SpreadsheetEvaluationPolicy::Force,
            |workbook| match workbook.delete_sheet(&sheet_id) {
                None => Err(AppApiError::NotFound(format!(
                    "sheet {sheet_id} was not found"
                ))),
                Some(false) => Err(AppApiError::Conflict(
                    "cannot delete the last spreadsheet sheet".to_string(),
                )),
                Some(true) => Ok(()),
            },
        )?;
        self.journal_spreadsheet_operation(
            &format!("delete sheet {sheet_id}"),
            AppSpreadsheetOperation::DeleteSheet {
                sheet_id,
                sheet,
                named_ranges,
            },
        );
        Ok(self.document())
    }

    pub fn restore_spreadsheet_sheet(
        &mut self,
        sheet_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        if self
            .workbook
            .sheets
            .iter()
            .any(|sheet| sheet.id == sheet_id)
        {
            return Err(AppApiError::Conflict(format!(
                "sheet {sheet_id} already exists"
            )));
        }
        let Some((sheet, named_ranges)) = self.deleted_sheet_restore_payload(&sheet_id) else {
            return Err(AppApiError::NotFound(format!(
                "deleted sheet {sheet_id} was not found"
            )));
        };
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.restore_sheet(sheet.clone(), named_ranges.clone())?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("restore sheet {sheet_id}"),
            AppSpreadsheetOperation::RestoreSheet {
                sheet_id,
                sheet,
                named_ranges,
            },
        );
        Ok(self.document())
    }

    pub fn add_spreadsheet_row(
        &mut self,
        sheet_id: impl AsRef<str>,
        row: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let row = normalize_row_label(row.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .add_row(&sheet_id, &row)
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("add row {sheet_id}!{row}"),
            AppSpreadsheetOperation::AddRow { sheet_id, row },
        );
        Ok(self.document())
    }

    pub fn delete_spreadsheet_row(
        &mut self,
        sheet_id: impl AsRef<str>,
        row: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let row = normalize_row_label(row.as_ref())?;
        let payload = self
            .workbook
            .row_restore_payload(&sheet_id, &row)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("sheet {sheet_id} or row {row} was not found"))
            })?;
        self.mutate_spreadsheet(
            SpreadsheetEvaluationPolicy::Force,
            |workbook| match workbook.delete_row(&sheet_id, &row) {
                None => Err(AppApiError::NotFound(format!(
                    "sheet {sheet_id} or row {row} was not found"
                ))),
                Some(false) => Err(AppApiError::Conflict(
                    "cannot delete the last spreadsheet row".to_string(),
                )),
                Some(true) => Ok(()),
            },
        )?;
        self.journal_spreadsheet_operation(
            &format!("delete row {sheet_id}!{row}"),
            AppSpreadsheetOperation::DeleteRow {
                sheet_id,
                row,
                payload,
            },
        );
        Ok(self.document())
    }

    pub fn restore_spreadsheet_row(
        &mut self,
        sheet_id: impl AsRef<str>,
        row: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let row = normalize_row_label(row.as_ref())?;
        // Deletes shift the following rows up, so the label is always back
        // in use; restoring re-opens the position and refills it.
        let payload = self
            .deleted_row_restore_payload(&sheet_id, &row)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("deleted row {sheet_id}!{row} was not found"))
            })?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.restore_row(&sheet_id, &row, payload.clone())?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("restore row {sheet_id}!{row}"),
            AppSpreadsheetOperation::RestoreRow {
                sheet_id,
                row,
                payload,
            },
        );
        Ok(self.document())
    }

    pub fn add_spreadsheet_column(
        &mut self,
        sheet_id: impl AsRef<str>,
        column: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let column = normalize_column_label(column.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .add_column(&sheet_id, &column)
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("add column {sheet_id}!{column}"),
            AppSpreadsheetOperation::AddColumn { sheet_id, column },
        );
        Ok(self.document())
    }

    pub fn delete_spreadsheet_column(
        &mut self,
        sheet_id: impl AsRef<str>,
        column: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let column = normalize_column_label(column.as_ref())?;
        let payload = self
            .workbook
            .column_restore_payload(&sheet_id, &column)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("sheet {sheet_id} or column {column} was not found"))
            })?;
        self.mutate_spreadsheet(
            SpreadsheetEvaluationPolicy::Force,
            |workbook| match workbook.delete_column(&sheet_id, &column) {
                None => Err(AppApiError::NotFound(format!(
                    "sheet {sheet_id} or column {column} was not found"
                ))),
                Some(false) => Err(AppApiError::Conflict(
                    "cannot delete the last spreadsheet column".to_string(),
                )),
                Some(true) => Ok(()),
            },
        )?;
        self.journal_spreadsheet_operation(
            &format!("delete column {sheet_id}!{column}"),
            AppSpreadsheetOperation::DeleteColumn {
                sheet_id,
                column,
                payload,
            },
        );
        Ok(self.document())
    }

    pub fn restore_spreadsheet_column(
        &mut self,
        sheet_id: impl AsRef<str>,
        column: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let column = normalize_column_label(column.as_ref())?;
        let payload = self
            .deleted_column_restore_payload(&sheet_id, &column)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("deleted column {sheet_id}!{column} was not found"))
            })?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.restore_column(&sheet_id, &column, payload.clone())?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("restore column {sheet_id}!{column}"),
            AppSpreadsheetOperation::RestoreColumn {
                sheet_id,
                column,
                payload,
            },
        );
        Ok(self.document())
    }

    pub fn add_spreadsheet_cell_comment(
        &mut self,
        sheet_id: impl AsRef<str>,
        address: impl AsRef<str>,
        author: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let address = normalize_cell_address(address.as_ref())?;
        let author = author.into().trim().to_string();
        let body = body.into();
        AppCellComment {
            id: "pending-cell-comment".to_string(),
            author: author.clone(),
            body: body.clone(),
            deleted: false,
        }
        .validate_source()?;
        let comment_id = format!("cell-comment-{}-{}", self.actor_id, self.next_seq);
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .add_cell_comment(&sheet_id, &address, &comment_id, &author, &body)
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("comment {sheet_id}!{address}"),
            AppSpreadsheetOperation::AddCellComment {
                sheet_id,
                address,
                comment_id,
                author,
                body,
            },
        );
        Ok(self.document())
    }

    pub fn update_spreadsheet_cell_comment(
        &mut self,
        comment_id: impl AsRef<str>,
        body: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let comment_id = parse_id(comment_id.as_ref())?.to_string();
        let body = body.into();
        if body.trim().is_empty() {
            return Err(AppApiError::Format(
                "cell comment body is empty".to_string(),
            ));
        }
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .update_cell_comment(&comment_id, &body)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("cell comment {comment_id} was not found"))
                })?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("update cell comment {comment_id}"),
            AppSpreadsheetOperation::UpdateCellComment { comment_id, body },
        );
        Ok(self.document())
    }

    pub fn delete_spreadsheet_cell_comment(
        &mut self,
        comment_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let comment_id = parse_id(comment_id.as_ref())?.to_string();
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.delete_cell_comment(&comment_id).ok_or_else(|| {
                AppApiError::NotFound(format!("cell comment {comment_id} was not found"))
            })?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("delete cell comment {comment_id}"),
            AppSpreadsheetOperation::DeleteCellComment { comment_id },
        );
        Ok(self.document())
    }

    pub fn restore_spreadsheet_cell_comment(
        &mut self,
        comment_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let comment_id = parse_id(comment_id.as_ref())?.to_string();
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.restore_cell_comment(&comment_id).ok_or_else(|| {
                AppApiError::NotFound(format!("deleted cell comment {comment_id} was not found"))
            })?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("restore cell comment {comment_id}"),
            AppSpreadsheetOperation::RestoreCellComment { comment_id },
        );
        Ok(self.document())
    }

    pub fn set_spreadsheet_frozen_axes(
        &mut self,
        sheet_id: impl AsRef<str>,
        frozen_rows: u32,
        frozen_columns: u32,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let (frozen_rows, frozen_columns) =
            self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
                workbook
                    .set_frozen_axes(&sheet_id, frozen_rows, frozen_columns)
                    .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))
            })?;
        self.journal_spreadsheet_operation(
            &format!("freeze {sheet_id} rows={frozen_rows} columns={frozen_columns}"),
            AppSpreadsheetOperation::SetFrozenAxes {
                sheet_id,
                frozen_rows,
                frozen_columns,
            },
        );
        Ok(self.document())
    }

    pub fn set_spreadsheet_cell_validation(
        &mut self,
        sheet_id: impl AsRef<str>,
        address: impl AsRef<str>,
        kind: impl AsRef<str>,
        values: Vec<String>,
        strict: bool,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let address = normalize_cell_address(address.as_ref())?;
        let validation = AppCellValidation::new(kind.as_ref(), values, strict)?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .set_cell_validation(&sheet_id, &address, validation.clone())
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("validate {sheet_id}!{address} {}", validation.kind),
            AppSpreadsheetOperation::SetCellValidation {
                sheet_id,
                address,
                validation,
            },
        );
        Ok(self.document())
    }

    pub fn clear_spreadsheet_cell_validation(
        &mut self,
        sheet_id: impl AsRef<str>,
        address: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let address = normalize_cell_address(address.as_ref())?;
        let validation = self
            .workbook
            .cell_validation(&sheet_id, &address)
            .ok_or_else(|| {
                AppApiError::NotFound(format!(
                    "cell validation {sheet_id}!{address} was not found"
                ))
            })?
            .clone();
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .clear_cell_validation(&sheet_id, &address)
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("clear validation {sheet_id}!{address}"),
            AppSpreadsheetOperation::ClearCellValidation {
                sheet_id,
                address,
                validation,
            },
        );
        Ok(self.document())
    }

    pub fn restore_spreadsheet_cell_validation(
        &mut self,
        sheet_id: impl AsRef<str>,
        address: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let address = normalize_cell_address(address.as_ref())?;
        if self.workbook.cell_validation(&sheet_id, &address).is_some() {
            return Err(AppApiError::Conflict(format!(
                "cell validation {sheet_id}!{address} already exists"
            )));
        }
        let validation = self
            .deleted_cell_validation_restore_payload(&sheet_id, &address)
            .ok_or_else(|| {
                AppApiError::NotFound(format!(
                    "deleted cell validation {sheet_id}!{address} was not found"
                ))
            })?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.restore_cell_validation(&sheet_id, &address, validation.clone())?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("restore validation {sheet_id}!{address}"),
            AppSpreadsheetOperation::RestoreCellValidation {
                sheet_id,
                address,
                validation,
            },
        );
        Ok(self.document())
    }

    pub fn merge_spreadsheet_cells(
        &mut self,
        sheet_id: impl AsRef<str>,
        range: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let range = normalize_merge_range(range.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.merge_cells(&sheet_id, &range).ok_or_else(|| {
                AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
            })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("merge {sheet_id}!{range}"),
            AppSpreadsheetOperation::MergeCells { sheet_id, range },
        );
        Ok(self.document())
    }

    pub fn unmerge_spreadsheet_cells(
        &mut self,
        sheet_id: impl AsRef<str>,
        range: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let range = normalize_cell_range(range.as_ref())?;
        let merge = self
            .workbook
            .merge_range(&sheet_id, &range)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("merge {sheet_id}!{range} was not found"))
            })?
            .clone();
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .unmerge_cells(&sheet_id, &range)
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("unmerge {sheet_id}!{range}"),
            AppSpreadsheetOperation::UnmergeCells {
                sheet_id,
                range,
                merge,
            },
        );
        Ok(self.document())
    }

    pub fn restore_spreadsheet_merge(
        &mut self,
        sheet_id: impl AsRef<str>,
        range: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let range = normalize_merge_range(range.as_ref())?;
        if self.workbook.merge_range(&sheet_id, &range).is_some() {
            return Err(AppApiError::Conflict(format!(
                "merge {sheet_id}!{range} already exists"
            )));
        }
        let merge = self
            .deleted_merge_restore_payload(&sheet_id, &range)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("deleted merge {sheet_id}!{range} was not found"))
            })?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.restore_merge(&sheet_id, merge.clone())?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("restore merge {sheet_id}!{range}"),
            AppSpreadsheetOperation::RestoreMerge {
                sheet_id,
                range,
                merge,
            },
        );
        Ok(self.document())
    }

    pub fn set_spreadsheet_basic_filter(
        &mut self,
        sheet_id: impl AsRef<str>,
        range: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let range = normalize_cell_range(range.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .set_basic_filter(&sheet_id, &range)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("filter {sheet_id}!{range}"),
            AppSpreadsheetOperation::SetBasicFilter { sheet_id, range },
        );
        Ok(self.document())
    }

    pub fn set_spreadsheet_basic_filter_options(
        &mut self,
        sheet_id: impl AsRef<str>,
        criteria: Vec<AppSheetFilterCriterion>,
        sort_specs: Vec<AppSheetFilterSortSpec>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let criteria = normalize_filter_criteria(criteria)?;
        let sort_specs = normalize_filter_sort_specs(sort_specs)?;
        validate_filter_option_payload(&criteria, &sort_specs)?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .set_basic_filter_options(&sheet_id, criteria.clone(), sort_specs.clone())
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("filter options {sheet_id}"),
            AppSpreadsheetOperation::SetBasicFilterOptions {
                sheet_id,
                criteria,
                sort_specs,
            },
        );
        Ok(self.document())
    }

    pub fn clear_spreadsheet_basic_filter(
        &mut self,
        sheet_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let filter = self
            .workbook
            .basic_filter(&sheet_id)
            .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} has no filter")))?
            .clone();
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .clear_basic_filter(&sheet_id)
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("clear filter {sheet_id}"),
            AppSpreadsheetOperation::ClearBasicFilter { sheet_id, filter },
        );
        Ok(self.document())
    }

    pub fn restore_spreadsheet_basic_filter(
        &mut self,
        sheet_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        if self.workbook.basic_filter(&sheet_id).is_some() {
            return Err(AppApiError::Conflict(format!(
                "sheet {sheet_id} already has a filter"
            )));
        }
        let filter = self
            .deleted_basic_filter_restore_payload(&sheet_id)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("deleted filter for sheet {sheet_id} was not found"))
            })?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.restore_basic_filter(&sheet_id, filter.clone())?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("restore filter {sheet_id}"),
            AppSpreadsheetOperation::RestoreBasicFilter { sheet_id, filter },
        );
        Ok(self.document())
    }

    pub fn add_spreadsheet_protected_range(
        &mut self,
        sheet_id: impl AsRef<str>,
        range: impl AsRef<str>,
        description: impl Into<String>,
        warning_only: bool,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let range = normalize_cell_range(range.as_ref())?;
        let description = normalize_protected_range_description(description.into());
        validate_protected_range_description(&description)?;
        if !warning_only {
            self.push_model_warning(
                "protected-range-warning-only",
                format!(
                    "protected range {sheet_id}!{range} was downgraded to warning-only because v0 does not enforce spreadsheet permissions"
                ),
            );
        }
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .add_protected_range(&sheet_id, &range, &description, true)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("protect {sheet_id}!{range}"),
            AppSpreadsheetOperation::AddProtectedRange {
                sheet_id,
                range,
                description,
                warning_only: true,
            },
        );
        Ok(self.document())
    }

    pub fn delete_spreadsheet_protected_range(
        &mut self,
        sheet_id: impl AsRef<str>,
        range: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let range = normalize_cell_range(range.as_ref())?;
        let protected_range = self
            .workbook
            .protected_range(&sheet_id, &range)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("protected range {sheet_id}!{range} was not found"))
            })?
            .clone();
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .delete_protected_range(&sheet_id, &range)
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("delete protected range {sheet_id}!{range}"),
            AppSpreadsheetOperation::DeleteProtectedRange {
                sheet_id,
                range,
                protected_range,
            },
        );
        Ok(self.document())
    }

    pub fn restore_spreadsheet_protected_range(
        &mut self,
        sheet_id: impl AsRef<str>,
        range: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let range = normalize_cell_range(range.as_ref())?;
        if self.workbook.protected_range(&sheet_id, &range).is_some() {
            return Err(AppApiError::Conflict(format!(
                "protected range {sheet_id}!{range} already exists"
            )));
        }
        let protected_range = self
            .deleted_protected_range_restore_payload(&sheet_id, &range)
            .ok_or_else(|| {
                AppApiError::NotFound(format!(
                    "deleted protected range {sheet_id}!{range} was not found"
                ))
            })?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.restore_protected_range(&sheet_id, protected_range.clone())?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("restore protected range {sheet_id}!{range}"),
            AppSpreadsheetOperation::RestoreProtectedRange {
                sheet_id,
                range,
                protected_range,
            },
        );
        Ok(self.document())
    }

    pub fn update_spreadsheet_protected_range(
        &mut self,
        sheet_id: impl AsRef<str>,
        range: impl AsRef<str>,
        description: impl Into<String>,
        warning_only: bool,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let range = normalize_cell_range(range.as_ref())?;
        let description = normalize_protected_range_description(description.into());
        validate_protected_range_description(&description)?;
        if !warning_only {
            self.push_model_warning(
                "protected-range-warning-only",
                format!(
                    "protected range {sheet_id}!{range} was downgraded to warning-only because v0 does not enforce spreadsheet permissions"
                ),
            );
        }
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .update_protected_range(&sheet_id, &range, &description, true)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("update protected range {sheet_id}!{range}"),
            AppSpreadsheetOperation::UpdateProtectedRange {
                sheet_id,
                range,
                description,
                warning_only: true,
            },
        );
        Ok(self.document())
    }

    pub fn set_spreadsheet_cell_in_sheet(
        &mut self,
        sheet_id: impl AsRef<str>,
        address: impl AsRef<str>,
        value: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let address = normalize_cell_address(address.as_ref())?;
        let value = value.into();
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .set_cell_in_sheet(&sheet_id, &address, value.clone())
                .ok_or_else(|| AppApiError::NotFound(format!("sheet {sheet_id} was not found")))?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("set {sheet_id}!{address}"),
            AppSpreadsheetOperation::SetCell {
                sheet_id,
                address,
                value,
            },
        );
        Ok(self.document())
    }

    pub fn set_spreadsheet_cells_in_sheet(
        &mut self,
        sheet_id: impl AsRef<str>,
        entries: Vec<(String, String)>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let entries = entries
            .into_iter()
            .map(|(address, value)| {
                normalize_cell_address(&address).map(|address| (address, value))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            for (address, value) in &entries {
                workbook
                    .set_cell_in_sheet(&sheet_id, address, value.clone())
                    .ok_or_else(|| {
                        AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                    })?;
            }
            Ok(())
        })?;
        for (address, value) in entries {
            self.journal_spreadsheet_operation(
                &format!("set {sheet_id}!{address}"),
                AppSpreadsheetOperation::SetCell {
                    sheet_id: sheet_id.clone(),
                    address,
                    value,
                },
            );
        }
        Ok(self.document())
    }

    pub fn set_spreadsheet_cell_format(
        &mut self,
        sheet_id: impl AsRef<str>,
        address: impl AsRef<str>,
        property: impl AsRef<str>,
        value: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let address = normalize_cell_address(address.as_ref())?;
        let property = property.as_ref().trim().to_string();
        let value = value.into();
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .set_cell_format(&sheet_id, &address, &property, value.clone())
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("format {sheet_id}!{address} {property}"),
            AppSpreadsheetOperation::SetCellFormat {
                sheet_id,
                address,
                property,
                value,
            },
        );
        Ok(self.document())
    }

    /// Stores an explicit row height in pixels. `height == 0` clears the
    /// explicit height so the row falls back to the default.
    pub fn set_spreadsheet_row_height(
        &mut self,
        sheet_id: impl AsRef<str>,
        row: impl AsRef<str>,
        height: u32,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let row = normalize_row_label(row.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::RespectDeferred, |workbook| {
            workbook
                .set_row_height(&sheet_id, &row, height)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} or row {row} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("set row height {sheet_id}!{row} {height}"),
            AppSpreadsheetOperation::SetRowHeight {
                sheet_id,
                row,
                height,
            },
        );
        Ok(self.document())
    }

    /// Stores an explicit column width in pixels. `width == 0` clears the
    /// explicit width so the column falls back to the default.
    pub fn set_spreadsheet_column_width(
        &mut self,
        sheet_id: impl AsRef<str>,
        column: impl AsRef<str>,
        width: u32,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let column = normalize_column_label(column.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::RespectDeferred, |workbook| {
            workbook
                .set_column_width(&sheet_id, &column, width)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!(
                        "sheet {sheet_id} or column {column} was not found"
                    ))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("set column width {sheet_id}!{column} {width}"),
            AppSpreadsheetOperation::SetColumnWidth {
                sheet_id,
                column,
                width,
            },
        );
        Ok(self.document())
    }

    pub fn copy_spreadsheet_range(
        &mut self,
        sheet_id: impl AsRef<str>,
        source_range: impl AsRef<str>,
        target_address: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let source_range = normalize_cell_range(source_range.as_ref())?;
        let target_address = normalize_cell_address(target_address.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .copy_range(&sheet_id, &source_range, &target_address)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("copy {sheet_id}!{} to {target_address}", source_range),
            AppSpreadsheetOperation::CopyRange {
                sheet_id,
                source_range,
                target_address,
            },
        );
        Ok(self.document())
    }

    /// Sorts the rows of a range by one of its columns (SH-7).
    pub fn sort_spreadsheet_range(
        &mut self,
        sheet_id: impl AsRef<str>,
        range: impl AsRef<str>,
        column: impl AsRef<str>,
        descending: bool,
        has_header: bool,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let range = normalize_cell_range(range.as_ref())?;
        let column = normalize_column_label(column.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .sort_range(&sheet_id, &range, &column, descending, has_header)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!(
                "sort {sheet_id}!{range} by {column} {}",
                if descending {
                    "descending"
                } else {
                    "ascending"
                }
            ),
            AppSpreadsheetOperation::SortRange {
                sheet_id,
                range,
                column,
                descending,
                has_header,
            },
        );
        Ok(self.document())
    }

    /// Expands a source range over the range a fill-handle drag covered
    /// (SH-28). The series itself is inferred in `opendoc-spreadsheet`.
    pub fn fill_spreadsheet_range(
        &mut self,
        sheet_id: impl AsRef<str>,
        source_range: impl AsRef<str>,
        target_range: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let source_range = normalize_cell_range(source_range.as_ref())?;
        let target_range = normalize_cell_range(target_range.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .fill_range(&sheet_id, &source_range, &target_range)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("fill {sheet_id}!{source_range} over {target_range}"),
            AppSpreadsheetOperation::FillRange {
                sheet_id,
                source_range,
                target_range,
            },
        );
        Ok(self.document())
    }

    /// Imports CSV/TSV text into an existing sheet, starting at `origin`
    /// (SH-47). Every imported cell is journalled as an ordinary cell edit,
    /// so an import replays and undoes like any other block of typing.
    pub fn import_spreadsheet_csv(
        &mut self,
        sheet_id: impl AsRef<str>,
        origin: impl AsRef<str>,
        text: impl AsRef<str>,
        delimiter: Option<&str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let origin = normalize_cell_address(origin.as_ref())?;
        let delimiter = delimiter
            .map(str::trim)
            .filter(|delimiter| !delimiter.is_empty());
        let cells = AppSpreadsheetWorkbook::csv_cell_edits(&origin, text.as_ref(), delimiter)?;
        if cells.is_empty() {
            return Ok(self.document());
        }
        self.set_spreadsheet_cells_in_sheet(sheet_id, cells)
    }

    /// Serializes one sheet as CSV/TSV using display text (SH-47).
    pub fn export_spreadsheet_csv(
        &self,
        sheet_id: impl AsRef<str>,
        delimiter: Option<&str>,
    ) -> Result<String, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let delimiter = delimiter
            .map(str::trim)
            .filter(|delimiter| !delimiter.is_empty());
        Ok(self.workbook.evaluated().export_csv(&sheet_id, delimiter)?)
    }

    /// Replaces the workbook with an imported XLSX file (SH-47), mirroring
    /// the Google Sheets JSON import.
    pub fn import_spreadsheet_xlsx(
        &mut self,
        title: impl AsRef<str>,
        base64: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let title = normalize_sheet_title(title.as_ref());
        let imported = AppSpreadsheetWorkbook::from_xlsx_base64(base64.as_ref(), &title)?;
        imported.validate_source()?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            *workbook = imported;
            Ok(())
        })?;
        self.push_app_operation("import-spreadsheet-xlsx", "import XLSX workbook");
        Ok(self.document())
    }

    /// Serializes the workbook as a base64-encoded XLSX file (SH-47).
    pub fn export_spreadsheet_xlsx(&self) -> Result<String, AppApiError> {
        Ok(self.workbook.evaluated().to_xlsx_base64()?)
    }

    pub fn add_spreadsheet_named_range(
        &mut self,
        sheet_id: impl AsRef<str>,
        name: impl AsRef<str>,
        range: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let name = normalize_named_range_name(name.as_ref())?;
        let range = normalize_cell_range(range.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .add_named_range(&sheet_id, &name, &range)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("name {sheet_id}!{range} as {name}"),
            AppSpreadsheetOperation::AddNamedRange {
                sheet_id,
                name,
                range,
            },
        );
        Ok(self.document())
    }

    pub fn delete_spreadsheet_named_range(
        &mut self,
        name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let name = normalize_named_range_name(name.as_ref())?;
        let range = self
            .workbook
            .named_range(&name)
            .ok_or_else(|| AppApiError::NotFound(format!("named range {name} was not found")))?
            .clone();
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.delete_named_range(&name).ok_or_else(|| {
                AppApiError::NotFound(format!("named range {name} was not found"))
            })?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("delete named range {name}"),
            AppSpreadsheetOperation::DeleteNamedRange { name, range },
        );
        Ok(self.document())
    }

    pub fn restore_spreadsheet_named_range(
        &mut self,
        name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let name = normalize_named_range_name(name.as_ref())?;
        if self
            .workbook
            .named_ranges
            .iter()
            .any(|range| range.name == name)
        {
            return Err(AppApiError::Conflict(format!(
                "named range {name} already exists"
            )));
        }
        let range = self
            .deleted_named_range_restore_payload(&name)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("deleted named range {name} was not found"))
            })?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook.restore_named_range(range.clone())?;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("restore named range {name}"),
            AppSpreadsheetOperation::RestoreNamedRange { name, range },
        );
        Ok(self.document())
    }

    pub fn update_spreadsheet_named_range(
        &mut self,
        sheet_id: impl AsRef<str>,
        name: impl AsRef<str>,
        range: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let name = normalize_named_range_name(name.as_ref())?;
        let range = normalize_cell_range(range.as_ref())?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            workbook
                .update_named_range(&sheet_id, &name, &range)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                })??;
            Ok(())
        })?;
        self.journal_spreadsheet_operation(
            &format!("update named range {name} to {sheet_id}!{range}"),
            AppSpreadsheetOperation::UpdateNamedRange {
                sheet_id,
                name,
                range,
            },
        );
        Ok(self.document())
    }
}
