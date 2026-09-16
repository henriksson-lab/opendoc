# ADR 0037: Comment reactions are document-local actor sets

Comment reactions are review metadata on a comment thread.  A reaction is an
emoji plus the sorted set of document actors who currently hold it.  The
operation says whether one actor is present, rather than toggling implicitly;
that makes replay and undo explicit and deterministic.

The core model accepts short visible emoji sequences and rejects whitespace,
controls, plain words, duplicate emoji entries, and duplicate actors.  Empty
reaction entries are removed by merge.  A joined service session binds the
reaction actor to its authenticated subject using the same rule as comment
authorship.  Local/offline documents retain their existing caller-asserted
author model.

This does not create reactions in an identity provider and does not imply
email, push, activity-feed, approval, or notification behaviour.  The Google
review extension round-trips the OpenDoc fields; native Google comments have
no equivalent in this bounded importer.
