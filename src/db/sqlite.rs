// SPDX-License-Identifier: MPL-2.0
use crate::{
    Result,
    results::{Cell, Column, Data},
};
use rusqlite::{
    Connection, OpenFlags,
    hooks::{AuthAction, AuthContext, Authorization},
    types::ValueRef,
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub struct Sqlite {
    connection: Arc<Mutex<Connection>>,
    pub writable: bool,
}
fn error(e: rusqlite::Error) -> String {
    match e {
        rusqlite::Error::SqliteFailure(code, _) => format!("SQLite error: {:?}", code.code),
        _ => "SQLite statement failed (check syntax, columns and bindings)".into(),
    }
}
impl Sqlite {
    pub async fn open(path: String, writable: bool) -> Result<Self> {
        tokio::task::spawn_blocking(move || {
            let flags = if writable {
                OpenFlags::SQLITE_OPEN_READ_WRITE
            } else {
                OpenFlags::SQLITE_OPEN_READ_ONLY
            };
            let c = Connection::open_with_flags(path, flags).map_err(error)?;
            c.busy_timeout(Duration::from_millis(500)).map_err(error)?;
            c.execute_batch("PRAGMA foreign_keys=ON").map_err(error)?;
            if !writable {
                c.execute_batch("PRAGMA query_only=ON").map_err(error)?;
            }
            c.authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
                AuthAction::Pragma {
                    pragma_name: "table_info" | "foreign_key_list",
                    ..
                } => Authorization::Allow,
                AuthAction::Attach { .. }
                | AuthAction::Detach { .. }
                | AuthAction::Pragma { .. } => Authorization::Deny,
                AuthAction::Function { function_name }
                    if function_name.eq_ignore_ascii_case("load_extension") =>
                {
                    Authorization::Deny
                }
                _ => Authorization::Allow,
            }))
            .map_err(error)?;
            Ok(Self {
                connection: Arc::new(Mutex::new(c)),
                writable,
            })
        })
        .await
        .map_err(|_| "SQLite worker stopped".to_string())?
    }
    pub async fn execute(
        &self,
        sql: String,
        write: bool,
        cancel: CancellationToken,
        seconds: u64,
    ) -> Result<Data> {
        self.execute_bound(sql, Vec::new(), write, cancel, seconds)
            .await
    }
    pub async fn execute_bound(
        &self,
        sql: String,
        parameters: Vec<String>,
        write: bool,
        cancel: CancellationToken,
        seconds: u64,
    ) -> Result<Data> {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let c = connection
                .lock()
                .map_err(|_| "SQLite worker failed".to_string())?;
            let deadline = Instant::now() + Duration::from_secs(seconds);
            let stopped = cancel.clone();
            c.progress_handler(
                1000,
                Some(move || stopped.is_cancelled() || Instant::now() >= deadline),
            )
            .map_err(error)?;
            let operation = (|| {
                if cancel.is_cancelled() {
                    return Err("Query cancelled".into());
                }
                c.execute_batch("BEGIN").map_err(error)?;
                let mut stmt = c.prepare(&sql).map_err(error)?;
                if !write && !stmt.readonly() {
                    return Err("Read-only connection refused a write".into());
                }
                let mut data = Data {
                    columns: stmt
                        .columns()
                        .iter()
                        .map(|c| Column {
                            name: c.name().into(),
                            kind: c.decl_type().unwrap_or("dynamic").into(),
                        })
                        .collect(),
                    ..Data::default()
                };
                let count = stmt.column_count();
                let mut rows = stmt
                    .query(rusqlite::params_from_iter(parameters.iter()))
                    .map_err(error)?;
                while let Some(row) = rows.next().map_err(error)? {
                    let mut cells = Vec::with_capacity(count);
                    for i in 0..count {
                        let s = match row.get_ref(i).map_err(error)? {
                            ValueRef::Null => None,
                            ValueRef::Integer(v) => Some(v.to_string()),
                            ValueRef::Real(v) => Some(v.to_string()),
                            ValueRef::Text(v) => Some(match std::str::from_utf8(v) {
                                Ok(text) => text.to_owned(),
                                Err(_) => format!(
                                    "[invalid UTF-8 text] x\"{}\"{}",
                                    v.iter()
                                        .take(32768)
                                        .map(|b| format!("{b:02x}"))
                                        .collect::<String>(),
                                    if v.len() > 32768 { " [truncated]" } else { "" }
                                ),
                            }),
                            ValueRef::Blob(v) => Some(format!(
                                "x'{}'{}",
                                v.iter()
                                    .take(32768)
                                    .map(|b| format!("{b:02x}"))
                                    .collect::<String>(),
                                if v.len() > 32768 { " [truncated]" } else { "" }
                            )),
                        };
                        cells.push(Cell::new(s));
                    }
                    if !data.push(cells) {
                        break;
                    }
                }
                drop(rows);
                data.affected = if !stmt.readonly() && crate::query::affects_rows(&sql) {
                    Some(c.changes())
                } else {
                    None
                };
                if write && data.truncated {
                    return Err("Result limit reached; writable transaction rolled back".into());
                }
                if cancel.is_cancelled() {
                    return Err("Query cancelled".into());
                }
                Ok(data)
            })();
            c.progress_handler(0, None::<fn() -> bool>).map_err(error)?;
            if (operation.is_err() || !write) && !c.is_autocommit() {
                c.execute_batch("ROLLBACK")
                    .map_err(|_| "Transaction outcome unknown: rollback failed".to_string())?;
            }
            operation
        })
        .await
        .map_err(|_| "SQLite worker stopped".to_string())?
    }
    pub async fn settle(&self, commit: bool) -> Result<()> {
        let c = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let c = c.lock().map_err(|_| "SQLite worker failed".to_string())?;
            if !c.is_autocommit() {
                c.execute_batch(if commit { "COMMIT" } else { "ROLLBACK" })
                    .map_err(error)?;
            }
            Ok(())
        })
        .await
        .map_err(|_| "SQLite worker stopped".to_string())?
    }
}
