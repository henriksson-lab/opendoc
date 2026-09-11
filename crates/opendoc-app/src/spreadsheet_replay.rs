use super::*;
use opendoc_spreadsheet::SpreadsheetError;

pub(crate) fn apply_spreadsheet_envelopes(
    workbook: &mut AppSpreadsheetWorkbook,
    envelopes: &[AppOperationEnvelope],
) -> Result<Vec<AppWarning>, AppApiError> {
    let mut warnings = Vec::new();
    let deleted_rows_by_other_actor = spreadsheet_replay_deleted_rows_by_actor(envelopes);
    let deleted_columns_by_other_actor = spreadsheet_replay_deleted_columns_by_actor(envelopes);
    macro_rules! replay_sheet_id {
        ($sheet_id:expr, $action:expr) => {
            match normalize_sheet_id($sheet_id) {
                Ok(sheet_id) => sheet_id,
                Err(err) => {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-sheet",
                        format!("spreadsheet {} was ignored: {err}", $action),
                    );
                    continue;
                }
            }
        };
    }
    for envelope in envelopes {
        let previous_workbook = workbook.clone();
        match &envelope.spreadsheet {
            Some(AppSpreadsheetOperation::SetWorkbookMetadata {
                title,
                locale,
                timezone,
            }) => {
                if let Err(err) = workbook.set_metadata(title, locale, timezone) {
                    warnings.push(AppWarning {
                        code: "invalid-spreadsheet-workbook-metadata".to_string(),
                        message: format!("spreadsheet workbook metadata update was ignored: {err}"),
                    });
                }
            }
            Some(AppSpreadsheetOperation::AddSheet { sheet_id, title }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "sheet add");
                let normalized_title = normalize_sheet_title(title.clone());
                if let Some(existing) = workbook.sheets.iter().find(|sheet| sheet.id == sheet_id) {
                    if existing.title != normalized_title {
                        push_spreadsheet_warning(
                            &mut warnings,
                            "duplicate-spreadsheet-sheet",
                            format!(
                                "spreadsheet sheet add for {sheet_id} was ignored because the sheet id already exists with a different title"
                            ),
                        );
                    }
                } else {
                    workbook.add_sheet_with_id(&sheet_id, title);
                }
            }
            Some(AppSpreadsheetOperation::RenameSheet { sheet_id, title }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "sheet rename");
                let old_title = workbook
                    .sheets
                    .iter()
                    .find(|sheet| sheet.id == sheet_id)
                    .map(|sheet| sheet.title.clone());
                if workbook.rename_sheet(&sheet_id, title).is_none() {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "missing-spreadsheet-sheet",
                        format!(
                            "spreadsheet sheet rename for {sheet_id} was ignored because the sheet was missing"
                        ),
                    );
                } else if let Some(old_title) = old_title {
                    workbook.rewrite_formula_sheet_title_references(&old_title, title);
                }
            }
            Some(AppSpreadsheetOperation::DeleteSheet { sheet_id, .. }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "sheet delete");
                match workbook.delete_sheet(&sheet_id) {
                    None => push_spreadsheet_warning(
                        &mut warnings,
                        "missing-spreadsheet-sheet",
                        format!(
                            "spreadsheet sheet delete for {sheet_id} was ignored because the sheet was missing"
                        ),
                    ),
                    Some(false) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-sheet-delete",
                        "spreadsheet sheet delete was ignored because it would remove the last sheet"
                            .to_string(),
                    ),
                    Some(true) => {}
                }
            }
            Some(AppSpreadsheetOperation::RestoreSheet {
                sheet_id,
                sheet,
                named_ranges,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "sheet restore");
                if workbook
                    .sheets
                    .iter()
                    .any(|existing| existing.id == sheet_id)
                {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "duplicate-spreadsheet-sheet",
                        format!(
                            "spreadsheet sheet restore for {sheet_id} was ignored because the sheet already exists"
                        ),
                    );
                } else if let Err(err) = workbook.restore_sheet(sheet.clone(), named_ranges.clone())
                {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-sheet-restore",
                        format!("spreadsheet sheet restore for {sheet_id} was ignored: {err}"),
                    );
                }
            }
            Some(AppSpreadsheetOperation::AddRow { sheet_id, row }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "row add");
                match normalize_row_label(row) {
                    Ok(row) => {
                        if workbook.add_row(&sheet_id, &row).is_none() {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet row add {sheet_id}!{row} was ignored because the sheet was missing"
                                ),
                            );
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-row",
                        format!("spreadsheet row add {sheet_id}!{row} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::DeleteRow { sheet_id, row, .. }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "row delete");
                match normalize_row_label(row) {
                    Ok(row) => {
                        match workbook.delete_row(&sheet_id, &row) {
                            None => {
                                if workbook
                                .sheets
                                .iter()
                                .any(|sheet| sheet.id == sheet_id.as_str())
                                {
                                    push_spreadsheet_warning(
                                        &mut warnings,
                                        "missing-spreadsheet-row",
                                        format!(
                                            "spreadsheet row delete {sheet_id}!{row} was ignored because the row was missing"
                                        ),
                                    );
                                } else {
                                    push_spreadsheet_warning(
                                        &mut warnings,
                                        "missing-spreadsheet-sheet",
                                        format!(
                                            "spreadsheet row delete {sheet_id}!{row} was ignored because the sheet was missing"
                                        ),
                                    );
                                }
                            }
                            Some(false) => push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-row-delete",
                                "spreadsheet row delete was ignored because it would remove the last row"
                                    .to_string(),
                            ),
                            Some(true) => {}
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-row",
                        format!("spreadsheet row delete {sheet_id}!{row} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::RestoreRow {
                sheet_id,
                row,
                payload,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "row restore");
                match normalize_row_label(row) {
                    Ok(row) => {
                        let mut candidate = workbook.clone();
                        match candidate.restore_row(&sheet_id, &row, payload.clone()) {
                            Ok(()) => *workbook = candidate,
                            Err(SpreadsheetError::NotFound(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet row restore {sheet_id}!{row} was ignored because the sheet was missing"
                                ),
                            ),
                            Err(SpreadsheetError::Conflict(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "spreadsheet-row-conflict",
                                format!(
                                    "spreadsheet row restore {sheet_id}!{row} was ignored because the row already exists"
                                ),
                            ),
                            Err(err) => push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-row",
                                format!("spreadsheet row restore {sheet_id}!{row} was ignored: {err}"),
                            ),
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-row",
                        format!("spreadsheet row restore {sheet_id}!{row} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::AddColumn { sheet_id, column }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "column add");
                match normalize_column_label(column) {
                    Ok(column) => {
                        if workbook.add_column(&sheet_id, &column).is_none() {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet column add {sheet_id}!{column} was ignored because the sheet was missing"
                                ),
                            );
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-column",
                        format!("spreadsheet column add {sheet_id}!{column} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::DeleteColumn {
                sheet_id, column, ..
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "column delete");
                match normalize_column_label(column) {
                    Ok(column) => {
                        match workbook.delete_column(&sheet_id, &column) {
                            None => {
                                if workbook
                                .sheets
                                .iter()
                                .any(|sheet| sheet.id == sheet_id.as_str())
                                {
                                    push_spreadsheet_warning(
                                        &mut warnings,
                                        "missing-spreadsheet-column",
                                        format!(
                                            "spreadsheet column delete {sheet_id}!{column} was ignored because the column was missing"
                                        ),
                                    );
                                } else {
                                    push_spreadsheet_warning(
                                        &mut warnings,
                                        "missing-spreadsheet-sheet",
                                        format!(
                                            "spreadsheet column delete {sheet_id}!{column} was ignored because the sheet was missing"
                                        ),
                                    );
                                }
                            }
                            Some(false) => push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-column-delete",
                                "spreadsheet column delete was ignored because it would remove the last column"
                                    .to_string(),
                            ),
                            Some(true) => {}
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-column",
                        format!("spreadsheet column delete {sheet_id}!{column} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::RestoreColumn {
                sheet_id,
                column,
                payload,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "column restore");
                match normalize_column_label(column) {
                    Ok(column) => {
                        let mut candidate = workbook.clone();
                        match candidate.restore_column(&sheet_id, &column, payload.clone()) {
                            Ok(()) => *workbook = candidate,
                            Err(SpreadsheetError::NotFound(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet column restore {sheet_id}!{column} was ignored because the sheet was missing"
                                ),
                            ),
                            Err(SpreadsheetError::Conflict(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "spreadsheet-column-conflict",
                                format!(
                                    "spreadsheet column restore {sheet_id}!{column} was ignored because the column already exists"
                                ),
                            ),
                            Err(err) => push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-column",
                                format!(
                                    "spreadsheet column restore {sheet_id}!{column} was ignored: {err}"
                                ),
                            ),
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-column",
                        format!(
                            "spreadsheet column restore {sheet_id}!{column} was ignored: {err}"
                        ),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::AddCellComment {
                sheet_id,
                address,
                comment_id,
                author,
                body,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "cell comment add");
                match normalize_cell_address(address) {
                    Ok(address) => {
                        if spreadsheet_replay_cell_address_deleted_by_other_actor(
                            &mut warnings,
                            &deleted_rows_by_other_actor,
                            &deleted_columns_by_other_actor,
                            &envelope.record.actor,
                            &sheet_id,
                            &address,
                            "cell comment",
                        ) {
                            continue;
                        }
                        let comment = AppCellComment {
                            id: comment_id.clone(),
                            author: author.clone(),
                            body: body.clone(),
                            deleted: false,
                        };
                        if let Err(err) = comment.validate_source() {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-cell-comment",
                                format!(
                                    "spreadsheet cell comment {sheet_id}!{address} was ignored: {err}"
                                ),
                            );
                        } else if workbook
                            .add_cell_comment(&sheet_id, &address, comment_id, author, body)
                            .is_none()
                        {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet cell comment {sheet_id}!{address} was ignored because the sheet was missing"
                                ),
                            );
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-cell-address",
                        format!("spreadsheet cell comment {sheet_id}!{address} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::UpdateCellComment { comment_id, body }) => {
                if body.trim().is_empty() {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-cell-comment",
                        format!(
                            "spreadsheet cell comment {comment_id} update was ignored: cell comment body is empty"
                        ),
                    );
                } else if workbook.update_cell_comment(comment_id, body).is_none() {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "missing-spreadsheet-cell-comment",
                        format!(
                            "spreadsheet cell comment {comment_id} update was ignored because the comment was missing or deleted"
                        ),
                    );
                }
            }
            Some(AppSpreadsheetOperation::DeleteCellComment { comment_id }) => {
                if workbook.delete_cell_comment(comment_id).is_none() {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "missing-spreadsheet-cell-comment",
                        format!(
                            "spreadsheet cell comment {comment_id} delete was ignored because the comment was missing"
                        ),
                    );
                }
            }
            Some(AppSpreadsheetOperation::RestoreCellComment { comment_id }) => {
                if workbook.restore_cell_comment(comment_id).is_none() {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "missing-spreadsheet-cell-comment",
                        format!(
                            "spreadsheet cell comment {comment_id} restore was ignored because the deleted comment was missing"
                        ),
                    );
                }
            }
            Some(AppSpreadsheetOperation::SetFrozenAxes {
                sheet_id,
                frozen_rows,
                frozen_columns,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "frozen axes set");
                if workbook
                    .set_frozen_axes(&sheet_id, *frozen_rows, *frozen_columns)
                    .is_none()
                {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "missing-spreadsheet-sheet",
                        format!(
                            "spreadsheet frozen axes for {sheet_id} were ignored because the sheet was missing"
                        ),
                    );
                }
            }
            Some(AppSpreadsheetOperation::SetCellValidation {
                sheet_id,
                address,
                validation,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "cell validation set");
                match normalize_cell_address(address) {
                    Ok(address) => {
                        if spreadsheet_replay_cell_address_deleted_by_other_actor(
                            &mut warnings,
                            &deleted_rows_by_other_actor,
                            &deleted_columns_by_other_actor,
                            &envelope.record.actor,
                            &sheet_id,
                            &address,
                            "validation",
                        ) {
                            continue;
                        }
                        if let Err(err) = validation.validate_source() {
                            warnings.push(AppWarning {
                                code: "invalid-spreadsheet-cell-validation".to_string(),
                                message: format!(
                                    "spreadsheet validation for {sheet_id}!{address} was ignored: {err}"
                                ),
                            });
                        } else if workbook
                            .set_cell_validation(&sheet_id, &address, validation.clone())
                            .is_none()
                        {
                            warnings.push(AppWarning {
                                code: "missing-spreadsheet-sheet".to_string(),
                                message: format!(
                                    "spreadsheet validation for {sheet_id}!{address} was ignored because the sheet was missing"
                                ),
                            });
                        }
                    }
                    Err(err) => {
                        warnings.push(AppWarning {
                            code: "invalid-spreadsheet-cell-address".to_string(),
                            message: format!(
                                "spreadsheet validation for {sheet_id}!{address} was ignored: {err}"
                            ),
                        });
                    }
                }
            }
            Some(AppSpreadsheetOperation::ClearCellValidation {
                sheet_id, address, ..
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "cell validation clear");
                match normalize_cell_address(address) {
                    Ok(address) => {
                        if workbook
                            .clear_cell_validation(&sheet_id, &address)
                            .is_none()
                        {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet validation clear {sheet_id}!{address} was ignored because the sheet was missing"
                                ),
                            );
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-cell-address",
                        format!(
                            "spreadsheet validation clear {sheet_id}!{address} was ignored: {err}"
                        ),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::RestoreCellValidation {
                sheet_id,
                address,
                validation,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "cell validation restore");
                match normalize_cell_address(address) {
                    Ok(address) => {
                        let mut candidate = workbook.clone();
                        match candidate.restore_cell_validation(
                            &sheet_id,
                            &address,
                            validation.clone(),
                        ) {
                            Ok(()) => *workbook = candidate,
                            Err(SpreadsheetError::NotFound(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet validation restore {sheet_id}!{address} was ignored because the sheet was missing"
                                ),
                            ),
                            Err(SpreadsheetError::Conflict(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "spreadsheet-cell-validation-conflict",
                                format!(
                                    "spreadsheet validation restore {sheet_id}!{address} was ignored because the cell already has validation"
                                ),
                            ),
                            Err(err) => push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-cell-validation",
                                format!(
                                    "spreadsheet validation restore {sheet_id}!{address} was ignored: {err}"
                                ),
                            ),
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-cell-address",
                        format!(
                            "spreadsheet validation restore {sheet_id}!{address} was ignored: {err}"
                        ),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::MergeCells { sheet_id, range }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "merge");
                apply_spreadsheet_result(
                    workbook,
                    &mut warnings,
                    "missing-spreadsheet-sheet",
                    format!(
                        "spreadsheet merge {sheet_id}!{range} was ignored because the sheet was missing"
                    ),
                    "invalid-spreadsheet-merge-range",
                    format!("spreadsheet merge {sheet_id}!{range} was ignored"),
                    |candidate| candidate.merge_cells(&sheet_id, range),
                );
            }
            Some(AppSpreadsheetOperation::UnmergeCells {
                sheet_id, range, ..
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "unmerge");
                match normalize_cell_range(range) {
                    Ok(range) => {
                        if workbook.unmerge_cells(&sheet_id, &range).is_none() {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet unmerge {sheet_id}!{range} was ignored because the sheet was missing"
                                ),
                            );
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-merge-range",
                        format!("spreadsheet unmerge {sheet_id}!{range} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::RestoreMerge {
                sheet_id,
                range,
                merge,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "merge restore");
                match normalize_merge_range(range) {
                    Ok(range) => {
                        let mut candidate = workbook.clone();
                        match candidate.restore_merge(&sheet_id, merge.clone()) {
                            Ok(()) => *workbook = candidate,
                            Err(SpreadsheetError::NotFound(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet merge restore {sheet_id}!{range} was ignored because the sheet was missing"
                                ),
                            ),
                            Err(SpreadsheetError::Conflict(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "spreadsheet-merge-conflict",
                                format!(
                                    "spreadsheet merge restore {sheet_id}!{range} was ignored because the merge already exists or overlaps"
                                ),
                            ),
                            Err(err) => push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-merge-range",
                                format!("spreadsheet merge restore {sheet_id}!{range} was ignored: {err}"),
                            ),
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-merge-range",
                        format!("spreadsheet merge restore {sheet_id}!{range} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::SetBasicFilter { sheet_id, range }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "filter set");
                apply_spreadsheet_result(
                    workbook,
                    &mut warnings,
                    "missing-spreadsheet-sheet",
                    format!(
                        "spreadsheet filter {sheet_id}!{range} was ignored because the sheet was missing"
                    ),
                    "invalid-spreadsheet-filter-range",
                    format!("spreadsheet filter {sheet_id}!{range} was ignored"),
                    |candidate| candidate.set_basic_filter(&sheet_id, range),
                );
            }
            Some(AppSpreadsheetOperation::SetBasicFilterOptions {
                sheet_id,
                criteria,
                sort_specs,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "filter options set");
                apply_spreadsheet_result(
                    workbook,
                    &mut warnings,
                    "missing-spreadsheet-sheet",
                    format!(
                        "spreadsheet filter options for {sheet_id} were ignored because the sheet was missing"
                    ),
                    "invalid-spreadsheet-filter-options",
                    format!("spreadsheet filter options for {sheet_id} were ignored"),
                    |candidate| {
                        candidate.set_basic_filter_options(
                            &sheet_id,
                            criteria.clone(),
                            sort_specs.clone(),
                        )
                    },
                );
            }
            Some(AppSpreadsheetOperation::ClearBasicFilter { sheet_id, .. }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "filter clear");
                if workbook.clear_basic_filter(&sheet_id).is_none() {
                    push_spreadsheet_warning(
                        &mut warnings,
                        "missing-spreadsheet-sheet",
                        format!(
                            "spreadsheet filter clear for {sheet_id} was ignored because the sheet was missing"
                        ),
                    );
                }
            }
            Some(AppSpreadsheetOperation::RestoreBasicFilter { sheet_id, filter }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "filter restore");
                let mut candidate = workbook.clone();
                match candidate.restore_basic_filter(&sheet_id, filter.clone()) {
                    Ok(()) => *workbook = candidate,
                    Err(SpreadsheetError::NotFound(_)) => push_spreadsheet_warning(
                        &mut warnings,
                        "missing-spreadsheet-sheet",
                        format!(
                            "spreadsheet filter restore for {sheet_id} was ignored because the sheet was missing"
                        ),
                    ),
                    Err(SpreadsheetError::Conflict(_)) => push_spreadsheet_warning(
                        &mut warnings,
                        "spreadsheet-filter-conflict",
                        format!(
                            "spreadsheet filter restore for {sheet_id} was ignored because the sheet already has a filter"
                        ),
                    ),
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-filter",
                        format!("spreadsheet filter restore for {sheet_id} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::AddProtectedRange {
                sheet_id,
                range,
                description,
                warning_only,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "protected range add");
                apply_spreadsheet_result(
                    workbook,
                    &mut warnings,
                    "missing-spreadsheet-sheet",
                    format!(
                        "spreadsheet protected range {sheet_id}!{range} was ignored because the sheet was missing"
                    ),
                    "invalid-spreadsheet-protected-range",
                    format!("spreadsheet protected range {sheet_id}!{range} was ignored"),
                    |candidate| {
                        candidate.add_protected_range(&sheet_id, range, description, *warning_only)
                    },
                );
            }
            Some(AppSpreadsheetOperation::UpdateProtectedRange {
                sheet_id,
                range,
                description,
                warning_only,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "protected range update");
                let normalized = normalize_cell_range(range);
                let description = normalize_protected_range_description(description.to_string());
                match normalized {
                    Ok(range) => {
                        if let Err(err) = validate_protected_range_description(&description) {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-protected-range",
                                format!(
                                    "spreadsheet protected range update {sheet_id}!{range} was ignored: {err}"
                                ),
                            );
                            continue;
                        }
                        let mut candidate = workbook.clone();
                        match candidate.update_protected_range(
                            &sheet_id,
                            &range,
                            &description,
                            *warning_only,
                        ) {
                            Some(Ok(())) => *workbook = candidate,
                            Some(Err(SpreadsheetError::NotFound(_))) => push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-protected-range",
                                format!(
                                    "spreadsheet protected range update {sheet_id}!{range} was ignored because the protected range was missing"
                                ),
                            ),
                            Some(Err(err)) => push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-protected-range",
                                format!(
                                    "spreadsheet protected range update {sheet_id}!{range} was ignored: {err}"
                                ),
                            ),
                            None => push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet protected range update {sheet_id}!{range} was ignored because the sheet was missing"
                                ),
                            ),
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-protected-range",
                        format!(
                            "spreadsheet protected range update {sheet_id}!{range} was ignored: {err}"
                        ),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::DeleteProtectedRange {
                sheet_id, range, ..
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "protected range delete");
                match normalize_cell_range(range) {
                    Ok(range) => {
                        if workbook.delete_protected_range(&sheet_id, &range).is_none() {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet protected range delete {sheet_id}!{range} was ignored because the sheet was missing"
                                ),
                            );
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-protected-range",
                        format!(
                            "spreadsheet protected range delete {sheet_id}!{range} was ignored: {err}"
                        ),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::RestoreProtectedRange {
                sheet_id,
                range,
                protected_range,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "protected range restore");
                match normalize_cell_range(range) {
                    Ok(range) => {
                        let mut candidate = workbook.clone();
                        match candidate.restore_protected_range(&sheet_id, protected_range.clone())
                        {
                            Ok(()) => *workbook = candidate,
                            Err(SpreadsheetError::NotFound(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet protected range restore {sheet_id}!{range} was ignored because the sheet was missing"
                                ),
                            ),
                            Err(SpreadsheetError::Conflict(_)) => push_spreadsheet_warning(
                                &mut warnings,
                                "spreadsheet-protected-range-conflict",
                                format!(
                                    "spreadsheet protected range restore {sheet_id}!{range} was ignored because the protected range already exists"
                                ),
                            ),
                            Err(err) => push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-protected-range",
                                format!(
                                    "spreadsheet protected range restore {sheet_id}!{range} was ignored: {err}"
                                ),
                            ),
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-protected-range",
                        format!(
                            "spreadsheet protected range restore {sheet_id}!{range} was ignored: {err}"
                        ),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::SetCell {
                sheet_id,
                address,
                value,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "cell edit");
                match normalize_cell_address(address) {
                    Ok(address) => {
                        if spreadsheet_replay_cell_address_deleted_by_other_actor(
                            &mut warnings,
                            &deleted_rows_by_other_actor,
                            &deleted_columns_by_other_actor,
                            &envelope.record.actor,
                            &sheet_id,
                            &address,
                            "cell edit",
                        ) {
                            continue;
                        }
                        if workbook
                            .set_cell_in_sheet(&sheet_id, &address, value.clone())
                            .is_none()
                        {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet cell edit {sheet_id}!{address} was ignored because the sheet was missing"
                                ),
                            );
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-cell-address",
                        format!("spreadsheet cell edit {sheet_id}!{address} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::SetCellFormat {
                sheet_id,
                address,
                property,
                value,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "cell format set");
                match normalize_cell_address(address) {
                    Ok(address) => {
                        if spreadsheet_replay_cell_address_deleted_by_other_actor(
                            &mut warnings,
                            &deleted_rows_by_other_actor,
                            &deleted_columns_by_other_actor,
                            &envelope.record.actor,
                            &sheet_id,
                            &address,
                            "cell format",
                        ) {
                            continue;
                        }
                    }
                    Err(err) => {
                        push_spreadsheet_warning(
                            &mut warnings,
                            "invalid-spreadsheet-cell-address",
                            format!(
                                "spreadsheet cell format {sheet_id}!{address} {property} was ignored: {err}"
                            ),
                        );
                        continue;
                    }
                }
                apply_spreadsheet_result(
                    workbook,
                    &mut warnings,
                    "missing-spreadsheet-sheet",
                    format!(
                        "spreadsheet cell format {sheet_id}!{address} was ignored because the sheet was missing"
                    ),
                    "invalid-spreadsheet-cell-format",
                    format!(
                        "spreadsheet cell format {sheet_id}!{address} {property} was ignored"
                    ),
                    |candidate| {
                        candidate.set_cell_format(&sheet_id, address, property, value.clone())
                    },
                );
            }
            Some(AppSpreadsheetOperation::SetRowHeight {
                sheet_id,
                row,
                height,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "row height set");
                match normalize_row_label(row) {
                    Ok(row) => apply_spreadsheet_result(
                        workbook,
                        &mut warnings,
                        "missing-spreadsheet-row",
                        format!(
                            "spreadsheet row height {sheet_id}!{row} was ignored because the row was missing"
                        ),
                        "invalid-spreadsheet-row-height",
                        format!("spreadsheet row height {sheet_id}!{row} was ignored"),
                        |candidate| candidate.set_row_height(&sheet_id, &row, *height),
                    ),
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-row",
                        format!("spreadsheet row height {sheet_id}!{row} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::SetColumnWidth {
                sheet_id,
                column,
                width,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "column width set");
                match normalize_column_label(column) {
                    Ok(column) => apply_spreadsheet_result(
                        workbook,
                        &mut warnings,
                        "missing-spreadsheet-column",
                        format!(
                            "spreadsheet column width {sheet_id}!{column} was ignored because the column was missing"
                        ),
                        "invalid-spreadsheet-column-width",
                        format!("spreadsheet column width {sheet_id}!{column} was ignored"),
                        |candidate| candidate.set_column_width(&sheet_id, &column, *width),
                    ),
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-column",
                        format!("spreadsheet column width {sheet_id}!{column} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::CopyRange {
                sheet_id,
                source_range,
                target_address,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "range copy");
                apply_spreadsheet_result(
                    workbook,
                    &mut warnings,
                    "missing-spreadsheet-sheet",
                    format!(
                        "spreadsheet copy {sheet_id}!{source_range} was ignored because the sheet was missing"
                    ),
                    "invalid-spreadsheet-copy-range",
                    format!(
                        "spreadsheet copy {sheet_id}!{source_range} to {target_address} was ignored"
                    ),
                    |candidate| candidate.copy_range(&sheet_id, source_range, target_address),
                );
            }
            Some(AppSpreadsheetOperation::AddNamedRange {
                sheet_id,
                name,
                range,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "named range add");
                match (normalize_named_range_name(name), normalize_cell_range(range)) {
                    (Ok(name), Ok(range)) => {
                        apply_spreadsheet_result(
                            workbook,
                            &mut warnings,
                            "missing-spreadsheet-sheet",
                            format!(
                                "spreadsheet named range {name} was ignored because sheet {sheet_id} was missing"
                            ),
                            "invalid-spreadsheet-named-range",
                            format!(
                                "spreadsheet named range {name} on {sheet_id}!{range} was ignored"
                            ),
                            |candidate| candidate.add_named_range(&sheet_id, &name, &range),
                        );
                    }
                    (Err(err), _) | (_, Err(err)) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-named-range",
                        format!(
                            "spreadsheet named range {name} on {sheet_id}!{range} was ignored: {err}"
                        ),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::UpdateNamedRange {
                sheet_id,
                name,
                range,
            }) => {
                let sheet_id = replay_sheet_id!(sheet_id, "named range update");
                match (normalize_named_range_name(name), normalize_cell_range(range)) {
                    (Ok(name), Ok(range)) => {
                        let mut candidate = workbook.clone();
                        match candidate.update_named_range(&sheet_id, &name, &range) {
                            Some(Ok(())) => *workbook = candidate,
                            Some(Err(SpreadsheetError::NotFound(_))) => push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-named-range",
                                format!(
                                    "spreadsheet named range {name} update to {sheet_id}!{range} was ignored because the named range was missing"
                                ),
                            ),
                            Some(Err(err)) => push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-named-range",
                                format!(
                                    "spreadsheet named range {name} update to {sheet_id}!{range} was ignored: {err}"
                                ),
                            ),
                            None => push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet named range {name} update was ignored because sheet {sheet_id} was missing"
                                ),
                            ),
                        }
                    }
                    (Err(err), _) | (_, Err(err)) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-named-range",
                        format!(
                            "spreadsheet named range {name} update to {sheet_id}!{range} was ignored: {err}"
                        ),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::DeleteNamedRange { name, .. }) => {
                match normalize_named_range_name(name) {
                    Ok(name) => {
                        if workbook.delete_named_range(&name).is_none() {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-named-range",
                                format!(
                                    "spreadsheet named range delete {name} was ignored because it was missing"
                                ),
                            );
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-named-range",
                        format!("spreadsheet named range delete {name} was ignored: {err}"),
                    ),
                }
            }
            Some(AppSpreadsheetOperation::RestoreNamedRange { name, range }) => {
                match normalize_named_range_name(name) {
                    Ok(name) => {
                        if workbook.named_range(&name).is_some() {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "duplicate-spreadsheet-named-range",
                                format!(
                                    "spreadsheet named range restore {name} was ignored because it already exists"
                                ),
                            );
                        } else if !workbook
                            .sheets
                            .iter()
                            .any(|sheet| sheet.id == range.sheet_id)
                        {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "missing-spreadsheet-sheet",
                                format!(
                                    "spreadsheet named range restore {name} was ignored because sheet {} was missing",
                                    range.sheet_id
                                ),
                            );
                        } else if let Err(err) = workbook.restore_named_range(range.clone()) {
                            push_spreadsheet_warning(
                                &mut warnings,
                                "invalid-spreadsheet-named-range",
                                format!(
                                    "spreadsheet named range restore {name} was ignored: {err}"
                                ),
                            );
                        }
                    }
                    Err(err) => push_spreadsheet_warning(
                        &mut warnings,
                        "invalid-spreadsheet-named-range",
                        format!("spreadsheet named range restore {name} was ignored: {err}"),
                    ),
                }
            }
            None => {}
        }
        let evaluated_workbook = workbook.evaluated();
        if let Err(err) = evaluated_workbook.validate_source() {
            *workbook = previous_workbook;
            push_spreadsheet_warning(
                &mut warnings,
                "invalid-spreadsheet-source",
                format!(
                    "spreadsheet operation {}#{} was ignored because it would create invalid source: {err}",
                    envelope.record.actor, envelope.record.seq
                ),
            );
        } else {
            *workbook = evaluated_workbook;
        }
    }
    Ok(warnings)
}

pub(crate) fn merge_spreadsheet_envelope_streams(
    mut base: AppSpreadsheetWorkbook,
    streams: &[&[AppOperationEnvelope]],
) -> Result<(AppSpreadsheetWorkbook, Vec<AppWarning>), AppApiError> {
    let mut ordered: BTreeMap<String, &AppOperationEnvelope> = BTreeMap::new();
    let mut warnings = Vec::new();
    for stream in streams {
        for envelope in *stream {
            if envelope.spreadsheet.is_none() {
                continue;
            }
            match ordered.get(envelope_id_key(envelope).as_str()) {
                Some(existing) if envelope_payload_matches(existing, envelope) => {}
                Some(existing) => {
                    let current_key = spreadsheet_merge_payload_sort_key(envelope);
                    let existing_key = spreadsheet_merge_payload_sort_key(existing);
                    if current_key < existing_key {
                        ordered.insert(envelope_id_key(envelope), envelope);
                    }
                    push_spreadsheet_warning(
                        &mut warnings,
                        "duplicate-spreadsheet-operation",
                        format!(
                            "spreadsheet operation {}#{} had conflicting payloads; deterministic winner was replayed",
                            envelope.record.actor, envelope.record.seq
                        ),
                    );
                }
                None => {
                    ordered.insert(envelope_id_key(envelope), envelope);
                }
            }
        }
    }
    let envelopes = ordered.into_values().cloned().collect::<Vec<_>>();
    warnings.extend(apply_spreadsheet_envelopes(&mut base, &envelopes)?);
    Ok((base, warnings))
}

fn spreadsheet_merge_payload_sort_key(envelope: &AppOperationEnvelope) -> String {
    serde_json::to_string(&envelope.spreadsheet).unwrap_or_else(|_| envelope.record.kind.clone())
}

fn spreadsheet_replay_deleted_rows_by_actor(
    envelopes: &[AppOperationEnvelope],
) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut deleted = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for envelope in envelopes {
        let Some(AppSpreadsheetOperation::DeleteRow { sheet_id, row, .. }) =
            envelope.spreadsheet.as_ref()
        else {
            continue;
        };
        let (Ok(sheet_id), Ok(row)) = (normalize_sheet_id(sheet_id), normalize_row_label(row))
        else {
            continue;
        };
        deleted
            .entry((sheet_id, row))
            .or_default()
            .insert(envelope.record.actor.clone());
    }
    deleted
}

fn spreadsheet_replay_deleted_columns_by_actor(
    envelopes: &[AppOperationEnvelope],
) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut deleted = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for envelope in envelopes {
        let Some(AppSpreadsheetOperation::DeleteColumn {
            sheet_id, column, ..
        }) = envelope.spreadsheet.as_ref()
        else {
            continue;
        };
        let (Ok(sheet_id), Ok(column)) =
            (normalize_sheet_id(sheet_id), normalize_column_label(column))
        else {
            continue;
        };
        deleted
            .entry((sheet_id, column))
            .or_default()
            .insert(envelope.record.actor.clone());
    }
    deleted
}

fn spreadsheet_replay_cell_address_deleted_by_other_actor(
    warnings: &mut Vec<AppWarning>,
    deleted_rows_by_actor: &BTreeMap<(String, String), BTreeSet<String>>,
    deleted_columns_by_actor: &BTreeMap<(String, String), BTreeSet<String>>,
    actor: &str,
    sheet_id: &str,
    address: &str,
    action: &str,
) -> bool {
    let Ok((column, row)) = cell_axis_labels(address) else {
        return false;
    };
    if deleted_rows_by_actor
        .get(&(sheet_id.to_string(), row.clone()))
        .is_some_and(|actors| actors.iter().any(|deleted_by| deleted_by != actor))
    {
        push_spreadsheet_warning(
            warnings,
            "missing-spreadsheet-row",
            format!(
                "spreadsheet {action} {sheet_id}!{address} was ignored because row {row} was deleted by another actor"
            ),
        );
        return true;
    }
    if deleted_columns_by_actor
        .get(&(sheet_id.to_string(), column.clone()))
        .is_some_and(|actors| actors.iter().any(|deleted_by| deleted_by != actor))
    {
        push_spreadsheet_warning(
            warnings,
            "missing-spreadsheet-column",
            format!(
                "spreadsheet {action} {sheet_id}!{address} was ignored because column {column} was deleted by another actor"
            ),
        );
        return true;
    }
    false
}

fn apply_spreadsheet_result<F>(
    workbook: &mut AppSpreadsheetWorkbook,
    warnings: &mut Vec<AppWarning>,
    missing_code: &str,
    missing_message: String,
    invalid_code: &str,
    invalid_message: String,
    mut apply: F,
) where
    F: FnMut(&mut AppSpreadsheetWorkbook) -> Option<Result<(), SpreadsheetError>>,
{
    let mut candidate = workbook.clone();
    match apply(&mut candidate) {
        Some(Ok(())) => *workbook = candidate,
        Some(Err(err)) => warnings.push(AppWarning {
            code: invalid_code.to_string(),
            message: format!("{invalid_message}: {err}"),
        }),
        None => warnings.push(AppWarning {
            code: missing_code.to_string(),
            message: missing_message,
        }),
    }
}

pub(crate) fn envelope_id_key(envelope: &AppOperationEnvelope) -> String {
    format!("{}:{}", envelope.record.actor, envelope.record.seq)
}

pub(crate) fn envelope_payload_matches(
    left: &AppOperationEnvelope,
    right: &AppOperationEnvelope,
) -> bool {
    left.operation == right.operation
        && left.spreadsheet == right.spreadsheet
        && left.blob == right.blob
}
