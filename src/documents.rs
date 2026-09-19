// SPDX-License-Identifier: MPL-2.0
//! Query documents are editable guidance; these comments never establish identity.
use crate::Result;
use std::time::{SystemTime, UNIX_EPOCH};
pub fn initial(name: &str, writable: bool, sql: &str) -> String {
    let name = crate::results::escape(name);
    format!(
        "-- {name} · {} at creation; mode may change. Comments do not select a database.\n\
-- Write one statement. ::db-run executes this whole buffer, including unsaved edits.\n\
-- ::db-run-selection executes one nonempty selection.\n\
-- Each buffer has its own association; ::db-use associates/reassociates after reconnect or restart.\n\
-- :w saves SQL text; optional, and never executes SQL.\n\
-- ::db-return returns to the source view; ::db opens database browsing.\n\
-- Writable runs require captured review, confirmation, then ::db-commit or ::db-rollback.\n\
-- Rollback discards pending changes, not commits. ::db-cancel cancels a running query.\n\
-- Pending changes expire after five idle minutes. ::db-transactions lists them.\n\n{sql}",
        if writable {
            "READ AND WRITE"
        } else {
            "READ ONLY"
        }
    )
}
pub fn filename(name: &str, serial: u64) -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Clock unavailable")?;
    let seconds: libc::time_t = now.as_secs().try_into().map_err(|_| "Clock out of range")?;
    let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
    // localtime_r writes only to the provided tm; no process-global TZ mutation.
    let tm = unsafe {
        if libc::localtime_r(&seconds, local.as_mut_ptr()).is_null() {
            return Err("Local clock unavailable".into());
        }
        local.assume_init()
    };
    let name = safe_name(name);
    Ok(format!(
        "{name}-query-{:04}{:02}{:02}-{:02}{:02}{:02}-{:09}-{serial}.sql",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        now.subsec_nanos()
    ))
}
fn safe_name(name: &str) -> String {
    let name = name
        .chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    let name = name.trim_matches('-');
    if name.is_empty() {
        "database".into()
    } else {
        name.into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments_are_one_statement_and_filenames_are_safe() {
        for pg in [false, true] {
            crate::query::validate(&initial("name\n--", false, "SELECT 1;"), pg).unwrap();
        }
        assert_eq!(safe_name("../a:b/c"), "a-b-c");
        assert_eq!(safe_name("/\n"), "database");
        assert_ne!(
            filename("ledger", 1).unwrap(),
            filename("ledger", 2).unwrap()
        );
        assert!(filename("ledger", 1).unwrap().starts_with("ledger-query-"));
    }
}
