# ru-dbviewer

A native SQLite and PostgreSQL database viewer for Runyte, written in Rust.
Browse tables, inspect their schema and records, and execute SQL from ordinary
editor buffers. Queries run in background jobs; the editor stays responsive.

Targets Linux and macOS on x86-64 and ARM64. SQLite is bundled; PostgreSQL uses
Rustls. End users need no Python, virtual environment, compiler, `psql`, libpq,
or system OpenSSL. System CA certificates are used for verified TLS.

## Install

Build on Linux or macOS with Rust 1.88+ and a C compiler installed (on macOS,
install Xcode Command Line Tools). From the plugin repository:

```sh
cargo build --release --locked
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/ru-dbviewer "$HOME/.local/bin/ru-dbviewer"
```

Add this entry to the `plugins` list in your Runyte YAML configuration. Replace
`/absolute/path/to/ru-dbviewer` with the installed executable's full path, such
as `/home/alice/.local/bin/ru-dbviewer` on Linux or
`/Users/alice/.local/bin/ru-dbviewer` on macOS. Use an absolute path, not `~` or
`$HOME` in the YAML. If you already have a `plugins` list, append the entry
without adding a second `plugins` key.

```yaml
plugins:
  - id: dbviewer
    enabled: true
    executable: /absolute/path/to/ru-dbviewer
    args: []
    api: runyte-1
    runyte: ">=0.3.0, <0.4.0"
    capabilities:
      - views
      - interaction
      - documents
      - text
      - selections
      - jobs
      - settings
      - state
      - activity
```

To print the same configuration as JSON with your installed path filled in:

```sh
"$HOME/.local/bin/ru-dbviewer" --print-config
```

Keep the executable at that location. After changing the configuration, restart
Runyte (including any persistent host), then run `::db-connect` to connect to a
database. After rebuilding and replacing the executable at the same path,
`:plugin-restart dbviewer` reloads it.

Requires Runyte **>=0.3.0, <0.4.0**, with the stable `runyte-1` protocol. Restart
Runyte after configuration changes; an existing persistent host must restart too.
`:plugin-restart dbviewer` reloads the executable using the host's loaded config.

The release workflow builds four archives with checksums; no release has been
published by this implementation. Linux artifacts target musl. macOS artifacts
are unsigned, target macOS 13+, and require native CI acceptance before release.
Local acceptance is recorded in [VALIDATION.md](VALIDATION.md).

## Connect and browse

1. Run `::db-connect`, choose SQLite or PostgreSQL, and enter a unique profile name.
2. SQLite opens an **existing** file. Relative paths use the workspace root.
   PostgreSQL accepts a hostname or Unix socket directory, port, database and user.
3. PostgreSQL passwords come from a masked prompt or a named inherited environment
   variable. An empty prompt supports socket or certificate authentication.
4. In the catalog, move onto a table row and press Enter. Status/header lines
   are not selectable data rows. Press Enter over a result row to inspect its
   fields, then Enter over a field to inspect its retained value.
5. Tab exposes contextual actions: schema details, refresh, paging, visible
   columns, access mode, new SQL document and transaction controls.

`::db` opens saved profiles. Select a profile and press Enter to connect or browse.
At most two databases may be connected, with one operation per connection.
Profiles are private, nonsecret, workspace-scoped Runyte plugin state. Passwords,
SQL history and result values are not saved there. Plugin restart preserves the
profiles but requires explicit reconnection and SQL buffer reassociation.

TCP PostgreSQL defaults to certificate **and hostname** verification. A custom
CA PEM and client certificate/key PEM paths are supported. Plaintext TCP is an
explicit profile option; failed TLS never silently downgrades. Unix sockets do
not use TLS. The plugin does not read every libpq option or `.pgpass`.

## SQL buffers

| Command | Behavior |
| --- | --- |
| `::db-query` | Create an unsaved `.sql` document associated with the chosen database |
| `::db-use` | Associate the current buffer with a live connection |
| `::db-run` | Execute the captured whole buffer |
| `::db-run-selection` | Execute exactly one nonempty native selection |
| `::db-cancel` | Request cancellation of the connection's current operation |
| `::db-commit` | Commit a pending transaction |
| `::db-rollback` | Roll back a pending transaction |
| `::db-disconnect` | Disconnect; offer rollback if changes are pending |

