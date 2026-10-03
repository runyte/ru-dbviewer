<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-017: Temporary SQLite tables shadow captured main-table metadata

Confirmed independently with an isolated installed-SQLite fixture: the metadata
queries pass only table names to table-valued PRAGMAs. SQLite resolves a
same-named temporary table first, even though the captured table identity names
the main schema. Allowed CREATE TEMP TABLE SQL therefore changes key discovery
for a different main table: `temp_id` from the temporary table is used to order
`main.items`, whose actual primary key is `id`. Browsing fails and schema output
mixes temporary columns with main-table indexes.

Pass the captured schema as the second argument to each metadata PRAGMA and
qualify the index catalog. A real adapter regression should create temporary
and main tables with the same name but different keys, columns and constraints,
then check both captured identities independently.

Validation: the temporary SQLite adapter regression failed before the fix with
`temp_id` returned for `main.items`, and passed after schema qualification. It
checks both captured schemas, their keys, columns, indexes, foreign keys and
browse results. Final independent source re-review is pending.
