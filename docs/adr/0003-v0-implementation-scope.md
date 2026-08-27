# ADR 0003: V0 Implementation Scope

Status: accepted for first implementation pass.

## Context

The research design now spans collaborative rich text, spreadsheets, binary object storage, signing, typed semantic signatures, S3-compatible storage, local offline mode, Google import/export, and cold archive lookup. V0 needs a narrower implementation scope so prototypes prove the architecture without expanding indefinitely.

## Decisions Recorded

- The first serious prototype optimizes for a robust Google Docs-style rich-text editing system with collaboration support.
- The first storage target is on-disk object storage. S3-compatible storage remains the long-term default target, but is not required for the first working prototype.
- Server-managed commits are not assumed for v0. Single-editor local usage is integrated first; multi-user behavior is validated through deterministic simulations and tests.
- Merge design is a top research risk. The implementation must not assume DOM/tree merging is correct until tested. Alternatives include state-machine/event models, paragraph-like blocks with invisible local UUIDs, and CRDT-native sequence/block identities.
- Use OpenDAL early if the local filesystem backend requires little configuration; otherwise start with a small local object-store trait and add OpenDAL as the first adapter.
- Deterministic CBOR is acceptable for research-stage manifests and metadata. Backward compatibility is not required until the project moves beyond research.
- Cross-platform signing must work on macOS, Windows, and Linux with minimal user-installed dependencies.
- Google Docs import is the first Google compatibility proof.
- Equation support should cover the kind of equations Google Docs supports, with a TeX/LaTeX-like representation as the likely internal authoring/interchange form.
- Lookup/index/archive rules should be soft and fail gracefully when stale, missing, or inconsistent.
- Tape/archive strategy remains a research task; performance matters within the constraints of local object storage and common S3-compatible stores. For HPC2N-like environments, assume generic archive locators first, with IBM Spectrum Protect/TSM and SweStore/dCache as concrete research targets.
- First prototype can be API/object-format/schema focused with tests, not a full UI.
- Paragraph/block UUIDs are acceptable as invisible durable structure, but they should not affect document-content signatures unless a signature profile explicitly includes structure IDs.
- Merge validation uses realistic synthetic scenarios plus fuzz testing.
- Import/conversion may use tools that are easy to install on Linux, macOS, and Windows. If necessary, Linux-only tools are acceptable if they do not require root access to install.
- Invisible block UUIDs are an implementation aid for merging and likely unrelated to import/export stability.
- Rendered PDF/output signatures are out of scope; signatures cover source document state and typed content, not rendered exports.
- Equations use one canonical store. Prefer a TeX/LaTeX-like source form and derive browser MathML/rendered output as needed.
- Comments and suggestions are essential v0 features and must be included in merge tests from the start.
- Local object storage must mitigate many-small-file overhead, while not letting offline mode overcomplicate the overall design.
- Users should never need to know about compaction.
- Deleted content is retained for now; garbage collection is deferred.
- Cross-document references should degrade gracefully with warnings when exact targets, latest heads, indexes, blobs, or archive data are unavailable.
- Merge should always converge to a valid document. Failure modes should degrade gracefully with warnings or review metadata, not block opening/editing.
- Suggestions should follow Google Docs-style track-changes semantics, but `@user` mentions are not required beyond syntax highlighting.
- Comments are signed as part of document state.
- Comments may be deleted from current state while retained in signed/history data.
- Anyone may sign a document version; no one is required to sign it.
- Suggestions do not require per-suggester signatures, but accepted suggestions should preserve attribution/history in signed metadata when available.
- Block UUIDs may be exposed in debug/export tooling if useful; they are not secret.
- Local pack files can start simple, but storage APIs must allow changing the local physical format later.
- `.doc` import may use external conversion tools for the first proof.
- `.doc` import should preserve comments and suggestions if the converter exposes them.
- Imported Google/internal IDs do not need to be preserved, but their design should be studied for lessons.
- Formula evaluation must be deterministic across platforms, even where edge cases differ from Google Sheets.
- Formula signatures cover formula source, not cached computed values.
- Formula recalculation may be lazy on view, eager in background, or hybrid, depending on performance.
- Open performance can be optimized later, as long as the design includes a strategy for snapshots, pack indexes, shallow loading, and compaction.
- Cross-document references default to latest branch with warnings, with an option to pin an exact manifest.
- “Official” or “published” status follows from presence and policy validation of signatures, not a manual flag.
- Signature UI state should support at least `unsigned`, `signed`, `trusted`, `untrusted`, and `broken`.
- A version may have multiple signatures.
- Signing covers current state plus retained history reachable from the manifest, not unrelated full historical material.
- Deleted comments are hidden by default and restorable/auditable when retained data is available; otherwise degrade gracefully.
- Accepted suggestions remain as provenance metadata, not visible track-change markup.
- Formula computed values are not stored durably; they may be cached in RAM.
- Imported `.doc` source files are not retained by default after conversion.
- Missing attachments/images are allowed in normal editing mode and shown as placeholders.
- First merge fuzzer operates at operation level.
- CLI commands are optional and should exist only when helpful for testing.
- Deleted text/comments are exposed only in audit or recovery views, not normal editing views.
- Signatures include user-visible metadata such as title, author name, and timestamps.
- V0 does not enforce collaborator permissions without a server; design the code so server-backed permissions can be added later.
- Comments support threads/replies in v0 without overcomplicating the model.
- Suggestions support formatting changes immediately, not only text insert/delete.
- Equations should support both inline and block equations; implementation order is left to prototype needs.
- Local maintenance may rewrite pack files while preserving logical object identities and verification.
- First import proof may target both `.doc` and `.docx` if tooling allows.
- Google Docs import proof means converting into our Google-Docs-shaped schema without requiring Google credentials.
- Passive viewers should be treated as potential editors in design and simulations where practical.
- Audit/recovery views require local access to retained history, not trusted signatures.
- Signer metadata can be self-declared, derived from OpenSSH allowed-signers, or both. OpenSSH identity is useful but not inherently trusted.
- Comments anchor to text ranges that reference stable UUID-backed document positions. Study the Google Docs API anchoring model for design ideas.
- If a comment anchor cannot resolve exactly, attach it to the nearest surviving block with a warning.
- Suggestions are allowed inside comments.
- Equations merge as atomic inline/block objects, not editable token streams.
- Repeat `.doc/.docx` imports do not need to reproduce the same block UUID structure.
- Passive viewers and active editors should use the same update path where practical.
- Permission metadata may exist as non-enforcing hints in local/raw modes if easy.
- Pack rewrite should be crash-safe: write a new pack, verify it, then atomically swap the pack index.
- Comment ranges may span multiple blocks, matching Google Docs-style behavior.
- Comments do not survive deletion of all referenced text by default.
- Suggestion discussion/thread behavior should follow Google Docs behavior where practical.
- Equation objects do not need display-text fallbacks in v0.
- Hash algorithm agility is required immediately.
- Object references may use whichever hash-reference syntax keeps code simplest, as long as the algorithm is explicit.
- Branch names are mostly internal, with `main` as the default.
- Local object storage supports multiple documents in one repository/bucket from day one.
- UUID lookup for deleted/archived documents is left to implementation judgment, but must degrade gracefully.
- Import failures abort import rather than creating partial documents.
- Spreadsheet v0 includes formula evaluation, not only formula storage.
- Image semantic signatures and FASTQ typed-content profiles are design constraints for the signing model. They do not necessarily need full production implementation in the first editor prototype, but the storage/signing architecture must not preclude them.
- Compatibility priority is Google Docs and Google Sheets API import/export. This is the primary proof that the document and spreadsheet schemas are correctly shaped.
- Expected collaboration scale is small: 1-3 active editors and up to 5 passive viewers.
- Storage performance should be optimized for local on-disk object layout and common S3-compatible object stores.
- Unsigned documents are openable normally. Signatures are a visual trust/compliance indicator, not a general access gate.
- Signing is mainly for 21 CFR style compliance, scientific fraud investigation, and patent precedence workflows.
- V0 signer and collaborator identity should reuse OpenSSH-style keys and signing workflows instead of inventing a new identity mechanism.

## Open Product-Priority Questions

- Exact editor frontend for the rich-text prototype.
- Exact merge representation for rich-text documents: tree/DOM projection, state-machine model, block UUID graph, CRDT-native sequence, or hybrid.
- Exact on-disk object backend abstraction after quick OpenDAL filesystem evaluation.
- Exact Rust-native OpenSSH-compatible signing crate and fallback optional `ssh-keygen` integration.
- Exact small-file mitigation strategy for local object storage.
- Whether pack files themselves are signed, or only the logical objects inside them are signed by hash.
- Exact graceful-degradation UI/API states for cross-document references.
- Exact future permission model boundaries for server-backed deployments.

## Implementation Implications

- Formula evaluation is now part of the critical path for spreadsheet prototypes.
- Google API import/export should be used as a design validation target before broader Word/Excel/PDF compatibility.
- Collaboration algorithms can target small active-editor counts first, but storage layout should still avoid per-keypress S3 objects and excessive object fanout.
- Typed semantic signatures need profile interfaces and test vectors early, even if only byte signatures are production-ready first.
- First implementation should provide local object store tests and simulated multi-editor merge tests before any real server deployment.
