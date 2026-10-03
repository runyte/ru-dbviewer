<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-006: Returning to a complete document loses metadata and duplicates text

Status: fixed; independent re-review clean.

Initial full-value publication uses `Document::model`, but browser navigation
rebuilds the same document through generic content presentation. Returning from
an SQL buffer removes the JSON/escaped-text title suffix, complete-byte metadata,
and descriptive raw/format action label even though the captured value is intact.
Navigation also retains the rebuilt JSON model, duplicating up to 8 MiB of
document text that initial completion deliberately stores only in `Document`.

Resolution: render already loaded documents through the same negotiated
document model builder and keep only lightweight model identity in page history.

Regression: compare document models before and after an SQL round trip, both
for a large plain-text document and formatted/raw JSON. Existing cancellation
and navigation regressions continue to cover staged publication.

Validation: the nested browser reviewer reproduced the lost title suffix,
byte metadata, and action label using temporary SQLite fixtures. Both SQL-return
regressions fail before the fix and pass afterward. All 12 full-value wire cases
pass on Linux x86-64, including cancellation, rejected staging, raw toggles,
large retained documents, and transaction independence. Final re-review is
recorded in the application review register.
