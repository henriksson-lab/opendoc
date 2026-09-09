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
