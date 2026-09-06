# App API Contract v0

This is the prototype projection contract between the Rust app core, the Tauri
command layer, the browser demo backend, and future server modes. The Rust
source of truth is `opendoc_app_api::AppDocument`.

Compatibility is not promised yet. Changes are allowed while OpenDoc remains a
research prototype, but every change to this contract must update the Rust
projection tests and the TypeScript rendering/mock backend together.

## Top-Level Document

`AppDocument` contains:

- `uuid`, `title`, `locale`, `doi`, `visible_text`
- `is_open`
- `blocks`
- `footnotes`
- `comments`
- `suggestions`
- `citations`
- `workbook`
- `blobs`
- `warnings`
- `signature_state`
- `signature`
- `signatures`
- `repository_root`
- `repository_backend`
- `repository_namespace`
- `recent_documents`
- `has_unsaved_changes`
- `last_manifest`
- `operation_count`
- `operations`

`visible_text` is a display/search projection only. `repository_backend`,
`repository_namespace`, `recent_documents`, and `has_unsaved_changes` are
UI/API repository context only. Signing uses the canonical snapshot payload,
not rendered text or repository context.

Canonical `AppDocument` snapshots validate the combined source payload before
they are treated as durable state: the core document tree, workbook source,
blob metadata, exact-byte signature targets, typed semantic signature source
fields, and archive tombstone metadata must be structurally valid. Projection
and repository context fields remain outside authored source semantics.

`close_document` returns an `AppDocument` projection with `is_open = false`,
clears current repository/signature/undo state, and preserves
`recent_documents` so shells can reopen without exposing repository internals.
Commands that mutate, save, sign, verify, or export source state fail with a
clear `no document is open` conflict until a document is created, imported, or
opened again.

`get_audit_view` returns `AppAuditView`, a recovery/audit projection containing
repository context, warnings, document signatures, current and deleted blob
exact-byte signatures, current and deleted typed semantic blob signatures,
deleted document comments, restorable deleted spreadsheet sheet operation records,
restorable deleted spreadsheet named ranges, deleted spreadsheet cell comments, resolved
suggestions, deleted citation records, invalid serverless candidate-head
records, and operation history. It is not additional signed source state; it
exposes source/history and repository audit records already retained by the
document and repository model.

`get_runtime_profile` returns `OpenDocRuntimeProfile` for `tauri-local`,
`browser-local`, `hpc-single-user`, or `multi-user-service`. The profile
describes shell policy: enabled storage backends, whether private-key signing is
available, whether browser signing is explicitly deferred, and whether the
service layer enforces permissions. Runtime profiles are not document source
state and are excluded from signed snapshot payloads.

`get_runtime_session` returns `OpenDocRuntimeSession` for the same runtime
modes. It adds the authenticated subject, optional document UUID, service-layer
permission grants, presence peers, and shell/runtime warnings. In local,
browser, and HPC single-user modes the permission grant list is empty because
the document format does not enforce permissions. In multi-user service mode,
wrappers may supply service-layer grants explicitly; when no grants are
supplied, an authenticated subject receives default `read`, `comment`, `write`,
and `share` grants scoped to the document UUID when one is supplied, otherwise
to the repository. Supplied permission grants ignore non-object entries and
entries without a non-empty subject, action, and scope, then trim string fields
before session projection or authorization matching. Presence peers are
normalized shell/session records and may point at source IDs such as inline
anchors, but they are not signed source state. Presence parsing ignores
non-object entries and entries without a subject, trims string fields, defaults
blank display names to the subject, defaults blank roles to `viewer`, trims
empty cursor anchors to `null`, and clamps missing or negative `last_seen_ms`
values to `0`.

`authorize_runtime_command` returns `OpenDocAuthorizationDecision`. It maps an
app command to a required action (`read`, `comment`, or `write`), checks runtime
capabilities such as local/OpenDAL repository access and private-key signing,
and, in `multi-user-service` mode, requires an authenticated subject with a
matching normalized service permission grant. Local, browser, Tauri, and HPC
modes do not perform document-level permission checks, but runtime capability
limits still apply. Unknown commands are denied. This decision object is
shell/service state, not document source state, and is excluded from signatures.

`create_runtime_share_invite` returns `OpenDocShareInvite`. It is a
multi-user-service primitive for creating service-layer grants for another
subject. The issuer must be authenticated and authorized for the `share` action
on the document. Issuer, target subject, document UUID, and action names are
normalized before authorization and grant creation. Supported target actions are
`read`, `comment`, `write`, and `share`; unsupported or duplicate actions are
ignored. Missing, non-array, or mixed-type action inputs are treated as an empty
or partially unsupported action list and degrade to an empty invite with a
warning when no supported actions remain. Local, browser, Tauri, and HPC
runtimes return a denied/empty invite with a warning because durable sharing
requires a service. Share invites are not document source state and are excluded
from signatures.

`relay_runtime_sync` returns `OpenDocSyncRelayResult`. It is a
multi-user-service primitive for classifying operation envelopes that a service
relay would accept, defer for candidate-head reconciliation, or reject before
commit integration. The caller must be authenticated and authorized for `write`
on the document. Local, browser-local, Tauri-local, and HPC single-user modes
return a denied result because relay ordering is a service responsibility.
Malformed operation inputs from wrappers are normalized into invalid envelopes;
malformed envelopes, including empty IDs, actors, kinds, or zero sequence
numbers, are then rejected after envelope-string normalization. Duplicate
operation IDs and duplicate actor/sequence pairs are deferred, and operations
based on a stale manifest are deferred rather than silently rewritten. Presence
echoed by the result is runtime/session state, not document source state, and
relay results are excluded from signatures.

