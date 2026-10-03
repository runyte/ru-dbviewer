# TEST-006: PostgreSQL fixture inherits caller libpq connection configuration

Status: fixed; independent re-review clean.

The native PostgreSQL launcher supplies explicit `psql` host, port, user and
database arguments but still inherits all caller `PG*` environment variables.
`PGHOSTADDR` can select a network destination independently of the explicit
socket-directory host. `PGSERVICE`/`PGSERVICEFILE` and passfile settings can also
load caller configuration. Disabling `psqlrc` with `-X` (TEST-004) does not disable
libpq's separate connection-option/environment processing.

The fixture's setup helper now removes inherited `PG*` options from its `psql`
environment, confines passfile/service lookup to fixture-owned paths, disables
unused SSL/GSS modes for its explicit Unix socket, and never prompts for a
password. Cargo and coverage environments remain unchanged.

An installed PostgreSQL 18.6 `psql` independently reproduced address parsing from
a synthetic `PGHOSTADDR` and service lookup from a temporary `PGSERVICEFILE`,
despite `-X` and an explicit temporary socket host. No network/database service
or personal configuration was used. `tests/postgres_fixture.py` checks coverage
environment preservation and invokes the installed client against a nonexistent
fixture socket with both hostile settings supplied. The corrected helper reaches
only that socket. CI now runs these regressions before the native cluster suites.

Native server acceptance remains unexecuted locally because `initdb`/`pg_ctl`
are not installed; this check establishes client isolation, not cluster setup.
