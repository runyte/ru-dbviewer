# dbviewer implementation plan

Status: implementation delivered; native macOS/ARM64 and release-artifact acceptance remain CI gates.
Date: 2026-09-17.


## Implementation record (2026-09-17)

The Rust plugin, SQLite/PostgreSQL adapters, native workflows, transaction controls,
public-wire/native integration tests, and four-target CI/artifact workflows are
implemented. See VALIDATION.md for what actually passed and the outstanding
platform release gates. No release or commit was made during implementation.

Implementation refinements relative to the proposal:

- SQLite runs on Tokio's bounded blocking-worker pool with exclusive per-connection
  ownership, rather than retaining one dedicated OS thread per idle database.
- Browsing currently uses primary-key-ordered offset pages. Keyset pagination is
  deferred; changing external data can shift pages and keyless views are unstable.
- The 4 MiB result budget applies per retained result, with twelve native views
  bounding retained collections. Record/value views share their result allocation.
  A total per-connection retained-result budget is not implemented.
- PostgreSQL uses prepared single-statement validation plus streaming text results
  so database-defined types retain their server representation. Connections are
  retired after cancellation/result truncation to isolate late cancellation packets.
- One 600-second activity lease covers a query and up to five minutes awaiting
  transaction resolution; the idle deadline is capped before lease expiry. No
  periodic lease renewal or database polling is needed.
- Reconnection requires explicit reassociation of existing SQL buffers. Captured
  write reviews also retain the exact connection generation.
- The plugin-specific coverage floor starts at 75% across combined Rust, wire,
  real-database and native-editor tests. The measured Linux result is recorded
  separately; Runyte's existing coverage register is unchanged.

## Objective and scope

Build `ru-dbviewer`, an independent Rust executable that provides native SQL
database browsing and query execution inside Runyte. The configured plugin ID
is `dbviewer`. Support local SQLite files and local/remote PostgreSQL databases
on Linux and macOS, with x86-64 and ARM64 release binaries. Users need no Python,
venv, Rust toolchain, database CLI, or separately installed database client library.

The first usable milestone is SQLite browsing and read-only SQL execution. The
initial release additionally includes PostgreSQL, deliberate writable execution,
connection profiles, cancellation, and tested binary installation.

Keep Runyte's editor central: ordinary SQL buffers, native application views,
forms, actions and jobs. No separate terminal UI, private Runyte API dependency,
or host change is assumed. If a public API gap blocks a milestone, record it
and propose the smallest host change separately before expanding this scope.

Out of scope for the first release: editable result cells, multiple-statement
scripts, arbitrary transaction-control SQL, commands requiring execution outside
a transaction, COPY streaming, database creation, migration management, ORM
features, SQL completion, automatic query history, exports, managed SSH tunnels,
remote SQLite transfer, credential-manager integration, and Windows.
Existing external tunnels can be addressed as ordinary PostgreSQL endpoints.

## Architecture and dependencies

Use an independent Cargo package with a committed lockfile, MPL-2.0-compatible
sources and retained notices for any reused example code. Set the Rust minimum
after checking the chosen dependency versions, then enforce it in CI.

Preferred dependencies:

- `tokio`, `serde`, `serde_json`: scheduling and the public JSON wire protocol.
- `rusqlite` with bundled SQLite and hooks: a dedicated blocking worker owns
  each SQLite connection; interruption and authorization remain available.
- `tokio-postgres`: asynchronous PostgreSQL connections, prepared statements,
  streaming results and explicit cancellation tokens.
- Rustls with a maintained PostgreSQL TLS adapter: certificate and hostname
  validation, system trust roots and an explicit custom CA option.
- A dialect-aware SQL parser if needed for the deliberately restricted execution
  policy; it does not replace database enforcement of read-only access.

This refines the earlier SQLx suggestion. A database viewer benefits from direct
access to cancellation, SQLite authorization, and backend-specific metadata.
Keep a small application-level adapter interface rather than forcing identical
driver APIs. Do not implement a PostgreSQL wire driver or TLS stack.