`resolve_runtime_document_lookup` returns `OpenDocRuntimeLookupResult`. It is a
read-only runtime primitive for resolving a document UUID or DOI through
service-layer lookup acceleration when available, with deterministic repository
scan fallback for serverless/local/HPC modes. Multi-user service mode requires a
matching `read` grant; local, browser, Tauri, and HPC modes rely on repository
access rather than document-level permissions. Service-index hits are preferred
only in multi-user service mode. Scan fallback is explicit through
`lookup_source = "scan-fallback"`, `used_scan = true`, and a warning. Runtime
lookup entries ignore malformed wrapper inputs and entries without a document
UUID, then normalize fields before matching or returning results. Runtime lookup
results are shell/service state and are excluded from signatures.

`undo_current_edit` and `redo_current_edit` restore app-level source-state
checkpoints for command-dispatched local edits and append audit operation
records. They clear document/version signatures because current source/history
changed. Collaborative per-actor undo remains part of the later merge/editor
model work.

## Import And Export Commands

- `import_google_docs_json` imports a constrained Google Docs API-shaped
  document body into the rich document schema.
  List `nestingLevel` values outside `0..=8` abort import instead of being
  normalized into a different source structure.
- `export_google_docs_json` exports the current v0 rich document subset as
  Google Docs-shaped JSON text.
- `import_doc_or_docx_path` imports a local `.doc` or `.docx` file through the
  native converter path in Tauri/local mode. Browser mock mode returns an
  explicit placeholder warning because browsers cannot read arbitrary local
  paths directly.
- `import_google_sheets_json` imports a constrained Google Sheets API-shaped
  workbook into the spreadsheet schema.
- `export_google_sheets_json` exports the current v0 workbook subset as Google
  Sheets-shaped JSON text.

Google-shaped JSON is compatibility/debug interchange, not canonical storage
and not the signed binary source format.

App-level Google Docs-shaped export includes an `opendocBlobs` top-level
extension for current attachment/image blob references. The extension preserves
blob IDs, names, media types, content hashes, sizes, archive tombstone
metadata, and typed semantic signature envelopes, but it does not embed binary
object bytes. Importing `opendocBlobs` therefore creates shallow blob refs with
`available = false`; exact-byte signature display metadata is treated as
sidecar-only and produces an `opendoc-blob-exact-signature-metadata-only`
warning instead of pretending the sidecar signature can be verified from the
debug export alone. Imported blob content hashes and typed-signature
`source_blob` fields are trimmed before validation and source projection.
Google Docs-shaped OpenDoc extension identifiers are also trimmed before
validation/projection when they identify source objects or anchors: list IDs,
block IDs, inline IDs, equation IDs, footnote IDs, citation/reference IDs,
comment/suggestion IDs, nearest-block anchors, and text-range endpoints.

OpenDoc image blocks round-trip through an explicit Google Docs-shaped
`opendocImage` extension with `blockId`, `blobHash`, and `altText`. This keeps
image bytes in content-addressed blob storage while proving the source schema in
the adapter. Importing an `opendocImage` reference without available blob bytes
creates a missing current blob placeholder plus an
`image-blob-reference-restored` warning, so the document remains openable and
shallow-clone-compatible. Native Google Docs inline image/object elements remain
unsupported until a dedicated importer can map them without silently losing blob
semantics.

OpenDoc inline and block equations round-trip through `opendocEquation` and
`opendocEquationBlock` with stable IDs, `sourceFormat`, and `source`. Native
Google Docs inline `equation` elements remain a lossy placeholder import
because that API shape does not expose TeX/LaTeX source.

OpenDoc mentions round-trip through inline `opendocMention` elements with
`inlineId` and `label`, so mention text is preserved as typed source instead of
being flattened into a plain text run.

OpenDoc page-break blocks export as Google Docs paragraph-level `pageBreak`
elements. Import accepts those native page-break paragraphs and the older
metadata-only `sectionBreak` shape, but mixed page-break/content paragraphs
abort because their ordering semantics are ambiguous.

Google Docs-shaped table cells import and export supported nested OpenDoc
blocks through the same paragraph, page-break, image, and block-equation adapter
paths used by top-level body content. Nested tables abort explicitly on import
and export in v0.

## Local Repository Commands

- `save_local_repository` writes a local on-disk repository commit using the
  app's last opened/saved manifest as the expected branch head. If the head has
  moved, it fails with a conflict instead of silently parenting stale state.
- `save_local_repository_or_candidate` uses the same binary snapshot and
  operation-segment format, but writes a serverless candidate head when the
  mutable branch head cannot be advanced.
- `save_flat_repository` and `save_flat_repository_or_candidate` use the same
  repository semantics under a filesystem-backed `bucket/prefix` namespace that
  mirrors S3/OpenDAL key layout and deliberately avoids local pack files.
- `save_opendal_fs_repository` and
  `save_opendal_fs_repository_or_candidate` use the same repository semantics
  through the feature-gated OpenDAL filesystem adapter. This is the app/API
  path for the HPC/local-disk OpenDAL mode before S3 credentials are available.
- `autosave_current_repository` saves to the current local, flat, or OpenDAL
  filesystem repository context using candidate fallback. It fails with a clear
  conflict if no repository has been opened or saved yet.
- `add_text_mark` accepts every v0 mark kind. `value` is optional for simple
  marks and carries source values for `color`, `background`, `font`, and
  `size`. `remove_text_mark` removes marks of the same kind from a stable
  inline ID; when `value` is supplied, only that valued mark is removed.
- `open_local_repository` opens the current branch head by document UUID.
- `open_local_repository_by_doi` opens through the optional DOI lookup alias.
- Repository document UUID and DOI command inputs are trimmed before head,
  candidate, and lookup operations. Paths and namespaces are not trimmed because
  those values can legitimately contain whitespace.
- `open_flat_repository` and `open_flat_repository_by_doi` open through the
  same manifest, snapshot, operation-segment, blob, signature, UUID, and DOI
  records from the flat namespaced backend.
