# CORE-004: SQLite path completion omits usable symbolic links

Status: fixed; independent review clean.

`paths::resolve` accepts an exact symbolic link to a SQLite file, following it
with `Path::is_file` and resolving the final file with `canonicalize`. Directory
and prefix completion instead inspect only `DirEntry::file_type`, which reports
the link itself. Those same usable file links, and links to directories, never
appear in completion choices. A user who can connect using the full link name
cannot complete that name through the older-host fallback picker.

For matching symbolic links, inspect the target's metadata and admit regular
files or directories. Preserve the 4,096-entry scan and 62-choice bounds; skip
dangling links and unsupported file kinds. Regressions must use temporary files,
links and directories only.

The fallback now follows metadata only for matching symbolic-link entries.
Regular files and directories retain their existing handling; broken links,
loops and links to special files are skipped without opening the target.
The entry and choice limits are unchanged.

Validation on Linux x86-64:

- Public-wire reproduction with a temporary SQLite database: exact
  `alias.sqlite` connected, while its `ali` prefix returned no matches before
  the fix; the prefix now opens a picker and selecting the alias connects.
- The new Rust regression failed before the fix with no matching paths and
  passes afterward. It checks relative links to files and directories, dangling
  links, loops and a link to a temporary FIFO, including resolved file identity
  and directory traversal.
- `cargo test --locked paths::tests -- --nocapture`: two passed.
- Existing interactive completion/validation cases pass against both legacy and
  negotiated public-wire hosts. No native macOS execution is claimed.
- `rustfmt --edition 2024 --check src/paths.rs` passed.
