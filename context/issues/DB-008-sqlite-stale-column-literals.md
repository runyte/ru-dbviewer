<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-008: Stale SQLite browse columns silently become string literals

Confirmed in `Browse::compile`: projections and predicates use unqualified
double-quoted column names. SQLite's legacy double-quoted-string fallback treats
a missing column as a literal. After another connection drops a captured field,
refresh therefore fabricates cells containing its former name; filters may also
silently compare that literal.

Qualify every projected and filtered identifier with the captured table and
schema, just as sorting already does. A temporary SQLite fixture captures a
quoted field, drops it through a separate connection, and verifies that refresh,
bound filtering and independently executable generated SQL fail instead of
returning invented values.

Validation: the new SQLite regression failed before the fix because stale
refresh succeeded. After the fix all 15 SQLite database cases, three browse
unit tests and seven isolated PostgreSQL 17.9 cases passed on Linux x86-64.
