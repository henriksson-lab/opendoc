# ADR 0023: Bookmarks Name Stable Blocks, Not Text Offsets

## Decision

OpenDoc bookmarks are durable records with an id, a portable unique name, a
target `block_id`, revision, and deletion tombstone. A bookmark names a block,
not a byte or character offset inside a mutable inline run. The name is an
ASCII Word-compatible identifier (letters/underscore first, then letters,
digits, hyphen or underscore, maximum 40 bytes).

`UpsertBookmark` is the only mutation. Its revision is last-writer-wins for a
bookmark id; when two live records claim one name, the operation later in the
normal deterministic merge order wins and the other is tombstoned. A dangling
target is valid source state: deleting a block must not make a document
unsaveable, and an undo or concurrent reinsert can restore it.

The HTML renderer emits a zero-size named anchor at the start of the target
block, preserving `data-block-id` separately for the editor. Google-shaped
OpenDoc JSON stores bookmarks in an explicit `opendocBookmarks` extension and
warns that this is not native Google Docs bookmark interoperability. DOCX and
ODT export a bookmark on a text-bearing target block as a native, zero-width
start/end pair at the beginning of its paragraph. This preserves the named
navigation target without claiming a character range. A bookmark whose target
has no text paragraph carrier (for example a table or image) is named in an
export warning rather than silently dropped.

## Consequences

This gives links and a future TOC a stable navigation primitive that converges
under collaboration. Native Google Docs import is deliberately bounded to a
zero-width body bookmark exactly at an imported structural-element start. An
interior position or non-empty supplied range is warned and omitted: projecting
either to a whole block would falsely preserve a character anchor. It
deliberately does **not** solve character-granular bookmark ranges, native
Google *range* import, DOCX/ODT *range import*, or an updatable TOC block;
those need their own range/derived-content design.