- `open_opendal_fs_repository` and `open_opendal_fs_repository_by_doi` open
  through the same records from the OpenDAL filesystem backend when compiled
  with `opendal-store`.
- `merge_local_repository_candidates` first consumes fast-forward candidates,
  then repeatedly loads planned divergent candidate ranges, runs operation-level
  document merge for replayable operations, and commits merged snapshots until
  no divergent document-operation candidates remain or the safety limit is hit.
- `compact_local_repository` packs loose local disk repository objects into a
  named local pack file and returns a projection warning with pack statistics.
  This is a repository maintenance operation: it may update local repository
  context, but it does not append document operations, mutate signed source
  state, or clear signatures.
- `merge_flat_repository_candidates` runs the same candidate reconciliation and
  operation-level merge path against the flat S3/OpenDAL-shaped namespace.
  Because serverless candidate records are immutable, old visible candidates
  are skipped when their replayable operation IDs are already reachable from the
  current head.
- `merge_opendal_fs_repository_candidates` runs the same candidate
  reconciliation and operation-level merge path against the OpenDAL filesystem
  backend.

Operation segments store replayable operation envelopes. Each envelope contains
the user-visible `AppOperationRecord` plus an optional `opendoc-merge`
operation, an optional spreadsheet operation, and an optional attachment/blob
metadata operation. Summary-only operations are retained for audit/projection,
while merge execution uses replayable document, spreadsheet, and attachment
metadata operations.

`recent_documents` is a bounded projection list updated after successful
repository save/open/merge operations. It records document UUID, title, optional
DOI, repository root, backend kind, optional namespace, latest known manifest,
and update time. It is convenience state for shells and is not signed source
content.

Replayable spreadsheet operations currently include cell value edits, cell
format edits, sheet creation/deletion/rename with explicit sheet ID, row/column
lifecycle, cell comments, frozen panes, validations, merged ranges, basic
filters, protected ranges, range copy, and named-range creation/update.

Replayable attachment operations currently include content-addressed attachment
registration, attachment display metadata updates, and attachment deletion from
current state. Blob bytes remain content-addressed repository objects;
operation replay carries the source metadata needed for deterministic
candidate merges.

## Blobs

Each blob reference contains:

- `id`
- `name`
- `media_type`
- `hash`
- `size`
- `available`
- `signature_state`
- `signatures`
- `typed_signatures`
- `archive_tombstone`

Blob bytes are stored as content-addressed repository objects and referenced by
manifest hash. A missing blob is an availability problem, not a document-open
failure; clients render a placeholder and surface a warning.
Blob command hash inputs are trimmed before content-hash parsing or comparison.
This applies to blob signing, typed blob signing, metadata updates,
delete/restore, archive tombstone recording, image block insertion, and image
blob replacement.
If the repository has a tombstone for a missing blob, `archive_tombstone`
contains archive locator, restore hint, creation time, and signer metadata for
audit/recovery views. This is repository recovery metadata, not signed authored
document content.
`record_blob_archive_tombstone` writes a binary repository tombstone sidecar for
a current or retained deleted blob by content hash. It requires an opened or
saved repository context, non-empty locator, restore hint, signer, and signature
bytes. Locator, restore hint, and signer fields are trimmed before validation
and storage. The command does not mutate authored source state, does not clear
source signatures, and does not make missing bytes trusted; after recall,
restored bytes must still hash to the blob hash and verify any blob or typed
signatures.
`update_binary_blob_metadata` changes the source-level attachment display name
and media type by blob hash while exact-byte blob sidecar signatures remain
attached to the unchanged content hash.
`delete_binary_blob` removes the blob reference from the current attachment
list by content hash and appends a replayable delete operation. The operation
history, immutable blob object bytes, exact-byte blob signature sidecars, and
typed semantic signature metadata remain available for audit/recovery when
present, but the current projection no longer lists the attachment. It fails
with a conflict while any current image block still references the blob hash;
call `update_image_blob_hash` or delete the image block first. During automatic
merge, delete wins over concurrent attachment display metadata updates so a
stale rename cannot resurrect a removed current-state attachment. If a
concurrent document operation makes that blob visible through an image block,
the merged document keeps the image block and restores the blob metadata to the
current attachment list with an `image-blob-reference-restored` warning instead
of leaving a dangling source reference.
`restore_binary_blob` reactivates a deleted blob from retained audit history by
content hash and appends a replayable restore operation carrying the recovered
attachment metadata and typed semantic signature metadata. Exact-byte blob
signatures remain keyed by content hash sidecars, so restoring an attachment
does not rehash or rewrite the underlying binary object. It fails cleanly if
the blob is already current or if no retained deleted audit record exists.
`simulate_shallow_clone` is a local/runtime test command that clears available
blob bytes from the current app session and marks current blob references as
missing while preserving attachment metadata, exact-byte signatures, typed
semantic signatures, archive tombstones, and version signatures. It emits
`missing-blob` warnings so the GUI renders placeholders. It is not authored
source deletion and does not make the document unopenable.
`AppAuditBlobSignature.deleted` distinguishes deleted retained blob audit rows
from current attachment rows. Deleted typed semantic signatures are verified
against retained blob bytes when available and become `untrusted` when bytes are
missing. Missing deleted blobs do not create normal document warnings because
they are not current rendered attachments; their tombstone and trust state are
exposed through audit/recovery.
`add_image_block` inserts a document block that references an existing blob by
content hash and stores source-level alt text. `insert_image_block_after` uses
the same blob validation but inserts after a stable top-level block anchor for
keyboard-driven image insertion. `update_image_alt_text` edits the alt text by
stable image block ID without changing the referenced blob hash.
`update_image_blob_hash` replaces the referenced blob by stable image block ID
without changing the block anchor or alt text. The image bytes remain in the
blob layer, so detached blob signatures and shallow-clone missing-blob warnings
continue to work without embedding bytes in document source.
`sign_fastq_blob_with_openssh_private_key` signs an attached FASTQ blob through
either `opendoc.fastq.sequence.v0` or `opendoc.fastq.full.v0`. Typed signature
entries record the source blob hash, semantic digest, included/excluded fields,
signature metadata, and a projection verification state. Available blobs are
rechecked during projection and after save/open; missing bytes make the typed
signature `untrusted`, while semantic digest or signature mismatches make it
`broken`.
`sign_image_pixels_blob_with_openssh_private_key` signs decoded RGBA8 image
pixels through `opendoc.image.pixels.v0`. The signature excludes compression,
container metadata, and storage paths, so PNG/JPEG/WebP decoder adapters can
reuse the same semantic signature once they supply normalized pixels.

