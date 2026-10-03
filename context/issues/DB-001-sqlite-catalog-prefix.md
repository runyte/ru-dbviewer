<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-001: SQLite catalog hides ordinary table names

Status: resolved and independently reviewed.

`Database::catalog(false)` excludes names using `LIKE 'sqlite_%'`. In SQL LIKE,
the underscore matches any character, so valid user tables such as
`sqliteXaudit` disappear from the catalog. SQLite reserves the literal prefix
`sqlite_`; the catalog must exclude that prefix only.

Resolution: compare the literal prefix, preserving ordinary user names while
keeping SQLite internal objects hidden unless system objects are requested.

Regression: create a temporary database with `sqliteXaudit` and an AUTOINCREMENT
table; verify the normal catalog includes the user table and excludes
`sqlite_sequence`, while the system catalog includes both.

Validation: Linux x86-64 targeted `sqlite_catalog_preserves_user_names_resembling_system_prefix`
integration test passed. The independent SQLite module reviewer found no issues
in the fix or temporary fixture. Full suite results belong in the session report.
