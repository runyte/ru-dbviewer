# Third-party notices

ru-dbviewer sources are licensed under MPL-2.0; see LICENSE.
The public schema, protocol fixtures and adapted test harness are MPL-2.0;
see VENDOR.md for exact origins and revisions.

SQLite is bundled from the libsqlite3-sys source package. SQLite is public domain.
rusqlite is MIT licensed. Rust PostgreSQL (tokio-postgres, postgres-types and
postgres-protocol), Tokio, and tokio-postgres-rustls use MIT/Apache-2.0 licensing;
Rustls uses Apache-2.0/ISC/MIT options. Serde and serde_json use MIT/Apache-2.0.
SQLParser uses Apache-2.0. Ring contains ISC and other permissively licensed
cryptographic sources with its own third-party notices.

Cargo.lock records the exact complete dependency graph. Release packaging must
include the license/notice files supplied by every distributed dependency, using
scripts/dependency_licenses.py after cargo fetch --locked. This file is a summary
and does not replace those upstream license texts.
