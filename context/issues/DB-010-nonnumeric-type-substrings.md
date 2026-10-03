<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-010: Nonnumeric PostgreSQL types get numeric-only filters

Confirmed in `browse::numeric`: substring tests for `int`, `real`, etc. classify
PostgreSQL `point`, `interval`, `_int4` and `int4range` as scalar numbers. The UI
omits contains and rejects their ordinary server text values; numeric comparisons
can generate operators PostgreSQL cannot apply to those types.

Recognize known scalar numeric names and their declared precision/scale forms.
Unknown/domain/non-scalar names keep text comparisons. Cover actual PostgreSQL
point, interval, array and range fields, and preserve numeric comparisons for
SQLite INTEGER, DOUBLE PRECISION and DECIMAL declarations.

Validation: the isolated PostgreSQL 17.9 fixture failed before the fix because
`point` omitted the contains operator. After the fix all eight PostgreSQL cases
passed, including equality and contains for all four affected types. The SQLite
INTEGER/DOUBLE PRECISION/DECIMAL regression also passed. Linux x86-64 only.
