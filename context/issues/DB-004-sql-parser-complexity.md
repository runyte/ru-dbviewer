<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-004: Long flat SQL expressions abort the plugin

Status: fixed and tested; independent re-review clean.

A public-wire run of `SELECT ` followed by 131,000 `1` operands joined by `+`
contains 262,006 bytes, within the documented 256 KiB input limit, but aborts
the compiled plugin with SIGABRT and `thread 'main' has overflowed its stack`.
The parser's nesting guard does not protect a long left-associated expression
tree and its recursive destruction. This ordinary SQL buffer terminates all
connections and work in the process before database execution.

Resolution: impose a lexical complexity budget before building the SQL AST,
using the selected dialect's tokenizer so literal contents and comments do not
count as operators or words. Apply the same guarded parser path to validation
and SQLite affected-row classification. Preserve the existing byte limit.

Regression: reject over-budget SQL before job admission through the public
wire and prove subsequent SQL works on the same association; unit-test both
dialects, structural limits, near-limit single literals/comments, and redacted
tokenizer errors.

Validation: all six query unit tests and the compiled-plugin public-wire
admission regression passed on Linux x86-64. The regression uses a bounded
projection list to verify the admission policy.

Independent review confirmed that the explicit tokenizer/parser path preserves
sqlparser 0.59's default dialect, unescaping, locations and parser options below
the new budget. Comment tokens are whitespace and do not consume the structural
budget; byte/NUL admission and constant diagnostics remain intact. The documented
16,384-token limit is reasonable for interactive queries while allowing large
single literals and comments. Both validation and affected-row classification
use the same guarded construction path. The reviewer independently reran all six
query unit tests (0.26 seconds) and the focused public-wire admission/recovery
case (0.616 seconds), with no findings. This is Linux x86-64 evidence; no separate
macOS boundary run or previous failure reproduction was performed.