Before committing to driver versions, prove cancellation, arbitrary-column
results, statement boundaries, transaction-state handling and TLS on both OSes.
If the preferred drivers cannot meet these requirements, record the evidence
and revise this section before building the rest of the application.

Suggested ownership:

```text
src/main.rs                 CLI and process lifetime
src/protocol/               bounded framing, DTOs, correlation, negotiation
src/app/                    commands, captured invocations, jobs, lifecycle
src/db/mod.rs               database operations and presentation-neutral values
src/db/sqlite.rs            SQLite connection worker and metadata queries
src/db/postgres.rs          PostgreSQL connection task and metadata queries
src/query.rs                immutable execution intent and execution policy
src/results.rs              bounded results, identities and value formatting
src/views/                  connection, catalog, data, record and result models
src/profiles.rs             nonsecret connection profiles and secret resolution
tests/                     protocol, database, lifecycle and installation tests
```

One actor owns each live database connection. Serialize operations on that
connection; allow at most two connected databases initially, one operation per
connection, bounded queues and no connection pool. Other connections and the
editor remain usable while one query runs. Browse using the same connection so
its identity and transaction semantics stay explicit.

## Runyte contract

Target `runyte-1` and author the initial supported range as
`>=0.3.0, <0.4.0`, subject to acceptance on the actual supported hosts. Record
the schema/example source revision separately. A bootstrap candidate is not
evidence of a published release. Do not import private frontend DTOs or link
the Runyte crate as the plugin API.

Implement only the required public methods, with typed DTOs and conformance
fixtures. The existing Rust todo example is a reference, not a complete SDK.
Keep stdout exclusively for protocol traffic. Bound encoded frames before JSON
decoding, outbound bytes, pending requests, callbacks and database work. Keep
the reader, writer and cancellation path independent of long queries. A blocked
writer has a deadline; EOF and protocol failure stop admitting database work.

Expected grants: `views`, `interaction`, `documents`, `text`, `selections`,
`jobs`, `settings`, `state`, and `activity` for unresolved transactions.
Add `workspace` only if a concrete operation needs its metadata. No providers,
filesystem mutation, terminal, or external-process grants are needed initially.

Respect limits advertised by the host, including the 1 MiB encoded frame limit,
view/model quotas and outstanding-request limits. Job creation must complete
before returning its handle as accepted work. Handle cancellation arriving
before the creation response is processed.

Capture invocation, pane/buffer/view handles, revisions and connection generation
with every action. Background results update an existing owned view; they never
follow the active pane or reopen a closed view. Create/show a pending result view
while foreground authority is available, then publish into it. Reject stale
actions and abandon stale publications without rerunning SQL.

## User workflow

Proposed aliases, all registered through Runyte's command registry:

| Alias | Action |
| --- | --- |
| `::db` | Open connection profiles and live connection status |
| `::db-connect` | Choose/create a profile and connect |
| `::db-disconnect` | Disconnect; resolve any pending transaction first |
| `::db-query` | Create a named, unsaved `.sql` document for a connection |
| `::db-use` | Associate an existing SQL buffer with a connection |
| `::db-run` | Execute the captured whole SQL buffer |
| `::db-run-selection` | Execute exactly one captured, nonempty operative span |
| `::db-cancel` | Request cancellation of the selected connection's operation |
| `::db-commit` / `::db-rollback` | Resolve a pending writable transaction |

Local command names omit the `db-` prefix; full names remain
`:plugin.dbviewer.<name>`. No default global bindings in the first release.
Native Enter activates a connection/table/row; Tab exposes registered contextual
actions including refresh, schema inspection, paging, column selection and mode
changes. Preserve ordinary navigation, search, copying and pane operations.

Connection browser -> schema/table/view list -> paged data -> record details.
Catalog details include column names/types/nullability/defaults, primary and
foreign keys and indexes, with backend differences shown honestly. Show system
schemas only on request. Use catalog identity rather than interpolated display
names. Quote identifiers with the backend's rules; bind filter values.

