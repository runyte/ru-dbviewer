// SPDX-License-Identifier: MPL-2.0
//! Bounded browse expressions. Only validated ordinals select quoted identifiers.
use crate::{Result, db::Table, query, results::Column};
#[derive(Clone, Debug, Default)]
pub struct Browse {
    pub columns: Vec<Column>,
    pub keys: Vec<String>,
    pub selected: Vec<usize>,
    pub filters: Vec<Filter>,
    pub any: bool,
    pub sort: Option<(usize, bool)>,
    pub page_size: usize,
}
#[derive(Clone, Debug)]
pub struct Filter {
    pub column: usize,
    pub op: Operator,
    pub value: String,
    pub enabled: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Equal,
    Contains,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Null,
    NotNull,
}
impl Operator {
    pub const NAMES: [&str; 8] = [
        "equals",
        "contains (literal)",
        "less than",
        "at most",
        "greater than",
        "at least",
        "is NULL",
        "is not NULL",
    ];
    pub fn from_name(name: &str) -> Result<Self> {
        Self::NAMES
            .iter()
            .position(|s| *s == name)
            .map(|i| {
                [
                    Self::Equal,
                    Self::Contains,
                    Self::Less,
                    Self::LessEqual,
                    Self::Greater,
                    Self::GreaterEqual,
                    Self::Null,
                    Self::NotNull,
                ][i]
            })
            .ok_or("Choose a supported operator".into())
    }
    pub fn name(self) -> &'static str {
        Self::NAMES[match self {
            Self::Equal => 0,
            Self::Contains => 1,
            Self::Less => 2,
            Self::LessEqual => 3,
            Self::Greater => 4,
            Self::GreaterEqual => 5,
            Self::Null => 6,
            Self::NotNull => 7,
        }]
    }
}
fn numeric(kind: &str) -> bool {
    let k = kind.to_ascii_lowercase();
    ["int", "numeric", "decimal", "real", "float", "double"]
        .iter()
        .any(|t| k.contains(t))
}
pub fn operators(kind: &str) -> Vec<String> {
    Operator::NAMES
        .iter()
        .filter(|name| !numeric(kind) || **name != "contains (literal)")
        .map(|s| s.to_string())
        .collect()
}
fn number(s: &str) -> bool {
    // A strict decimal syntax, kept as text to preserve precision until the database casts it.
    let mut digits = 0;
    let mut dots = 0;
    for (i, c) in s.bytes().enumerate() {
        if c.is_ascii_digit() {
            digits += 1;
        } else if c == b'.' {
            dots += 1;
        } else if i != 0 || (c != b'+' && c != b'-') {
            return false;
        }
    }
    digits > 0 && dots <= 1
}
impl Browse {
    pub fn size(&self) -> usize {
        if self.page_size == 0 {
            100
        } else {
            self.page_size
        }
    }
    pub fn check_filter(&self, f: &Filter) -> Result<()> {
        let col = self
            .columns
            .get(f.column)
            .ok_or("Column no longer exists")?;
        if f.value.len() > 4096 || f.value.contains('\0') {
            return Err("Filter value exceeds 4096 bytes or contains NUL".into());
        }
        if matches!(f.op, Operator::Null | Operator::NotNull) {
            return Ok(());
        }
        if numeric(&col.kind) && (f.op == Operator::Contains || !number(&f.value)) {
            return Err(
                "Numeric columns require a decimal value; contains is only for text".into(),
            );
        }
        Ok(())
    }
    pub fn compile(
        &self,
        t: &Table,
        page: usize,
        keys: &[String],
        pg: bool,
        literals: bool,
    ) -> Result<(String, Vec<String>)> {
        if self.filters.len() > 16 || self.selected.len() > 8 || !(1..=100).contains(&self.size()) {
            return Err("Browse limits exceeded".into());
        }
        let selected = if self.selected.is_empty() {
            (0..self.columns.len()).collect::<Vec<_>>()
        } else {
            self.selected.clone()
        };
        let projection = selected
            .iter()
            .map(|i| {
                self.columns
                    .get(*i)
                    .ok_or("Invalid selected column")
                    .map(|c| {
                        if pg && !literals {
                            format!(
                                "{}::text AS {}",
                                query::ident(&c.name),
                                query::ident(&c.name)
                            )
                        } else {
                            query::ident(&c.name)
                        }
                    })
            })
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join(", ");
        let projection = if projection.is_empty() {
            "*".into()
        } else {
            projection
        };
        let mut values = Vec::new();
        let mut conditions = Vec::new();
        for f in self.filters.iter().filter(|f| f.enabled) {
            self.check_filter(f)?;
            let col = &self.columns[f.column];
            let column = query::ident(&col.name);
            if matches!(f.op, Operator::Null | Operator::NotNull) {
                conditions.push(format!(
                    "{column} IS {}NULL",
                    if f.op == Operator::NotNull {
                        "NOT "
                    } else {
                        ""
                    }
                ));
                continue;
            }
            values.push(f.value.clone());
            let param = if literals {
                literal(&f.value, pg)
            } else if pg {
                format!("${}", values.len())
            } else {
                "?".into()
            };
            let (column, param) = if numeric(&col.kind) {
                (column, format!("CAST(CAST({param} AS TEXT) AS NUMERIC)"))
            } else {
                (format!("CAST({column} AS TEXT)"), param)
            };
            conditions.push(if f.op == Operator::Contains {
                if pg {
                    format!("strpos({column}, {param}) > 0")
                } else {
                    format!("instr({column}, {param}) > 0")
                }
            } else {
                format!(
                    "{column} {} {param}",
                    match f.op {
                        Operator::Equal => "=",
                        Operator::Less => "<",
                        Operator::LessEqual => "<=",
                        Operator::Greater => ">",
                        Operator::GreaterEqual => ">=",
                        _ => unreachable!(),
                    }
                )
            });
        }
        let mut order = Vec::new();
        if let Some((index, desc)) = self.sort {
            let col = self.columns.get(index).ok_or("Invalid sort column")?;
            order.push(format!(
                "{}.{}.{} {}",
                query::ident(&t.schema),
                query::ident(&t.name),
                query::ident(&col.name),
                if desc { "DESC" } else { "ASC" }
            ));
        }
        for key in keys {
            if self.sort.is_none_or(|(i, _)| self.columns[i].name != *key) {
                order.push(format!(
                    "{}.{}.{}",
                    query::ident(&t.schema),
                    query::ident(&t.name),
                    query::ident(key)
                ));
            }
        }
        Ok((
            format!(
                "SELECT {projection} FROM {}.{}{}{} LIMIT {} OFFSET {}",
                query::ident(&t.schema),
                query::ident(&t.name),
                if conditions.is_empty() {
                    String::new()
                } else {
                    format!(
                        " WHERE {}",
                        conditions.join(if self.any { " OR " } else { " AND " })
                    )
                },
                if order.is_empty() {
                    String::new()
                } else {
                    format!(" ORDER BY {}", order.join(", "))
                },
                self.size(),
                page.saturating_mul(self.size())
            ),
            values,
        ))
    }
    pub fn summary(&self) -> String {
        let list = self
            .filters
            .iter()
            .filter(|f| f.enabled)
            .map(|f| {
                format!(
                    "{} {} {}",
                    self.columns.get(f.column).map_or("?", |c| c.name.as_str()),
                    f.op.name(),
                    if matches!(f.op, Operator::Null | Operator::NotNull) {
                        ""
                    } else {
                        &f.value
                    }
                )
            })
            .collect::<Vec<_>>();
        format!(
            "Match {}: {}",
            if self.any { "ANY" } else { "ALL" },
            if list.is_empty() {
                "no active filters".into()
            } else {
                list.join("; ")
            }
        )
    }
}
pub fn literal(value: &str, pg: bool) -> String {
    if pg {
        format!("E'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
    } else {
        query::literal(value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_parameters_null_and_disabled_conditions() {
        let mut b = Browse {
            columns: vec![Column {
                name: "odd\"name".into(),
                kind: "TEXT".into(),
            }],
            ..Browse::default()
        };
        b.filters.push(Filter {
            column: 0,
            op: Operator::Contains,
            value: "%' OR 1=1 --".into(),
            enabled: true,
        });
        b.filters.push(Filter {
            column: 0,
            op: Operator::Null,
            value: String::new(),
            enabled: true,
        });
        let table = Table {
            schema: "main".into(),
            name: "items".into(),
            kind: "table".into(),
        };
        for pg in [false, true] {
            let (sql, params) = b.compile(&table, 1, &[], pg, false).unwrap();
            assert_eq!(params.len(), 1);
            assert!(!sql.contains("1=1"));
            assert!(sql.contains("IS NULL"));
            let (sql, _) = b.compile(&table, 1, &[], pg, true).unwrap();
            query::validate(&sql, pg).unwrap();
        }
        b.any = true;
        b.filters[0].enabled = false;
        assert!(
            b.compile(&table, 0, &[], false, false)
                .unwrap()
                .1
                .is_empty()
        );
        b.filters[1].column = 9;
        assert!(b.compile(&table, 0, &[], false, false).is_err());
    }
    #[test]
    fn numeric_values_are_explicit_and_precise() {
        let b = Browse {
            columns: vec![Column {
                name: "n".into(),
                kind: "numeric".into(),
            }],
            ..Browse::default()
        };
        for value in ["", "NaN", "1;drop", "1.2.3"] {
            assert!(
                b.check_filter(&Filter {
                    column: 0,
                    op: Operator::Equal,
                    value: value.into(),
                    enabled: true
                })
                .is_err()
            );
        }
        assert!(
            b.check_filter(&Filter {
                column: 0,
                op: Operator::Equal,
                value: "12345678901234567890.123".into(),
                enabled: true
            })
            .is_ok()
        );
    }
}
