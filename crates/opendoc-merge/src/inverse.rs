//! Inverting a document operation: the operation that undoes it.
//!
//! See `docs/adr/0017-collaborative-undo-as-inverse-operations.md`. An undo is
//! not a rewind of the operation log; it is an ordinary edit that happens to
//! say the opposite of an earlier one. That is what makes it survive
//! collaboration: the inverse only ever addresses the author's own
//! contribution, so it cannot delete a collaborator's work, and it is new
//! history rather than a rewrite of old history, so a service that refuses to
//! rewrite history accepts it.
//!
//! # Two kinds of operation, two moments to invert
//!
//! Almost every operation in the vocabulary addresses its target by identity —
//! a [`StableId`], a [`opendoc_core::BlockPropertyKey`], a thread and comment
//! id. Identities do not shift when somebody else edits, so such an inverse is
//! correct whenever it is applied and can be computed **when the operation is
//! written**, which is also the only moment the state it has to capture (the
//! block a delete removed, the value a set overwrote) still exists.
//!
//! [`OperationKind::InsertText`] and [`OperationKind::DeleteText`] are the
//! exceptions: their payload is an offset into a text run, and an offset means
//! something different once a concurrent edit has landed in the same run. Their
//! inverse is therefore [`Inversion::Deferred`] — computed at undo time by
//! [`invert_text_operations`], which re-derives the run's character identities
//! from (base, operation set) exactly as the merge does and asks where *this*
//! operation's characters are now.

use crate::causal::{causal_order, OperationId};
use crate::inline_ops::inline_id;
use crate::operation::{BlockTextStyle, Operation, OperationKind};
use crate::text_sequence::{collect_text_run_edits, resolve_run_atoms, runs_written_wholesale};
use opendoc_core::{
    Block, BlockKind, Bookmark, Comment, CommentThread, Document, Inline, InsertPosition, Mark,
    StableId, TableCell, TableColumn, TableRow,
};
use std::collections::BTreeMap;

/// What inverting one operation produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Inversion {
    /// Apply these, in this order, and the operation is undone.
    Operations(Vec<OperationKind>),
    /// The operation changed nothing, so undoing it is a no-op. A mark that
    /// was already there, a property cleared that was already inheriting.
    Nothing,
    /// The operation addresses a text run by offset. Its inverse depends on
    /// what the run looks like when the undo is written, so it is computed
    /// then, by [`invert_text_operations`].
    Deferred,
    /// The operation vocabulary cannot express the inverse. The string names
    /// why, and it is a name, not a message: callers match on behaviour, and
    /// the reason is for the human reading the refusal.
    Irreversible(&'static str),
}

impl Inversion {
    /// Whether an undo built out of this can be submitted like any other edit.
    pub fn is_expressible(&self) -> bool {
        !matches!(self, Inversion::Irreversible(_))
    }
}