SQL documents use collision-free workspace-relative names through `buffer.create`;
this creates an unsaved document, not a disk write. `:write` saves SQL text and
never executes it. Existing files work through `::db-use`. Buffer associations
are in memory and must be chosen again after plugin restart. Connection switching
elsewhere cannot retarget an already associated buffer or captured execution.

Read buffer text through a revision-bound immutable snapshot and use native
selection spans. Reject multiple selections in v1 rather than concatenating
them. Capture SQL, connection, mode and limits once; later edits do not change
accepted work. Results identify the connection and source revision. A confirmation
authorizes the exact captured statement, never a fresh read of changed text.

## Result representation and browsing

Native models allow eight columns and clip their rendered cells at 32 terminal
cells. Select up to eight visible columns; retain column ordinals as identities
so duplicate SQL column labels remain distinct. Provide record details with all
columns and a bounded full-value inspection action. Do not promise an unlimited
spreadsheet grid through this API.

Initial defaults: 100 rows per browse page; 1,000 retained rows per ad hoc query;
4 MiB retained result payload per connection; 64 KiB per retained value;
256 KiB SQL input; 10-second connection timeout and 30-second query timeout.
Count actual encoded model bytes and reduce publication chunks/pages to fit host
limits. Mark every truncation; never silently substitute NULL or empty text.
Document that driver/server allocations for one very large field may precede
application truncation: these budgets are not a hard process-RSS guarantee.

Distinguish NULL, empty string, text, numbers, booleans and blobs. Preserve integer
and decimal precision, escape control characters for native views, and show
backend type names. PostgreSQL arrays, JSON, UUIDs, temporal values, enums,
domains and unknown extension types need explicit formatting tests. Unknown
types get an honest type/unsupported marker, not a guessed decoding or crash.

Prefer primary-key ordering for browsing and keyset paging where supported.
For tables without a suitable key and for views, use bounded offset paging and
label unstable ordering. Do not add an implicit full COUNT query. Refresh starts
a new browse generation; concurrent external writes can change pages. No stable
database-wide snapshot is promised across interactive browsing.

Ad hoc SQL is executed once. Retain and page its bounded result locally; never
rerun a statement to display another result page or fetch a previously truncated
value. Re-execution requires a fresh user action. Streaming stops at the result
budget; the adapter must settle cancellation/transaction state before reuse.
Reaching a result cap is not proof that a writable query completed successfully.

## Execution and transaction policy

Profiles start in read-only mode. SQLite opens an existing file read-only, does
not silently create missing databases, disables extension loading, and uses
authorization/query-only controls to prevent mode-changing SQL and attachment
escapes. PostgreSQL runs accepted reads within explicit read-only transactions.
Recommend a database role with read-only permissions for stronger enforcement;
database read-only transactions are not a sandbox for arbitrary server functions.

V1 accepts one statement per run, including a trailing terminator. Determine
boundaries using backend preparation and dialect-aware parsing, never a split
on semicolons. SQL strings, comments, CTEs and PostgreSQL dollar quotes must work.
Validate the whole input before executing any part. Reject multiple statements,
transaction-control SQL, COPY and unsupported administrative commands explicitly.

Writable mode requires a deliberate native action for the specific connection.
Each writable execution shows the target and captured SQL for confirmation,
then runs one supported DML/DDL statement in a plugin-owned transaction.
Success leaves that transaction pending with explicit Commit and Rollback actions;
no further SQL or browsing on that connection runs until it is resolved.
This includes potentially mutating functions and SELECT-like statements: do not
derive write permission from the first SQL keyword. Reverting to read-only mode
requires transaction resolution and a newly configured connection.

Query errors and cancellation request rollback. Acquire a renewable activity
lease before retaining a pending writable transaction; if protection cannot be
established, roll back. Lease cancellation rolls back before releasing protection.
An idle pending transaction has a five-minute deadline and is rolled back when
it expires. Lease renewal runs only while needed. Closing a result view does not
commit; the connection view continues to expose unresolved state. Disconnect
offers rollback or cancellation, never implicit commit.

