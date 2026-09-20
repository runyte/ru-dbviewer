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
    bindings: {back: "-"}
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
2. SQLite opens an **existing** file. Both Profile name and Existing database path
   are required. Tab/Shift-Tab moves between fields; Enter validates them without
   losing entered values. Absolute paths work; relative paths use the workspace
   root. Enter a directory or filename prefix to open a searchable completion
   picker, then choose a directory to continue or a file to connect. The catalog
   shows the resolved path. Completion scans at most 4,096 entries and returns
   at most 62 matches; refine the path if the limit is exceeded. Paths are literal:
   no shell, environment-variable or tilde expansion is performed.
   PostgreSQL accepts a hostname or Unix socket directory, port, database and user.
3. PostgreSQL passwords come from a masked prompt or a named inherited environment
   variable. An empty prompt supports socket or certificate authentication.
4. Titles identify the current view: **[databases]**, **[tables]**, **[rows]**,
   **[record]**, **[value]**, **[results]**, **[schema]**, **[filters]** or
   **[transactions]**. Database, table, row and field breadcrumbs remain visible;
   PostgreSQL table names include their schema. Labelled metadata above the content
   shows the database path, access mode, rows, filters and sort as relevant.
5. Select a table row and press Enter, then Enter on a result row to inspect its
   fields, then Enter on a field to open its formatted preview. Metadata lines are
   not selectable database rows. `-` or **Tab → Back** returns through Value →
   Record → Rows → Tables → Databases. Parents retain their cursor, viewport and
   results; returning never replays SQL. If a parent closed, Back opens Databases.
6. Tab opens one searchable menu with relevant commands grouped under headings
   such as **Inspect**, **Navigation**, **Filter and sort**, **Columns and paging**,
   **Table**, **SQL**, **Database** and **Pending changes**. Headings are not actions.
   Enter performs the current primary action without a separate Activate menu entry.
   **Add database** creates a profile; **Connect** appears for a disconnected profile.
   Pending Commit/Roll back actions retain their confirmations and connection checks.
   Record/value menus focus on inspection and navigation; global `db-*` aliases
   keep database lifecycle commands available. Back never disconnects.
7. **Tab → Show full value**, from a selected record field or its preview, loads
   the complete captured value into one read-only document. Normal search,
   selections and copying cover all its text. **Show raw text** / **Format JSON**
   switches display without querying the database. `::db-cancel` from the loading
   value cancels its display job; Back returns directly to its record.

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
| `::db-return` | Return from SQL to its source browsing view, or Databases if closed |
| `::db-transactions` | Inspect and settle pending transactions across connected databases |

The installation configuration binds `-` to back only in database views. Ordinary
SQL buffers keep normal editing keys, including Tab. Aliases are independent of the plugin
ID; full names are `:plugin.dbviewer.<local-name>`, such as
`:plugin.dbviewer.run`. The catalog's mode action is `:plugin.dbviewer.mode`.

New query buffers use `<profile>-query-YYYYMMDD-HHMMSS-nanoseconds-counter.sql`
with local time and filename-safe profile names. Host collision checks and retries
protect existing files and open buffers. Creating a query writes no file. Initial
SQL comments teach execution, association, saving and return navigation; editing
those comments never changes the authoritative connection association.

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

Connections start read-only. **Access mode** on a selected connected profile or catalog
opens exactly **READ ONLY** and **READ AND WRITE**, with the current choice in the
title. Switching to writable requires confirmation; read-only does not. Pending
changes must be committed or rolled back before choosing another mode. Reconnect
invalidates previous SQL associations: use `::db-use` again. Every writable run first opens an immutable
SQL review buffer. Select its SQL row, press Enter, and confirm execution.
Editing the original buffer after capture does not change the reviewed statement.

Successful writable execution leaves a **PENDING COMMIT** transaction. Explicitly
Commit or Rollback before running another operation on that connection. Editor
text undo does not undo database changes. Pending transactions protect normal
workspace shutdown with an activity lease and roll back after at most five idle
minutes. Cancelling the activity rolls back. Closing a result buffer does not
commit. Detach retains the plugin, connection and pending transaction.

`::db-transactions` and **Tab → Transactions** show each pending database, state,
age at refresh, bounded statement summary and known affected-row count. Unknown
counts stay unknown. Select a row and use **Tab → Commit**, **Roll back**, or
**Disconnect**. The list updates on settlement, failures and idle rollback; age
display adds no polling timer. Summaries remain in memory, never saved profile
state or logs.

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

Browse pages default to 100 rows; **Page size** accepts 1–100. Top metadata shows the
visible range and page size, including empty pages, without claiming a total.
Database paging issues a bounded query; SQL-result paging reads retained data.
**Sort rows** chooses a column and direction; primary keys break ties where available.
Without suitable keys, ordering can be unstable. External writes can shift offset
pages. No automatic COUNT query or long-lived browsing snapshot is created.

**Edit filters** opens an editable draft. Add, edit, remove, temporarily disable or clear
up to sixteen conditions, then **Apply filters** returns to page one. **Match all or any**
chooses ALL (AND) or ANY (OR) for enabled conditions. Sorting, page size and column
choices survive application; subsequent pages retain the filters. Active filters
appear above the rows under **Filters**, beside **Sort** and **Page size**. Nested Boolean groups remain a SQL use case.

