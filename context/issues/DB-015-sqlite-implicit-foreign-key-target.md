<!-- SPDX-License-Identifier: MPL-2.0 -->
# DB-015: SQLite implicit primary-key references lose their target in schema

Confirmed with a temporary SQLite table containing `REFERENCES parent`:
`pragma_foreign_key_list` returns NULL for the target column when the referenced
primary key is implicit. The schema query concatenates that NULL with the table
name, so the whole definition becomes NULL and hides the referenced table.

Keep the referenced table name and append a parenthesized target column only
when SQLite reports one. The regression covers both implicit and explicit
foreign-key targets through the adapter's schema API.

Validation: the temporary SQLite adapter regression first returned NULL instead
of the parent table name. After the fix, both SQLite schema regressions passed
on Linux x86-64, covering generated columns and explicit/implicit FK targets.
