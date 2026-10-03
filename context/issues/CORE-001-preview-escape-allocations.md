# CORE-001: Preview escaping allocates once per Unicode scalar

Status: fixed; independent re-review clean.

`results::escape` constructs a temporary `Vec<char>` for every scalar, including
ordinary text. Escaping a maximum-size ASCII cell therefore makes over 65,536
heap allocations before result-table clipping discards most of the text. The
same helper runs synchronously for table, record, catalog and inspection models.

Replace per-scalar vectors with direct appends to one output string, preserving
control escaping and Unicode. A checked-in allocation-counting integration test
established the baseline and prevents per-character allocations returning.

On Linux x86-64, the regression failed before the change with **65,550 allocations
for 65,536 ASCII bytes**. After the change it passes with **one allocation** for
ASCII and ordinary Unicode inputs and three allocations for the expanding
control-character fixture. `cargo test --locked --test formatting -- --nocapture`
passed with one Cargo job, incremental compilation disabled, and one test thread.
The independent results-module reviewer repeated that check plus the five
`results::` unit tests and found no remaining issue in the correction.
