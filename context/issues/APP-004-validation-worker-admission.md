<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-004: Form validation queues blocking workers without a permit

Status: fixed; independent re-review clean.

`App::validate_form` in `src/app/input.rs` calls `try_acquire_owned()` on
`path_slots`, but calls `spawn_blocking` even when acquiring a permit failed.
The worker merely computes `available = permit.is_some()` and eventually replies
`unavailable`. The semaphore therefore limits filesystem work but not queued
validation workers or their retained request payloads.

If both admitted path operations and the remaining database/storage workers are
occupied, further validation replies wait behind those operations. Host requests
can expire and be replaced while their workers remain queued. This undermines
the transport's bounded late-reply assumption and can produce a large completion
burst when blocking workers become available.

Resolution: return the bounded `unavailable` validation response without entering
the blocking pool when no path permit is available or the input is unrelated
to the SQLite form.

Validation: a deterministic Rust regression occupies a runtime's sole blocking
worker and both path permits, then requests validation. The previous behavior
times out; the fix immediately returns correlated `unavailable` field statuses.
The fixture uses channels and never reads filesystem state. The new Rust test
and five public-wire path-validation feature variants pass on Linux x86-64.
The independent protocol and supporting reviewers confirmed the original path;
final re-review is recorded in the application review register.
