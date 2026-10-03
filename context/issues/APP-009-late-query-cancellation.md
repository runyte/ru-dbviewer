<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-009: Late SQL job cancellation leaves writable changes pending

Status: fixed; independent re-review clean.

Data completion checks the cancellation token once, marks successful writable
results pending, and then awaits view publication and job completion. A job
cancellation arriving during either host request updates the token directly,
but no application event follows. The cancelled job can therefore finish while
its transaction and activity lease remain pending; an explicit subsequent
Commit can persist those supposedly cancelled changes.

Resolution: check for cancellation after publication and again after the
job-finish acknowledgement. Roll back and retire a cancelled pending transaction
before completing cleanup; preserve the uncertain-outcome marker if rollback
cannot be confirmed.

Regression: inject job cancellation during result publication and during the
job-finish response for an ordinary temporary SQLite update. Verify lease and
marker release after rollback, no pending Commit, and unchanged stored rows.

Validation: independent lifecycle reviewer reproduced the publication race,
including a host `cancelled` rejection and a subsequent Commit that persisted
the cancelled update. Both new late-cancellation regressions fail before the
fix and pass afterward. The 21-case public-wire suite passed, followed by a
separate additional regression proving a refused old connection-job completion
cannot cancel its follow-up catalog job. The lifecycle reviewer independently
passed five cancellation/settlement cases and re-reviewed APP-009/APP-007 clean.
Formatting and locked all-target Clippy passed on Linux x86-64.
