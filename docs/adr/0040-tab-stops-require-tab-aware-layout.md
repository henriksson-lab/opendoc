# ADR 0040: Tab stops require tab-aware layout

Custom tab stops are a durable paragraph property, not a UI-only ruler. A
future first slice may support explicit left stops only: an ordered, validated
set of positive leading-edge offsets relative to the paragraph content box.
The property is one atomic LWW value per block, so merge/undo/clear cannot
leave independently edited stop fragments with an invented order.

That property cannot be imported or exposed until layout treats a `\t` as a
tab advance: on every visual line it must select the next applicable explicit
stop (then the documented default-stop fallback), measure the resulting
advance, and include it in wrapping, paint, HTML projection, and PDF. RTL,
right/centre/decimal tabs, leaders, and tabs inside list/table geometry remain
outside the first slice; they must warn rather than be coerced to left tabs.

DOCX `w:tabs` and Google `paragraphStyle.tabStops` remain named dropped-input
warnings until that common layout rule exists. Once it does, import/export may
round-trip only explicit left stops; unsupported alignments/leaders still warn.