`doi` is optional document metadata. When present, local repository saves write a
DOI lookup alias alongside the UUID lookup record. DOI lookup is an acceleration
path only; the document UUID remains the canonical cross-reference identifier.
If the direct DOI alias index is absent, local and flat repository DOI open
paths fall back to scanning lookup records, matching the serverless bucket-scan
mode required when no central lookup service exists. DOI edits are represented
as source operations, so divergent candidate merges replay DOI metadata changes
instead of treating them as shell-only state.

`locale` is document-level source metadata, separate from citation-style locale
and spreadsheet workbook locale. `set_document_locale` trims and rejects empty
values at the app boundary, records a source operation, clears existing document
signatures, and participates in automatic candidate merges.

## Blocks

Each block contains:

- `id`
- `kind`
- `level`
- `ordered`
- `equation_source`
- `blob_hash`
- `alt_text`
- `content`
- `rows`
- `row_ids`
- `cell_ids`

Supported `kind` values:

- `paragraph`
- `heading`
- `list-item`
- `table`
- `equation-block`
- `image`
- `page-break`

Table rows are nested as `rows[row][cell][block]`. Nested blocks use the same
`AppBlock` shape as top-level blocks. `row_ids[row]` and
`cell_ids[row][cell]` expose stable table identities for operation-backed table
editing commands; older/debug projections may omit them and readers should
degrade gracefully.

Block and table editing commands:

- `set_document_title`
- `insert_paragraph_after`
- `split_paragraph_at_inline`
- `join_paragraph_with_previous`
- `delete_block`
- `set_block_text_style`
- `update_heading_level`
- `insert_equation_block_after`
- `insert_list_item_after`
- `update_list_item`
- `insert_page_break_after`
- `insert_table_after`
- `add_table_row`
- `delete_table_row`
- `add_table_cell`
- `delete_table_cell`
- `add_image_block`
- `insert_image_block_after`
- `update_image_alt_text`
- `update_image_blob_hash`
- `update_mention_label`
- `update_inline_equation_source`
- `update_block_equation_source`
- `insert_citation`
- `insert_citation_group`
- `insert_footnote_citation_group`
- `update_citation_group_items`
- `add_bibliography_reference`
- `update_bibliography_reference_metadata`
- `update_footnote_body`

Table block, row, and cell insert replay validates inserted table shape, row
shape, cell shape, and nested block payloads before mutation. Empty tables,
rows, cells, or malformed nested content degrade to deterministic warnings and
preserve the previous valid table state.

