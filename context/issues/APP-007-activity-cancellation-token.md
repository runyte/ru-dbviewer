<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-007: Activity cancellation does not independently cancel running SQL

Status: fixed; independent re-review clean.

Writable execution registers the activity lease's cancellation token but drops
the returned handle. Its SQL worker observes a different job token. Although
the protocol reader immediately cancels the lease token, SQL continues until
the application actor can process the queued activity event. An unrelated host
RPC can therefore delay cancellation, and early activity cancellation needlessly
admits work before that event is handled.

Resolution: register the activity and SQL job with the same cancellation token,
preserving protocol handling of early cancellation. Reject an already-cancelled
lease before admitting an SQL job. This needs no watcher tasks or added queues.

Regression: exercise early and late cancellation through shared tracked handles,
including handle release, and send activity cancellation before the acquisition
response in a public-wire writable operation. No SQL job or database change may
be admitted after that early cancellation.

Validation: the independent work-module reviewer confirmed the separate tokens
and delayed cancellation while the actor awaited another host RPC. The committed
regressions use bounded token fixtures and ordinary temporary-database SQL.
The shared-token test covers early/late cancellation through either handle and
independent handle release. The new public-wire early-admission case and existing
activity rollback/refused-admission cases pass on Linux x86-64.