Track ready, running, cancelling, pending-commit, failed-transaction,
outcome-unknown and disconnected states. A PostgreSQL cancellation request is
inherently racy; its successful transmission does not prove cancellation.
Wait for authoritative completion/rollback where possible; otherwise retire
the connection. Loss of a commit acknowledgement is outcome-unknown. Never
automatically reconnect and replay SQL or claim rollback undoes nontransactional
effects such as sequence advances or external function side effects.

Detach preserves plugin ownership and accepted jobs. Stop/EOF cancels work,
attempts bounded rollback and closes connections; forced termination cannot
promise a final report. A fresh process never restores a live transaction.
Uncertain writes retain a nonsecret recovery marker for the profile/operation;
reconnection does not resolve the previous outcome. Query text and row data
are not persisted in that marker. Review the database before explicit acknowledgement.

## Profiles, authentication and storage

Use Runyte's versioned nonsecret workspace state for saved profiles, the last
selected profile and display preferences. Use settings for execution limits.
Profiles include SQLite path or PostgreSQL host/socket, port, database, username,
TLS options and an optional environment-variable name for the password.
Resolve relative SQLite paths against the workspace root, not the plugin's
installation directory. Require explicit profile selection; no automatic scan
of project configuration or network endpoints.

Passwords come from a masked native prompt or the named inherited environment
variable. Never put passwords or credential-bearing URLs in command arguments,
saved profiles, view titles or diagnostics. Do not log SQL or result values by
default; sanitize database errors that can echo input or connection strings.
Do not claim compatibility with every libpq option or `.pgpass` until implemented.

For remote PostgreSQL, default to TLS with hostname and certificate verification.
Support a custom CA and client certificate/key paths. Local Unix sockets need
no TLS; plaintext TCP is a deliberate per-profile option, with no automatic
downgrade. Credentials remain in memory only as long as needed for the connection.
Saving query text as an ordinary file is an explicit editor action.

## Packaging and platform contract

Produce archives plus SHA-256 checksums for Linux x86-64/ARM64 and macOS
Intel/Apple Silicon. Prefer Linux musl builds, validated on native architecture
runners; macOS uses native builds. Pin and document minimum supported OS versions
after the first native compatibility spike. Do not call an artifact portable
merely because cross-compilation succeeded.

Bundle SQLite and use Rustls so installing database client libraries or OpenSSL
is unnecessary. System trust certificates and ordinary OS facilities still
matter. Verify linked dependencies and license notices for every release target.

CLI: normal invocation speaks the plugin protocol; `--help`, `--version`, and
`--print-config [--plugin-id ID]` perform no database connection or state write.
Generated configuration uses the installed executable's absolute path, authored
host range and required grants. Installation is unpack, place at a permanent
path, generate configuration, enable and restart the persistent host if needed.

Release from a pinned commit with locked dependencies and passing target tests.
Document unsigned macOS status honestly unless signing/notarization credentials
are provisioned. Signing, notarization, Homebrew and crates.io publication are
separate distribution steps, not prerequisites for implementing the plugin.
No publish, push or release occurs as part of this plan.

## Implementation sequence and acceptance

1. **Foundation and risk probes.** Add project guidance, Cargo package, license,
   notices, minimal CLI, protocol transport and Linux/macOS CI. Prove handshake,
   foreground result-view creation, named SQL documents, bounded backpressure,
   EOF, driver cancellation, statement boundaries and TLS. Verify driver choices
   and platform minimums. Gate: executable launches in a real supported host;
   paused/full protocol pipes cannot strand cancellation indefinitely.
2. **SQLite browsing.** Add profiles/forms, read-only connection worker, catalog,
   schema details, data paging, column selection and record inspection. Gate:
   real temporary databases, quoted/Unicode identifiers, empty/wide tables,
   views, composite keys, missing/locked files and large values behave correctly.
