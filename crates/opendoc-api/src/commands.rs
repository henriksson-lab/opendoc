pub use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

macro_rules! arg {
    ($name:literal, $ty:ident) => {
        CommandArg {
            name: $name,
            ty: CommandArgType::$ty,
            optional: false,
        }
    };
    ($name:literal, $ty:ident, optional) => {
        CommandArg {
            name: $name,
            ty: CommandArgType::$ty,
            optional: true,
        }
    };
}

macro_rules! command {
    ($name:literal, $returns:ident, [$($arg:expr),* $(,)?], $undoable:literal, $closed:literal, $action:expr) => {
        CommandSpec {
            name: $name,
            returns: CommandReturn::$returns,
            args: &[$($arg),*],
            undoable: $undoable,
            allowed_without_open_document: $closed,
            required_action: $action,
        }
    };
}

mod annotation_specs;
mod blob_specs;
mod block_specs;
mod citation_specs;
mod document_specs;
mod editor_specs;
mod import_export_specs;
mod inline_specs;
mod mark_and_image_specs;
mod repository_specs;
mod session_specs;
mod signing_specs;
mod spreadsheet_specs;
mod table_specs;
mod version_specs;

/// The command registry, in contract order.
///
/// Each module below owns one contiguous section of it, and `GROUPS` is the
/// order the generated contract is written in — moving an entry between
/// modules moves it in `commands.v0.json` too. The flattening below is a
/// compile-time concatenation, so `COMMANDS` is still one static slice.
const GROUPS: &[&[CommandSpec]] = &[
    session_specs::SPECS,
    import_export_specs::SPECS,
    blob_specs::SPECS,
    document_specs::SPECS,
    block_specs::SPECS,
    inline_specs::SPECS,
    table_specs::SPECS,
    citation_specs::SPECS,
    annotation_specs::ADDITIONS,
    inline_specs::EDITS,
    annotation_specs::RESOLUTIONS,
    mark_and_image_specs::SPECS,
    spreadsheet_specs::SPECS,
    citation_specs::BIBLIOGRAPHY,
    repository_specs::SPECS,
    signing_specs::SPECS,
    version_specs::SPECS,
    editor_specs::SPECS,
];

const TOTAL: usize = {
    let mut total = 0;
    let mut group = 0;
    while group < GROUPS.len() {
        total += GROUPS[group].len();
        group += 1;
    }
    total
};

const fn flattened() -> [CommandSpec; TOTAL] {
    let mut out = [GROUPS[0][0]; TOTAL];
    let mut at = 0;
    let mut group = 0;
    while group < GROUPS.len() {
        let specs = GROUPS[group];
        let mut index = 0;
        while index < specs.len() {
            out[at] = specs[index];
            at += 1;
            index += 1;
        }
        group += 1;
    }
    out
}

static FLATTENED: [CommandSpec; TOTAL] = flattened();

pub static COMMANDS: &[CommandSpec] = &FLATTENED;

pub fn command_spec(command: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|spec| spec.name == command)
}

pub fn command_names() -> Vec<String> {
    COMMANDS.iter().map(|spec| spec.name.to_string()).collect()
}

pub fn is_undoable_command(command: &str) -> bool {
    command_spec(command)
        .map(|spec| spec.undoable)
        .unwrap_or(false)
}

pub fn is_closed_state_command(command: &str) -> bool {
    command_spec(command)
        .map(|spec| spec.allowed_without_open_document)
        .unwrap_or(false)
}

pub fn runtime_command_required_action(command: &str) -> Option<&'static str> {
    command_spec(command).and_then(|spec| spec.required_action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names_are_unique() {
        let mut names = COMMANDS.iter().map(|spec| spec.name).collect::<Vec<_>>();
        let original_len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), original_len);
    }
}
