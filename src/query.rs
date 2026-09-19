// SPDX-License-Identifier: MPL-2.0
use crate::Result;
use sqlparser::{
    ast::Statement,
    dialect::{PostgreSqlDialect, SQLiteDialect},
    parser::Parser,
};
pub const MAX_SQL: usize = 256 * 1024;

pub fn validate(sql: &str, postgres: bool) -> Result<()> {
    if sql.len() > MAX_SQL || sql.contains('\0') {
        return Err("SQL exceeds the input limit or contains NUL".into());
    }
    let dialect: &dyn sqlparser::dialect::Dialect = if postgres {
        &PostgreSqlDialect {}
    } else {
        &SQLiteDialect {}
    };
    let statements = Parser::parse_sql(dialect, sql)
        .map_err(|_| "SQL is not supported by the statement parser".to_string())?;
    if statements.len() != 1 {
        return Err("Execute exactly one SQL statement".into());
    }
    // A deliberately small grammar: transaction/session/administrative commands
    // cannot escape the transaction owned by the application.
    match &statements[0] {
        Statement::Query(_) | Statement::Insert(_) | Statement::Update{..} | Statement::Delete(_) |
        Statement::CreateTable(_) | Statement::CreateIndex(_) | Statement::CreateView{..} |
        Statement::AlterTable{..} | Statement::Drop{..} => Ok(()),
        _=>Err("Use a query or transactional DML/DDL; scripts, COPY and transaction control are unsupported".into())
    }
}
/// SQLite changes() is undefined/stale for DDL and queries. Only DML supplies a count.
pub fn affects_rows(sql: &str) -> bool {
    Parser::parse_sql(&SQLiteDialect {}, sql)
        .ok()
        .is_some_and(|s| {
            s.len() == 1
                && matches!(
                    s[0],
                    Statement::Insert(_) | Statement::Update { .. } | Statement::Delete(_)
                )
        })
}
pub fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
pub fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn statements_are_not_split_on_semicolons() {
        for sql in [
            "select ';'",
            "select ' INTO '",
            "select $$a;b$$",
            "with x as (select 1) select * from x; -- done",
        ] {
            assert!(validate(sql, true).is_ok(), "{sql}");
        }
        for sql in [
            "select 1; delete from t",
            "commit",
            "set transaction read write",
            "copy t to '/tmp/x'",
            "attach database 'x' as other",
            "pragma query_only=off",
        ] {
            assert!(validate(sql, true).is_err(), "{sql}");
        }
    }
    #[test]
    fn quote_identifiers() {
        assert_eq!(ident("a\";drop"), "\"a\"\";drop\"");
        assert_eq!(literal("O'Reilly"), "'O''Reilly'");
    }
}
