# TEST-008: Failed native PostgreSQL startup can leave a running fixture server

Status: fixed; independent re-review clean.

The native PostgreSQL launcher records `started` only after `pg_ctl -w start`
succeeds. A startup failure therefore skips shutdown and deletes temporary server
storage. PostgreSQL's documented contract explicitly permits a timed-out startup
to continue and eventually succeed in the background:
[pg_ctl documentation](https://www.postgresql.org/docs/18/app-pg-ctl.html).

The launcher now attempts shutdown whenever startup was attempted. If shutdown
fails, it accepts only `pg_ctl status` exit code 3 as confirmation that no server
remains. Otherwise temporary storage is retained, and cleanup diagnostics are
attached to an existing startup/test failure instead of masking it. Successful
test execution with failed cleanup reports its own failure. Startup/shutdown
timeouts explicitly use 60 seconds instead of inheriting `PGCTLTIMEOUT`.

Six deterministic regressions cover successful shutdown, startup failure followed
by shutdown, confirmed no-server status, still-running/unknown status with retained
storage, preservation of the original exception, and failure before startup.
`python3 tests/postgres_fixture.py` passed all nine cases on Linux x86-64; Python
syntax parsing and `git diff --check` passed. These lifecycle regressions mock
subprocess outcomes around the checked-in helper and establish launcher control
flow, not real-server acceptance. No generated executables were used. Installed
native PostgreSQL server tools remain unavailable locally.
