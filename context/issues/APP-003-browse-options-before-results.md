<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-003: Failed browse changes relabel retained rows with unexecuted options

Status: fixed; independent re-review clean.

Applying filters, sorting, or changing page size mutates the retained page's
`Browse` settings before the new read is admitted or completes. A refused
`job.create`, disconnected connection, or database query failure leaves the
old captured rows attached to the new options. Returning to the retained page
then claims those rows were filtered or sorted using settings never executed;
page-size changes can also misnumber retained records and ranges.

Resolution: capture browse options with each read and adopt them only alongside
its successful result. Preserve the previous page's options if admission or
execution fails, and retain independently changed display-column selections.

Regression: reject job creation for a filter application, and separately drop
the fixture table before applying a filter; both paths must retain the original
row data and original filter metadata. Refused sort and page-size changes also
retain the exact original model.

Validation: the filter regressions demonstrated incorrect retained metadata
against the original executable. After rebuilding on Linux x86-64, all 15
failure-path feature variants passed, along with five full filter/sort/export
workflows and five 1,000-row paging workflows. `cargo fmt --check` and locked
all-target Clippy with warnings denied passed. Full suite and independent
re-review are recorded in the final review register.
