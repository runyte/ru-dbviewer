<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-006: PostgreSQL refresh replaces original column types with text

Confirmed in `Browse::compile` and `Database::browse_with`: once the initial
browse has captured columns, the generated PostgreSQL projection casts every
column to text for bound-filter decoding. The resulting `Data.columns` then
reports `text` for numeric, JSON and other fields, including in record inspection.
An ordinary refresh or next page changes the displayed types without a schema
change.

Preserve the captured browse column metadata alongside the text transport
projection. Extend the shared database browse contract to compare types after
refresh and after a numeric bound filter, so SQLite and PostgreSQL both exercise
the invariant on temporary fixtures.

Validation: the isolated PostgreSQL 17.9 fixture on Linux x86-64 first reproduced
`text,text,text` instead of `int4,text,numeric`; all six PostgreSQL database
suites passed after the fix. The shared SQLite browse regression also passed.
