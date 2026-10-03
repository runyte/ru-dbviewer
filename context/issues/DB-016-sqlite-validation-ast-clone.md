<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-016: SQLite validation still recursively clones admitted deep expressions

Confirmed by independent source and existing-debug-binary inspection:
`CheckedSqliteDialect::parse_infix` recursively clones its entire left expression
for MATCH/REGEXP. A long arithmetic expression followed by MATCH fits the
16,384-token admission bound but has thousands of left-associated AST nodes.
The compiled derived `Expr::clone` frame is 4,904 bytes, making the admitted
8,000-node example require far more than ordinary worker or main stack limits.
The earlier DB-004 boundary regression covered construction/drop, not this clone.
No crash/stress probe was run for this finding.

The parsed AST is private to statement validation and top-level affected-row
classification; execution always uses the original captured SQL. The hook now
uses a constant left placeholder while preserving BinaryOp shape, operator and
fully parsed right operand. Independent design review verified that the pinned
parser never uses this discarded left subtree for SQLite admission. Both the
dialect and private helper explicitly prohibit AST rendering/execution.

Validation: all seven query unit tests passed on Linux x86-64 after the fix,
including both operators at the exact token boundary, ordinary SQLite grammar,
redacted malformed operands and CTE classification. Non-operator examples retain
their exact original-dialect AST equality checks. The dedicated independent query reviewer completed a clean source re-review;
no remaining actionable finding was reported.
