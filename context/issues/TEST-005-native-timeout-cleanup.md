# TEST-005: Native fixture teardown can leave a detached host running

Status: fixed; independent re-review clean.

`NativeEditor.__exit__` removes its temporary directory in an unconditional
`finally`. If the `--session-stop` command times out, or the subsequent host-exit
poll times out, captured detached hosts are not terminated before that removal.
They can outlive their fixture with open database/plugin resources and missing
configuration/state files.

The independent native reviewer reproduced this using an installed `sleep`
process as an isolated fixture-host stand-in and an injected stop timeout: the
temporary directory was removed while the stand-in remained alive. The reviewer
then terminated and reaped that process. No generated executable was used.

Add bounded fallback termination of verified fixture-owned hosts on teardown
failure. Check captured process identity before signalling to avoid affecting
reused or unrelated PIDs. Delete fixture storage only after its processes have
stopped; retain a clear error if cleanup cannot safely complete.

`FixtureHost` now captures the process start time and full command through
GNU/Darwin-compatible `ps` fields. Fallback termination requires the exact
fixture command, including its private project and configuration paths. Linux
uses a pidfd and rechecks the captured identity before signalling, so a reused
PID cannot redirect the signal. A failed stop or exit wait still reports its
original error after successful cleanup. Frontend PTY/process cleanup precedes
host observation, including when observation itself fails.

If identity cannot be verified or a safe termination handle is unavailable,
teardown reports the retained fixture path and leaves its state intact. Normal
macOS stop/exit polling remains supported; timeout fallback deliberately retains
state there because Python provides no pidfd-equivalent signal handle. No macOS
fallback-termination or acceptance claim is made. `TemporaryDirectory` now uses
`delete=False` so retained storage survives garbage collection; explicit normal
cleanup and constructor-failure cleanup still remove it. This requires Python
3.12+, already the CI version, and is stated in the native module docstring.

Six checked-in `NativeFixtureTests` run with the existing native command. They
use only the installed `sleep` program and temporary fixtures to cover stop
timeout, exit-wait timeout, changed process identity, unverified ownership,
missing safe handles, retention after garbage collection, and frontend cleanup
after observation failure. All six passed on Linux x86-64 with Python 3.14.7.
Before the final observation-failure regression was added, the other five plus
the ordinary standalone SQLite and persistent detach/reattach cases passed
(seven tests, 6.766 seconds) against the existing Runyte 0.3.5 debug build with
all five feature expectation flags enabled. A separate isolated real persistent
host probe confirmed its command matched ownership verification and injected
stop timeout terminated that host before deleting its temporary files.

The complete native suite after the final cleanup correction passed all 17 tests
in 64.147 seconds on Linux x86-64 against the same Runyte 0.3.5 debug build with
all five current-feature expectation flags enabled.
