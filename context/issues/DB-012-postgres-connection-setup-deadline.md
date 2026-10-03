<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-012: PostgreSQL connection setup can exceed its deadline

Confirmed in `Postgres::open`: the ten-second timeout wraps only the network
connect future. TLS configuration precedes it and session SET commands follow it
without a deadline. A server/proxy that finishes authentication but withholds a
SET reply leaves the connection job pending indefinitely. The spawned driver
also lacks its owning `Postgres` drop guard until SET has succeeded.

Use one absolute deadline across TLS configuration, network connection and
session initialization. Construct the owning database wrapper before awaiting
initialization, ensuring failure or caller cancellation aborts the driver.
Verify with a proxy to the isolated PostgreSQL fixture that withholds its first
SET completion, checking both timeout and connection closure.

Validation: on Linux x86-64 the isolated PostgreSQL 17.9 proxy regression first
hit its outer twelve-second guard. After the fix all ten PostgreSQL suites and
both PostgreSQL public-wire cases passed. The stalled setup returned its own
timeout and the proxy observed connection closure. TLS worker waiting shares
the deadline, but cancelling an
already running blocking filesystem read is not provided by Tokio; bounded PEM
file handling is reviewed separately.
