# TEST-002: Native navigation acceptance assumes legacy path completion

Status: fixed; independent re-review clean.

`test_native_completion_back_and_unsaved_sql` always expects the plugin's
`Resolved paths` picker. A host negotiating `input-path-completion` instead
completes the path in the SQLite form, so the test times out before it can test
navigation, unsaved execution or saving. The dedicated live-completion case
already passes on that same host.

The Linux x86-64 baseline against local Runyte 0.3.5 (`ee679f45`) reproduced the
failure with `../tasks.sqlite3` visible in the form. Update the test to follow
the native Tab-completion path when explicitly expected and retain the legacy
submitted-path picker on the historical CI host.
Without the feature-expectation flag, accept either the legacy picker or the
completed current-host form so ordinary current-host runs remain usable too.

The focused native test now passes on Linux x86-64 with the same local Runyte
0.3.5 host and all five current-feature expectation flags enabled (6.839 seconds).
Historical-host and other-platform execution are not claimed by this check.
