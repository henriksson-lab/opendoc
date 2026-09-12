//! Accepting and rejecting tracked-change suggestions.

use crate::inline_ops::{
    delete_inline_range, insert_inlines_after_anchor, AnchorInsertResult, RangeEditResult,
};
use crate::marks::{add_mark_range, MarkRangeResult};
use crate::validate::{inline_sequence_payload_valid_for_merge, marks_valid_for_merge};
use opendoc_core::{
    Anchor, Document, Inline, Mark, ModelWarning, StableId, Suggestion, SuggestionKind,
    SuggestionState,
};

pub(crate) fn push_provenance_once(suggestion: &mut Suggestion, value: &str) {
    if !suggestion.provenance.iter().any(|item| item == value) {
        suggestion.provenance.push(value.to_string());
    }
}

pub(crate) fn reviewer_provenance_value(
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    action: &str,
    reviewer: &str,
) -> String {
    let reviewer = reviewer.trim();
    if reviewer.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-suggestion-reviewer".to_string(),
            message: format!(
                "suggestion {suggestion_id} {action} reviewer was empty; recorded unknown reviewer"
            ),
        });
        "unknown".to_string()
    } else {
        reviewer.to_string()
    }
}

pub(crate) fn canonical_diagnostic_value(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn accept_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    accepted_by: &str,
) {
    let accepted_by = reviewer_provenance_value(warnings, suggestion_id, "accept", accepted_by);
    let Some(index) = document
        .suggestions
        .iter()
        .position(|item| &item.id == suggestion_id)
    else {
        warnings.push(ModelWarning {
            code: "missing-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} was missing"),
        });
        return;
    };

    if document.suggestions[index].state != SuggestionState::Proposed {
        warnings.push(ModelWarning {
            code: "resolved-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} was already resolved"),
        });
        return;
    }

    let kind = document.suggestions[index].kind.clone();
    let accepted = match kind {
        SuggestionKind::Insert { anchor, content } => {
            accept_insert_suggestion(document, warnings, suggestion_id, &anchor, content)
        }
        SuggestionKind::Delete { range } => {
            accept_delete_suggestion(document, warnings, suggestion_id, &range);
            true
        }
        SuggestionKind::Format { range, marks } => {
            accept_format_suggestion(document, warnings, suggestion_id, &range, marks)
        }
    };

    let suggestion = &mut document.suggestions[index];
    if accepted {
        suggestion.state = SuggestionState::Accepted;
        push_provenance_once(suggestion, &format!("accepted-by:{accepted_by}"));
    } else {
        suggestion.state = SuggestionState::Rejected;
        suggestion.kind = SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text("[invalid suggestion payload]")],
        };
        push_provenance_once(suggestion, "auto-rejected:invalid-accept-payload");
    }
}

pub(crate) fn accept_insert_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    anchor: &Anchor,
    content: Vec<Inline>,
) -> bool {
    if content.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} ignored empty insert content"),
        });
        return false;
    }
    if !inline_sequence_payload_valid_for_merge(
        &content,
        warnings,
        &format!("suggestion {suggestion_id} acceptance"),
    ) {
        return false;
    }
    match insert_inlines_after_anchor(document, anchor, content) {
        AnchorInsertResult::Applied => {}
        AnchorInsertResult::Degraded => warnings.push(ModelWarning {
            code: "suggestion-anchor-degraded".to_string(),
            message: format!("suggestion {suggestion_id} inserted at degraded anchor"),
        }),
        AnchorInsertResult::Missing => warnings.push(ModelWarning {
            code: "suggestion-anchor-degraded".to_string(),
            message: format!("suggestion {suggestion_id} could not find an insertion anchor"),
        }),
    }
    true
}

pub(crate) fn accept_delete_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    range: &opendoc_core::TextRange,
) {
    match delete_inline_range(document, range) {
        RangeEditResult::Applied => {}
        RangeEditResult::Degraded => warnings.push(ModelWarning {
            code: "suggestion-range-degraded".to_string(),
            message: format!("suggestion {suggestion_id} deleted surviving range endpoint"),
        }),
        RangeEditResult::Missing => warnings.push(ModelWarning {
            code: "suggestion-range-missing".to_string(),
            message: format!("suggestion {suggestion_id} had no surviving range to delete"),
        }),
    }
}

pub(crate) fn accept_format_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    range: &opendoc_core::TextRange,
    marks: Vec<Mark>,
) -> bool {
    if marks.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} ignored empty format marks"),
        });
        return false;
    }
    for mark in marks {
        if !marks_valid_for_merge(
            std::slice::from_ref(&mark),
            warnings,
            &format!("suggestion {suggestion_id} acceptance"),
            &range.start,
        ) {
            return false;
        }
        match add_mark_range(document, range, mark) {
            MarkRangeResult::Applied => {}
            MarkRangeResult::Degraded => warnings.push(ModelWarning {
                code: "suggestion-range-degraded".to_string(),
                message: format!("suggestion {suggestion_id} formatted surviving range endpoint"),
            }),
            MarkRangeResult::Missing => warnings.push(ModelWarning {
                code: "suggestion-range-missing".to_string(),
                message: format!("suggestion {suggestion_id} had no surviving range to format"),
            }),
        }
    }
    true
}
