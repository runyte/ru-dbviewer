<!-- SPDX-License-Identifier: MPL-2.0 -->
# TEST-001: Reproducible PostgreSQL coverage without native server tools

Status: resolved and independently reviewed. Category: validation infrastructure.

The ordinary Rust suite deliberately ignores PostgreSQL integration cases, and
`scripts/postgres_tests.py` requires locally installed server binaries. This
review environment has PostgreSQL 16/17 container images but only native client
tools. A disposable container fixture will make the same real database, TLS,
client certificate, Unix socket and lost-commit tests reproducible here.

Use only an explicitly selected, already installed image; bind an ephemeral
loopback port; create certificates and socket storage in a private temporary
root; remove only the container created by the launcher in a finally block.
No application behavior or CI/native launcher defaults change.

Validation: all five existing PostgreSQL integration suites passed on Linux
x86-64 with PostgreSQL 17.9 from the already installed `postgres:17` image,
including verified/rejected TLS, client certificates, Unix sockets, cancellation,
and lost commit acknowledgement. The database team's nested PostgreSQL reviewer
found no actionable fixture issues. Native macOS and ARM64 are not claimed.
