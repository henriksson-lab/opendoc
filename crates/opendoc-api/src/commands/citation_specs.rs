//! Citation insertion and style commands.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "insert_citation",
        AppDocument,
        [
            arg!("referenceId", String),
            arg!("afterInlineId", NullableString),
            arg!("locator", NullableString),
            arg!("label", NullableString),
            arg!("prefix", NullableString),
            arg!("suffix", NullableString),
            arg!("suppressAuthor", Boolean)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_citation_group",
        AppDocument,
        [
            arg!("items", CitationItems),
            arg!("afterInlineId", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_footnote_citation_group",
        AppDocument,
        [arg!("footnoteId", String), arg!("items", CitationItems)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_footnote_citation_after",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("afterInlineId", NullableString),
            arg!("items", CitationItems)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_citation_group_items",
        AppDocument,
        [arg!("citationId", String), arg!("items", CitationItems)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_citation_style",
        AppDocument,
        [arg!("style", String), arg!("locale", String)],
        true,
        false,
        Some("write")
    ),
];

pub(crate) const BIBLIOGRAPHY: &[CommandSpec] = &[
    command!(
        "import_bibtex",
        AppDocument,
        [arg!("source", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_bibliography_reference",
        AppDocument,
        [
            arg!("referenceId", String),
            arg!("title", String),
            arg!("issued", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_bibliography_reference_metadata",
        AppDocument,
        [
            arg!("referenceId", String),
            arg!("title", String),
            arg!("authors", StringArray),
            arg!("issued", NullableString),
            arg!("doi", NullableString),
            arg!("url", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_bibliography_reference",
        AppDocument,
        [
            arg!("title", String),
            arg!("authors", StringArray),
            arg!("issued", NullableString),
            arg!("doi", NullableString),
            arg!("url", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_bibliography_reference",
        AppDocument,
        [arg!("referenceId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_bibliography_reference",
        AppDocument,
        [arg!("referenceId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_citation_group",
        AppDocument,
        [arg!("citationId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_citation_group",
        AppDocument,
        [arg!("citationId", String)],
        true,
        false,
        Some("write")
    ),
];
