//! Document identity, paragraphs, pagination and page setup.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "set_document_doi",
        AppDocument,
        [arg!("doi", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_document_title",
        AppDocument,
        [arg!("title", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_document_locale",
        AppDocument,
        [arg!("locale", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_paragraph",
        AppDocument,
        [arg!("text", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_paragraph_after",
        AppDocument,
        [arg!("afterBlockId", NullableString), arg!("text", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "split_paragraph_at_inline",
        AppDocument,
        [arg!("inlineId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "split_paragraph_at_text_offset",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("inlineId", String),
            arg!("offset", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "join_paragraph_with_previous",
        AppDocument,
        [arg!("blockId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_block",
        AppDocument,
        [arg!("blockId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_text_style",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("style", String),
            arg!("level", Number),
            arg!("listKind", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_style",
        AppDocument,
        [
            arg!("selection", EditorSelection),
            arg!("style", String),
            arg!("level", Number),
            arg!("listKind", String)
        ],
        true,
        false,
        Some("write")
    ),
    // ---- Pagination (PLAN77 B8, ADR 0014) --------------------------------
    // Read-only and deterministic: the layout is a function of the document
    // and its page setup, so it is not undoable, it leaves the open document
    // in place, and it needs only read permission. It does need a document,
    // so it is not allowed without one.
    command!(
        "layout_document",
        AppDocumentLayout,
        [],
        false,
        false,
        Some("read")
    ),
    // ---- Page setup and page furniture (PLAN77 B7) ----------------------
    // Page geometry is written whole rather than one dimension at a time, so
    // a concurrent merge cannot combine one actor's width with another's
    // height into a page neither chose. See
    // `docs/adr/0009-pagination-and-page-geometry.md`.
    command!(
        "set_page_setup",
        AppDocument,
        [
            arg!("widthTwips", Number),
            arg!("heightTwips", Number),
            arg!("marginTopTwips", Number),
            arg!("marginBottomTwips", Number),
            arg!("marginStartTwips", Number),
            arg!("marginEndTwips", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_page_orientation",
        AppDocument,
        [arg!("orientation", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_page_furniture",
        AppDocument,
        [
            arg!("slot", String),
            arg!("text", String),
            arg!("field", String),
            arg!("alignment", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_page_furniture",
        AppDocument,
        [arg!("slot", String)],
        true,
        false,
        Some("write")
    ),
];