`set_document_title` changes the document source title through the operation
journal and rejects empty titles instead of normalizing them into signed source
state. `insert_paragraph_after` inserts a paragraph after a stable top-level
block ID, or appends when `afterBlockId` is null.
`split_paragraph_at_inline` inserts a new paragraph after the block containing
the target inline and moves that inline into the new paragraph without changing
the inline UUID; this is the v0 operation boundary for paragraph splitting so
formatting and comment anchors can survive automatic merge.
`join_paragraph_with_previous` moves all inline UUIDs from a top-level
paragraph into the immediately preceding top-level paragraph, preserving inline
identity and order, then deletes the now-empty source paragraph through the
same operation log. `delete_block` removes a
top-level or nested block from current source state through the same operation
journal used for collaborative merge and repository replay.
`set_block_text_style` converts an existing paragraph, heading, or list item
between normal text, heading levels, and list-item style while preserving the
stable block ID, inline IDs, comments, suggestions, and merge anchors.
`insert_list_item_after` inserts a list item after a stable top-level block
anchor and is the keyboard-list continuation command used by the GUI.
`insert_page_break_after` inserts a page-break block after a stable top-level
block anchor and is the keyboard slash-command path for page breaks in the GUI.
`insert_equation_block_after` inserts a TeX/LaTeX-like equation block after a
stable top-level block anchor and is the keyboard slash-command path for block
equations in the GUI.
`insert_table_after` inserts the default v0 table after a stable top-level
block anchor and is the keyboard slash-command path for table creation in the
GUI.
`add_heading`, `update_heading_level`, and heading-style conversion reject
levels outside `1..=6`. `add_list_item`, `update_list_item`, and list-style
conversion reject levels outside `0..=8`, matching the nine public Google Docs
API nesting levels. Merge replay degrades malformed stored heading, list, text
style, or inserted-block updates to deterministic warnings.
`update_link_href` changes the target URL for a stable link inline. `add_link`
rejects empty link text or target URLs at the app boundary. Empty link targets
in replayed operations degrade to `invalid-link-href` warnings during merge
replay.
`update_mention_label` changes the label for a stable mention inline through a
dedicated structured operation. `add_mention` and `update_mention_label` reject
empty labels at the app boundary; malformed replayed mention labels degrade to
`invalid-mention-label` warnings during merge replay.
`insert_inline_text` inserts a text inline into a stable block, either after
another stable inline ID or at the end of the block when `afterInlineId` is
null. Merge replay rejects malformed inserted inline payloads, including empty
link targets, empty mentions, empty equation source, and invalid mark values,
as deterministic warnings before source mutation. `delete_inline` removes a
text/link/mention/footnote/citation/equation
inline atom by stable inline ID. Comments and suggestions anchored to removed
content degrade through the merge repair rules rather than creating manual
conflicts.
Single-inline and range-level mark operations also validate mark payloads
before mutation, so missing valued-mark values or unexpected boolean-mark
values are rejected at the app boundary and degrade to `invalid-mark-value`
warnings during merge replay.
`add_equation`, `add_equation_block`, `insert_equation_block_after`,
`update_inline_equation_source`, and `update_block_equation_source` reject empty
TeX/LaTeX-like source at the app boundary. Generic `update_inline_text` does not
mutate equations, mentions, citations, or footnote references. Empty inline or
block equation source in replayed operations degrades to equation-source
warnings during merge replay.
Comment body and insert-suggestion content updates validate replacement inline
sequences before mutation; empty or malformed replacements are warnings and
preserve the previous valid comment/suggestion source state.
App command creation and update paths reject empty comment authors, comment
bodies, suggestion authors, and insert-suggestion content before adding source
operations. Delete and format suggestions share the same non-empty author
requirement.
Footnote, comment, and insert-suggestion bodies are semantically non-empty:
empty arrays and whitespace-only text bodies are invalid source. Replayed
malformed footnotes, comments, or suggestions degrade to warnings without
replacing the previous valid source state.
Accepting a corrupted insert or format suggestion payload does not apply the
corrupt edit. Replay sanitizes the retained suggestion audit record, marks it
rejected with `auto-rejected:invalid-accept-payload`, and preserves the live
document source state.
`add_image_block` requires an existing blob hash, stores the hash in
`blob_hash`, and stores user-facing source alt text in `alt_text`.
`update_image_alt_text` changes only that source alt text, so exact-byte blob
signatures remain attached to the unchanged content hash.
`update_image_blob_hash` requires another existing blob hash and changes only
the image block's source reference, so comments/history can keep using the same
block anchor. Missing referenced blob bytes are rendered as placeholders with
warnings on open rather than preventing the document from loading.
`insert_citation` creates a document-local citation group for an existing
bibliography reference and inserts a citation inline label after a stable inline
ID, or at the end of the document when no anchor is supplied.
`insert_citation_group` does the same for a multi-item citation occurrence,
validating every referenced bibliography record and storing one structured
citation group plus one inline label.
`insert_footnote_citation_group` stores a structured multi-item citation group
with `footnote` placement by stable footnote ID. It validates the live footnote
and every referenced bibliography record, renders citation labels from the
document-local bibliography database, and does not create an inline citation
label in the document body.
`update_citation_group_items` replaces the structured item list for a live
citation group while preserving its inline or footnote placement. It validates
that the replacement list is nonempty and references live bibliography records,
then rerenders dependent inline citation labels from the document-local
bibliography database.
`add_footnote_ref` creates both a stable inline footnote reference and a
document-local footnote body. `update_footnote_body` changes that source body
by stable footnote ID through the operation journal. Empty footnote bodies are
rejected at the app boundary and degrade to `invalid-footnote` warnings during
merge replay. Replayed footnote references whose target footnote is missing or
deleted are removed from current source with a
`footnote-reference-target-missing` warning, including references nested in
table cells.

## Inlines

Each inline contains:

- `id`
- `kind`
- `text`
- `href`
- `target_id`
- `marks`

Supported `kind` values:

- `text`
- `link`
- `citation`
- `footnote-ref`
- `mention`
- `equation`

Citation inline text is a rendered cache from the document-local citation
database. Citation edits must update the database and rerender dependent
citation groups instead of mutating citation label text directly.
Citation occurrence insertion is operation-backed through `insert_citation`
and `insert_citation_group`;
the older `add_citation` command remains a sample fixture helper.
Rendered citation labels and citation-group `rendered_cache` values are
projection data and are stripped from the normal source signature payload.

## Footnotes

Footnotes are document-local source records:

- `id`
- `revision`
- `body`
- `deleted`

The inline `footnote-ref` points to a footnote `id`. The body is signed source
state and is edited through `update_footnote_body`; the inline reference text is
a projection label.

## Comments And Suggestions

Comment threads contain:

- `id`
- `anchor`
- `comments`
- `deleted`

Suggestion records contain:

- `id`
- `author`
- `kind`
- `text`
- `state`
- `anchor`
- `range_start`
- `range_end`
- `marks`
- `content`
- `provenance`

Deleted comments stay in history and are hidden by normal views. Suggestions
support proposed, accepted, and rejected states.
`add_comment_reply` appends a stable-ID reply to one live comment thread as
signed source state. `update_comment` edits one live comment body as signed
source state.
`delete_comment_thread` marks an entire thread deleted.
`restore_comment_thread` reactivates a retained deleted thread and all comments
in it. `delete_comment` marks one comment deleted inside a live thread and
marks the thread deleted when no live comments remain. `restore_comment`
reactivates one retained deleted comment by stable thread and comment ID,
reopening the thread when needed. Delete commands retain the deleted source
state for audit/recovery.
`add_delete_suggestion` creates a proposed delete suggestion over a stable
inline ID. `add_format_suggestion` creates a proposed format suggestion over a
stable inline ID with one v0 mark payload. `update_suggestion` edits the
plain-text content of a proposed insert suggestion. Delete and format
suggestions currently expose empty `text` until range projection is implemented.
The app projection preserves insert anchors, structured insert inline content,
delete/format range endpoints, format mark labels, and provenance so
suggestions can round-trip through the API without losing signed source
anchors. Insert suggestion `text` remains the compact editable text projection
for simple UI controls; `content` is the source-shaped inline payload.
Unknown suggestion `kind` or `state` values are format errors at the app/API
boundary because falling back to insert/proposed would silently change signed
source semantics.
`accept_suggestion` applies proposed insert, delete, and format suggestions to
source state before marking them accepted; missing anchors or ranges degrade
with warnings. `reject_suggestion` records rejection provenance without changing
the referenced source content.

