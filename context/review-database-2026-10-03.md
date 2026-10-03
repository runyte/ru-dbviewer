<!-- SPDX-License-Identifier: MPL-2.0 -->
# Database and browse review, 2026-10-03

Scope: `src/db/mod.rs`, `src/db/sqlite.rs`, `src/db/postgres.rs`,
`src/browse.rs`, `src/query.rs` (final independent review), and
`tests/databases.rs`. Read the repository guide, README, PLAN and validation
register. Other reviewers own application and result storage work. Earlier database/query fixes
DB-001–004 were implemented by the earlier database review lead; final review
includes their current behavior. The initial database lead and a later coordinating lead encountered platform
content restrictions. Their completed changes were preserved. The root and
supporting reviewer own the remaining ordinary corrections and final validation.

## Confirmed findings

- DB-001: SQLite system-catalog filtering used LIKE underscore as a wildcard,
  hiding user names such as sqliteXaudit. Fixed and committed; exact prefix
  filtering and actual SQLite catalog fixtures cover it.
- DB-002: SQLite parser MATCH/REGEXP hooks unwrapped malformed right operands.
  Fixed and committed with a checked dialect wrapper and redacted errors.
- DB-003: SQLite CTE DML discarded authoritative affected-row counts. Fixed and
  committed with a shared SQLite/PostgreSQL contract, including zero-row writes.
- DB-004: The byte-only SQL limit admitted oversized recursive ASTs. A lexical
  token limit was added and committed. Final independent query review found a
  remaining unguarded MATCH/REGEXP left-AST clone path; DB-016 resolved it.
- DB-005: SQLite schema omits generated columns. Fixed and committed. A temporary
  SQLite regression failed before the fix and passed afterward.
- DB-006: PostgreSQL refreshed/bound browse displays all field types as text.
  Fixed and committed. Shared SQLite/PostgreSQL regression preserves captured
  types; PostgreSQL failed before the fix and passed afterward.
- DB-007: PostgreSQL primary-key INCLUDE fields incorrectly become sort keys.
  Fixed and committed. An isolated JSON payload fixture failed key discovery
  before the fix and browses successfully after it.
- DB-008: SQLite stale projected/filter columns become string literals through
  DQS fallback. Fixed and committed. External DROP COLUMN now causes errors
  for refresh, filtering and generated SQL.
- DB-009: Clipped catalog/key previews are reused as identifiers, and key result
  truncation is ignored. Fixed and committed. All 16 SQLite
  cases passed, including oversized identifiers and a 1,001-column key.
- DB-010: Scalar numeric classification matched substrings in point, interval,
  arrays and ranges. Fixed and committed. All eight PostgreSQL
  17.9 suites and SQLite's declared-type regression passed after the correction.
- DB-011: PostgreSQL retirement relied on asynchronous task-abort completion.
  Fixed and committed. The deterministic immediate-usability
  test failed before the fix; all nine PostgreSQL 17.9 suites passed afterward.
  Cancellation, timeout, read cap and writable cap all reject immediate reuse.
- DB-012: The connection deadline omitted TLS configuration and session SET.
  Fixed and committed. A real-server proxy first exceeded the outer guard, then
  verified the internal deadline and closure after the fix.
- DB-013: Custom TLS file opens and PEM reads were unbounded. Fixed and committed.
  A temporary FIFO reproduced the blocking read; regular-file and 4 MiB checks
  now reject it and oversized input while preserving symlinked CA acceptance.
- DB-014: The first row beyond MAX_ROWS unnecessarily captured full values.
  Fixed and committed. A shared SQLite/PostgreSQL fixture reduced retained spool
  use from 1,118,576 bytes to 70,000 bytes without changing retained results.
- DB-015: SQLite implicit FK targets lost their referenced table name through
  NULL concatenation. Fixed and committed. Both implicit and explicit target
  regressions passed after reproducing the original NULL definition.

