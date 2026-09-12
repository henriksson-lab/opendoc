use crate::*;
use opendoc_core::{Block, Document, Inline, Mark};

pub(crate) fn assert_export_error_contains(document: &Document, expected: &str) {
    let err = export_google_docs_json(document).unwrap_err();
    assert!(
        err.to_string().contains(expected),
        "expected export error containing {expected:?}, got {err:?}"
    );
}

pub(crate) fn text_marks(block: &Block) -> &[Mark] {
    match &block.content[0] {
        opendoc_core::Inline::Text { marks, .. } => marks,
        other => panic!("expected text inline, got {other:?}"),
    }
}

pub(crate) fn block_plain_text(block: &Block) -> String {
    block
        .content
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
