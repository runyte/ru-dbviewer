<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-005: The reconnecting browser cannot cancel its own connection attempt

Status: fixed; independent re-review pending.

A mode change creates a loading catalog that inherits the old connection's
generation, then retires that connection while opening its replacement.
`::db-cancel` resolves the loading page through the old generation and rejects
it before consulting the pending-connection map. The displayed connection
attempt therefore cannot be cancelled through its documented command.

Resolution: retain the loading page identity with each connection attempt and
allow cancellation of that exact captured attempt. Preserve all generation
checks for other actions and for unrelated retained pages.

Regression: queue `db-cancel` from the loading catalog before replying to the
reconnection job's creation request. The original executable rejects it with
an unchanged page revision. A completed publication may legitimately stale an
already queued invocation; otherwise cancellation must reach the captured job.

Validation: independently reproduced with the public interactive harness and
a disposable SQLite database. After rebuilding, all five public-wire feature
variants pass on Linux x86-64. The regression permits a genuinely stale
invocation only after its captured page revision has changed. Final re-review
is recorded in the application review register.
