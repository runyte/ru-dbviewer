<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-009: Clipped metadata previews are reused as database identities

Confirmed in `Database::catalog` and `Database::browse_keys`: both reconstruct
identifiers from `Cell.text`, which is a preview capped at 64 KiB. SQLite permits
larger table and column names. Catalog navigation and primary-key ordering thus
use a different name than the database owns. Key discovery also ignores its
1,000-row result cap, silently dropping part of unusually wide composite keys.

Refuse clipped catalog identities and incomplete key metadata before producing
navigation or SQL. Ordinary result previews retain their existing behavior.
Temporary SQLite fixtures cover an oversized table name, an oversized key name,
and a 1,001-column composite key. This retains bounded metadata instead of
silently inventing identities or weakening ordering.

Validation: the temporary SQLite catalog regression failed before the fix. After
the fix, all 16 SQLite database cases passed on Linux x86-64, including oversized
table/key identities and the 1,001-column key case.