## Citations

The citation database contains:

- `style`
- `locale`
- `references`
- `bibliography`
- `citations`

References contain the document-local source bytes as UTF-8 text in `source`,
plus parsed summary fields:

- `id`
- `revision`
- `format`
- `source`
- `title`
- `authors`
- `issued`
- `doi`
- `url`
- `deleted`

The v0 preferred source format label is `citum-native`.
`bibliography` is a rendered projection list with `reference_id` and `text`.
It is regenerated from live references and citation style; it is not signed
source state.
Citation groups expose `placement`. Inline citations use `"inline"` and no
`footnote_id`; footnote citations use `"footnote"` plus `footnote_id` so the
document-local footnote anchor survives app/API and import/export round-trips.
`set_citation_style` updates signed source-state bibliography style/locale
metadata and rerenders non-deleted citation groups as projection/cache updates.
Empty style or locale input is rejected at the command boundary; malformed
replayed history degrades by preserving the current style/locale and adding an
`invalid-citation-style` warning.
The temporary renderer supports author-year labels for the default style family
and bracketed numeric labels for `numeric`/`ieee`; full `citum` rendering
remains the adapter target.
`update_bibliography_reference` edits a live reference and rerenders dependent
citation labels. `add_bibliography_reference` creates a document-local
`citum-native` reference from title, author list, issued year/date, DOI, and URL
summary fields, regenerating the stored source bytes from that signed source
summary. Empty titles or author lists are rejected at the command boundary.
`update_bibliography_reference_metadata` applies the same full-field validation
to a live reference, normalizes it to `citum-native` source, regenerates source
bytes, and rerenders dependent citation labels. The older
`update_bibliography_reference` command remains a title/issued compatibility
wrapper.
`delete_bibliography_reference` marks a live reference deleted
from current bibliography state, keeps it available in audit/recovery state,
and rerenders dependent citation labels with deterministic missing-reference
fallbacks rather than deleting citation occurrences.
`restore_bibliography_reference` reactivates a retained deleted reference,
rerenders dependent citation labels, records the restore as an upsert operation,
and persists through repository save/open. `update_citation_group_items`
updates locators, labels, prefixes, suffixes, suppress-author flags, and
multi-reference item composition as signed citation source state while keeping
rendered labels outside source signatures. `delete_citation_group`
marks a citation group deleted, hides it from normal citation-group views,
keeps it available in audit/recovery state, and causes inline citation labels
that still reference it to render a stable missing-group fallback.
`restore_citation_group` reactivates a retained deleted citation group,
rerenders its structured label, records the restore as an upsert operation, and
persists through repository save/open.

## Spreadsheet

