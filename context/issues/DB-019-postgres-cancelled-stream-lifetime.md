<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-019: An interrupted PostgreSQL stream can block rollback acknowledgement

Status: fixed; adapter re-review found no remaining actionable issue.

Source review confirmed that `execute_bound` keeps its owned `work` future in
scope after cancellation or timeout wins `tokio::select!`. The unfinished future
retains its query stream and response receiver while cancellation and rollback
run. Locked tokio-postgres 0.7.18 uses a bounded response channel and stops reading
responses when that channel fills. An abandoned streaming result can therefore
block receipt of the rollback acknowledgement until the settlement deadline,
reporting an unknown writable outcome and retaining an unnecessary recovery marker.

The owned work future, its pin and selection now occupy a lexical block returning
the result and interruption flag. Exiting that block drops the unfinished future
and stream before cancellation or rollback. Dropping only the `Pin<&mut _>` from
`tokio::pin!` would leave the underlying future alive. Retirement, cancellation
deadlines and settlement behavior are unchanged.

Validation on Linux x86-64: `cargo fmt --check` passed. The checked-in
`scripts/postgres_container_tests.py --image postgres:17` launcher passed all 13
PostgreSQL Rust cases against disposable PostgreSQL 17.9 in 12.31 seconds and
both PostgreSQL public-wire cases in 2.743 seconds. The existing retirement and
transaction/cancellation cases passed, as did DB-018's simple-query and bound-text
byte-cap regression. The launcher removed its container and temporary files.

Whole-module source re-review establishes drop-before-settle directly and confirms
that DB-018's capture checkpoints, rejected-row cleanup and earlier retained-value
ownership remain intact. No new stress or network-fault fixture was added, and
these runs do not establish macOS or ARM64 acceptance.
