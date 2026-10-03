<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-002: Malformed SQLite MATCH/REGEXP SQL panics during validation

- Status: resolved and independently reviewed.
- Severity: high (plugin process termination from ordinary SQL input).
- Affected source: `src/query.rs`, through sqlparser 0.59.0's SQLite dialect.

## Evidence

`SELECT 1 MATCH` and `SELECT 1 REGEXP` reach SQLiteDialect's custom infix
parser, which calls `parser.parse_expr().unwrap()` for the right operand.
Missing or malformed operands therefore panic instead of returning the generic
validation error. A public-wire reproduction terminated the plugin with exit
status 101. A malformed operand may also put sensitive SQL tokens in the panic
diagnostic because the unwrapped parser error contains input text.

Both `validate` and `affects_rows` use this dialect. Validation runs before SQL
execution, so a malformed SQL buffer can terminate a healthy plugin session.

## Resolution

Wrapped the locked SQLite dialect, forwarding its SQLite-specific behavior and
type identity while returning the infix operand's parser error normally. The
public validation error remains generic, valid MATCH/REGEXP support is preserved,
and both parser entry points use the safe dialect. Regressions cover malformed
operands, sensitive literals, and SQLite grammar compatibility.

## Validation

On Linux x86-64, `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1
cargo test --locked query::tests` passed all four query tests. The compatibility
test compares ASTs with the original dialect for valid MATCH/REGEXP, NOTNULL,
empty IN lists, comma LIMIT, quoted and Unicode identifiers, aggregate FILTER,
AUTOINCREMENT, descending keys, WITHOUT ROWID, REPLACE, INSERT OR REPLACE and
dollar placeholders. `rustfmt --check --edition 2024 src/query.rs` passed.
The independent database review confirmed all SQLite overrides and type identity
are preserved and both entry points avoid the original panic.