3. **Read-only query workflow.** Add SQL document association, immutable captures,
   explicit whole-buffer/selection execution, finite jobs, bounded result views
   and cancellation. Gate: editing, switching panes, closing results and detach
   cannot change the target, steal focus, rerun SQL or write to a read-only DB.
4. **PostgreSQL parity.** Add native driver, metadata, formatting, TLS, password
   handling and Unix sockets. Gate: real isolated PostgreSQL fixtures on Linux
   and macOS, including SCRAM auth, TLS verification failures, custom CA, query
   cancellation, server disconnect and unsupported values. Pin tested server
   majors (initially 16 and 17); extend support claims only with evidence.
5. **Writable execution.** Add mode changes, exact-intent confirmation, managed
   transaction state, commit/rollback, activity lease and recovery markers.
   Gate: SQLite and PostgreSQL tests demonstrate refusal before execution,
   durable commit, rollback, failed/cancelled writes, result-cap handling,
   disconnect during commit and no replay after restart.
6. **Release readiness.** Complete reproducible real-host smoke tests, native
   four-target binary checks, installation/configuration documentation and user
   workflow examples. Gate: artifact runs without Rust/Python/database client
   installs; clean stop, restart and persistent detach pass on both OSes.

Each milestone is a reviewable change with its behavior tests and documentation.
Do not defer PostgreSQL/TLS/cancellation experiments until after the SQLite UI
has made the architecture expensive to change.

## Validation strategy

- Unit tests cover formatting, precise numbers, identifier quoting, statement
  admission, page identity, profile redaction, immutable execution intent and
  state transitions. Test observable behavior rather than DTO copies alone.
- Public-wire tests cover registration, actual encoded limits, stale handles,
  request correlation, early cancellation, full queues, stalled output and EOF.
- Database integration tests use temporary SQLite files and private temporary
  PostgreSQL clusters. Real servers are required for cancellation and transaction
  outcomes; mocks alone do not establish these guarantees. Fault injection
  exercises lost replies and commit uncertainty without production credentials.
- Real-host tests pin Runyte revisions and exercise rendering, forms, aliases,
  jobs and lifecycle using public protocol/native paths. A private test harness
  may validate the host, but private DTOs never enter the shipped plugin.
- Run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  and `cargo test --locked` on Linux and macOS. Record coverage and introduce a
  plugin-specific meaningful baseline after initial behavior exists. Any separately
  approved Runyte change must preserve Runyte's own coverage floor and full checks.
- All fixtures and database/state paths are temporary. Never touch personal
  databases, configuration or cache paths. Never execute a test-written script;
  use checked-in fixtures or installed test programs. Keep test output redacted.
- Verify idle behavior: no periodic database polling, model publication, or
  activity renewal after all queries and transactions have settled.

## References and implementation checkpoints

Runyte source references, checked during planning: `docs/plugins.md`,
`docs/plugins/authoring.md`, `docs/plugins/applications.md`,
`docs/plugins/runyte-1.schema.json`, `docs/plugins/compatibility.md`,
`docs/plugins/conformance.md`, `docs/plugins/todo/rust/`, and the UI vocabulary
and keymap registers under `context/reference/`. Pin copied fixtures with source
revision and license; do not rely on a sibling checkout at runtime.

Driver references:

- [rusqlite connection hooks and interruption](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html)
- [rusqlite bundled builds](https://github.com/rusqlite/rusqlite)
- [tokio-postgres client operations](https://docs.rs/tokio-postgres/latest/tokio_postgres/struct.Client.html)
- [PostgreSQL cancellation semantics](https://docs.rs/tokio-postgres/latest/tokio_postgres/struct.CancelToken.html)

The foundation milestone must settle exact dependency versions/TLS adapter,
minimum Rust/OS versions, and driver transaction-state/statement-admission
mechanisms. These are implementation checkpoints, not claims already verified.
