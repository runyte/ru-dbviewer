# TEST-004: Native PostgreSQL fixture reads the caller's psql startup file

Status: fixed; independent re-review clean.

`scripts/postgres_tests.py` invokes installed `psql` without `-X`. The installed
client's `psql --help` documents that `-X, --no-psqlrc` disables reading
`~/.psqlrc`. Without it, temporary-database setup can execute personal startup
commands, violating this repository's fixture-isolation requirement and making
database acceptance depend on local configuration.

Pass `-X` to the fixture's sole `psql` invocation. The connection destination,
database, role and server storage already explicitly belong to the temporary
fixture. No personal configuration is opened to verify this correction.

Python syntax parsing passed. The native PostgreSQL server launcher has not been
executed locally because installed `initdb`/`pg_ctl` are unavailable; container
database acceptance does not establish this native-launcher path.
