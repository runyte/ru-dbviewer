# CORE-002: Small table previews repeatedly format complete retained cells

Status: fixed; independent re-review clean.

`views::Content::model` formats each complete retained cell (up to 64 KiB) before
clipping its visible table preview to 512 bytes. When publication must shrink
previews, each retry repeats that work. Record rows similarly format complete
cells before their 2,000-byte display cap. This performs allocations and Unicode
escaping synchronously on the application loop for text that cannot be shown.

Add bounded cell formatting preserving the existing visible prefix and ellipsis,
and reuse the initial 512-byte cell previews when shrinking row budgets. Keep
all retained rows, column identities, original values and full-value sources.
Use allocation-volume regressions, Unicode/control boundary comparisons, and
existing large-page public-wire acceptance to validate the correction.

The checked-in allocation-volume regression failed before the correction at
**7,707,319 allocated bytes** to publish seven rows of eight 64 KiB cells. It now
passes at **397,975 bytes**, a 94.8% reduction in cumulative allocation volume
for this fixture (not a process-RSS or latency measurement). All three formatting
integration tests pass, including byte-for-byte comparisons with the previous
display/clipping behavior at Unicode and control boundaries, NULL, empty text,
truncated retained values, and limits from zero through 512 KiB.

All ten existing large-page and 1,000-row interactive cases pass across legacy,
row-action, presentation, help and native-path-completion feature profiles
(17.824 seconds, Linux x86-64, disposable SQLite fixtures).
