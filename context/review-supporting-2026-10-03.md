<!-- SPDX-License-Identifier: MPL-2.0 -->
# Supporting-module reliability and performance review

This is the supporting review's handoff for the `exp` branch. It covers the
modules below and the Python/native fixture tooling. It is not a claim that the
separate application/database review or the complete repository audit is done.
No changes were pushed, published, or merged. The parent coordinator integrated
each confirmed issue in a separate commit on `exp`.

## Implemented corrections

| Issue | Correction | Evidence |
| --- | --- | --- |
| CORE-001 | Append escaped text directly instead of allocating a vector for every character. | 65,536 ASCII bytes: 65,550 allocations before, one after. |
| CORE-002 | Stop formatting cells at their visible preview limit and reuse previews across table budget retries. | Seven rows of eight 64 KiB cells: cumulative allocation volume fell from 7,707,319 to 397,975 bytes, 94.8%. |
| CORE-003 | Skip JSON tree parsing when the user requests the raw inspection view. | A 2,000-object raw JSON preview fell from 18,378 allocations to 355 with identical displayed text. |
| CORE-004 | Follow valid matching file/directory symlinks during fallback SQLite path completion. | Unit fixtures include valid aliases, dangling links, a loop, and a FIFO; public-wire completion cases pass. |
| CORE-005 | Validate canonical SQLite paths before accepting a saved connection. | A clean alias to a newline-containing filename previously made saved profiles invalid on restart; rejection now preserves profiles. Non-UTF-8 filename fixture is Linux-only because APFS disallows it. |
| CORE-006 | Reject non-UTF-8 CLI arguments and executable paths with a constant diagnostic. | Invalid argument fixtures now exit 2 without a panic or argument contents in stderr. |
| TEST-002 | Follow negotiated native path completion in the navigation acceptance case. | Current-host native and non-expectation branches pass. |
| TEST-003 | Search help action and group heading independently after horizontal scrolling. | Current-host focused native case passes and verifies both matches. |
| TEST-004 | Disable `psql` startup files with `-X`. | Installed client contract checked; no personal configuration read. |
| TEST-005 | Verify detached fixture host ownership and stop it before deleting fixture storage after teardown failures. | Six installed-process regressions and a real persistent-host timeout probe pass; unverifiable hosts retain their fixture storage. |
| TEST-006 | Isolate the native fixture's `psql` environment and passfile/service paths. | Two regressions pass, including installed PostgreSQL 18.6 against a nonexistent temporary Unix socket. |
| TEST-007 | Generate fixture certificates and initialize/control the native server with an explicit private OpenSSL configuration. | Installed OpenSSL verifies matching TLS purpose/hostname and rejects incorrect ones; all six real PostgreSQL 17.9 cases pass. |
| TEST-008 | Attempt shutdown even after failed native PostgreSQL startup; retain data when shutdown cannot be confirmed. | Six lifecycle regressions verify failure propagation, status handling and retained storage. |

Allocation results measure allocator calls or cumulative allocated bytes, not
process RSS or application latency. Exact Unicode/control escaping, empty/NULL
values, truncation markers and preview byte boundaries have regression coverage.

## Independent module reviews

Each listed source module had a dedicated child review. Fixed modules received
another review after correction. Reviews of related modules also checked their
integration boundaries; no new UX was introduced.

| Module | Review result |
| --- | --- |
| `protocol.rs` | Bounded transport and frame handling reviewed; no local issue. APP-004 found at the application validation admission boundary and forwarded to its owner. |
| `profiles.rs` | Profile validation, persistence and diagnostics reviewed; no local issue. Independently checked CORE-005. |
| `paths.rs` | CORE-004 and CORE-005 fixed and independently reviewed. |
| `documents.rs` | Naming, serial bounds, collisions, local-time conversion, SQL guidance escaping and buffer associations reviewed clean. |
| `results.rs` | CORE-001 fixed and reviewed; database identifier truncation concern forwarded to the database owner. |
| `result_storage.rs` | Storage quotas, ranges, concurrency and cancellation reviewed; no actionable local finding. |
| `inspection.rs` | CORE-003 fixed and reviewed, plus canonical-path fixture portability review. |
| `full_value.rs` | Full-value bounds and JSON paths reviewed; locked serde_json raw-value skipping inspected for deep nesting. No actionable local finding. |
| `views.rs` | CORE-002 fixed and reviewed; retained identities and publication budgets checked. |
| `main.rs` | CORE-006 fixed and reviewed. |
| `lib.rs` | Integration boundary and constants reviewed; independently checked CORE-006. |