Column names use stable ordinal identities even when labels are duplicate or
empty. Filter values are bounded to 4,096 bytes and always bound parameters.
Equality, ordered comparisons, literal contains and NULL checks are supported.
NULL checks need no value; empty text means an empty string. `%`, `_`, quotes and
backslashes in contains are literal characters, not patterns or SQL. Numeric
columns require decimal text (no exponent, NaN or infinity) and reject contains;
SQLite applies its NUMERIC conversion and dynamic typing. Other columns compare
server text representations. Conversion failures report an error and do not apply
the condition. Disabled conditions are excluded from SQL.

**Open current browse as SQL** creates an unsaved, independently runnable statement with selected
columns, enabled filters, ordering, and the current LIMIT/OFFSET page. It uses
quoted identifiers and dialect-specific literal rendering, with no unresolved
parameters. Creation never executes the statement.

SQL results retain at most 1,000 rows and 4 MiB of previews per result, with
64 KiB per preview. `…` marks shortened text. Hitting the row or total preview
limit explicitly marks an incomplete result and rolls back writable execution;
shortening an individual preview preserves the transaction and connection.

The original database read captures larger complete values in private anonymous
temporary files, never by replaying SQL. Record/value descendants share those
immutable sources even if the database changes. Files are unlinked when created,
closed on final-reference release, and disappear after process termination; no
values go into profiles, query history or durable cache files. Limits are 8 MiB per
complete textual representation, 32 MiB per result, 64 MiB and 32 result files per
plugin process. An unavailable source retains its preview and explains why full
inspection is unavailable. Close old database buffers to release retained sources
or the twelve-view budget.

Full inspection requires negotiated `view-document` and `job-feedback` features
of `runyte-1`. Older hosts retain previews and navigation without receiving new
wire fields; they cannot load full documents through this action. Readable action
presentation and top metadata independently negotiate `view-action-presentation`
and `view-metadata`; older hosts use legacy menus and a labelled status summary.
Command identifiers and aliases remain unchanged: spaces belong in visible labels,
so `connect-new` remains the binding identifier for **Add database**.

Native tables show up to eight selected columns, clipped by Runyte to 32 terminal
cells each. **Choose columns** is a searchable checklist: toggle named entries and choose
**Apply selections**. **Find column…** searches all names; paged choices keep wide
results bounded. Selections survive searches. Duplicate column labels remain distinct. Enter opens record
and value inspection. Wide records shorten field previews to fit the view budget;
every column remains selectable for retained-value inspection. Empty column names
appear as `(unnamed)` with distinct column identities. NULL, empty strings, binary values and truncation are
explicit; control characters are escaped. PostgreSQL values use their server text
representation, preserving decimals, arrays, enums, domains and extension types.

JSON previews open as an indented tree when they fit its 64-level/4,096-node
budget. Enter expands/collapses containers. A shortened prefix can be indented
without inventing missing delimiters and ends with `…`; malformed or oversized
preview trees remain readable text. Raw preview mode preserves the retained
prefix, with controls escaped.

Full-value formatting is separate from that optional preview tree: it preserves
duplicate keys, exact number spellings, Unicode and every string, including values
with more than 10,000 formatted lines. Original raw text remains available. Complete
text and formatted output are each bounded to 8 MiB and 250,000 lines; if formatting
would exceed the bound, complete raw text is shown with an explanation. Publication
has a 16 MiB encoded-model limit. A refused publication keeps the previous preview
readable and offers a retry. One cancellable display job runs at a time, with a
60-second deadline; it never commits or rolls back a pending database transaction.

Binary values and invalid UTF-8 use complete labelled hexadecimal representations
within the same bound. Text with unsupported controls uses a labelled lossless
escaped representation: controls become `\u{hex}` and literal backslashes are
doubled. Copying copies the displayed representation. Ordinary UTF-8 text, tabs
and newlines remain unchanged in raw mode. NULL and empty text need no full load.

SQL input is limited to 256 KiB, and encoded host/model limits can impose a smaller
limit on heavily escaped content. The default query timeout is 30 seconds;
configure `settings: {query_timeout_seconds: 60}` in the plugin entry (1–300).
Connections have a ten-second timeout. Driver allocations for one exceptionally
large field can precede bounded capture; retention limits are not hard RSS limits.
There is no idle database polling.

## Development and checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tests/wire.py
python3 tests/interactive.py
PG_BIN=/path/to/postgresql/bin python3 scripts/postgres_tests.py
RUNYTE_BIN=/path/to/current/runyte DBVIEWER_EXPECT_ROW_ACTIONS=1 DBVIEWER_EXPECT_FULL_VALUES=1 python3 tests/native.py
```

The wire tests need Python's `jsonschema` package, only during development. Native
PostgreSQL tests need the server tools and `openssl`; they create and remove a
private temporary cluster with SCRAM, TLS, client certificate and socket tests.
Run as an ordinary user. A missing fixture must not be counted as a passing
integration check. Native Runyte tests use a PTY and isolated configuration,
workspace, runtime and data directories.

See [PLAN.md](PLAN.md) for the design, [VALIDATION.md](VALIDATION.md) for evidence
and remaining platform gates, and [VENDOR.md](VENDOR.md) for fixture provenance.