The workbook projection contains sheets, stable row and column axes, display
row/column labels, signed named ranges, a formula dependency graph, and cells.
`set_spreadsheet_workbook_metadata` edits workbook `title`, `locale`, and
`timezone` together as signed source metadata. Empty fields are rejected before
source mutation, and candidate replay treats the metadata update as an atomic
spreadsheet operation. Malformed candidate metadata preserves the current
workbook metadata and emits `invalid-spreadsheet-workbook-metadata`.
`set_spreadsheet_cell` edits one cell on the current sheet and rerenders the
projection immediately. `set_spreadsheet_cell_in_sheet` does the same by stable
sheet ID. `set_spreadsheet_cells` and `set_spreadsheet_cells_in_sheet` accept an
array of cell edits, record the same replayable per-cell operations as
individual edits, and evaluate formulas once at the command boundary. These
commands exist for paste/import-style batches; batching changes persistence and
recalculation cost, not render semantics for already-applied local edits.
Sheet-scoped spreadsheet command inputs trim wrapper whitespace from `sheetId`
before sheet lookup and before recording replayable spreadsheet operations.
Candidate replay applies the same trim before using operation `sheet_id`
values; empty replayed sheet IDs emit `invalid-spreadsheet-sheet` and skip the
operation.
Malformed replayed cell addresses emit `invalid-spreadsheet-cell-address`
instead of creating invalid sparse cells.
`add_spreadsheet_sheet` allocates an opaque stable sheet ID; clients must read
the returned workbook state instead of deriving IDs from visible sheet order.
This prevents concurrent sheet additions from racing on the same ID.
`rename_spreadsheet_sheet` updates a sheet title by stable sheet ID without
changing cell, row, column, or named-range identities. Sheet add and rename
commands normalize blank titles before recording replayable operations, so
replay, merge, and save/open observe the same canonical sheet title as the
immediate projection.
`delete_spreadsheet_sheet` removes a sheet from current workbook state by stable
sheet ID, removes named ranges attached to that sheet, and leaves the operation
history intact for audit/recovery. Interactive commands reject deletion of the
last remaining sheet. Operation replay treats duplicate or no-longer-possible
sheet deletes as already resolved so candidate reconciliation still opens a
valid workbook.
`add_spreadsheet_row`, `delete_spreadsheet_row`, `restore_spreadsheet_row`,
`add_spreadsheet_column`, `delete_spreadsheet_column`, and
`restore_spreadsheet_column` edit the visible sparse grid axes by stable sheet
ID and label. Row and column deletion remove current cells on the deleted axis
and drop intersecting merges, filters, protected ranges, and named ranges, but
the delete operation retains a restore payload containing the axis metadata
plus those source records for audit/recovery. `restore_spreadsheet_row` and
`restore_spreadsheet_column` reactivate that payload when the sheet still
exists and the axis label has not already been reused; conflicting ranges are
skipped or rejected according to the same source validators as the normal add
commands. Direct formulas that still reference removed cells degrade through
normal deterministic formula errors; v0 does not shift formula source for
positional row/column insertion. Malformed replayed row and column labels on
add, delete, or restore operations emit `invalid-spreadsheet-row` or
`invalid-spreadsheet-column` instead of mutating workbook axes.
Candidate replay validates the workbook after each spreadsheet envelope. If an
operation is individually well-formed but would create invalid aggregate source,
such as a duplicate sheet title or another cross-object invariant failure,
replay restores the previous workbook and emits `invalid-spreadsheet-source`.
`add_spreadsheet_cell_comment`, `update_spreadsheet_cell_comment`, and
`delete_spreadsheet_cell_comment` store signed source-state comments on sparse
cells by stable comment ID. Adding a comment creates the cell if needed.
Deleting a cell comment marks it deleted for audit/recovery while normal GUI
rendering hides it. `restore_spreadsheet_cell_comment` reactivates a retained
deleted cell comment, records the restore as a spreadsheet operation, and
persists through save/open. Malformed replayed cell-comment add/update payloads
emit `invalid-spreadsheet-cell-comment` and preserve current cell comments.
`set_spreadsheet_frozen_axes` stores signed sheet viewport metadata by stable
sheet ID. Counts are clamped to the current visible grid dimensions so the
source state remains valid.
`set_spreadsheet_cell_validation` stores signed cell validation source state by
stable sheet ID and A1 address. v0 supports list-style validations with bounded
string values and a strict/warning flag; the schema keeps a validation `kind` so
number, text, range, and formula validation rules can be added without changing
the command shape. `clear_spreadsheet_cell_validation` removes the current cell
validation while the operation retains the full validation payload for
audit/recovery. `restore_spreadsheet_cell_validation` restores the retained
validation when the sheet and cell still exist without a current validation;
stale replay emits warnings instead of making the workbook unopenable.
Malformed replayed validation payloads preserve previous cell validation and emit
`invalid-spreadsheet-cell-validation`; validation operations targeting a missing
sheet emit `missing-spreadsheet-sheet`.
`merge_spreadsheet_cells` stores signed sheet-level merged range metadata by
stable sheet ID and A1 range. Single-cell merges and overlaps with current
merged ranges are rejected interactively; replay treats duplicate merge ranges
as already applied so candidate reconciliation remains deterministic. Invalid
replayed merge/filter/protected-range/copy-range/cell-format operations are
transactional: they preserve the previous workbook source and emit a specific
`invalid-spreadsheet-*` warning instead of leaving partially created cells.
`unmerge_spreadsheet_cells` removes the matching current merge range while the
operation retains the full merge payload for audit/recovery.
`restore_spreadsheet_merge` restores the retained merge when the sheet still
exists and the range does not already exist or overlap current merges; stale
replay emits warnings instead of making the workbook unopenable.
`set_spreadsheet_basic_filter` stores signed sheet-level basic filter metadata
by stable sheet ID and A1 range. v0 keeps one basic filter per sheet, matching
the Google Sheets `basicFilter` shape; setting a new filter replaces the current
one. `set_spreadsheet_basic_filter_options` records Google-shaped criteria and
sort specs on the existing filter. Criteria support `text_contains`,
`text_equals`, `number_greater`, `number_less`, and `number_equal`; criteria
and sorts must reference columns inside the filter range.
`clear_spreadsheet_basic_filter` removes current filter metadata while the
operation retains the full filter payload for audit/recovery.
`restore_spreadsheet_basic_filter` restores the retained range, criteria, and
sort specs when the sheet still exists and does not already have a current
filter; stale replay emits warnings instead of making the workbook unopenable.
`add_spreadsheet_protected_range` stores signed sheet-level protected range
metadata by stable sheet ID and A1 range. v0 treats protected ranges as
warning/provenance metadata, not permission enforcement, so local and
serverless editing remains openable and service permissions can be layered
outside the document format later. `delete_spreadsheet_protected_range` removes
the matching current metadata while history retains the full protected-range
payload for audit/recovery. `restore_spreadsheet_protected_range` restores the
retained warning/provenance metadata when the sheet still exists and the range
is not already present; stale replay emits warnings instead of failing open.
`update_spreadsheet_protected_range` edits an existing protected range's
description and warning-only flag without changing its range identity. App/API
updates for missing ranges fail closed; replayed stale updates emit
`missing-spreadsheet-protected-range`. Malformed protected range replay is
transactional and emits `invalid-spreadsheet-protected-range` without mutating
the sheet.
The workbook contains:

- `title`
- `locale`
- `timezone`
- `named_ranges`
- `dependency_graph`
- `sheets`

Workbook source validation rejects empty metadata, duplicate sheet IDs or
titles, named ranges pointing at missing sheets, duplicate named-range IDs or
normalized names, invalid visible row/column labels, corrupt axis metadata,
duplicate cells, overlapping merged ranges, duplicate filters/protected ranges,
invalid cell comments, invalid validations, and invalid cell formatting before
the workbook is exported or written as a canonical app snapshot.

Each named range contains:

- `id`
- `name`
- `sheet_id`
- `range`

