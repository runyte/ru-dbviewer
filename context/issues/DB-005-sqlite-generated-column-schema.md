<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-005: SQLite schema inspection omits generated columns

Confirmed from `Database::schema` in `src/db/mod.rs`: its `pragma_table_info`
query omits both virtual and stored generated columns. Browsing `SELECT *`
returns these fields, so the schema view disagrees with the record view for an
ordinary table with generated columns.

Use `pragma_table_xinfo` for schema inspection and admit that read-only metadata
pragma in the SQLite authorizer. The regression fixture contains ordinary,
virtual generated and stored generated fields and checks their names, declared
types and values through the actual adapter. All database files are temporary.

Validation: on Linux x86-64 the regression first failed with only the ordinary
column returned, then passed after the metadata query and authorizer fix.
