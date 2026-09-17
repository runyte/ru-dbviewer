# Validation register

## Initial implementation — 2026-09-17

Evidence applies to the uncommitted implementation delivered with this record.
No public plugin release, tag or CI run is claimed.

Local platform: Linux x86-64 (Fedora 44, kernel 7.1.13), Rust/Cargo 1.97.1,
Python 3.14.7. Rust 1.88 is checked separately. The real Runyte host reports
0.3.0. CI pins host source at
`cd711f294716a52a800d701016374026036c9b71`. The final local host build
included unrelated uncommitted editor changes; exact-pin acceptance remains
a separate CI gate.

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
not been run on this machine, where only the PostgreSQL client is installed.

Public-wire tests also cover stale view/selection refusal, multi-selection
refusal, retained-result paging without re-execution, cancelled write confirmation,
rollback after an execution error, activity cancellation, connection-generation
reassociation, and failure to acquire a job/activity before any SQL is executed.

## Coverage reproduction

Install cargo-llvm-cov 0.9.0 and the matching toolchain's llvm-tools-preview.
Build the pinned Runyte host separately, before exporting coverage variables.
Use a disposable database fixture; never point these tests at personal data.

```sh
source <(cargo llvm-cov show-env --sh)
cargo clean -p ru-dbviewer
cargo test --locked
python3 tests/wire.py
PG_BIN=/path/to/postgresql/bin python3 scripts/postgres_tests.py
RUNYTE_BIN=/path/to/runyte python3 tests/native.py
cargo llvm-cov report --summary-only --fail-under-lines 75
```

Cleaning only this package before the instrumented build matters: changing
wrapper-specific environment variables alone can reuse an uninstrumented
library. Include the external plugin processes in the same LLVM_PROFILE_FILE
environment. Unit-only coverage is not the combined behavior measurement.
The CI floor belongs to this plugin; Runyte's coverage threshold is unchanged.

## Remaining release gates

- Native macOS Intel/Apple Silicon and Linux ARM64 CI must pass. Workflow presence
  is not evidence that these platforms have executed successfully.
- Run the artifact workflow to validate musl linking and each packaged executable.
  The provided local executable is a GNU build for this machine, not the musl
  artifact or a universal executable.
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
PostgreSQL 16, and all three native Runyte PTY cases. The persistent-host test
required permission to bind its temporary Unix socket outside the sandbox.
Combined LLVM line coverage is 79.69% (2,064 of 2,590 lines), above the 75% floor.
The README Install YAML was parsed and compared with generated configuration.
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

An independent fresh subagent reviewed the fixes and surrounding lifecycle and
model paths against the host source, ran all 16 wire tests, and reported no
findings. No additional review round was needed. Existing platform release
gates still apply; this does not claim macOS or ARM64 execution.
