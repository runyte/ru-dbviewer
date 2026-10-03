# CORE-005: Canonical SQLite paths can create profiles that fail on restart

Status: fixed; independent review clean.

The SQLite form validates the entered path before resolving it. `paths::resolve`
then returns its canonical target, and `App::sqlite_destination` persists that
path without repeating profile validation. A clean symbolic-link name may target
a file whose canonical name contains a control character. Saving that result
produces a profile which the next plugin process rejects during `Saved::validate`.
Non-UTF-8 canonical targets were already refused before persistence, but the
resolver reported them valid to the form before the later submission rejection.

The independent paths reviewer reproduced the control-character case with a
temporary `alias.sqlite` pointing at a temporary SQLite filename containing a
newline: the first connection succeeded, saved state contained the control,
and the next plugin process exited while loading that state.

Validate the resolved path against profile path requirements before returning
`Destination::File`, with a constant diagnostic that does not echo the path.
Preserve existing nonsecret saved-state and no-automatic-reconnect behavior.

The resolver now checks the canonical path for UTF-8, the existing 4,096-byte
profile limit and absence of controls before returning a file destination.
The length check preserves the profile invariant; an overlong resolved-path
restart failure was not reproduced.

Validation on Linux x86-64:

- The new temporary-symlink Rust regression failed before the change because
  the newline-bearing target was returned as a valid file. It now rejects both
  that target and an invalid-UTF-8 target with the same path-free diagnostic.
- `cargo test --locked paths::tests -- --nocapture`: three passed.
- A public-wire temporary SQLite reproduction verified that the refused alias
  leaves previously saved valid profiles unchanged. A fresh plugin process
  loads that saved state and opens the database list successfully.
- The existing public-wire path completion/validation cases passed in all five
  legacy and negotiated configurations.
- Formatting and diff-whitespace checks passed for `src/paths.rs`. No macOS
  or ARM64 execution is claimed.

Portability re-review limits the invalid-UTF-8 filename fixture to Linux: Apple's
[APFS documentation](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html)
states that file creation accepts valid UTF-8 names only. The newline-target
restart regression remains enabled on every supported Unix target. This is a
fixture compatibility correction, not evidence of executing tests on macOS.
