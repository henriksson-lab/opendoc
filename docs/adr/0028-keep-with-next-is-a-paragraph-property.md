# ADR 0028: Keep-with-next is an inheritable paragraph property

Status: accepted.

## Context

Page breaking is owned by `opendoc-layout`, not by a browser's incidental CSS
pagination. Google Docs calls the relationship `keepWithNext`; Word calls it
`w:keepNext`; ODF calls it `fo:keep-with-next`. It is neither an instruction
on the following block nor a document-global style: a heading, caption, or
ordinary paragraph says that *it* should not be separated from its direct
following sibling.

## Decision

`BlockProperties::keep_with_next: Option<bool>` is a typed source property.
`None` inherits, while both `Some(true)` and `Some(false)` are durable: the
latter can override a value supplied by an imported style. `SetBlockProperty`
and `ClearBlockProperty` therefore retain their existing per-property LWW and
inverse rules without inventing a second merge operation.

Before placing a requested block, layout measures whether the block plus its
following fragment fits in the current content box, including collapsed
margins. If it does not and the requested block is not already the first item
on that page, the pair opens the next page. An over-tall pair is not retried
forever: the normal overflow rule remains visible and deterministic.

The property projects to CSS `break-after: avoid-page`, Google
`keepWithNext`, DOCX `w:keepNext`, and ODF `fo:keep-with-next`. PDF uses the
same layout result as screen placement.

## Consequences

This is intentionally only the one relationship. Keep-lines-together and
widow/orphan control require line-fragment pagination rather than merely
looking ahead one block, so they remain separate future model work. Tabs,
paragraph borders, and shading likewise remain distinct geometry/painting
features rather than fields smuggled into this boolean.
