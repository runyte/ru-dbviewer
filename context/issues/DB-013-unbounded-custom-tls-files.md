<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-013: Custom TLS files can strand a blocking worker or consume unbounded input

Confirmed in `tls_config`: profile-specified CA, certificate and key paths use
blocking `File::open` and unbounded PEM parsing. A FIFO waits for a writer even
after the connection caller times out; a very large or endless input has no
read budget. Tokio cannot cancel a running blocking task.

Open custom TLS inputs nonblocking, require a regular file on the opened
descriptor, and cap each PEM input at 4 MiB including a bounded read after the
size check. This permits symlinked regular certificate files. Keep diagnostics
limited to fixed file roles and limits, without paths or key contents.

Regression fixtures use an owned temporary FIFO with an explicit release writer,
an oversized sparse regular file and a fixture-owned listening socket. Existing
real PostgreSQL TLS checks additionally exercise a symlink to the fixture CA.
The platform's native root loader remains provided by rustls-native-certs; an
already running blocking filesystem operation cannot be forcibly interrupted.

Validation: the owned FIFO first exceeded the two-second regression guard
before the fix. After the fix all 11 PostgreSQL 17.9 cases and both PostgreSQL
public-wire cases passed on Linux x86-64, including FIFO/oversized-file refusal,
symlinked CA acceptance, hostname/CA rejection, client certificates and sockets.
The launcher's private empty native-root overrides were active for this run.
