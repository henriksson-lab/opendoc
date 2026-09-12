//! Signing the current source state and verifying signatures.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "sign_with_openssh_private_key",
        AppDocument,
        [arg!("privateKeyPem", String), arg!("signerDisplay", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "verify_current_signature",
        String,
        [arg!("privateKeyPem", String)],
        false,
        false,
        Some("read")
    ),
    command!(
        "verify_current_signatures",
        String,
        [],
        false,
        false,
        Some("read")
    ),
];