/// The operation that undoes `kind`, computed against `before`: the document
/// as it was **immediately before `kind` applied**.
///
/// `before` is not a convenience. A delete's inverse has to carry the content
/// that was removed, and a set's inverse has to carry the value that was
/// overwritten; neither exists afterwards. Every arm that needs state says so
/// by reading it out of `before`, and the match is exhaustive, so an operation
/// added to the vocabulary later does not compile until somebody decides
/// whether it can be undone.
pub fn invert_operation(before: &Document, kind: &OperationKind) -> Inversion {
    match kind {
        // ---- document-level scalars: the inverse is the previous value ----
        OperationKind::SetDocumentTitle { title } => {
            if *title == before.title {
                Inversion::Nothing
            } else {
                one(OperationKind::SetDocumentTitle {
                    title: before.title.clone(),
                })
            }
        }
        OperationKind::SetDocumentDoi { doi } => {
            if *doi == before.doi {
                Inversion::Nothing
            } else if before.doi.is_none() && doi.is_some() {
                // `SetDocumentDoi { doi: None }` clears it, so this one *can*
                // be said. The empty-string payload is the case the applier
                // ignores, and an ignored operation inverts to nothing.
                one(OperationKind::SetDocumentDoi { doi: None })
            } else {
                one(OperationKind::SetDocumentDoi {
                    doi: before.doi.clone(),
                })
            }
        }
        OperationKind::SetDocumentLocale { locale } => {
            if *locale == before.locale {
                Inversion::Nothing
            } else {
                one(OperationKind::SetDocumentLocale {
                    locale: before.locale.clone(),
                })
            }
        }
        OperationKind::UpsertBookmark { bookmark } => {
            match before.bookmarks.iter().find(|item| item.id == bookmark.id) {
                Some(previous) if *previous == *bookmark => Inversion::Nothing,
                Some(previous) => {
                    let mut restored = previous.clone();
                    restored.revision = bookmark.revision.max(previous.revision) + 1;
                    one(OperationKind::UpsertBookmark { bookmark: restored })
                }
                None => one(OperationKind::UpsertBookmark {
                    bookmark: Bookmark {
                        id: bookmark.id.clone(),
                        name: bookmark.name.clone(),
                        block_id: bookmark.block_id.clone(),
                        revision: bookmark.revision + 1,
                        deleted: true,
                    },
                }),
            }
        }
        OperationKind::SetPageSetup { page_setup } => {
            if *page_setup == before.page_setup {
                Inversion::Nothing
            } else {
                one(OperationKind::SetPageSetup {
                    page_setup: before.page_setup,
                })
            }
        }
        OperationKind::SetPageFurniture { slot, blocks } => {
            let previous = before.furniture(*slot).to_vec();
            if *blocks == previous {
                Inversion::Nothing
            } else if slot.is_override() && !before.has_furniture_override(*slot) {
                one(OperationKind::ClearPageFurnitureOverride { slot: *slot })
            } else {
                one(OperationKind::SetPageFurniture {
                    slot: *slot,
                    blocks: previous,
                })
            }
        }
        OperationKind::ClearPageFurnitureOverride { slot } => {
            if !slot.is_override() || !before.has_furniture_override(*slot) {
                Inversion::Nothing
            } else {
                one(OperationKind::SetPageFurniture {
                    slot: *slot,
                    blocks: before.furniture(*slot).to_vec(),
                })
            }
        }
        OperationKind::SetSectionPageSetup {
            section_id,
            page_setup,
        } => match before.sections.get(section_id) {
            Some(section) if section.page_setup != *page_setup => {
                one(OperationKind::SetSectionPageSetup {
                    section_id: section_id.clone(),
                    page_setup: section.page_setup,
                })
            }
            _ => Inversion::Nothing,
        },
        OperationKind::SetSectionFurniture {
            section_id,
            slot,
            blocks,
        } => match before.sections.get(section_id) {
            Some(section) if section.furniture(*slot) != blocks => {
                if slot.is_override() && !section.has_furniture_override(*slot) {
                    one(OperationKind::ClearSectionFurnitureOverride {
                        section_id: section_id.clone(),
                        slot: *slot,
                    })
                } else {
                    one(OperationKind::SetSectionFurniture {
                        section_id: section_id.clone(),
                        slot: *slot,
                        blocks: section.furniture(*slot).to_vec(),
                    })
                }
            }
            _ => Inversion::Nothing,
        },
        OperationKind::ClearSectionFurnitureOverride { section_id, slot } => {
            match before.sections.get(section_id) {
                Some(section) if slot.is_override() && section.has_furniture_override(*slot) => {
                    one(OperationKind::SetSectionFurniture {
                        section_id: section_id.clone(),
                        slot: *slot,
                        blocks: section.furniture(*slot).to_vec(),
                    })
                }
                _ => Inversion::Nothing,
            }
        }
        OperationKind::UpdateCitationStyle { style, locale } => {
            let database = &before.citation_database;
            if *style == database.style && *locale == database.locale {
                Inversion::Nothing
            } else {
                one(OperationKind::UpdateCitationStyle {
                    style: database.style.clone(),
                    locale: database.locale.clone(),
                })
            }
        }

        // ---- blocks ----
        OperationKind::InsertBlock { block, .. } => {
            if block_anywhere(&before.blocks, &block.id).is_some() {
                // The applier refuses a duplicate, so the operation changed
                // nothing and deleting the block would destroy the one that
                // was already there.
                Inversion::Nothing
            } else {
                one(OperationKind::DeleteBlock {
                    block_id: block.id.clone(),
                })
            }
        }
        OperationKind::DeleteBlock { block_id } => {
            match block_place(&before.blocks, block_id, false) {
                None => Inversion::Nothing,
                Some(place) => one(OperationKind::InsertBlock {
                    position: place.position,
                    block: place.block.clone(),
                }),
            }
        }
        OperationKind::MoveBlock { block_id, .. } => {
            match block_place(&before.blocks, block_id, false) {
                Some(place) => one(OperationKind::MoveBlock {
                    block_id: block_id.clone(),
                    position: place.position,
                }),
                None => Inversion::Nothing,
            }
        }
        OperationKind::InsertSection {
            boundary_id,
            section,
            ..
        } => {
            if before.sections.contains_key(&section.id)
                || block_anywhere(&before.blocks, boundary_id).is_some()
            {
                Inversion::Nothing
            } else {
                one(OperationKind::DeleteSection {
                    section_id: section.id.clone(),
                })
            }
        }
        OperationKind::DeleteSection { section_id } => {
            let Some(section) = before.sections.get(section_id) else {
                return Inversion::Nothing;
            };
            let Some(index) = before.blocks.iter().position(|block| {
                matches!(&block.kind, BlockKind::SectionBreak { section_id: id } if id == section_id)
            }) else {
                return Inversion::Nothing;
            };
            let Some(next) = before.blocks.get(index + 1) else {
                return Inversion::Nothing;
            };
            one(OperationKind::InsertSection {
                before_block_id: next.id.clone(),
                boundary_id: before.blocks[index].id.clone(),
                section: section.clone(),
            })
        }
        OperationKind::SetBlockTextStyle { block_id, style } => {
            match block_anywhere(&before.blocks, block_id).map(|block| &block.kind) {
                None => Inversion::Nothing,
                Some(kind) => match text_style_of(kind) {
                    None => Inversion::Nothing,
                    Some(previous) if previous == *style => Inversion::Nothing,
                    Some(previous) => one(OperationKind::SetBlockTextStyle {
                        block_id: block_id.clone(),
                        style: previous,
                    }),
                },
            }
        }
        OperationKind::UpdateHeadingLevel { block_id, level } => {
            match block_anywhere(&before.blocks, block_id).map(|block| &block.kind) {
                Some(BlockKind::Heading { level: previous }) if previous != level => {
                    one(OperationKind::UpdateHeadingLevel {
                        block_id: block_id.clone(),
                        level: *previous,
                    })
                }
                _ => Inversion::Nothing,
            }
        }
        OperationKind::UpdateListItem {
            block_id,
            level,
            kind,
        } => match block_anywhere(&before.blocks, block_id).map(|block| &block.kind) {
            Some(BlockKind::ListItem {
                level: previous_level,
                kind: previous_kind,
                ..
            }) if previous_level != level || previous_kind != kind => {
                one(OperationKind::UpdateListItem {
                    block_id: block_id.clone(),
                    level: *previous_level,
                    kind: *previous_kind,
                })
            }
            _ => Inversion::Nothing,
        },
        OperationKind::SetListStart {
            list_id,
            level,
            start,
        } => {
            let previous = before
                .list_properties
                .get(list_id)
                .map(|properties| properties.start_for(*level))
                .unwrap_or(1);
            if previous == *start {
                Inversion::Nothing
            } else {
                one(OperationKind::SetListStart {
                    list_id: list_id.clone(),
                    level: *level,
                    start: previous,
                })
            }
        }
        OperationKind::SetListFormat {
            list_id,
            level,
            format,
        } => {
            let previous = before
                .list_properties
                .get(list_id)
                .map(|properties| properties.format_for(*level))
                .unwrap_or_else(|| opendoc_core::OrderedListFormat::inherited_at(*level));
            if previous == *format {
                Inversion::Nothing
            } else {
                one(OperationKind::SetListFormat {
                    list_id: list_id.clone(),
                    level: *level,
                    format: previous,
                })
            }
        }
        OperationKind::SetListBulletMarker {
            list_id,
            level,
            marker,
        } => {
            let previous = before
                .list_properties
                .get(list_id)
                .map(|properties| properties.bullet_marker_for(*level))
                .unwrap_or_else(|| opendoc_core::BulletListMarker::inherited_at(*level));
            if previous == *marker {
                Inversion::Nothing
            } else {
                one(OperationKind::SetListBulletMarker {
                    list_id: list_id.clone(),
                    level: *level,
                    marker: previous,
                })
            }
        }
        OperationKind::SetBlockProperty { block_id, property } => {
            let Some(block) = block_anywhere(&before.blocks, block_id) else {
                return Inversion::Nothing;
            };
            match block.properties.get(property.key()) {
                Some(previous) if previous == *property => Inversion::Nothing,
                Some(previous) => one(OperationKind::SetBlockProperty {
                    block_id: block_id.clone(),
                    property: previous,
                }),
                None => one(OperationKind::ClearBlockProperty {
                    block_id: block_id.clone(),
                    key: property.key(),
                }),
            }
        }
        OperationKind::ClearBlockProperty { block_id, key } => {
            match block_anywhere(&before.blocks, block_id)
                .and_then(|block| block.properties.get(*key))
            {
                None => Inversion::Nothing,
                Some(previous) => one(OperationKind::SetBlockProperty {
                    block_id: block_id.clone(),
                    property: previous,
                }),
            }
        }
        OperationKind::UpdateBlockEquationSource { block_id, source } => {
            match block_anywhere(&before.blocks, block_id).map(|block| &block.kind) {
                Some(BlockKind::EquationBlock { equation }) if equation.source != *source => {
                    one(OperationKind::UpdateBlockEquationSource {
                        block_id: block_id.clone(),
                        source: equation.source.clone(),
                    })
                }
                _ => Inversion::Nothing,
            }
        }
        OperationKind::UpdateImageAltText { block_id, alt_text } => {
            match block_anywhere(&before.blocks, block_id).map(|block| &block.kind) {
                Some(BlockKind::Image {
                    alt_text: previous, ..
                }) if previous != alt_text => one(OperationKind::UpdateImageAltText {
                    block_id: block_id.clone(),
                    alt_text: previous.clone(),
                }),
                _ => Inversion::Nothing,
            }
        }
        OperationKind::UpdateImageBlobHash {
            block_id,
            blob_hash,
        } => match block_anywhere(&before.blocks, block_id).map(|block| &block.kind) {
            Some(BlockKind::Image {
                blob_hash: previous,
                ..
            }) if previous != blob_hash => one(OperationKind::UpdateImageBlobHash {
                block_id: block_id.clone(),
                blob_hash: previous.clone(),
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::UpdateImageLayout { block_id, layout } => {
            match block_anywhere(&before.blocks, block_id).map(|block| &block.kind) {
                Some(BlockKind::Image {
                    layout: previous, ..
                }) if previous != layout => one(OperationKind::UpdateImageLayout {
                    block_id: block_id.clone(),
                    layout: previous.clone(),
                }),
                _ => Inversion::Nothing,
            }
        }

        // ---- inlines ----
        OperationKind::InsertInline { inline, .. } => {
            if find_inline(&before.blocks, inline_id(inline)).is_some() {
                Inversion::Nothing
            } else {
                one(OperationKind::DeleteInline {
                    inline_id: inline_id(inline).clone(),
                })
            }
        }
        OperationKind::DeleteInline { inline_id: target } => {
            match find_inline(&before.blocks, target) {
                None => Inversion::Nothing,
                // `First` when it was the first inline, exactly as for a
                // block: the position names no anchor, so it cannot degrade.
                Some(found) => one(OperationKind::InsertInline {
                    block_id: found.block_id.clone(),
                    position: found.position(),
                    inline: found.inline.clone(),
                }),
            }
        }
        OperationKind::MoveInlineToBlock {
            inline_id: target, ..
        } => match find_inline(&before.blocks, target) {
            None => Inversion::Nothing,
            Some(found) => one(OperationKind::MoveInlineToBlock {
                inline_id: target.clone(),
                target_block_id: found.block_id.clone(),
                position: found.position(),
            }),
        },
        OperationKind::UpdateInlineText {
            inline_id: target,
            text,
        } => match find_inline(&before.blocks, target).map(|found| found.inline.clone()) {
            Some(Inline::Text { text: previous, .. })
            | Some(Inline::Link { text: previous, .. })
                if previous != *text =>
            {
                one(OperationKind::UpdateInlineText {
                    inline_id: target.clone(),
                    text: previous,
                })
            }
            _ => Inversion::Nothing,
        },
        OperationKind::UpdateLinkHref {
            inline_id: target,
            href,
        } => match find_inline(&before.blocks, target).map(|found| found.inline.clone()) {
            Some(Inline::Link { href: previous, .. }) if previous != *href => {
                one(OperationKind::UpdateLinkHref {
                    inline_id: target.clone(),
                    href: previous,
                })
            }
            _ => Inversion::Nothing,
        },
        OperationKind::UpdateMentionLabel {
            inline_id: target,
            label,
        } => match find_inline(&before.blocks, target).map(|found| found.inline.clone()) {
            Some(Inline::Mention {
                label: previous, ..
            }) if previous != *label => one(OperationKind::UpdateMentionLabel {
                inline_id: target.clone(),
                label: previous,
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::SelectDropdownOption {
            inline_id: target,
            option_id,
        } => match find_inline(&before.blocks, target).map(|found| found.inline.clone()) {
            Some(Inline::Dropdown {
                selected_option_id: previous,
                ..
            }) if previous != *option_id => one(OperationKind::SelectDropdownOption {
                inline_id: target.clone(),
                option_id: previous,
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::UpdateDateChip {
            inline_id: target,
            date,
        } => match find_inline(&before.blocks, target).map(|found| found.inline.clone()) {
            Some(Inline::DateChip { date: previous, .. }) if previous != *date => {
                one(OperationKind::UpdateDateChip {
                    inline_id: target.clone(),
                    date: previous,
                })
            }
            _ => Inversion::Nothing,
        },
        OperationKind::UpdateInlineEquationSource {
            inline_id: target,
            source,
        } => match find_inline(&before.blocks, target).map(|found| found.inline.clone()) {
            Some(Inline::Equation { equation, .. }) if equation.source != *source => {
                one(OperationKind::UpdateInlineEquationSource {
                    inline_id: target.clone(),
                    source: equation.source,
                })
            }
            _ => Inversion::Nothing,
        },

        // ---- character operations: inverted at undo time ----
        OperationKind::InsertText { .. } | OperationKind::DeleteText { .. } => Inversion::Deferred,

        // ---- marks ----
        OperationKind::AddMark { text_id, mark } => {
            let Some(marks) = marks_of(&before.blocks, text_id) else {
                return Inversion::Nothing;
            };
            if marks.contains(mark) {
                return Inversion::Nothing;
            }
            if marks
                .iter()
                .any(|existing| existing.kind == mark.kind && existing.value == mark.value)
            {
                // `RemoveMark` matches on kind and value, so it would take the
                // one that was already there with it.
                return Inversion::Irreversible("mark-differs-only-in-expansion");
            }
            one(OperationKind::RemoveMark {
                text_id: text_id.clone(),
                kind: mark.kind.clone(),
                value: mark.value.clone(),
            })
        }
        OperationKind::RemoveMark {
            text_id,
            kind,
            value,
        } => {
            let Some(marks) = marks_of(&before.blocks, text_id) else {
                return Inversion::Nothing;
            };
            let removed: Vec<Mark> = marks
                .iter()
                .filter(|mark| {
                    mark.kind == *kind
                        && value
                            .as_deref()
                            .is_none_or(|wanted| mark.value.as_deref() == Some(wanted))
                })
                .cloned()
                .collect();
            if removed.is_empty() {
                return Inversion::Nothing;
            }
            Inversion::Operations(
                removed
                    .into_iter()
                    .map(|mark| OperationKind::AddMark {
                        text_id: text_id.clone(),
                        mark,
                    })
                    .collect(),
            )
        }
        OperationKind::AddMarkRange { range, mark } => {
            // `add_mark_range` marks whole runs between the two endpoints; it
            // never splits one, so the inverse is a `RemoveMark` per run that
            // did not already carry the mark. Runs the mark was already on are
            // left alone, which is why this needs `before` at all.
            let ids = editable_run_ids(&before.blocks);
            let Some(start) = ids.iter().position(|id| id == &range.start) else {
                return Inversion::Nothing;
            };
            let Some(end) = ids.iter().position(|id| id == &range.end) else {
                return Inversion::Nothing;
            };
            let mut operations = Vec::new();
            for id in &ids[start.min(end)..=start.max(end)] {
                let Some(marks) = marks_of(&before.blocks, id) else {
                    continue;
                };
                if marks.contains(mark) {
                    continue;
                }
                if marks
                    .iter()
                    .any(|existing| existing.kind == mark.kind && existing.value == mark.value)
                {
                    return Inversion::Irreversible("mark-differs-only-in-expansion");
                }
                operations.push(OperationKind::RemoveMark {
                    text_id: id.clone(),
                    kind: mark.kind.clone(),
                    value: mark.value.clone(),
                });
            }
            if operations.is_empty() {
                Inversion::Nothing
            } else {
                Inversion::Operations(operations)
            }
        }

        // ---- comments ----
        OperationKind::AddCommentThread { thread } => {
            if before.comments.iter().any(|item| item.id == thread.id) {
                Inversion::Nothing
            } else {
                one(OperationKind::DeleteCommentThread {
                    thread_id: thread.id.clone(),
                })
            }
        }
        OperationKind::AddCommentReply { thread_id, comment } => {
            match thread_of(before, thread_id) {
                None => Inversion::Nothing,
                Some(thread) if thread.comments.iter().any(|item| item.id == comment.id) => {
                    Inversion::Nothing
                }
                Some(_) => one(OperationKind::DeleteComment {
                    thread_id: thread_id.clone(),
                    comment_id: comment.id.clone(),
                }),
            }
        }
        OperationKind::ResolveCommentThread { thread_id, .. } => match thread_of(before, thread_id)
        {
            Some(thread) if !thread.deleted => one(OperationKind::ReopenCommentThread {
                thread_id: thread_id.clone(),
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::ReopenCommentThread { thread_id } => match thread_of(before, thread_id) {
            Some(thread) if matches!(thread.state, opendoc_core::CommentThreadState::Resolved) => {
                one(OperationKind::ResolveCommentThread {
                    thread_id: thread_id.clone(),
                    resolved_by: thread.resolved_by.clone().unwrap_or_default(),
                    resolved_at_ms: thread.resolved_at_ms.unwrap_or(0),
                })
            }
            _ => Inversion::Nothing,
        },
        OperationKind::SetCommentThreadAction { thread_id, .. } => {
            match thread_of(before, thread_id) {
                Some(thread) if !thread.deleted => one(OperationKind::SetCommentThreadAction {
                    thread_id: thread_id.clone(),
                    assignee: thread.action_assignee.clone(),
                    due_at_ms: thread.action_due_at_ms,
                    completed_by: thread.action_completed_by.clone(),
                    completed_at_ms: thread.action_completed_at_ms,
                }),
                _ => Inversion::Nothing,
            }
        }
        OperationKind::SetCommentThreadReaction {
            thread_id,
            emoji,
            actor,
            ..
        } => match thread_of(before, thread_id) {
            Some(thread) if !thread.deleted => one(OperationKind::SetCommentThreadReaction {
                thread_id: thread_id.clone(),
                emoji: emoji.clone(),
                actor: actor.clone(),
                present: thread
                    .reactions
                    .iter()
                    .find(|reaction| reaction.emoji == *emoji)
                    .is_some_and(|reaction| reaction.actors.iter().any(|item| item == actor)),
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::DeleteCommentThread { thread_id } => match thread_of(before, thread_id) {
            Some(thread) if !thread.deleted => one(OperationKind::RestoreCommentThread {
                thread_id: thread_id.clone(),
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::RestoreCommentThread { thread_id } => match thread_of(before, thread_id) {
            Some(thread) if thread.deleted => one(OperationKind::DeleteCommentThread {
                thread_id: thread_id.clone(),
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::DeleteComment {
            thread_id,
            comment_id,
        } => match comment_of(before, thread_id, comment_id) {
            Some(comment) if !comment.deleted => one(OperationKind::RestoreComment {
                thread_id: thread_id.clone(),
                comment_id: comment_id.clone(),
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::RestoreComment {
            thread_id,
            comment_id,
        } => match comment_of(before, thread_id, comment_id) {
            Some(comment) if comment.deleted => one(OperationKind::DeleteComment {
                thread_id: thread_id.clone(),
                comment_id: comment_id.clone(),
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::UpdateCommentBody {
            thread_id,
            comment_id,
            body,
        } => match comment_of(before, thread_id, comment_id) {
            Some(comment) if comment.body != *body => one(OperationKind::UpdateCommentBody {
                thread_id: thread_id.clone(),
                comment_id: comment_id.clone(),
                body: comment.body.clone(),
            }),
            _ => Inversion::Nothing,
        },

        // ---- suggestions ----
        OperationKind::AddSuggestion { suggestion } => {
            if before
                .suggestions
                .iter()
                .any(|item| item.id == suggestion.id)
            {
                Inversion::Nothing
            } else {
                // The vocabulary has accept and reject, not withdraw, and both
                // of those *resolve* a suggestion — they record who decided
                // and apply or discard its content. Neither is "this was never
                // proposed".
                Inversion::Irreversible("no-operation-withdraws-a-suggestion")
            }
        }
        OperationKind::UpdateSuggestionInsertContent {
            suggestion_id,
            content,
        } => match before
            .suggestions
            .iter()
            .find(|item| item.id == *suggestion_id)
        {
            Some(suggestion) => match suggestion_insert_content(suggestion) {
                Some(previous) if previous != *content => {
                    one(OperationKind::UpdateSuggestionInsertContent {
                        suggestion_id: suggestion_id.clone(),
                        content: previous,
                    })
                }
                _ => Inversion::Nothing,
            },
            None => Inversion::Nothing,
        },
        OperationKind::AcceptSuggestion { .. } | OperationKind::RejectSuggestion { .. } => {
            // Resolution is terminal by design: the merge records which actors
            // resolved a suggestion so that two replicas resolving it
            // concurrently converge, and there is no operation that puts a
            // resolved suggestion back into review.
            Inversion::Irreversible("suggestion-resolution-is-terminal")
        }

        // ---- footnotes, bibliography, citations ----
        OperationKind::UpsertFootnote { footnote } => {
            match before.footnotes.iter().find(|item| item.id == footnote.id) {
                Some(previous) if *previous == *footnote => Inversion::Nothing,
                Some(previous) => {
                    let mut restored = previous.clone();
                    restored.revision = footnote.revision.max(previous.revision) + 1;
                    one(OperationKind::UpsertFootnote { footnote: restored })
                }
                None => {
                    // There is no `DeleteFootnote`; the tombstone is an upsert
                    // with `deleted: true`, which is how the footnote commands
                    // delete one too.
                    let mut tombstone = footnote.clone();
                    tombstone.revision = footnote.revision + 1;
                    tombstone.deleted = true;
                    one(OperationKind::UpsertFootnote {
                        footnote: tombstone,
                    })
                }
            }
        }
        OperationKind::SetEndnotePlacement {
            footnote_id,
            revision,
            ..
        } => match before.footnotes.iter().find(|note| note.id == *footnote_id) {
            Some(note) if !note.deleted => one(OperationKind::SetEndnotePlacement {
                footnote_id: footnote_id.clone(),
                revision: (*revision).max(note.revision) + 1,
                endnote: before.endnote_ids.contains(footnote_id),
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::UpsertBibliographyReference { reference } => {
            match before
                .citation_database
                .references
                .iter()
                .find(|item| item.id == reference.id)
            {
                Some(previous) if *previous == *reference => Inversion::Nothing,
                Some(previous) => {
                    let mut restored = previous.clone();
                    restored.revision = reference.revision.max(previous.revision) + 1;
                    one(OperationKind::UpsertBibliographyReference {
                        reference: restored,
                    })
                }
                None => one(OperationKind::DeleteBibliographyReference {
                    reference_id: reference.id.clone(),
                    revision: reference.revision + 1,
                }),
            }
        }
        OperationKind::DeleteBibliographyReference {
            reference_id,
            revision,
        } => match before
            .citation_database
            .references
            .iter()
            .find(|item| item.id == *reference_id)
        {
            Some(previous) if !previous.deleted => {
                let mut restored = previous.clone();
                restored.revision = revision.max(&previous.revision) + 1;
                one(OperationKind::UpsertBibliographyReference {
                    reference: restored,
                })
            }
            _ => Inversion::Nothing,
        },
        OperationKind::UpsertCitationGroup { citation } => match before
            .citation_database
            .citations
            .iter()
            .find(|item| item.id == citation.id)
        {
            Some(previous) if *previous == *citation => Inversion::Nothing,
            Some(previous) => {
                let mut restored = previous.clone();
                restored.revision = citation.revision.max(previous.revision) + 1;
                one(OperationKind::UpsertCitationGroup { citation: restored })
            }
            None => one(OperationKind::DeleteCitationGroup {
                citation_id: citation.id.clone(),
                revision: citation.revision + 1,
            }),
        },
        OperationKind::DeleteCitationGroup {
            citation_id,
            revision,
        } => match before
            .citation_database
            .citations
            .iter()
            .find(|item| item.id == *citation_id)
        {
            Some(previous) if !previous.deleted => {
                let mut restored = previous.clone();
                restored.revision = revision.max(&previous.revision) + 1;
                one(OperationKind::UpsertCitationGroup { citation: restored })
            }
            _ => Inversion::Nothing,
        },

        // ---- tables ----
        OperationKind::InsertTableRow {
            table_block_id,
            row,
            ..
        } => match table_of(before, table_block_id) {
            Some((_, rows)) if rows.iter().any(|item| item.id == row.id) => Inversion::Nothing,
            Some(_) => one(OperationKind::DeleteTableRow {
                table_block_id: table_block_id.clone(),
                row_id: row.id.clone(),
            }),
            None => Inversion::Nothing,
        },
        OperationKind::DeleteTableRow {
            table_block_id,
            row_id,
        } => match table_of(before, table_block_id) {
            None => Inversion::Nothing,
            Some((columns, rows)) => match rows.iter().position(|item| item.id == *row_id) {
                None => Inversion::Nothing,
                Some(index) => {
                    // The restored row names the columns its cells sit in, so
                    // an undo that lands after somebody else deleted a column
                    // puts each cell back where it belongs rather than one
                    // column to the left. The grid is rectangular here, so
                    // zipping cells with columns is the binding, not a guess.
                    let row = rows[index].clone();
                    let cell_columns = row
                        .cells
                        .iter()
                        .zip(columns.iter())
                        .map(|(cell, column)| (cell.id.clone(), column.id.clone()))
                        .collect();
                    one(OperationKind::InsertTableRow {
                        table_block_id: table_block_id.clone(),
                        position: position_of(index, rows.iter().map(|row| row.id.clone())),
                        row,
                        cell_columns,
                    })
                }
            },
        },
        OperationKind::InsertTableColumn {
            table_block_id,
            column,
            ..
        } => match table_of(before, table_block_id) {
            Some((columns, _)) if columns.iter().any(|item| item.id == column.id) => {
                Inversion::Nothing
            }
            Some(_) => one(OperationKind::DeleteTableColumn {
                table_block_id: table_block_id.clone(),
                column_id: column.id.clone(),
            }),
            None => Inversion::Nothing,
        },
        OperationKind::DeleteTableColumn {
            table_block_id,
            column_id,
        } => match table_of(before, table_block_id) {
            None => Inversion::Nothing,
            Some((columns, rows)) => match columns.iter().position(|item| item.id == *column_id) {
                None => Inversion::Nothing,
                Some(index) => {
                    // `InsertTableColumn` carries no cells: they are *derived*
                    // from the row and column ids so a row another actor
                    // inserted concurrently gets one too. Derived cells are
                    // empty, so a column that held content cannot be restored
                    // by re-inserting it.
                    let held_content = rows.iter().any(|row| {
                        row.cells
                            .get(index)
                            .is_some_and(|cell| !cell_is_empty(cell))
                    });
                    if held_content {
                        Inversion::Irreversible("table-column-cells-are-derived")
                    } else {
                        one(OperationKind::InsertTableColumn {
                            table_block_id: table_block_id.clone(),
                            position: position_of(
                                index,
                                columns.iter().map(|column| column.id.clone()),
                            ),
                            column: columns[index].clone(),
                        })
                    }
                }
            },
        },
        OperationKind::InsertTableCell {
            table_block_id,
            row_id,
            cell,
            ..
        } => match row_of(before, table_block_id, row_id) {
            Some(row) if row.cells.iter().any(|item| item.id == cell.id) => Inversion::Nothing,
            Some(_) => one(OperationKind::DeleteTableCell {
                table_block_id: table_block_id.clone(),
                row_id: row_id.clone(),
                cell_id: cell.id.clone(),
            }),
            None => Inversion::Nothing,
        },
        OperationKind::DeleteTableCell {
            table_block_id,
            row_id,
            cell_id,
        } => match row_of(before, table_block_id, row_id) {
            None => Inversion::Nothing,
            Some(row) => match row.cells.iter().position(|item| item.id == *cell_id) {
                None => Inversion::Nothing,
                Some(index) => one(OperationKind::InsertTableCell {
                    table_block_id: table_block_id.clone(),
                    row_id: row_id.clone(),
                    position: position_of(index, row.cells.iter().map(|cell| cell.id.clone())),
                    cell: row.cells[index].clone(),
                }),
            },
        },
        OperationKind::SetTableColumnWidth {
            table_block_id,
            column_id,
            width,
        } => match table_of(before, table_block_id) {
            None => Inversion::Nothing,
            Some((columns, _)) => match columns.iter().find(|item| item.id == *column_id) {
                Some(column) if column.width != *width => one(OperationKind::SetTableColumnWidth {
                    table_block_id: table_block_id.clone(),
                    column_id: column_id.clone(),
                    width: column.width,
                }),
                _ => Inversion::Nothing,
            },
        },
        OperationKind::SetTableRowHeight {
            table_block_id,
            row_id,
            height,
        } => match row_of(before, table_block_id, row_id) {
            Some(row) if row.height != *height => one(OperationKind::SetTableRowHeight {
                table_block_id: table_block_id.clone(),
                row_id: row_id.clone(),
                height: row.height,
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::SetTableRowHeader {
            table_block_id,
            row_id,
            header,
        } => match row_of(before, table_block_id, row_id) {
            Some(row) if row.header != *header => one(OperationKind::SetTableRowHeader {
                table_block_id: table_block_id.clone(),
                row_id: row_id.clone(),
                header: row.header,
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::ReorderTableRows {
            table_block_id,
            row_ids,
        } => match table_of(before, table_block_id) {
            Some((_, rows)) => {
                let before_ids: Vec<StableId> = rows.iter().map(|row| row.id.clone()).collect();
                (before_ids != *row_ids)
                    .then(|| OperationKind::ReorderTableRows {
                        table_block_id: table_block_id.clone(),
                        row_ids: before_ids,
                    })
                    .map(one)
                    .unwrap_or(Inversion::Nothing)
            }
            None => Inversion::Nothing,
        },
        OperationKind::SetTableBorder {
            table_block_id,
            border,
        } => match block_anywhere(&before.blocks, table_block_id).map(|block| &block.kind) {
            Some(BlockKind::Table { properties, .. }) if properties.border != *border => {
                one(OperationKind::SetTableBorder {
                    table_block_id: table_block_id.clone(),
                    border: properties.border,
                })
            }
            _ => Inversion::Nothing,
        },
        OperationKind::SetTableAlignment {
            table_block_id,
            alignment,
        } => match block_anywhere(&before.blocks, table_block_id).map(|block| &block.kind) {
            Some(BlockKind::Table { properties, .. }) if properties.alignment != *alignment => {
                one(OperationKind::SetTableAlignment {
                    table_block_id: table_block_id.clone(),
                    alignment: properties.alignment,
                })
            }
            _ => Inversion::Nothing,
        },
        OperationKind::SetTableCellSpan { cell_id, span } => match cell_of(before, cell_id) {
            Some(cell) if cell.span != *span => one(OperationKind::SetTableCellSpan {
                cell_id: cell_id.clone(),
                span: cell.span,
            }),
            _ => Inversion::Nothing,
        },
        OperationKind::SetTableCellProperty { cell_id, property } => {
            let Some(cell) = cell_of(before, cell_id) else {
                return Inversion::Nothing;
            };
            match cell.properties.get(property.key()) {
                Some(previous) if previous == *property => Inversion::Nothing,
                Some(previous) => one(OperationKind::SetTableCellProperty {
                    cell_id: cell_id.clone(),
                    property: previous,
                }),
                None => one(OperationKind::ClearTableCellProperty {
                    cell_id: cell_id.clone(),
                    key: property.key(),
                }),
            }
        }
        OperationKind::ClearTableCellProperty { cell_id, key } => {
            match cell_of(before, cell_id).and_then(|cell| cell.properties.get(*key)) {
                None => Inversion::Nothing,
                Some(previous) => one(OperationKind::SetTableCellProperty {
                    cell_id: cell_id.clone(),
                    property: previous,
                }),
            }
        }
    }
}

/// The operations that undo the character operations in `targets`, resolved
/// against the runs they touched as those runs stand **now**.
///
/// `base` and `operations` are the pair the current document is a merge of —
/// the session's merge base and its whole operation set, or the state before an
/// undo step and just that step's operations. Both are legitimate: the merge is
/// a pure function of (base, set), so either pair describes the same runs.
///
/// The answer is not the operations' own offsets. Those were true in the causal
/// context they were written in; by now a collaborator may have typed ahead of
/// them. The runs' character identities are re-derived exactly as the merge
/// derives them, and the question asked of them is "which characters do
/// **these** operations still own, and where are they in the visible run".
///
/// `targets` is a set rather than one operation because a single undo step
/// routinely holds several edits to one run — a replace-all is a delete and an
/// insert per occurrence — and their inverses share one coordinate space. Taken
/// one at a time each would be correct against the run before the undo and
/// wrong against the run the previous inverse just changed.
///
/// The emitted order is load-bearing: every delete first, in descending
/// position, then every insert, in descending position. Deleting at a higher
/// offset never moves a lower one, and the insert offsets are counted in the
/// sequence the deletes leave behind.
pub fn invert_text_operations(
    base: &Document,
    operations: &[Operation],
    targets: &[OperationId],
) -> Inversion {
    let ordered = ordered_operations(operations);
    let (edits_by_run, resets) = collect_text_run_edits(&ordered);
    let mut runs: Vec<StableId> = Vec::new();
    for target in targets {
        let Some(operation) = ordered.iter().find(|operation| operation.id == *target) else {
            continue;
        };
        let run = match &operation.kind {
            OperationKind::InsertText { inline_id, .. }
            | OperationKind::DeleteText { inline_id, .. } => inline_id.clone(),
            _ => continue,
        };
        if !runs.contains(&run) {
            runs.push(run);
        }
    }

    let mut operations_out = Vec::new();
    for run in runs {
        let Some(mut edits) = edits_by_run.get(&run).cloned() else {
            // Every edit was an empty insert or a zero-width delete: the merge
            // never collected them and they changed nothing.
            continue;
        };
        let reset = resets.get(&run);
        if let Some(reset_rank) = reset {
            edits.retain(|edit| edit.rank > *reset_rank);
        }
        // Indices of the edits being undone. An edit missing from this list was
        // discarded by a whole-run write ordered after it, so there is nothing
        // left of it to undo.
        let undone: Vec<usize> = edits
            .iter()
            .enumerate()
            .filter(|(_, edit)| targets.contains(&edit.id))
            .map(|(index, _)| index)
            .collect();
        if undone.is_empty() {
            continue;
        }
        let atoms = resolve_run_atoms(&run_base_text(base, &run, &ordered, reset), &edits);

        // Characters these operations inserted and that are still visible.
        // Grouped into visible ranges, because a collaborator may have typed
        // inside the inserted text and that is not ours to remove.
        let mut deletes: Vec<(usize, usize)> = Vec::new();
        // Characters these operations removed, that nobody else also removed,
        // and that they did not themselves insert — an insert being undone in
        // the same step must stay gone, not come back because the same step
        // also deleted it.
        let mut restores: Vec<(usize, String)> = Vec::new();
        let mut visible = 0usize;
        let mut kept = 0usize;
        let mut restore_open = false;
        for atom in &atoms {
            let ours = atom
                .inserted_by
                .is_some_and(|index| undone.contains(&index));
            if atom.visible() {
                if ours {
                    match deletes.last_mut() {
                        Some(last) if last.1 == visible => last.1 = visible + 1,
                        _ => deletes.push((visible, visible + 1)),
                    }
                } else {
                    // Survives the undo, so it is one of the positions the
                    // restored text is counted against.
                    kept += 1;
                }
                visible += 1;
                restore_open = false;
                continue;
            }
            let only_ours = !atom.deleted_by.is_empty()
                && atom.deleted_by.iter().all(|index| undone.contains(index));
            if !only_ours || ours {
                // Still deleted by somebody else, or inserted by an operation
                // this same undo is removing. Either way it stays invisible,
                // and it breaks nothing: the characters either side of it were
                // adjacent before and stay adjacent.
                continue;
            }
            if restore_open {
                if let Some(last) = restores.last_mut() {
                    last.1.push(atom.ch);
                }
            } else {
                restores.push((kept, atom.ch.to_string()));
                restore_open = true;
            }
        }

        deletes.sort_unstable_by_key(|(start, _)| *start);
        deletes.reverse();
        operations_out.extend(
            deletes
                .into_iter()
                .map(|(start, end)| OperationKind::DeleteText {
                    inline_id: run.clone(),
                    start,
                    end,
                }),
        );
        restores.sort_by_key(|(offset, _)| std::cmp::Reverse(*offset));
        operations_out.extend(restores.into_iter().map(|(offset, text)| {
            OperationKind::InsertText {
                inline_id: run.clone(),
                offset,
                text,
            }
        }));
    }

    if operations_out.is_empty() {
        Inversion::Nothing
    } else {
        Inversion::Operations(operations_out)
    }
}

/// Which operations of a batch the merge throws away, as a flag per position
/// in the order the batch will be merged in.
///
/// ADR 0007's whole-run reset is a property of the operation **set**, not of
/// any one operation: a character operation loses to a later operation that
/// writes the whole of the run it addresses, so the merge never applies it and
/// the document never holds the characters it names.
///
/// That is why this exists. A fold that merges a batch one operation at a time
/// — which is what `opendoc-app` does to recover the intermediate states an
/// inverse has to be captured against (ADR 0017) — puts each operation alone
/// in its own merge, where there is no later write for it to lose to. The fold
/// therefore applies a character operation the batch discards, and the *next*
/// operation's inverse captures a previous value describing a state the
/// document was never in. Undoing that step puts the discarded characters back
/// in: an undo that adds text. Telling the fold which operations the batch
/// discards is what keeps its states states the batch really passes through.
///
/// The rule is read out of [`runs_written_wholesale`], the same per-operation
/// answer [`collect_text_run_edits`] gives the merge and
/// [`invert_text_operations`], so the three cannot disagree about which writes
/// reset a run.
///
/// An operation that is not offset-addressed is never discarded, so its flag is
/// always `false`.
pub fn discarded_by_a_later_whole_run_write<'a>(
    batch: impl IntoIterator<Item = &'a OperationKind>,
) -> Vec<bool> {
    let batch: Vec<&OperationKind> = batch.into_iter().collect();
    // The *last* write wins, which is also the rank the merge keeps: an
    // operation between two whole-run writes is discarded by the later one
    // just as surely as one before the first.
    let mut last_whole_run_write: BTreeMap<StableId, usize> = BTreeMap::new();
    for (rank, kind) in batch.iter().enumerate() {
        for run in runs_written_wholesale(kind) {
            last_whole_run_write.insert(run, rank);
        }
    }
    batch
        .iter()
        .enumerate()
        .map(|(rank, kind)| {
            text_run_of(kind).is_some_and(|run| {
                last_whole_run_write
                    .get(run)
                    .is_some_and(|reset| rank < *reset)
            })
        })
        .collect()
}

/// The run one character operation addresses, or `None` for any other
/// operation. What an undo needs in order to group a step's character
/// operations by the coordinate space they share.
pub fn text_run_of(kind: &OperationKind) -> Option<&StableId> {
    match kind {
        OperationKind::InsertText { inline_id, .. }
        | OperationKind::DeleteText { inline_id, .. } => Some(inline_id),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn one(kind: OperationKind) -> Inversion {
    Inversion::Operations(vec![kind])
}

/// The operation set in the merge's own order, so a rank here is the rank the
/// merge gave the same operation.
fn ordered_operations(operations: &[Operation]) -> Vec<&Operation> {
    let order = causal_order(operations);
    order.into_iter().map(|index| &operations[index]).collect()
}

/// The text the sequence CRDT resolves a run's edits against.
///
/// Normally the run as the merge base holds it. When an operation rewrote the
/// whole run — `UpdateInlineText`, or an insert whose payload carried the run —
/// that payload is the base instead, and the edits before it were already
/// dropped. If the run is in neither, there is nothing to anchor to and the
/// empty string is the honest answer.
fn run_base_text(
    base: &Document,
    run: &StableId,
    ordered: &[&Operation],
    reset_rank: Option<&usize>,
) -> String {
    if let Some(rank) = reset_rank {
        if let Some(operation) = ordered.get(*rank) {
            if let Some(text) = run_text_in_payload(&operation.kind, run) {
                return text;
            }
        }
    }
    find_inline(&base.blocks, run)
        .map(|found| match &found.inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => text.clone(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

fn run_text_in_payload(kind: &OperationKind, run: &StableId) -> Option<String> {
    match kind {
        OperationKind::UpdateInlineText { inline_id, text } if inline_id == run => {
            Some(text.clone())
        }
        OperationKind::InsertInline { inline, .. } => {
            run_text_in_inlines(std::slice::from_ref(inline), run)
        }
        OperationKind::InsertBlock { block, .. } => {
            run_text_in_blocks(std::slice::from_ref(block), run)
        }
        OperationKind::InsertTableRow { row, .. } => row
            .cells
            .iter()
            .find_map(|cell| run_text_in_blocks(&cell.blocks, run)),
        OperationKind::InsertTableCell { cell, .. } => run_text_in_blocks(&cell.blocks, run),
        _ => None,
    }
}

fn run_text_in_blocks(blocks: &[Block], run: &StableId) -> Option<String> {
    for block in blocks {
        if let Some(text) = run_text_in_inlines(&block.content, run) {
            return Some(text);
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(text) = run_text_in_blocks(&cell.blocks, run) {
                        return Some(text);
                    }
                }
            }
        }
    }
    None
}

fn run_text_in_inlines(inlines: &[Inline], run: &StableId) -> Option<String> {
    inlines.iter().find_map(|inline| match inline {
        Inline::Text { id, text, .. } | Inline::Link { id, text, .. } if id == run => {
            Some(text.clone())
        }
        _ => None,
    })
}

/// A block plus the stable sibling anchor needed to put it back. `First` is
/// enough for the document body, but a cell has no parent id in `InsertBlock`;
/// its first child must therefore be restored *before its next sibling*.
/// `Before` gives the operation that identity without inventing a fragile
/// numeric table path.
struct BlockPlace<'a> {
    position: InsertPosition,
    block: &'a Block,
}

fn block_place<'a>(
    blocks: &'a [Block],
    block_id: &StableId,
    nested: bool,
) -> Option<BlockPlace<'a>> {
    for (index, block) in blocks.iter().enumerate() {
        if block.id == *block_id {
            let position = match index {
                0 if blocks.len() == 1 && nested => return None,
                0 if nested => InsertPosition::Before(blocks[1].id.clone()),
                0 => InsertPosition::First,
                _ => InsertPosition::After(blocks[index - 1].id.clone()),
            };
            return Some(BlockPlace { position, block });
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(place) = block_place(&cell.blocks, block_id, true) {
                        return Some(place);
                    }
                }
            }
        }
    }
    None
}

fn block_anywhere<'a>(blocks: &'a [Block], block_id: &StableId) -> Option<&'a Block> {
    for block in blocks {
        if block.id == *block_id {
            return Some(block);
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = block_anywhere(&cell.blocks, block_id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

/// Where an inline sits: which block owns it, and the [`InsertPosition`] that
/// puts it back there.
///
/// The position is the whole answer, including for the first inline of a
/// block: the struct used to carry the index and the sibling count as well,
/// because `after: Option<_>` could not name the front and the caller had to
/// notice.
struct InlinePlace<'a> {
    block_id: &'a StableId,
    position: InsertPosition,
    inline: &'a Inline,
}

impl InlinePlace<'_> {
    fn position(&self) -> InsertPosition {
        self.position.clone()
    }
}

fn find_inline<'a>(blocks: &'a [Block], target: &StableId) -> Option<InlinePlace<'a>> {
    for block in blocks {
        if let Some(index) = block
            .content
            .iter()
            .position(|inline| inline_id(inline) == target)
        {
            return Some(InlinePlace {
                block_id: &block.id,
                position: position_of(
                    index,
                    block.content.iter().map(|inline| inline_id(inline).clone()),
                ),
                inline: &block.content[index],
            });
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = find_inline(&cell.blocks, target) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

fn marks_of<'a>(blocks: &'a [Block], text_id: &StableId) -> Option<&'a [Mark]> {
    match find_inline(blocks, text_id)?.inline {
        Inline::Text { marks, .. } | Inline::Link { marks, .. } => Some(marks.as_slice()),
        _ => None,
    }
}

fn editable_run_ids(blocks: &[Block]) -> Vec<StableId> {
    let mut ids = Vec::new();
    for block in blocks {
        for inline in &block.content {
            if matches!(inline, Inline::Text { .. } | Inline::Link { .. }) {
                ids.push(inline_id(inline).clone());
            }
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    ids.extend(editable_run_ids(&cell.blocks));
                }
            }
        }
    }
    ids
}

fn text_style_of(kind: &BlockKind) -> Option<BlockTextStyle> {
    match kind {
        BlockKind::Paragraph => Some(BlockTextStyle::Paragraph),
        BlockKind::Title => Some(BlockTextStyle::Title),
        BlockKind::Subtitle => Some(BlockTextStyle::Subtitle),
        BlockKind::Heading { level } => Some(BlockTextStyle::Heading { level: *level }),
        BlockKind::ListItem {
            list_id,
            level,
            kind,
        } => Some(BlockTextStyle::ListItem {
            list_id: list_id.clone(),
            level: *level,
            kind: *kind,
        }),
        _ => None,
    }
}

fn thread_of<'a>(document: &'a Document, thread_id: &StableId) -> Option<&'a CommentThread> {
    document
        .comments
        .iter()
        .find(|thread| thread.id == *thread_id)
}

fn comment_of<'a>(
    document: &'a Document,
    thread_id: &StableId,
    comment_id: &StableId,
) -> Option<&'a Comment> {
    thread_of(document, thread_id)?
        .comments
        .iter()
        .find(|comment| comment.id == *comment_id)
}

fn suggestion_insert_content(suggestion: &opendoc_core::Suggestion) -> Option<Vec<Inline>> {
    match &suggestion.kind {
        opendoc_core::SuggestionKind::Insert { content, .. } => Some(content.clone()),
        _ => None,
    }
}

fn table_of<'a>(
    document: &'a Document,
    table_block_id: &StableId,
) -> Option<(&'a [TableColumn], &'a [TableRow])> {
    match &block_anywhere(&document.blocks, table_block_id)?.kind {
        BlockKind::Table { columns, rows, .. } => Some((columns.as_slice(), rows.as_slice())),
        _ => None,
    }
}

fn row_of<'a>(
    document: &'a Document,
    table_block_id: &StableId,
    row_id: &StableId,
) -> Option<&'a TableRow> {
    let (_, rows) = table_of(document, table_block_id)?;
    rows.iter().find(|row| row.id == *row_id)
}

fn cell_of<'a>(document: &'a Document, cell_id: &StableId) -> Option<&'a TableCell> {
    fn search<'a>(blocks: &'a [Block], cell_id: &StableId) -> Option<&'a TableCell> {
        for block in blocks {
            if let BlockKind::Table { rows, .. } = &block.kind {
                for row in rows {
                    for cell in &row.cells {
                        if cell.id == *cell_id {
                            return Some(cell);
                        }
                        if let Some(found) = search(&cell.blocks, cell_id) {
                            return Some(found);
                        }
                    }
                }
            }
        }
        None
    }
    search(&document.blocks, cell_id)
}

fn cell_is_empty(cell: &TableCell) -> bool {
    cell.blocks.iter().all(|block| {
        block.content.iter().all(|inline| match inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => text.is_empty(),
            _ => false,
        })
    })
}

/// The anchor that puts a sibling back at `index`, said the way the table
/// operations say it: identity, never an index, and `First` for the front.
fn position_of(index: usize, mut ids: impl Iterator<Item = StableId>) -> InsertPosition {
    match index.checked_sub(1) {
        None => InsertPosition::First,
        Some(previous) => match ids.nth(previous) {
            Some(id) => InsertPosition::After(id),
            None => InsertPosition::Last,
        },
    }
}
