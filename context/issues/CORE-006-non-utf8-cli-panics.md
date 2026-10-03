# CORE-006: Non-UTF-8 CLI input panics and exposes argument contents

Status: fixed; independent re-review clean.

`main` collects `std::env::args()`, which panics if an operating-system argument
contains invalid UTF-8. The panic includes the supplied argument, bypassing the
normal generic diagnostics and exit code 2. The independent main-module reviewer
reproduced exit code 101 and echoed fixture argument contents using the already
compiled project executable.

The related `--print-config` path serializes `current_exe()` through `json!`.
An executable path that is not UTF-8 cannot be represented by that JSON string
and can likewise panic. Validate OS-string conversion explicitly and fail with
constant, input-free diagnostics. Keep ordinary argument handling and generated
configuration unchanged. Regression arguments must be synthetic fixture values;
do not create or execute a generated test program.

The correction collects `args_os()` and rejects conversion failures with a
constant diagnostic and exit code 2. Configuration generation explicitly validates
the executable path's UTF-8 representation before constructing JSON. The Unix CLI
regression passes a synthetic invalid-UTF-8 argument both as the command and as a
plugin ID to Cargo's compiled project binary, asserting the exact generic error,
exit code 2, and empty stdout. The executable-path conversion is source-reviewed;
no copied or generated executable is used as a path fixture.

Validation on Linux: `cargo test --locked --test cli` passed all three tests;
`rustfmt --check --edition 2024 src/main.rs tests/cli.rs` passed. Self-review found
no remaining CLI panic on invalid argument or executable-path conversion. Full
project checks remain the coordinating review's responsibility.