The native suite and PostgreSQL/dependency/release scripts had separate child
reviews. Native teardown received an additional independent review from the
documents-module reviewer. PostgreSQL fixture corrections received an independent
parent review and a clean final child review, including the subsequent server
environment refinement.

## Validation actually executed

Environment: Linux x86-64, kernel 7.2.5-200.fc44.x86_64; Rust/Cargo 1.90.0;
Python 3.14.7; installed PostgreSQL client 18.6. Native acceptance uses the
existing `/home/krza/code/runyte/target/debug/runyte`, reporting Runyte 0.3.5
(sibling checkout observed at `ee679f45`; binary provenance is not asserted). All database/configuration fixtures are
temporary. Cargo runs use one build job, no incremental compilation, and one
test thread.

- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked`: 58 tests passed; six PostgreSQL tests remained ignored
  in this command and require their separate disposable server fixture.
- `python3 tests/wire.py`: 17 passed.
- `python3 tests/full_values.py`: 11 passed.
- `python3 tests/interactive.py`: 136 passed after the formatting fixes. The five
  affected completion variants were rerun after the path corrections and passed.
- Ten large-page/1,000-row interactive cases passed across feature profiles.
- Full native suite before teardown refactoring: 11 passed with all five feature
  expectation flags. Current-host completion and persistent live-path cases were
  rerun after the path changes: two passed.
- Native cleanup regressions: six passed; a separate injected-timeout probe
  against a real persistent host verified termination before directory removal.
- Final full native suite after teardown refactoring: 17 passed in 64.147 seconds
  with all five current-feature expectation flags.
- `python3 tests/postgres_fixture.py`: all ten final client/certificate/lifecycle
  regressions passed in 0.121 seconds during independent review.
- `python3 scripts/postgres_container_tests.py --image postgres:17`: all six real
  PostgreSQL suites passed with PostgreSQL 17.9 after the certificate correction.
- Python AST parsing, workflow YAML parsing and whitespace checks passed.

The initial public-wire/full-value runs preceded the later path/CLI changes.
The fresh full combined coverage run subsequently passed all 58 ordinary Rust
tests, 17 wire tests (18.667 seconds), 136 interactive tests (192.606 seconds),
11 full-value tests (15.007 seconds), six real PostgreSQL 17.9 cases, and 17
native tests (65.731 seconds). It also ran all nine then-current Python fixture
regressions; the final tenth OpenSSL-environment regression passed separately.

**Fresh combined LLVM line coverage: 91.54% (6,279 of 6,859 lines)**, above the
unchanged 75% floor. Profiles were cleared before the instrumented build. The
same profile environment reached all plugin subprocesses and the PostgreSQL
Cargo invocation. Coverage used cargo-llvm-cov 0.9.0 and Rust 1.90.0's matching
LLVM tools. The existing host remained uninstrumented. The ordinary debug plugin
was rebuilt after coverage; `cargo fmt --check`, locked all-target Clippy with
warnings denied, and `cargo test --locked` all passed again (58 passed, six
PostgreSQL cases intentionally ignored in that ordinary command).

No macOS, ARM64, Rust 1.88, or native PostgreSQL server acceptance is claimed.
`initdb` and `pg_ctl` are unavailable locally. Mocked native PostgreSQL lifecycle
tests establish launcher control flow, not native-server acceptance.

## Integration

All thirteen supporting issue corrections listed above are committed separately
on `exp`. The coordinated report records the final application/database fixes,
module review outcomes and fresh combined validation after integration. The
checkpoint measurements above remain historical evidence from this workstream.

A subsequent independent review checked DB-004's lexical SQL complexity guard,
its distinction between token count and literal/comment length, normal
transaction statements and rejection before job admission. Six query unit tests
and the new public-wire case passed; no further actionable finding remained.

The final integrated validation is recorded in
[the coordinated report](review-2026-10-03.md).

A final read-only integration review found no actionable issue in APP-007's
shared cancellation handles, APP-009's late writable rollback and follow-up-job
isolation, or TEST-009's private certificate-store environment. It did not run
additional tests or builds; final execution evidence belongs to the coordinator.

The supporting reviewer subsequently implemented DB-018 after the SQLite adapter
child found byte-cap capture retention. Seven storage tests, the SQLite case,
PostgreSQL simple/bound-text cases and both PostgreSQL wire workflows passed.
Root committed and independently reviewed the fix; the PostgreSQL module reviewer
also checked the integrated cleanup. This reclaims rejected storage while keeping
prior ranges immutable and accounting for failed I/O. Final whole-suite evidence
remains in the coordinating report.
