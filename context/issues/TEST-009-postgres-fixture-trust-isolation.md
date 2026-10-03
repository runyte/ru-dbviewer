<!-- SPDX-License-Identifier: MPL-2.0 -->
# TEST-009: PostgreSQL tests inherit the caller's certificate store

Status: fixed; independent review clean.

Both PostgreSQL launchers pass the caller's `SSL_CERT_FILE` and `SSL_CERT_DIR`
unchanged to the Rust tests. The locked rustls-native-certs implementation reads
those locations before the explicit fixture CA is added. Tests can therefore
read personal certificate configuration and depend on its contents despite
using disposable database servers.

Give test children a private empty certificate file and directory, while keeping
the fixture CA available only through its explicit test profile. Share the
environment constructor across both launchers and preserve coverage variables.
The regression checks inherited paths are replaced, the parent environment is
unchanged, and the explicit fixture CA is not made a default trusted root.
Real PostgreSQL TLS acceptance and rejection tests must still pass.

Validation: all eleven fixture-isolation tests pass. The disposable PostgreSQL
17.9 run passed all eleven database cases and both public-wire cases with the
private-root environment, including accepted fixture CA/client certificates and
rejected hostname/untrusted roots. Linux x86-64 only.
An independent nested fixture review verified the locked loader's environment
selection, reran all eleven fixture tests, and found no remaining issue.