- DB-016: SQLite validation recursively cloned left expression trees. Fixed and
  committed with a private classification-only placeholder; original captured SQL
  remains the execution source. Seven query tests passed and independent query
  re-review found no remaining finding.
- DB-017: Temporary SQLite objects shadowed captured main-table metadata. Fixed
  and committed by passing the captured schema to PRAGMA functions and qualifying
  the index catalog. The real adapter regression failed before and passed after;
  independent whole-module re-review is clean.
- DB-018: Rejected byte-cap rows retained unnecessary captured full values.
  Fixed and committed by the supporting reviewer; seven storage tests, the
  SQLite regression and both PostgreSQL result protocols passed. Root independently
  reviewed prefix lifetime, cleanup ordering and accounting; the PostgreSQL
  module reviewer also re-reviewed shared cleanup integration clean.
- DB-019: Cancelled PostgreSQL work retained its response stream during cleanup.
  Fixed and committed by placing the owned work future in a lexical scope that
  ends before cleanup. Parent source review was clean; the PostgreSQL reviewer
  completed a clean whole-module re-review and passed all thirteen database
  fixture cases plus both PostgreSQL wire cases on PostgreSQL 17.9.

## Review checks and deliberate limits

- TLS worker waiting is bounded by the connection deadline. Tokio cannot
  forcibly stop an already running blocking filesystem operation. Custom PEM
  inputs are bounded as recorded in DB-013; platform native roots remain loaded
  by rustls-native-certs.
- Restored PostgreSQL type metadata remains the originally captured browse
  metadata after external type changes, as do filter types. This is not a new
  regression introduced by DB-006; potential schema-change behavior requires
  separate evidence and design before adding metadata queries on each browse.

## Checks without further findings so far

Browse ordinal validation, enabled/disabled filters, ALL/ANY joins, decimal
literal binding, escaped identifiers and generated literal rendering were read.
Sort ordinals are validated before key comparison; pages saturate rather than
overflow; row/page/filter/selection limits remain enforced. SQLite open flags
require existing files, and metadata authorizer changes admit only read-only
metadata pragmas. Error adapters do not echo SQL values or connection URLs.
No additional confirmed issue in those paths at this point.

## Actual validation

Linux x86-64 only. After DB-014, all 18 SQLite database tests, all 12 PostgreSQL
database cases and both PostgreSQL public-wire tests passed. DB-015 then passed
both SQLite schema regression tests. All three browse unit tests also passed
after DB-008. PostgreSQL was an
isolated Docker `postgres:17` image reporting PostgreSQL 17.9; fixture data,
certificates and socket directories were temporary and removed afterward.
The PostgreSQL suites include TLS/hostname validation, client certificates,
Unix sockets, cancellation and lost-commit acknowledgement. No macOS or ARM64
acceptance is claimed. Full final checks and combined coverage belong to the
coordinating review and are not represented by these targeted runs.

## Final independent module review

| Module | Dedicated child | Result |
| --- | --- | --- |
| `src/browse.rs` | `browse_final_review` | Clean: identifiers, classification, filters, bound values, literals, ordering and paging. |
| `src/query.rs` | `query_final_review` | Found DB-016; final source re-review clean after correction. |
| `src/db/mod.rs` | `db_mod_final_review` | Found DB-017; whole-module re-review clean after correction, including metadata fixes DB-005/006/007/009/015. |
| `src/db/sqlite.rs` | `sqlite_final_review` | Found DB-018; supporting fix and regressions passed, root independently re-reviewed cleanup, and shared PostgreSQL/storage integration was re-reviewed clean. |
| `src/db/postgres.rs` | `postgres_final_review` | Found DB-019; whole-module re-review clean after correction, including DB-018 capture cleanup. |

All five modules received dedicated nested reviews. Their confirmed findings
are resolved with passing targeted validation and subsequent review. No known
actionable finding remains open. Final aggregate execution evidence belongs to
[the coordinating report](review-2026-10-03.md).
