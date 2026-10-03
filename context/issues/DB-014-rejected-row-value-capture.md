<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-014: The first row beyond the result cap is unnecessarily captured

Confirmed in both adapters: every row is converted and its full values written
before `Data::push` rejects row 1,001. If an earlier row retains the same spool,
the discarded row's bytes remain reserved until that result is released. A
single rejected large value therefore consumes storage quota and disk I/O even
though it can never be inspected.

Check the row-count cap after fetching the next driver row but before converting
or spooling its fields. Preserve the required extra-row fetch so exactly 1,000
rows are not incorrectly labelled incomplete. Shared temporary SQLite and
PostgreSQL fixtures retain a 70,000-byte first value and reject a 1 MiB last
value, asserting that only the retained value occupies storage.

Validation: on Linux x86-64 the SQLite regression first measured 1,118,576
retained bytes instead of 70,000. After the fix all 18 SQLite cases, all 12
isolated PostgreSQL 17.9 cases and both PostgreSQL public-wire cases passed.
Both adapters retain only 70,000 bytes in the regression and release them when
the captured result is dropped.