Named ranges are workbook-level signed source state. They resolve to sheet-local
A1 ranges and can be referenced from formulas, including range functions such as
`SUM(QTY)`. The formula dependency graph expands named ranges into concrete cell
dependencies as projection metadata. Malformed replayed named-range names or
ranges emit `invalid-spreadsheet-named-range` without changing current named
ranges. `update_spreadsheet_named_range` edits an existing named range by
normalized name, can move it to another sheet/range, and re-evaluates formulas
that depend on the name. App/API calls for missing names fail closed; replayed
stale updates emit `missing-spreadsheet-named-range`.
`delete_spreadsheet_named_range` removes a named range by normalized name,
retains the deleted named-range source record in the operation payload for
audit/recovery, and re-evaluates the workbook. Formulas that still reference
the deleted name become deterministic error cells instead of silently retaining
stale computed values. `restore_spreadsheet_named_range` reactivates the
retained source record when its sheet still exists and the name has not already
been reused. Replayed deletes for missing named ranges emit
`missing-spreadsheet-named-range`; malformed delete/restore names or invalid
retained payloads emit `invalid-spreadsheet-named-range`.

Each dependency graph entry contains:

- `sheet_id`
- `address`
- `dependencies`
- `dependents`
- `invalidation_order`

The graph is deterministic projection metadata derived from formula source and
is not included in normal document-content signature payloads.

Each sheet contains:

- `id`
- `title`
- `frozen_rows`
- `frozen_columns`
- `merges`
- `filters`
- `protected_ranges`
- `row_axes`
- `column_axes`
- `rows`
- `columns`
- `cells`

Each axis contains:

- `id`
- `label`

Axis IDs are stable merge/projection metadata. They are not included in normal
document-content signature payloads.

Each sheet merge contains:

- `id`
- `range`

Merged ranges are signed source state. Deleting a row, column, or sheet removes
affected current merge metadata; the operation history remains the audit source.

Each sheet filter contains:

- `id`
- `range`
- `criteria`
- `sort_specs`

Basic filters are signed source state. v0 records filter ranges, criteria, and
sort specs. Deleting a row, column, or sheet removes affected current filter
metadata.

Each sheet protected range contains:

- `id`
- `range`
- `description`
- `warning_only`

Protected ranges are signed source state. v0 records warning-only metadata and
does not block edits. Deleting a row, column, or sheet removes affected current
protected-range metadata.

Each cell contains:

- `address`
- `user_kind`
- `user_value`
- `format`
- `validation`
- `computed_kind`
- `computed_value`
- `dependencies`

Formula source lives in `user_value` and is part of persisted/signed state.
Computed values and dependencies are deterministic projections.

Cell `format` is signed source state and contains:

- `bold`
- `italic`
- `text_color`
- `background_color`
- `horizontal_align`
- `number_format`

Cell `validation` is optional signed source state and contains:

- `kind`
- `values`
- `strict`
- `show_dropdown`

`copy_spreadsheet_range` copies a rectangular source range within one sheet to
a target top-left cell. It preserves cell format and validation metadata,
preserves fully contained merged ranges at the shifted target range, shifts
relative A1 formula references by the same row/column offset, extends row/column
axes, and regenerates computed values plus the dependency graph.

`import_google_sheets_json` supports spreadsheet properties, sheet grid
properties, cell `userEnteredValue`, basic `userEnteredFormat`, and
cell `dataValidation` for the selected validation subset, sheet `merges`, sheet
`basicFilter`, sheet `protectedRanges` as warning-only metadata, and
`namedRanges`. It aborts high-risk structures such as charts and pivot tables
until those have explicit schema support.
`export_google_sheets_json` emits the same constrained Google Sheets-shaped
subset.

## Signatures

`signature_state` is one of:

- `unsigned`
- `signed`
- `trusted`
- `untrusted`
- `broken`

The normal document projection uses `unsigned` or `signed` unless verification
has detected stronger trust information. `verify_current_signatures` verifies
stored document signatures using their embedded public keys and returns
`unsigned`, `signed`, or `broken`. `verify_current_signature` additionally uses
the supplied private key as the trust anchor: signatures by that key return
`trusted`; valid signatures by another embedded key return `untrusted`; tampered
payloads or invalid signatures return `broken`.

Repository opens verify manifest signature sidecar objects before exposing the
journal or signed state. Local disk and flat object-store layouts reject missing
or hash-mismatched signature objects with deterministic errors instead of
treating the document as unsigned.

Detached exact-byte blob signature sidecars are optional trust metadata keyed by
blob content hash. Corrupt sidecar bytes or a sidecar whose target does not
match the addressed blob hash do not prevent the document from opening. The app
emits `invalid-blob-signature-sidecar`, leaves that blob unsigned in the normal
projection, and keeps the authored source state recoverable.

`signature` is the first signature for legacy/simple UI display. `signatures`
contains all sidecar signature records:

- `target`
- `signer`
- `signer_display`
- `title`
- `signed_at_ms`

Blob `typed_signatures` contain:

- `source_blob`
- `profile`
- `semantic_digest`
- `included_fields`
- `excluded_fields`
- `signature_state`
- `signature`
- `signature_bytes`
- `profile_payload`

`profile_payload` is `null` for typed profiles that can be verified directly
from the available blob bytes. It contains canonical profile input bytes for
profiles that intentionally sign decoded semantics rather than the stored object
bytes, such as decoded image pixels.

Any document operation clears signatures in the current editable state.

## Operations

`operations` is the app-facing journal segment projection. Each record contains:

- `actor`
- `seq`
- `kind`
- `summary`
- `created_at_ms`

The v0 GUI uses operation records for audit display. Merge semantics live in
`opendoc-merge` operation types and typed app operation envelopes. Save writes
new replayable envelopes to deterministic CBOR operation segments chained from
the previous segment and manifest; open verifies segment hashes, document/branch
identity, and chain links before reconstructing the operation history.
Local disk and flat object-store repository opens reject missing, hash-mismatched,
or semantically invalid operation segment records before trusting the journal.

The browser/Tauri command contract treats mutable commands as operation-backed
unless they are repository maintenance, read/verify/export commands, or
warning-only no-ops. Document-replacement imports must leave a non-empty import
operation journal. Signing and archive-tombstone commands append audit records
without clearing the signature sidecars they create, because those sidecars are
not authored document source edits.
