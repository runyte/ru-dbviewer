// SPDX-License-Identifier: MPL-2.0
use crate::Result;
use sqlparser::{
    ast::{BinaryOperator, Expr, Statement},
    dialect::{Dialect, PostgreSqlDialect, SQLiteDialect},
    keywords::Keyword,
    parser::{Parser, ParserError},
};
pub const MAX_SQL: usize = 256 * 1024;

// sqlparser 0.59's SQLite infix hook unwraps malformed MATCH/REGEXP operands.
// Forward its other overrides and identity so SQLite-specific parser paths keep
// working, while malformed input stays a redacted validation error, not a panic.
#[derive(Debug)]
struct CheckedSqliteDialect(SQLiteDialect);

impl Dialect for CheckedSqliteDialect {
    fn dialect(&self) -> std::any::TypeId {
        self.0.dialect()
    }

    fn is_delimited_identifier_start(&self, ch: char) -> bool {
        self.0.is_delimited_identifier_start(ch)
    }

    fn identifier_quote_style(&self, identifier: &str) -> Option<char> {
        self.0.identifier_quote_style(identifier)
    }

    fn is_identifier_start(&self, ch: char) -> bool {
        self.0.is_identifier_start(ch)
    }

    fn is_identifier_part(&self, ch: char) -> bool {
        self.0.is_identifier_part(ch)
    }

    fn supports_filter_during_aggregation(&self) -> bool {
        self.0.supports_filter_during_aggregation()
    }

    fn supports_start_transaction_modifier(&self) -> bool {
        self.0.supports_start_transaction_modifier()
    }

    fn parse_statement(
        &self,
        parser: &mut Parser,
    ) -> Option<std::result::Result<Statement, ParserError>> {
        self.0.parse_statement(parser)
    }

    fn parse_infix(
        &self,
        parser: &mut Parser,
        expr: &Expr,
        _precedence: u8,
    ) -> Option<std::result::Result<Expr, ParserError>> {
        let op = if parser.parse_keyword(Keyword::MATCH) {
            BinaryOperator::Match
        } else if parser.parse_keyword(Keyword::REGEXP) {
            BinaryOperator::Regexp
        } else {
            return None;
        };
        Some(parser.parse_expr().map(|right| Expr::BinaryOp {
            left: Box::new(expr.clone()),
            op,
            right: Box::new(right),
        }))
    }

    fn supports_in_empty_list(&self) -> bool {
        self.0.supports_in_empty_list()
    }

    fn supports_limit_comma(&self) -> bool {
        self.0.supports_limit_comma()
    }

    fn supports_asc_desc_in_column_definition(&self) -> bool {
        self.0.supports_asc_desc_in_column_definition()
    }

    fn supports_dollar_placeholder(&self) -> bool {
        self.0.supports_dollar_placeholder()
    }

    fn supports_notnull_operator(&self) -> bool {
        self.0.supports_notnull_operator()
    }
}

pub fn validate(sql: &str, postgres: bool) -> Result<()> {
    if sql.len() > MAX_SQL || sql.contains('\0') {
        return Err("SQL exceeds the input limit or contains NUL".into());
    }
    let dialect: &dyn sqlparser::dialect::Dialect = if postgres {
        &PostgreSqlDialect {}
    } else {
        &CheckedSqliteDialect(SQLiteDialect {})
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
    Parser::parse_sql(&CheckedSqliteDialect(SQLiteDialect {}), sql)
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

    #[test]
    fn malformed_sqlite_infix_operands_return_redacted_errors() {
        for sql in [
            "SELECT 1 MATCH",
            "SELECT 1 REGEXP",
            "SELECT 1 MATCH )",
            "SELECT 1 REGEXP (",
            "SELECT 'private-value' MATCH ('private-pattern' +)",
            "UPDATE t SET value = 'private-value' REGEXP ('private-pattern' +)",
        ] {
            assert_eq!(
                validate(sql, false).unwrap_err(),
                "SQL is not supported by the statement parser"
            );
            assert!(!affects_rows(sql));
        }
        assert!(validate("SELECT 1", false).is_ok());
    }

    #[test]
    fn checked_sqlite_dialect_preserves_valid_grammar() {
        let original = SQLiteDialect {};
        let checked = CheckedSqliteDialect(SQLiteDialect {});
        assert!((&checked as &dyn Dialect).is::<SQLiteDialect>());
        for sql in [
            "SELECT body MATCH 'term' FROM docs",
            "SELECT 'abc' REGEXP '^a'",
            "SELECT body MATCH ('term' || '*') FROM docs WHERE id IN ()",
            "SELECT [value], `other`, \"quoted\", café FROM t LIMIT 1, 2",
            "SELECT sum(value) FILTER (WHERE value NOTNULL) FROM t",
            "CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, value)",
            "CREATE TABLE t (id INTEGER PRIMARY KEY DESC, value TEXT) WITHOUT ROWID",
            "REPLACE INTO t (value) VALUES ($value)",
            "INSERT OR REPLACE INTO t (value) VALUES ('abc' REGEXP '^a')",
            "UPDATE t SET value = body MATCH 'term'",
            "DELETE FROM t WHERE value REGEXP '^a'",
        ] {
            let expected = Parser::parse_sql(&original, sql).unwrap();
            assert_eq!(Parser::parse_sql(&checked, sql).unwrap(), expected);
            assert!(validate(sql, false).is_ok(), "{sql}");
        }
        assert!(affects_rows("UPDATE t SET value = body MATCH 'term'"));
        assert!(affects_rows("DELETE FROM t WHERE value REGEXP '^a'"));
    }
}
