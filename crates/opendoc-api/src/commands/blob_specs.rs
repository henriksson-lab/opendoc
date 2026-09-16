//! Binary blob lifecycle, image blocks and blob signing.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "add_binary_blob",
        AppDocument,
        [
            arg!("name", String),
            arg!("mediaType", String),
            arg!("bytes", NumberArray)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_binary_blob_metadata",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("name", String),
            arg!("mediaType", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_image_block",
        AppDocument,
        [arg!("blobHash", String), arg!("altText", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_image_block_after",
        AppDocument,
        [
            arg!("afterBlockId", String),
            arg!("blobHash", String),
            arg!("altText", String)
        ],
        true,
        false,
        Some("write")
    ),
    // This is deliberately a byte-for-byte export, rather than an implicit
    // conversion. A document may hold PNG, JPEG, SVG, or a future image
    // format; saving it must not quietly discard metadata or pixels.
    command!(
        "export_image_blob",
        AppExport,
        [arg!("blobHash", String)],
        false,
        false,
        Some("read")
    ),
    command!(
        "sign_blob_with_openssh_private_key",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("privateKeyPem", String),
            arg!("signerDisplay", String)
        ],
        false,
        false,
        Some("write")
    ),
    command!(
        "sign_fastq_blob_with_openssh_private_key",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("profile", String),
            arg!("privateKeyPem", String),
            arg!("signerDisplay", String)
        ],
        false,
        false,
        Some("write")
    ),
    command!(
        "sign_image_pixels_blob_with_openssh_private_key",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("width", Number),
            arg!("height", Number),
            arg!("pixels", NumberArray),
            arg!("privateKeyPem", String),
            arg!("signerDisplay", String)
        ],
        false,
        false,
        Some("write")
    ),
    command!(
        "delete_binary_blob",
        AppDocument,
        [arg!("blobHash", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_binary_blob",
        AppDocument,
        [arg!("blobHash", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "simulate_shallow_clone",
        AppDocument,
        [],
        false,
        false,
        Some("write")
    ),
    command!(
        "record_blob_archive_tombstone",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("archiveLocator", String),
            arg!("restoreHint", String),
            arg!("signer", String),
            arg!("signature", NumberArray)
        ],
        true,
        false,
        Some("write")
    ),
];
