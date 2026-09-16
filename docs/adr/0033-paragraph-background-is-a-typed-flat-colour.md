# ADR 0033: Paragraph background is a typed flat colour

Paragraph shading is a `BlockProperty::Background(Color)`, with one LWW slot
per block. `None` means inherit; an explicit colour is canonical opaque sRGB.
This keeps concurrent paragraph painting and inverse/clear operations inside
the established typed block-property merge rule.

DOCX `w:shd` is imported and exported only for flat `clear`/`nil` fills.
Patterns, theme indirection and foreground/background combinations are warned
and not guessed. Google Docs' opaque RGB paragraph shading is native. ODF and
HTML receive their exact flat background spellings.

Paragraph borders, patterned shading and page-sensitive keep-lines/widows are
different features and remain outside this ADR.
