<!-- SPDX-License-Identifier: MPL-2.0 -->
# Application review — 2026-10-03

Scope: all ten `src/app` modules and public-wire, interactive, and full-value
behavior. Work remains on branch `exp`. The application owner read every module
and the repository guidance; independent module reviews are tracked below.

## Fixed findings

| Issue | Change | Focused evidence |
| --- | --- | --- |
| APP-001 | Reject a stale filter-column picker instead of panicking | New regression failed the old executable and passed all five feature profiles |
| APP-002 | Apply the 256-association limit to existing SQL documents | All five profiles reject overflow and allow reassociation/execution at the limit |
| APP-003 | Adopt filters/sort/page size only with successful captured results | Fifteen failure variants plus ten successful filter/paging variants passed |
| APP-004 | Refuse unavailable validation before dispatch to blocking workers | Deterministic saturated-worker regression failed before the fix; fixed Rust test and five path-validation variants passed |
| APP-005 | Bind pending connections to their loading page for cancellation | Independent runtime reproduction; five reconnect-cancellation variants passed |
| APP-006 | Preserve complete-document presentation and avoid another retained text copy on return | Both new SQL-return checks failed before the fix; all twelve full-value cases passed afterward |
| APP-007 | Share the activity and SQL job cancellation token, including early cancellation | Bounded token-lifecycle unit and early-admission/activity rollback wire cases passed |
| APP-008 | Preserve connection generation across PostgreSQL password forms | Both real PostgreSQL wire cases passed; temporary-table rollback verified in the same session |
| APP-009 | Roll back pending writes cancelled during publication or terminal acknowledgement | Both regressions failed before the fix; full 21-case wire suite and separate follow-up-catalog regression passed afterward |
| APP-010 | Correct confirmation, schema-parent, and JSON preview help | Topic bounds test and independent prose re-review passed; existing UX unchanged |

Formatting and locked all-target Clippy with warnings denied passed after the
latest application fixes. Parent owns final integrated coverage/native/database
validation. Application-owned local fixtures used temporary SQLite databases on
Linux x86-64. The database review owner also ran the two new wire cases against
a disposable PostgreSQL 17.9 container, alongside ten Rust PostgreSQL suites at
that checkpoint. No macOS or ARM64 acceptance is claimed by this review.

## Independent nested module reviews

| Module | Reviewer | Outcome |
| --- | --- | --- |
| `mod.rs` | `application_review/app_mod_review` | Found APP-005; commands reviewer independently re-reviewed the fix clean |
| `browser.rs` | `application_review/browser_review` | Found APP-006; full-value and presentation reviewers independently re-reviewed the fix clean |
| `full_value.rs` | `application_review/full_value_review` | Clean substantive review; independently passed all twelve full-value cases; APP-006 browser/presentation changes re-reviewed clean |
| `work.rs` | `application_review/work_review` | Found APP-007 and password-generation candidate APP-008; commands/lifecycle/input reviewers independently re-reviewed fixes clean |
| `browsing.rs` | `application_review/browsing_review` | Clean; re-reviewed APP-001/003 and passed four focused SQLite regressions |
| `commands.rs` | `application_review/commands_review` | Clean; re-reviewed APP-005/007 and passed seven focused wire regressions |
| `help.rs` | `application_review/help_review` | Found APP-010; all corrected prose independently re-reviewed clean |
| `input.rs` | `application_review/input_review` | Confirmed APP-008; re-reviewed APP-002/004/008 clean and ran the bounded validation test |
| `lifecycle.rs` | `application_review/lifecycle_review` | Found APP-009; re-reviewed APP-003/007/009 clean and passed five cancellation/settlement cases |
| `presentation.rs` | `application_review/presentation_review` | Clean whole-module review; APP-006 re-reviewed; independently passed all twelve full-value cases |

All ten source modules received separate nested reviews. No actionable
application finding remains open after the documented fixes and re-reviews.

## Resolved candidates and limits

The apparent stale `browse-sql` entry point is refused by the host: view commands
must appear in the current model's actions, and invocation revisions must still
match. Reconnection strips this action from retained pages. The browsing reviewer
checked that public host admission path; no redundant local guard was added.

The full-value reviewer observed a safe, transient refusal when Back arrives
after the host installs a staged document but before application completion is
processed. The revision fence rejects the racing invocation; retry succeeds.
No replay, data loss, or persistent unusability was found, and the fence remains
intact.

The final application child also independently reviewed TEST-009's temporary
PostgreSQL trust-store environment. Both launchers share the isolation helper,
coverage variables remain intact, and all eleven fixture tests passed on Linux.
That supporting finding is owned and recorded by the parent reviewer.
