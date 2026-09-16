# ADR 0039: Preconditioned paragraph-style suggestions

A paragraph-style proposal must name one stable block id, the source style the
author reviewed, and the proposed style. Its first vocabulary is deliberately
small: `Paragraph`, `Title`, `Subtitle`, and `Heading { level: 1..=6 }`.
List conversion is excluded because it also creates or splits durable
list-run/level state; it is not merely a block-kind change.

Acceptance finds that exact surviving text block and requires its current
style to equal the recorded source style. It then changes only `block.kind`.
It preserves the block id, current inline content, block properties, comments,
bookmarks, and any concurrent text edit. A missing/non-text target, invalid
style payload, or source-style mismatch auto-rejects with a named warning;
acceptance must never retarget a neighbouring block, turn a list into a
paragraph, or replace content through `BlockReplace`.

The app creates this proposal only for one selected eligible block and shows
the expected/proposed styles in review. The OpenDoc Google-shaped extension
round-trips it as `block_style_change`; native Google, DOCX, and ODT tracked
style-change interchange remains explicitly unsupported until each has its
own faithful mapping.
