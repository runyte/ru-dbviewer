<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-001: A stale filter-column picker can terminate the plugin

Status: fixed; independent re-review pending.

Submitting a filter-column choice after the owning browser closes indexes the
removed page in `App::browse_submit` (`src/app/browsing.rs`). The unchecked map
lookup panics instead of rejecting the expired input, terminating all database
connections and any unrelated pending work. Navigation that prunes the owning
page can produce the same stale input.

Resolution: validate that the captured page and selected column still exist
before reading their filter metadata; return a bounded error on stale input.

Regression: open a filter-column picker against a disposable SQLite table,
close its browser through the public `view.closed` event, submit the captured
choice, and verify rejection followed by a successful new Databases command.

Validation: the new public-wire regression failed with `plugin exited early`
against the original executable in all five feature profiles, then passed in
all five profiles after rebuilding the fix on Linux x86-64. Full suite and
independent re-review are recorded in the final review register.
