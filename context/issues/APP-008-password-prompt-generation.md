<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-008: A stale PostgreSQL password prompt can reconnect a retired connection

Status: fixed; independent re-review clean.

Mode selection and confirmation capture/check the current connection generation,
but the following PostgreSQL password form stores only its profile and access
mode. If that connection is disconnected while the form remains outstanding,
submitting it starts a writable replacement using authority from the retired
connection. An initially disconnected prompt likewise must not replace a later
connection that it never captured.

Resolution: retain the optional current generation in each password input and
require the same connection ownership before connecting. The existing busy,
pending-transaction, and concurrent-attempt checks still apply afterward.

Regression: a disposable PostgreSQL public-wire test connects, opens a writable
mode password prompt, disconnects, and submits the old prompt. Rejection must
happen before a connection job is created. A separate PostgreSQL wire workflow
checks ordinary read-only execution, mode reconnection, writable review, and
explicit rollback.

Validation: both PostgreSQL wire cases passed against the temporary PostgreSQL
17.9 container fixture, alongside all ten PostgreSQL Rust suites (run by the
database review owner). The write test creates a temporary table and verifies
its absence in the same session after rollback. Ten existing PostgreSQL-form
feature variants, formatting, and Clippy pass on Linux x86-64. Final independent
fix re-review is recorded in the application review register.
