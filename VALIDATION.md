# Validation register

## macOS native fixture paths and coverage setup — 2026-09-25

CI run [36106262496](https://github.com/runyte/ru-dbviewer/actions/runs/36106262496)
failed both macOS jobs because persistent native fixtures placed
`project/.runyte/host/workspace.sock` beneath the runner's long `/var/folders`
temporary directory. Runyte rejected the canonical socket path before attachment.
`NativeEditor` now creates its private temporary root directly under `/tmp`
(`/private/tmp` after macOS canonicalization). Configuration, databases, host
inventory and runtime state still live beneath that root and are cleaned up.
The existing detach/reattach test now sets both `TMPDIR` and Python's cached
temporary directory to a deliberately long path. On Linux with the CI-pinned
host, this regression failed with the same socket-path error before the fixture
fix and passed afterward.

Both macOS logs also contained `Broken pipe` from coverage environment setup.
CI now writes `cargo llvm-cov show-env --sh` to a completed file before sourcing
it. This avoids sourcing a live process-substitution pipe and makes a failed
export command fail the step before tests start. The 75% coverage floor is
unchanged, and the reproduction commands below use the same approach.

Local Linux x86-64 validation passed with Rust 1.90.0 and Python 3.14.7:
`cargo fmt --check`, locked all-target Clippy with warnings denied, 45 ordinary
Rust tests (including 11 SQLite database cases), 17 public-wire tests, 111
interactive tests and 11 full-value tests. Native PTY acceptance against host
`cd711f294716a52a800d701016374026036c9b71` ran 11 tests: seven passed and four
requiring newer host features skipped. Fresh combined LLVM line coverage was
**87.67%** (5,793 of 6,608 lines), above the unchanged 75% floor. Python parsing,
workflow YAML parsing and coverage-step Bash syntax checks also passed.
PostgreSQL's five fixture cases were not run locally because server tools are
not installed. macOS and ARM64 were not exercised locally; the next CI run must
confirm those platforms with the fix. The ordinary debug plugin was rebuilt
after coverage. Subagent implementation review found no actionable issues.

## Output queue under a full request window — 2026-09-25

CI intermittently failed `test_callback_window_during_host_request` with
"plugin exited early" on ubuntu-24.04 and macos-15. The plugin's output queue
held four frames and `Rpc::send` treats a full queue as a stalled host. The main
loop answers an admitted burst of 16 host requests back to back. When the writer
thread was not scheduled between replies, the fifth reply stopped the plugin.
Instrumentation attributed every reproduced exit to "Host output stalled". The
queue now holds the 16-request host window, the 12 plugin requests, and headroom
for registration, the stop sentinel and two late path-validation replies. The
writer's per-frame pipe deadline still detects a real stall. A Rust test fills
both windows with no draining writer and fails at the old size. Pinned to one
CPU, the wire test failed 13 of 30 runs before the change and passed 40 of 40 after.

On macos-15-intel, the discovery metadata test compared the catalog's resolved
database path with the unresolved `/var/folders` temporary path. It now compares
with the resolved path. This was reproduced and confirmed on Linux with `TMPDIR`
set to a symlinked directory, where all wire, interactive and full-value tests
pass. It was not run on macOS.

Linux x86-64 passed formatting, locked all-target Clippy with warnings denied,
45 Rust tests, 17 wire tests, 111 interactive tests and 11 full-value tests.
The native suite passed its 11 cases with four skips against Runyte debug host
`cd711f2`, the CI pin. Fresh combined line coverage is **87.67%** (5,793 of
6,608 lines), above the unchanged 75% floor. PostgreSQL tests did not run
because server tools are not installed here. These results establish Linux
SQLite/native-editor behavior, not native macOS or PostgreSQL acceptance. The
ordinary debug plugin was rebuilt after coverage.

## Contextual help topics — 2026-09-24

On hosts advertising `view-help`, registration sends nine workflow topics:
Databases, Tables, Rows, Record, Value, Schema, Filters, Review and
Transactions. Every browser model, including loaded complete values, names the
topic for its page. Runyte's `Space ?` renders that prose, then generates the
page's Tab actions and keys from its live registry. The topics therefore carry
no action or key tables. Older hosts receive no new registration field, and a
host that drops the feature from its acknowledgement is refused.

Linux x86-64 passed formatting, locked all-target Clippy with warnings denied,
44 Rust tests, 17 wire tests, 111 interactive tests, 11 full-value tests and all
13 native PTY cases. The interactive `HelpTests` rerun the presentation suite
with `view-help` and validate each model against the retained extension schema.
The native suite used the coordinated Runyte debug host with
`DBVIEWER_EXPECT_VIEW_HELP`, `DBVIEWER_EXPECT_DEFAULT_BINDINGS`,
`DBVIEWER_EXPECT_PATH_COMPLETION` and `DBVIEWER_EXPECT_FULL_VALUES` set; its
first run found and confirmed the host's registration-envelope fix.

Fresh combined line coverage is **87.59%** (5,751 of 6,566 lines), above the
unchanged 75% floor. PostgreSQL tests did not run because server tools are not
installed here. CI's pinned host predates `view-help`, so its native help case
skips until that pin moves. These results establish Linux SQLite/native-editor
behavior, not native macOS or PostgreSQL acceptance. The ordinary debug plugin
was rebuilt after coverage.

## Default Back binding and final acceptance — 2026-09-22

The plugin supplies `default_binding: "-"` for Back when the host advertises
`view-default-bindings`. Explicit configured keys take precedence in Runyte.
Legacy hosts receive no new registration field and still use the documented
`bindings: {back: "-"}` configuration. This removes the configuration dependency
for updated hosts without changing database-specific navigation. The reported
macOS failure was not reproduced locally.

Linux x86-64 passed formatting, locked all-target Clippy with warnings denied,
43 Rust tests, 17 wire tests, 85 interactive tests, 11 full-value tests and all
ten native PTY cases. Rust builds used one Cargo job and one test thread. The
native suite used the coordinated Runyte debug host with the configured Back
binding removed through `DBVIEWER_EXPECT_DEFAULT_BINDINGS=1`; both standalone
and persistent navigation received the plugin default.

Fresh combined line coverage is **87.36%** (5,683 of 6,505 lines), above the
unchanged 75% floor. One earlier instrumented attempt exited during
`test_callback_window_during_host_request`; three isolated repeats and the
fresh complete run passed. Five PostgreSQL fixture tests remained ignored
because server tools are not installed here. These results establish Linux
SQLite/native-editor behavior, not native macOS or PostgreSQL acceptance.
The ordinary debug plugin was rebuilt after coverage.

## Single browser buffer — 2026-09-22

`App::create` now keeps bounded logical pages behind one native view. Foreground
navigation republishes that view; background work updates its captured page and
cannot retarget the current page. Back cancels and drains an outstanding full-value
publication before reading the shared host revision, including when a staged
commit already won. Returning from SQL can restore a large captured document
through staging. Closing the browser drops history without settling transactions.

Linux formatting, locked all-target Clippy, and the Rust suite passed with one
Cargo job and one test thread. The 73 interactive tests, 11 full-value tests and
17 wire tests passed. Regressions in `tests/interactive.py` cover repeated
navigation without additional native views and SQL return; `tests/full_values.py`
covers Back during staging, refused navigation and large-document return. The
closed-publication cases in `tests/wire.py` inject failure after query admission
and still verify transaction protection and explicit commit. Four additional
wire cases verify selected-field restoration with a fresh selection revision.
All ten native editor cases passed in the coordinated development tree, including
Back immediately reopening the selected field after persistent reattachment.
A reassociated SQL document takes its connection generation from its buffer
association before consulting its retained navigation parent. The four
`test_reassociated_sql_uses_current_connection_generation` wire variants failed
before this correction and pass afterward. macOS and native PostgreSQL were not
exercised in this check.

## PostgreSQL connection form rejection — 2026-09-21

Choosing PostgreSQL after `::db` → Tab → Add database closed the picker without
opening the connection form on Linux and macOS. Notifications reported
`Plugin result rejected` and
`Plugin invocation command: Host refused operation: invalid_argument`.

`App::submit` in `src/app/input.rs` authored the form field as `password_env`.
Runyte's native input validator uses the host name grammar, which rejects
underscores, so the whole form failed before any database connection attempt.
The retained test schema accepts underscores in input field IDs; schema-only
validation and direct PostgreSQL adapter tests did not exercise this rejection.
The form and submission handler now use `password-env`. The saved profile still
uses `password_env`, preserving existing profiles and environment-based passwords.
No host protocol or database adapter change is required.

`test_postgres_form_and_password_handoff` in `tests/interactive.py` checks the
database action, native-compatible field IDs, default port, verified TLS,
unchanged saved keys, masked password prompt and cancellation before database IO.
`test_postgres_form_preserves_password_environment_name` checks that the submitted
environment name reaches password resolution. Both run with legacy and negotiated
feature profiles. `test_postgres_connection_form_from_database_actions` in
`tests/native.py` follows the real Tab-menu flow through form submission to the
password prompt in standalone and persistent modes, then checks notifications.
The new regressions failed against the original plugin and pass with this fix.

Linux x86-64 checks passed: formatting, locked all-target Clippy with warnings
denied, 42 ordinary Rust tests, 17 wire tests, 69 interactive tests, nine full-value
tests and all ten native PTY cases. Native tests used the local Runyte debug build
with row-action, full-value and path-completion expectations enabled. Rust builds
used `CARGO_BUILD_JOBS=1` and `CARGO_INCREMENTAL=0`; suites ran sequentially and
Rust tests used `RUST_TEST_THREADS=1`.

Fresh combined line coverage is **87.17%** (5,517 of 6,329 lines), above the
unchanged 75% floor. Five PostgreSQL server-fixture tests remained ignored;
server tools are not installed locally and database adapter code did not change.
The new native case deliberately stops at the password prompt without connecting
to a database. Existing CI runs the native suite and PostgreSQL server fixtures on
Linux and macOS. This run does not establish macOS, ARM64, exact-pinned-host or
packaged-release acceptance.

Independent review found one minor fixture-isolation issue: the missing-password-
environment test assumed its sentinel variable was absent from the inherited
environment. `Host` in `tests/wire.py` now accepts explicit child-environment
exclusions, and the interactive fixture excludes this sentinel without mutating
process-global environment. All 69 interactive tests passed again with the
sentinel deliberately set in the parent, followed by all 17 wire tests. Re-review
reported no remaining findings. The normal debug plugin was rebuilt after coverage.

## Connection forms and native path completion — 2026-09-20

The SQLite form now labels its path as a local file using a short, fully visible
field label. Database-type and PostgreSQL form titles distinguish local files
from database servers. HTTP/HTTPS SQLite paths are refused explicitly; remote
PostgreSQL continues to use the existing native database protocol and TLS support.

On hosts negotiating `input-path-completion`, the SQLite text field opts into
native local suggestions. Older hosts receive the original text field shape and
retain submitted-path completion. The new fixture is layered onto the frozen
schema only in feature-enabled tests; the frozen fixture bytes are unchanged.

Linux x86-64 validation passed formatting, locked all-target Clippy with warnings
denied, 42 ordinary Rust tests, 17 wire tests, 61 interactive tests, nine full-value
tests, and all nine native PTY cases. The final native run used the updated local
Runyte debug build with row-action, full-value and path-completion expectations
enabled. `test_native_live_path_completion_and_short_labels` in `tests/native.py`
checks the complete path label and live completion before Enter, in both standalone
and persistent modes, including after `:cd` changes the editor working directory.
`test_sqlite_form_has_short_local_label_and_negotiated_completion` in
`tests/interactive.py` covers new and legacy host shapes; the path-resolution unit
case in `src/paths.rs` covers explicit URL refusal. The native tests passed again
after the final host workspace-root correction.

Fresh combined line coverage is **90.09%** (5,702 of 6,329 lines), above the
unchanged 75% floor. Earlier profiles were removed before measurement. Database
adapter code did not change and the five optional PostgreSQL fixture cases were
not rerun for this connection-UI change. Native macOS/ARM64 and packaged release
acceptance remain separate gates.

## Browse page size — 2026-09-20

Browse pages now accept 1–1,000 rows and still default to 100. The renderer uses
that configured size and budgets encoded cell previews across every retained
row, avoiding gaps caused by silently dropping a page's publication tail.
SQL-result pages remain 100 rows over already retained results.

On Linux x86-64, formatting, locked all-target Clippy with warnings denied,
42 ordinary Rust tests, 17 public-wire tests, 44 interactive tests, nine
full-value tests, and eight native-editor PTY cases passed. Native acceptance
used the local Runyte debug executable with both row-action and full-value
feature expectations enabled. The shared database browsing contract also passed
against a disposable PostgreSQL 17 container. The other four PostgreSQL fixture
cases were not rerun for this change. macOS and ARM64 were not exercised.
Fresh combined coverage is 90.04% (5,686 of 6,315 lines), above the unchanged
75% floor; earlier profiles were removed before measurement.

`page_size_bounds_and_offsets_in_both_dialects` in `src/browse.rs` covers default,
minimum and maximum SQL page sizes, offsets, and rejection above the maximum.
`browse_filter_contract` in `tests/databases.rs` checks a full 1,000-row page,
a partial next page and an empty following page in SQLite and PostgreSQL.
`test_thousand_row_pages_preserve_navigation_and_inspection` in
`tests/interactive.py` covers native-input validation, default size, complete
row identities, generated SQL, next/previous navigation and last-record inspection.
`test_large_page_shortens_previews_without_dropping_rows` covers JSON-expanding
text, bounded publication, shortened cell previews and retained record values.
Both interactive regressions run with legacy, row-action and presentation feature
profiles.

## Actions, metadata and full-value documents — 2026-09-19

Linux x86-64 native acceptance passed all eight cases in `tests/native.py`, with
`DBVIEWER_EXPECT_ROW_ACTIONS=1` and `DBVIEWER_EXPECT_FULL_VALUES=1`. Both binaries
were current local debug builds, not release artifacts or the historical CI pin.
The plugin executable used for timing was copied before coverage instrumentation.
macOS and ARM64 were not exercised by these runs.

The native cases exercise contextual titles and metadata, selection-based row
activation, grouped labels without Activate, read-only browsing, writable SQL
review/commit, path completion, unsaved SQL, profile connect/disconnect, indented
previews, both Back controls, and persistent-session attachment. The large-value
case loads 4,944,933 raw bytes into 5,256,945 formatted bytes over 48,005 lines.
Native search reaches its trailing sentinel; ordinary `%y` and `p` copy every
chunk to a fixture-owned file. Parsed formatted content equals the original and
raw copying preserves the original text exactly. Detach/reattach preserves the
loaded value. Back restores the record field: Enter immediately reopens its preview.

Native acceptance exposed a full-value parent that still pointed at the source
preview. After correcting it to the record, all eight cases passed in 44.569s.
The cancellation case deliberately accepts an already committed successful result;
it proves responsive input and safe cancellation/commit races, not that cancellation
won. Deterministic public-wire cancellation cases provide that separate evidence.

A subsequent uninstrumented debug run of the large-value case passed in 14.152s.
Observed harness elapsed time was 0.801s to complete the load and 0.100s to open a
command prompt while loading was still active. These include the harness's 0.3s
send drain and 0.1s wait polling, and are not intrinsic latency measurements.
Fixture-host RSS/high-water observations were 50,052 KiB before loading, 72,392 KiB
after loading, and 144,888 KiB after copying formatted and raw values to separate
editable buffers. They exclude the plugin and frontend processes and are not hard
RSS bounds. After returning to the full document, moving to its start and allowing
two seconds to settle, the host consumed 0ms CPU in a one-second sample. An earlier
sample taken immediately after an editable copy was discarded as unsettled work.
Unavailable `/proc` measurements are reported as unavailable, never as measured zero.

Both database adapters preserve original captured sources. All five PostgreSQL
fixture suites passed separately against disposable PostgreSQL 17.9, including
TLS, client certificates, Unix sockets, cancellation and lost-commit acknowledgement.
Capture regressions cover anonymous-file ownership, immutable ranges, original
SQLite/PostgreSQL values, complete hexadecimal representations, quota exhaustion,
write failures, cancelled/expired reads and writable rollback behavior. Storage
reservations are released only after the underlying descriptor closes.

The final fresh-profile run passed formatting, locked all-target Clippy with
warnings denied, 41 ordinary Rust tests, 17 public-wire tests, 38 interactive tests,
nine full-value tests, all five PostgreSQL fixture suites and eight native PTY
cases. Combined line coverage is 89.90% (5,652 of 6,287 lines), above the unchanged
75% floor. Older profiles were removed before this measurement. The final native
suite passed in 47.458s with the plugin instrumented; the timing and memory
observations above deliberately use the separate uninstrumented run.

An earlier instrumented cancellation run exposed an intermittent plugin shutdown.
Inspection found a transport race: concurrent request callers could allocate
increasing IDs and then queue their frames in reverse order, which the host
rejects. `Rpc::request` now serializes allocation through queue insertion without
holding that lock during socket IO or while awaiting replies.
`protocol::tests::concurrent_large_frames_and_control_requests_keep_wire_ids_in_order`
covers 128 concurrent mixed-size requests. The race is source-confirmed, but was
not established as the cause of that particular shutdown. All suites above passed
after the fix. `cargo +1.88 check --locked --all-targets` also passed; the normal
uninstrumented debug executable was rebuilt afterward and locked Rust tests
passed again. Independent review rounds for capture, presentation, full documents,
transport ordering and tests/documentation finished without remaining findings;
earlier coverage sections remain historical evidence.

## Truncated JSON prefix display — 2026-09-19

On Linux x86-64, formatting, locked all-target Clippy with warnings denied,
25 Rust tests, 17 public-wire tests, 23 interactive tests and six native Runyte
PTY tests passed. Five PostgreSQL fixture tests remain ignored in this run;
macOS and ARM64 were not exercised. Fresh profiles from these suites measure
84.02% line coverage (4,026 of 4,792 lines), above the unchanged 75% floor.
Older profiles were excluded from this measurement.

`inspection::tests::incomplete_json_indents_without_repairing_strings_or_numbers`
and `incomplete_json_keeps_long_strings_and_bounds_expansion` in
`src/inspection.rs` cover unfinished strings, escapes, Unicode, duplicate keys,
number spelling, raw text preservation and bounded fallback.
`test_truncated_json_prefix_and_raw_keep_retained_data` in `tests/interactive.py`
checks real SQLite values above 64 KiB, both feature negotiations, raw round trips
and navigation without SQL replay. `test_truncated_json_value_and_back` in
`tests/native.py` checks visible indentation, raw inspection and the configured
`-` binding through Value → Record → Rows in a real editor.

## Initial implementation — 2026-09-17

Evidence applies to the implementation delivered with this record. No public
plugin release, tag or CI run is claimed.

Local platform: Linux x86-64, Rust/Cargo 1.97.1, Python 3.14.7. Rust 1.88 is
checked separately. The real Runyte host reports 0.3.0. CI pins host source at
`cd711f294716a52a800d701016374026036c9b71`. The local host build was not that
exact revision; exact-pin acceptance remains a separate CI gate.

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo test --locked` | 11 passed; four PostgreSQL tests require their explicit fixture run |
| `cargo +1.88 check --locked --all-targets` | Passed |
| `python3 tests/wire.py` | Nine public-protocol/SQLite workflow cases passed; outbound messages checked against the retained schema |
| `tests/databases.rs` PostgreSQL cases | All four passed on PostgreSQL 16.15; transaction/type/cancellation and TLS cases also passed on 17.9 |
| `tests/native.py` with the local host | Three real PTY tests passed: browsing/query/stop; write review/commit; persistent detach/reattach |
| Combined LLVM line coverage | 78.39% (2,006 of 2,559 lines); plugin CI floor starts at 75% |
| Dependency license collection | Linux and Apple Silicon target graphs collected successfully |
| Workflow files and Python scripts | YAML parsing and Python compilation passed |
| Local release build | Linux x86-64 GNU executable built; packaged musl/macOS builds await CI |

The four PostgreSQL cases exercise exact numeric and text representations,
read-only refusal, catalog/schema browsing, commit/rollback, cancellation,
verified TLS, rejected hostname/untrusted certificate, client-certificate
authentication, Unix sockets, and loss of a commit reply after the server has
committed. The last case verifies exactly one inserted row and an uncertain
client outcome, not an automatic retry. Tests use disposable databases only.

PostgreSQL fixtures here were isolated local containers. CI uses the same Rust
cases against native temporary clusters created by `scripts/postgres_tests.py`;
the launcher requires installed `initdb`, `pg_ctl`, `psql` and `openssl` and has
not been run locally, where only the PostgreSQL client is installed.

Public-wire tests also cover stale view/selection refusal, multi-selection
refusal, retained-result paging without re-execution, cancelled write confirmation,
rollback after an execution error, activity cancellation, connection-generation
reassociation, and failure to acquire a job/activity before any SQL is executed.

## Coverage reproduction

Install cargo-llvm-cov 0.9.0 and the matching toolchain's llvm-tools-preview.
Build the pinned Runyte host separately, before exporting coverage variables.
Use a disposable database fixture; never point these tests at personal data.

```sh
cargo llvm-cov clean --profraw-only
coverage_env=$(mktemp)
cargo llvm-cov show-env --sh > "$coverage_env"
source "$coverage_env"
rm "$coverage_env"
cargo clean -p ru-dbviewer
cargo test --locked
cargo build --locked
python3 tests/wire.py
python3 tests/interactive.py
python3 tests/full_values.py
PG_BIN=/path/to/postgresql/bin python3 scripts/postgres_tests.py
RUNYTE_BIN=/path/to/runyte python3 tests/native.py
cargo llvm-cov report --summary-only --fail-under-lines 75
```

Cleaning only this package before the instrumented build matters: changing
wrapper-specific environment variables alone can reuse an uninstrumented
library. Include the external plugin processes in the same LLVM_PROFILE_FILE
environment. Unit-only coverage is not the combined behavior measurement.
When validating a current host that advertises the new features, also set
`DBVIEWER_EXPECT_ROW_ACTIONS=1` and `DBVIEWER_EXPECT_FULL_VALUES=1` for native tests.
The pinned historical host instead exercises the negotiated fallback.
The CI floor belongs to this plugin; Runyte's coverage threshold is unchanged.

## Remaining release gates

- Native macOS Intel/Apple Silicon and Linux ARM64 CI must pass. Workflow presence
  is not evidence that these platforms have executed successfully.
- Run the artifact workflow to validate musl linking and each packaged executable.
  The local executable is a GNU build, not the musl artifact or a universal
  executable.
- macOS 13 is the configured deployment target; native tests currently target
  macOS 15 runners. Verify the minimum deployment OS before claiming acceptance
  on macOS 13. Public macOS artifacts remain unsigned unless signing/notarization
  is provisioned separately.
- Native-cluster orchestration and CI setup need their first CI execution.
- Expand protocol fault/backpressure, peer disconnect, expiry and concurrency
  coverage beyond the initial suites before treating this as a mature database
  administration tool. The retained recovery marker deliberately prefers an
  explicit review over inferring that a lost acknowledgement means no change.

## Deliberate v1 limits

Browsing uses offset paging with primary-key ordering where available, and makes
no snapshot guarantee across pages. Results are bounded per result, not by a
shared per-connection aggregate. Twelve views bound the number of retained
collections. SQL input, displayed columns and supported statement classes are
bounded as documented in README.md. Database creation, scripts, editable cells,
exports, managed tunnels and credential-manager integration remain out of scope.

## Review fixes — 2026-09-17

All six reported defects were confirmed and fixed. Public-wire regressions cover
control characters in table titles, publication to a closed view (`not_found`),
a 16-event callback burst during `state.set`, recovery-marker save failure before
SQL, page-preserving refresh, and schema inspection from a browse result. Database
regressions distinguish clipped cells from row/total-size caps: large SQLite blobs
preserve writable transactions, and PostgreSQL large text values preserve connection
reuse in read-only and writable execution. Row and byte limits remain enforced.

Linux x86-64 checks passed: formatting, Clippy with warnings denied, 13 ordinary
Rust tests, 13 public-wire tests, all four PostgreSQL cases against disposable
PostgreSQL 16, and all three native Runyte PTY cases. Combined LLVM line coverage
is 79.69% (2,064 of 2,590 lines), above the 75% floor. The README Install YAML
was parsed and compared with generated configuration.
macOS and ARM64 acceptance remain the existing CI gates.

## Follow-up review fixes — 2026-09-17

Publication treats the host's `not_found`, `closed`, and `cancelled` responses as
retired views, allowing job completion and explicit settlement of pending writes.
Empty column labels render as `(unnamed)` while retaining distinct ordinal IDs.
The incoming queue reserves 40 entries for the 16-request window plus view and
activity events. Record previews share an encoded JSON budget; every field in a
wide row keeps its selectable ID and opens its original retained value.

Linux x86-64 validation passed: `cargo fmt --check`, Clippy with warnings denied,
13 ordinary Rust tests, all 16 public-wire tests, four PostgreSQL 16 fixture tests,
and three real Runyte PTY tests. Fresh combined LLVM line coverage is 81.85%
(2,138 of 2,612 lines), above the unchanged 75% floor. The new wire cases cover
all three publication errors for reads and pending writes, a mixed burst of
16 requests plus 12 view events and two activity events, empty names in both
queries and tables, and a 600-column record with JSON-expanding text and full
retained-value inspection of its last column.

An independent review of the fixes and the surrounding lifecycle and model
paths against the host source ran all 16 wire tests and reported no findings.
Existing platform release gates still apply; this does not claim macOS or
ARM64 execution.


## Interactive browsing — 2026-09-19

Implemented the coordinating interactive-browsing plan in this plugin repository.
The public API audit and the profile-actions/completion adaptations are recorded
in [INTERACTIVE_BROWSING.md](INTERACTIVE_BROWSING.md). No Runyte host changes,
publication or release were made.

Actual local platform: Linux x86-64, Rust/Cargo 1.97.1, Python 3.14.7; the native
Runyte host reports 0.3.0. The local host is not claimed to be the exact CI pin.

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo test --locked` | 23 passed; five PostgreSQL cases run separately |
| `cargo +1.88 check --locked --all-targets` | Passed |
| `python3 tests/wire.py` | 17 passed, with outbound schema validation |
| `python3 tests/interactive.py` | 10 passed |
| PostgreSQL database cases | All five passed against disposable PostgreSQL 17.11 |
| `tests/native.py` with local Runyte | All four real PTY tests passed |
| Combined LLVM line coverage | 87.80% (4,046 of 4,608 lines), above the unchanged 75% floor |
| Independent review | Findings fixed and re-reviewed until no actionable findings remained |

The new behavior coverage exercises retained parent navigation and closed-parent
fallback; named unsaved SQL and collision handling; selected-profile ownership
through actions, forms and password prompts; connection generations; searchable
columns; page size; ALL/ANY, disabled, edited, empty-string and NULL filters;
sorting and independently executable generated SQL in both dialects; precise,
duplicate-key, deep and bounded JSON inspection; path validation/completion;
mode and disconnect protection; and transactions navigation. Real PTY acceptance
covers completion, both back controls, execution of unsaved edits, explicit save,
source return, writable review/commit, and persistent detach/reattach. These are
automated native checks, not a claim of separate human manual acceptance.

The PostgreSQL fixture used temporary certificates and container-owned data for
TLS hostname/CA validation, client-certificate authentication, Unix sockets,
cancellation, lost commit acknowledgement, and browsing semantics. The temporary
container and certificate storage were removed afterward. The checked-in native
cluster launcher remains a CI gate on this machine, which has client tools only.

Review regressions cover retained original column ordinals after selection and
refresh, generation-neutral database lists, bounded JSON row IDs and encoded
models, matching profile-name limits, asynchronous path completion, cached browse
ordering metadata, and source identity across chained input surfaces. A forced
small-pipe test covers the nonblocking stdout short-write stall found during
verification. Native fixture cleanup now waits for the captured persistent host
to exit after its shutdown acknowledgement before removing temporary storage;
this fixes an observed teardown race rather than retrying directory deletion.

macOS, ARM64, exact-pinned-host CI and packaged release artifacts remain the
existing acceptance gates. No new platform execution is claimed.


## Negotiated row actions — 2026-09-19

The coordinated host extension is the optional `view-row-actions` feature of
`runyte-1`; it does not require a protocol epoch change. On supporting hosts,
Databases publishes action lists per profile. The first Tab menu directly offers
connect, query, mode, disconnect, settlement or recovery as applicable. The
original per-profile picker remains the fallback when the feature is absent.
No runtime changes were needed in ru-time or the bundled demonstration plugins.

Actual Linux x86-64 checks passed: formatting, locked all-target Clippy with
warnings denied, 23 ordinary Rust tests, 17 public-wire tests, 21 interactive
cases covering both negotiated and legacy hosts, all five disposable PostgreSQL
17.11 cases, and five real Runyte PTY cases. Rust 1.88 all-target checking also
passed. The new native test requires direct row actions when
`DBVIEWER_EXPECT_ROW_ACTIONS=1`; without it, the same test accepts the legacy
profile-menu path for the pinned older CI host. Combined plugin coverage is
88.11% (4,111 of 4,666 lines), above the unchanged 75% floor.

The base schema and fixtures remain unchanged. Feature-aware wire tests layer
the separately recorded row-action definition over that schema, while fallback
tests still reject the new field. The host extension's required Rust checks,
coverage, frozen-client and unchanged-plugin checks are recorded in Runyte's
coverage register. Independent review found a transient-patch negotiation bypass;
worker validation now checks each inserted/updated row before later operations
can erase it. Re-review reported no remaining actionable findings.

These are local development builds reporting Runyte 0.3.0, not a new published
host release or exact-pin CI evidence. Existing macOS/ARM64 and packaging gates
remain outstanding. Current feature behavior is documented in
`INTERACTIVE_BROWSING.md` and the Runyte application contract.
