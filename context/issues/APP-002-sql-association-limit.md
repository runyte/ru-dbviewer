<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-002: Associating existing SQL buffers bypasses the memory bound

Status: fixed; independent re-review pending.

Creating a query document refuses a 257th SQL-buffer association, but `::db-use`
unconditionally inserts existing buffers into the same map. Repeatedly opening
and associating documents therefore grows application memory without the
documented implementation bound; closed-buffer associations are retained for
the process lifetime.

Resolution: apply the same 256-association admission bound when associating
an existing document, while allowing an already associated document to be
reassociated at the limit.

Regression: associate 256 fixture-owned SQL buffer handles through the public
wire, refuse the next handle, and verify that an existing association can still
be refreshed and executed.

Validation: the regression accepted the overflow association against the
original executable in all five feature profiles. After rebuilding, all five
profiles reject overflow and successfully reassociate/execute an existing
buffer on Linux x86-64. Full suite and independent re-review are recorded in
the final review register.
