# ADR 0032: Structural paragraph suggestion identity

Tracked structural inserts and replacements retain canonical `Block` payloads
and stable target/sibling identities. `BlockInsert` is rejected if its
`Before`/`After` sibling disappears; it must never inherit ordinary insertion's
append fallback. `BlockReplace` replaces its exact target in its current
container rather than deleting then reinserting it elsewhere.

The first payload is deliberately a plain `Paragraph` containing only text.
Tables, images and rich inline content have separate resource, child-identity
and review semantics, so treating them as a paragraph proposal would imply
fidelity this model cannot yet provide. The Google-shaped OpenDoc extension
round-trips `block_insert` and `block_replace`; native Google, DOCX and ODT
tracked structural interchange remains explicitly unsupported.
