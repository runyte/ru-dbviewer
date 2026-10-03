<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-011: PostgreSQL retirement is briefly reported as usable

Confirmed from the cancellation/truncation return path: it requests
`JoinHandle::abort`, then `usable()` tests `is_finished()` and client closure.
Aborting a Tokio task completes asynchronously, so immediately after a capped
result the application can retain a connection already scheduled for retirement.
Its next action then encounters a closed connection instead of requiring
explicit reconnection at completion.

Track retirement synchronously and reject new executions after retirement.
A current-thread PostgreSQL regression checks usability before yielding after
row-cap completion, writable cap rollback, cancellation and timeout.

Validation: the isolated PostgreSQL 17.9 current-thread regression first failed
because a capped connection remained usable immediately on return. After the
fix all nine PostgreSQL suites passed on Linux x86-64, including all four
retirement paths and refusal of subsequent execution.

The timeout fixture uses `pg_sleep(5)` with a one-second deadline, ensuring
actual timer interruption. An initial zero-second case was ambiguous: the
capture guard could report expiry before a cancel packet was sent, correctly
leaving a connection reusable after rollback. Cancellation and both row-cap
fixtures retain their original behavior.
