# dbviewer development guide

Read README.md, PLAN.md and VALIDATION.md before substantial work. This is an
independent Runyte application, not a private editor component. The public
runyte-1 protocol and retained schema are the integration boundary.

Preserve user changes and inspect Git status first. Keep SQL execution captured,
revision-bound and tied to a connection generation. Never automatically replay
SQL. Keep query, cancellation and protocol I/O independent and bounded. Database
errors and diagnostics must not echo passwords, SQL values or connection URLs.

Put fixtures in temporary directories. Never use personal databases, configuration
or caches in tests. Never run a test-generated executable; compile project sources,
use installed database tools, or run checked-in fixture programs. Keep SPDX headers
and MPL-2.0 compatibility. Don't publish, push or release without authorization.

Before handoff run cargo fmt --check, cargo clippy --locked --all-targets -- -D warnings,
and cargo test --locked. Run affected public-wire/native/database suites too.
Preserve the plugin CI line-coverage floor of 75% using the combined behavior suites.
Record exactly which platforms and fixtures actually passed. macOS CI configuration
is not evidence of macOS acceptance. Work on the sibling Runyte repository requires
its own AGENTS.md and coverage invariants.
