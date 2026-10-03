<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-007: PostgreSQL primary-key discovery includes non-key payload columns

Confirmed in `Database::browse_keys`: `unnest(pg_index.indkey)` returns key and
INCLUDE attributes, but the metadata query never limits ordinality to
`indnkeyatts`. A primary key including a JSON payload is valid PostgreSQL; the
generated browse order incorrectly adds that JSON field and fails because JSON
has no ordering operator.

Restrict returned attributes to actual key positions. A temporary PostgreSQL
table with `PRIMARY KEY(id) INCLUDE(payload)` and JSON payloads checks exact key
discovery and successful primary-key-ordered browsing.

Validation: on Linux x86-64 the isolated PostgreSQL 17.9 fixture first returned
`id,payload` as keys, failing the regression. After the fix all seven PostgreSQL
database suites passed, including successful JSON-payload browsing. Fixture DDL
uses the installed Rust PostgreSQL driver because the application's deliberately
limited SQL parser does not admit the INCLUDE syntax.
