use serde::{Deserialize, Serialize};

/// A caret position inside the document. `inline_id` is `None` for blocks
/// without inline content, in which case `offset` is `0` or `1`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditorPosition {
    pub block_id: String,
    #[serde(default)]
    pub inline_id: Option<String>,
    #[serde(default)]
    pub offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditorSelection {
    pub anchor: EditorPosition,
    pub focus: EditorPosition,
}

impl EditorSelection {
    pub fn collapsed(position: EditorPosition) -> Self {
        Self {
            anchor: position.clone(),
            focus: position,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditorInlineRange {
    pub start: String,
    pub end: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppEditorSelection {
    pub selected_block_ids: Vec<String>,
    #[serde(default)]
    pub focus_block_id: Option<String>,
    #[serde(default)]
    pub inline_range: Option<EditorInlineRange>,
}

/// One editing gesture from the frontend, modelled on the browser
/// `beforeinput` event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditorInput {
    pub selection: EditorSelection,
    pub input_type: String,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub html: Option<String>,
}

/// Apply, toggle, or remove a mark over the selected characters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditorMarkInput {
    pub selection: EditorSelection,
    pub mark_kind: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
}

/// How a find query is interpreted (parity ED-24).
///
/// The three toggles travel together because they only mean anything
/// together: `query` is a literal unless `regex` is set, `whole_word`
/// constrains whichever of the two it is, and `match_case` selects the
/// folding. Find and both replace commands take exactly this, so a replace
/// can never search differently from the find that listed its targets.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FindOptions {
    pub query: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

/// One match, as the selection that covers it.
///
/// `start` and `end` are ordinary [`EditorPosition`]s, so the frontend
/// highlights a match by handing them to the same `setSelection` it uses for
/// the caret. A match may begin in one inline run and end in another
/// (parity ED-25), which is why this is a pair of positions and not an
/// offset plus a length inside one run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppFindMatch {
    pub start: EditorPosition,
    pub end: EditorPosition,
    /// The matched text itself, so a caller can show it without re-reading
    /// the document (and so a regex match is inspectable).
    pub text: String,
    /// Which part of the document the match is in. See [`AppFindRegion`].
    pub region: AppFindRegion,
}

/// Where a match is, because `start` and `end` cannot say.
///
/// A search covers the header, the footer and the footnote bodies as well as
/// the body — and those three are *not* in `document.blocks`, so their block
/// ids are not in the editor's DOM either. A frontend that handed a header
/// match's positions to `setSelection` did nothing at all, silently: the
/// counter said "3 of 7" and pressing Next moved the number and not the
/// caret. Naming the region is what lets a caller take the match somewhere
/// the user can actually reach it — the header dialog, the footer dialog, the
/// footnote editor — instead of pretending it selected something.
///
/// The footnote's own id is `start.block_id`, which is what
/// `update_footnote_body` takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppFindRegion {
    /// `document.blocks` — the only region the editor surface can select in.
    Body,
    Header,
    Footer,
    /// An explicit first-page header override, outside `document.blocks`.
    FirstPageHeader,
    /// An explicit first-page footer override, outside `document.blocks`.
    FirstPageFooter,
    /// An explicit even-page header override, outside `document.blocks`.
    EvenPageHeader,
    /// An explicit even-page footer override, outside `document.blocks`.
    EvenPageFooter,
    Footnote,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppFindMatches {
    pub matches: Vec<AppFindMatch>,
}
