use crate::{
    editor::{DocumentIndex, Resolved},
    AppApiError, AppEditorSelection, EditorInlineRange, EditorResult, EditorSelection, OpenDocApp,
};

pub(crate) struct EditorSelectionService<'a> {
    app: &'a OpenDocApp,
}

impl<'a> EditorSelectionService<'a> {
    pub(crate) fn new(app: &'a OpenDocApp) -> Self {
        Self { app }
    }

    pub(crate) fn describe(
        &self,
        selection: EditorSelection,
    ) -> Result<AppEditorSelection, AppApiError> {
        let index = DocumentIndex::build(&self.app.document.blocks);
        let anchor = index.resolve(&selection.anchor)?;
        let focus = index.resolve(&selection.focus)?;
        let focus_entry = &index.blocks[focus.block];
        let selected_block_ids =
            if index.blocks[anchor.block].container == index.blocks[focus.block].container {
                let (from, to) = if anchor.block <= focus.block {
                    (anchor.block, focus.block)
                } else {
                    (focus.block, anchor.block)
                };
                index.blocks[from..=to]
                    .iter()
                    .map(|entry| entry.id.to_string())
                    .collect()
            } else {
                vec![focus_entry.id.to_string()]
            };
        Ok(AppEditorSelection {
            selected_block_ids,
            focus_block_id: Some(focus_entry.id.to_string()),
            inline_range: normalized_inline_range(&index, &selection),
        })
    }

    pub(crate) fn select_all(&self) -> Result<EditorResult, AppApiError> {
        let index = DocumentIndex::build(&self.app.document.blocks);
        let mut document_blocks = index
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| block.container == "document");
        let Some((first, _)) = document_blocks.next() else {
            return Err(AppApiError::Format("document has no blocks".to_string()));
        };
        let (last, last_block) = document_blocks
            .next_back()
            .unwrap_or((first, &index.blocks[first]));
        Ok(EditorResult {
            document: self.app.document(),
            selection: EditorSelection {
                anchor: index.position(Resolved {
                    block: first,
                    abs: 0,
                }),
                focus: index.position(Resolved {
                    block: last,
                    abs: last_block.len,
                }),
            },
            handled: true,
        })
    }
}

fn normalized_inline_range(
    index: &DocumentIndex,
    selection: &EditorSelection,
) -> Option<EditorInlineRange> {
    let mut ids = Vec::new();
    for entry in &index.blocks {
        for span in &entry.spans {
            ids.push(span.id.as_str());
        }
    }
    let anchor = selection
        .anchor
        .inline_id
        .as_ref()
        .and_then(|id| ids.iter().position(|candidate| candidate == id));
    let focus = selection
        .focus
        .inline_id
        .as_ref()
        .and_then(|id| ids.iter().position(|candidate| candidate == id));
    let from = anchor.or(focus)?;
    let to = focus.or(anchor)?;
    let (from, to) = if from <= to { (from, to) } else { (to, from) };
    Some(EditorInlineRange {
        start: ids[from].to_string(),
        end: ids[to].to_string(),
    })
}
