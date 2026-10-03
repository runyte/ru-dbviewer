<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-003: SQLite CTE writes lose their affected-row counts

Status: resolved and independently reviewed.

The SQL parser represents `WITH ... INSERT/UPDATE/DELETE` as a query wrapping
a DML body. `query::affects_rows` recognizes only top-level DML statements, so
SQLite discards its authoritative `changes()` value for CTE writes. Pending
transaction metadata reports an unknown count even when the write affected a
known number of rows (including zero).

Resolution: recognize DML query bodies while keeping ordinary reads and DDL
excluded from SQLite's potentially stale affected-row counter.

Regression: run the same temporary-database contract on SQLite and PostgreSQL,
checking CTE INSERT, UPDATE, zero-row UPDATE and DELETE counts and committed
contents. Check that SQLite reads and DDL do not inherit stale counts.

Validation: the targeted SQLite regression and all six PostgreSQL suites passed
on Linux x86-64 against an isolated PostgreSQL 17.9 container, including the new
shared contract. An independent database module reviewer found no concerns.
