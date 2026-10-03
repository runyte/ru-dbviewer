# TEST-007: PostgreSQL fixtures read caller OpenSSL configuration

Status: fixed; independent re-review clean.

Both PostgreSQL fixture launchers inherit `OPENSSL_CONF` when generating TLS
certificates. A temporary configuration containing a deliberately malformed line
made the launcher's installed `openssl req` command fail parsing that exact file.
Caller OpenSSL configuration can therefore alter fixture setup, violating the
requirement that tests do not use personal configuration.

Follow-up source review found that native PostgreSQL itself loads OpenSSL
configuration during TLS initialization. PostgreSQL 17 calls
`OPENSSL_init_ssl(OPENSSL_INIT_LOAD_CONFIG, NULL)` in
[`be_tls_init`](https://github.com/postgres/postgres/blob/REL_17_STABLE/src/backend/libpq/be-secure-openssl.c).
The native server and setup tools must also receive the fixture-owned OpenSSL
environment; changing certificate generation alone does not complete isolation.

Both launchers now share certificate generation with a fixture-owned OpenSSL
configuration and include directory. The CA explicitly carries CA/signing
constraints; server/client purpose and DNS extensions remain unchanged. Parent
environment values remain unchanged. Native cluster initialization, server
startup/shutdown/status and the setup client also receive those fixture OpenSSL
paths. Cargo keeps its original environment.

`python3 tests/postgres_fixture.py` passed all three cases on Linux x86-64.
The new case uses installed OpenSSL and temporary files with an inherited invalid
configuration. Generated server and client chains pass the matching purpose and
hostname checks; wrong client purpose and wrong hostname are rejected. Private
key permissions are checked. No personal configuration or database services were
used. Python syntax parsing and `git diff --check` also passed.

After the server environment refinement, all ten fixture regression cases passed
in 0.142 seconds. The additional environment case covers native startup, shutdown
and failed-stop status checking with inherited caller config paths and verifies
that coverage variables survive. Client environment checks cover the same OpenSSL
paths. Python syntax parsing and `git diff --check` passed again.

The native PostgreSQL server launcher was not executed locally because server
tools are unavailable. The corrected container launcher passed all six real
PostgreSQL 17.9 suites using the already installed `postgres:17` image, including
verified/rejected TLS, client certificates, Unix sockets, cancellation, and lost
commit acknowledgement. Its container and temporary files were removed.
