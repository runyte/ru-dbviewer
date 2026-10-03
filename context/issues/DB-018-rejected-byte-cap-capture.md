<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-018: Byte-cap rejection retains full values from discarded rows

Status: fixed; independent coordinator review clean.

Confirmed by independent SQLite adapter review: full values are captured before
`Data::push` checks its 4 MiB preview budget. Even after DB-014 prevents capture
of row 1,001, a row rejected by the byte cap can append large values to a spool
shared by earlier retained rows. Those unreachable bytes and quota reservations
remain until the captured result is released. This is bounded waste, not a
quota escape; the same control flow exists in PostgreSQL.

Reproduction fixture design: 64 one-column rows, with a 70,000-byte first value,
62 intermediate 65,536-byte values, and an 8 MiB final value. The final preview
exceeds the result budget, but its full value was already appended. Only the
first 70,000 captured bytes should remain reserved.

Each adapter now checkpoints the capture before converting a row. When
`Data::push` rejects and drops that row, the capture discards only the appended
suffix. An existing spool is truncated under its file mutex before releasing
the corresponding byte reservation; previously captured immutable ranges remain
valid. A spool created solely for the rejected row is closed and both its file
and byte reservations released. Failed truncation preserves the reservation,
and successful truncation never clears a previous failed-write state.

SQLite performs cleanup within its existing blocking worker. Both PostgreSQL
result protocols offload rejection cleanup to a blocking worker before returning
the capped result. This correction reclaims unreachable retained storage; it
does not avoid converting or initially writing a byte-cap-rejected row.

Storage regressions cover preserved earlier values, exact file/quota truncation,
subsequent appends, new-spool release, failed writes and failed truncation.
The shared database regression accepts 63 rows, rejects the final 8 MiB value,
checks the exact 70,000-byte retained reservation, and verifies the earlier full
value still loads after the original result is dropped. The PostgreSQL case
checks both simple-query and bound-text protocols with separate connections.

Validation on Linux x86-64: all seven storage tests passed (0.03 seconds), the
new SQLite byte-cap case passed (0.02 seconds), and the new PostgreSQL case passed
against the disposable PostgreSQL 17.9 container fixture (0.17 seconds). The same
fixture's two ordinary PostgreSQL public-wire cases also passed (2.722 seconds).
Only the new PostgreSQL Rust case was selected for this targeted run; it is not a
claim of a new full database-suite pass. The fixture removed its container and
temporary files. No personal databases/configuration or generated executable was
used. The coordinating reviewer independently checked both adapter paths,
suffix-drop ordering, immutable prefixes, accounting after failed I/O and
blocking-worker placement; no remaining actionable issue was found.
