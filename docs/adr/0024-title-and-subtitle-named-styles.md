# ADR 0024: Title and Subtitle are named non-outline block styles

Google Docs and Word both distinguish `TITLE`/`SUBTITLE` from numbered heading
styles.  They are large display paragraphs, but they do not create document
outline entries.  Treating them as Heading 1 and Heading 2 changed that
meaning, made them appear in the outline, and prevented a faithful export.

OpenDoc therefore represents the two styles as `BlockKind::Title` and
`BlockKind::Subtitle`, with matching `BlockTextStyle` operations.  They have
their own layout scale and HTML classes, but render as paragraphs rather than
heading tags.  This keeps accessibility and the document outline honest.

Google JSON maps them exactly to `TITLE` and `SUBTITLE`; DOCX maps them to the
standard `Title` and `Subtitle` paragraph styles; and ODT writes named
paragraph styles of the same names.  Other named styles remain intentionally
unmodelled rather than becoming arbitrary style-name strings.