There are no default global keybindings. Aliases are independent of the plugin
ID; full names are `:plugin.dbviewer.<local-name>`, such as
`:plugin.dbviewer.run`. The catalog's mode action is `:plugin.dbviewer.mode`.

One SQL statement per run is supported, including CTEs and a trailing semicolon.
DML and ordinary transactional table/index/view DDL are admitted. Transaction
control, session settings, COPY, multiple-statement scripts, and administrative
commands that require autocommit are outside this version's scope. SQL must also
be accepted by the selected dialect parser. `:write` saves the SQL file; it never
executes it. Unbound SQL parameters are not prompted for in this version.

Execution captures text, connection generation and source revision. Subsequent
edits, pane changes or active-database changes cannot retarget it. After reconnecting
or changing access mode, use `::db-use` again for existing SQL buffers.

## Writes and transactions

Connections start read-only. Use the catalog's **Change read-only/writable mode**
action to reconnect in writable mode. Every writable run first opens an immutable
SQL review buffer. Select its SQL row, press Enter, and confirm execution.
Editing the original buffer after capture does not change the reviewed statement.

Successful writable execution leaves a **PENDING COMMIT** transaction. Explicitly
Commit or Rollback before running another operation on that connection. Editor
text undo does not undo database changes. Pending transactions protect normal
workspace shutdown with an activity lease and roll back after at most five idle
minutes. Cancelling the activity rolls back. Closing a result buffer does not
commit. Detach retains the plugin, connection and pending transaction.

Query failures and cancellation attempt rollback. Failed connections are retired;
reconnect explicitly. A PostgreSQL connection is also retired after cancellation
or row/total-size truncation, preventing a late cancel packet from hitting later SQL.
A lost commit reply can leave the outcome unknown: the plugin keeps a recovery
marker, blocks reuse, and offers acknowledgement only after independent database
review. It never retries SQL automatically. Forced termination may leave a marker
even when the database ultimately rolled back.

PostgreSQL read-only transactions are an execution control, not a sandbox for
server functions; use a restricted database role where permissions matter.
Rollback cannot undo external function effects or PostgreSQL sequence advances.

## Results and limits

Browse pages contain 100 rows. Primary keys provide ordering where available;
this version uses offset paging. Tables without suitable keys and views may have
unstable order. External changes can shift pages even with primary-key ordering.
No automatic COUNT query or long-lived browsing snapshot is created.

SQL results retain at most 1,000 rows and 4 MiB per result, with 64 KiB per value.
Truncation is marked. Hitting the row or total-size limit rolls back writable
execution. Clipping an individual value preserves the transaction and connection.
Result paging and value inspection never rerun SQL. Close old database buffers
when the twelve-view limit is reached. Full record/value views share retained
result data; independent result views each own their bounded data.

Native tables show up to eight selected columns, clipped by Runyte to 32 terminal
cells each. **Choose visible column ordinals** accepts a comma-separated list,
for example `1,2,9`. Duplicate column labels remain distinct. Enter opens record
and value inspection. Wide records shorten field previews to fit the view budget;
every column remains selectable for retained-value inspection. Empty column names
appear as `(unnamed)` with distinct column identities. NULL, empty strings, binary values and truncation are
explicit; control characters are escaped. PostgreSQL values use their server text
representation, preserving decimals, arrays, enums, domains and extension types.

SQL input is limited to 256 KiB, and encoded host/model limits can impose a smaller
limit on heavily escaped content. The default query timeout is 30 seconds;
configure `settings: {query_timeout_seconds: 60}` in the plugin entry (1–300).
Connections have a ten-second timeout. Driver allocations for one exceptionally
large field can precede truncation; retained-data limits are not hard RSS limits.
There is no idle database polling.

## Development and checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tests/wire.py
PG_BIN=/path/to/postgresql/bin python3 scripts/postgres_tests.py
RUNYTE_BIN=/path/to/runyte python3 tests/native.py
```

The wire tests need Python's `jsonschema` package, only during development. Native
PostgreSQL tests need the server tools and `openssl`; they create and remove a
private temporary cluster with SCRAM, TLS, client certificate and socket tests.
Run as an ordinary user. A missing fixture must not be counted as a passing
integration check. Native Runyte tests use a PTY and isolated configuration,
workspace, runtime and data directories.

See [PLAN.md](PLAN.md) for the design, [VALIDATION.md](VALIDATION.md) for evidence
and remaining platform gates, and [VENDOR.md](VENDOR.md) for fixture provenance.
