// SPDX-License-Identifier: MPL-2.0
use crate::{
    db::Table,
    results::{Data, escape},
};
use serde_json::{Value, json};
use std::sync::Arc;
#[derive(Clone)]
pub enum Content {
    Connections,
    Filters {
        name: String,
        source: String,
        draft: crate::browse::Browse,
    },
    Transactions {
        entries: Vec<(String, u64)>,
    },
    Value {
        name: String,
        data: Arc<Data>,
        row: usize,
        column: usize,
        raw: bool,
        collapsed: std::collections::BTreeSet<String>,
    },
    Review {
        generation: u64,
        name: String,
        sql: String,
        source: String,
    },
    Catalog {
        name: String,
        tables: Vec<Table>,
        system: bool,
    },
    Result {
        name: String,
        data: Arc<Data>,
        table: Option<Table>,
        page: usize,
        offset: usize,
        columns: Vec<usize>,
        record: Option<usize>,
        source: String,
    },
}
pub struct View {
    pub published: tokio::time::Instant,
    pub revision: String,
    pub content: Content,
    pub model: Value,
    pub parent: Option<String>,
    pub generation: Option<u64>,
    pub browse: crate::browse::Browse,
}
pub fn text_model(title: &str, status: &str, rows: Vec<Value>, actions: &[&str]) -> Value {
    json!({"title":short(&escape(title),150),"purpose":"list","rows":rows,"status":{"text":short(&escape(status),1000),"role":"muted"},"actions":actions})
}
pub fn row(id: impl ToString, text: impl AsRef<str>) -> Value {
    json!({"id":id.to_string(),"text":short(&escape(text.as_ref()),2000),"role":"ordinary"})
}
fn column_label(name: &str) -> String {
    if name.is_empty() {
        "(unnamed)".into()
    } else {
        short(&escape(name), 100)
    }
}
fn record_row(index: usize, budget: usize, text: String) -> Value {
    let mut value = row(index, text);
    // Count JSON bytes, including quote/backslash escaping and row metadata.
    // Supported databases have at most 2,000 result columns, leaving ample
    // space for each field's ID even when previews must be shortened.
    while value.to_string().len() > budget {
        let text = value["text"].as_str().unwrap();
        if text.len() <= 4 {
            break;
        }
        value["text"] = json!(short(text, (text.len() / 2).max(4)));
    }
    value
}
pub fn short(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max.saturating_sub(4);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} …", &s[..end])
}
impl Content {
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Connections | Self::Transactions { .. } => None,
            Self::Filters { name, .. }
            | Self::Value { name, .. }
            | Self::Review { name, .. }
            | Self::Catalog { name, .. }
            | Self::Result { name, .. } => Some(name),
        }
    }
    pub fn model(&self, state: &str) -> Value {
        match self {
            Self::Filters { .. } => text_model("Filters", state, vec![], &["back"]),
            Self::Transactions { .. } => text_model("Transactions", state, vec![], &["back"]),
            Self::Value {
                name,
                data,
                row,
                column,
                raw,
                collapsed,
            } => crate::inspection::model(name, &data.rows[*row][*column], *raw, collapsed, state),
            Self::Connections => {
                text_model("Databases", state, vec![], &["activate", "connect-new"])
            }
            Self::Review {
                name, sql, source, ..
            } => {
                let rows=sql.chars().collect::<Vec<_>>().chunks(1024).enumerate().map(|(i,chunk)|json!({"id":i.to_string(),"text":escape(&chunk.iter().collect::<String>()),"role":"ordinary"})).collect();
                text_model(
                    &format!("{name} · captured SQL"),
                    &format!("Enter to confirm execution · {source} · controls are escaped"),
                    rows,
                    &["activate"],
                )
            }
            Self::Catalog {
                name,
                tables,
                system: _,
            } => text_model(
                &format!("{name} · catalog"),
                state,
                tables
                    .iter()
                    .enumerate()
                    .map(|(i, t)| row(i, format!("{}.{}  [{}]", t.schema, t.name, t.kind)))
                    .collect(),
                &[
                    "activate",
                    "back",
                    "transactions",
                    "schema",
                    "refresh",
                    "system",
                    "query",
                    "mode",
                    "disconnect",
                    "commit",
                    "rollback",
                    "acknowledge",
                ],
            ),
            Self::Result {
                name,
                data,
                table,
                page,
                offset,
                columns,
                record,
                source,
            } => {
                let status = format!(
                    "{state} · {source} · {} rows retained · {} affected{}",
                    data.rows.len(),
                    data.affected.map_or("unknown".into(), |n| n.to_string()),
                    if data.truncated {
                        " · TRUNCATED"
                    } else if data.cells_truncated {
                        " · RETAINED VALUES TRUNCATED"
                    } else {
                        ""
                    }
                );
                let count = data.rows.len().saturating_sub(*offset).min(100);
                let first = if count == 0 { 0 } else { page * 100 + 1 };
                let last = if count == 0 { 0 } else { page * 100 + count };
                let status = format!(
                    "{status} · rows {first}–{last} · page size 100 · {}",
                    if table.is_some() {
                        "database page"
                    } else {
                        "retained SQL results; no SQL replay"
                    }
                );
                if let Some(index) = record {
                    let rows = data
                        .rows
                        .get(*index)
                        .map(|r| {
                            // Divide the encoded model budget among every field so
                            // wide rows keep their original selectable column IDs.
                            let budget = 690_000 / r.len().max(1);
                            r.iter()
                                .enumerate()
                                .map(|(i, c)| {
                                    record_row(
                                        i,
                                        budget,
                                        format!(
                                            "{} [{}]: {}",
                                            data.columns.get(i).map_or("?", |c| &c.name),
                                            data.columns.get(i).map_or("?", |c| &c.kind),
                                            c.display()
                                        ),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    return text_model(
                        &format!("{name} · row {}", index + 1),
                        &format!("{status} · Field previews; Enter opens the retained value"),
                        rows,
                        &[
                            "activate",
                            "back",
                            "query",
                            "disconnect",
                            "commit",
                            "rollback",
                            "transactions",
                        ],
                    );
                }
                let columns = if columns.is_empty() {
                    (0..data.columns.len().min(8)).collect::<Vec<_>>()
                } else {
                    columns.clone()
                };
                let mut model = text_model(
                    &format!(
                        "{name} · {} · page {}",
                        table.as_ref().map_or("SQL", |t| t.name.as_str()),
                        page + 1
                    ),
                    &status,
                    vec![],
                    &[
                        "activate",
                        "next",
                        "previous",
                        "columns",
                        "refresh",
                        "back",
                        "query",
                        "commit",
                        "rollback",
                        "disconnect",
                        "transactions",
                    ],
                );
                if table.is_some() {
                    model["actions"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("schema"));
                    model["actions"]
                        .as_array_mut()
                        .unwrap()
                        .extend(["filters", "sort", "page-size", "browse-sql"].map(|a| json!(a)));
                }
                if !columns.is_empty() {
                    model["columns"]=json!(columns.iter().map(|&i|json!({"id":format!("c{i}"),"label":column_label(&data.columns[i].name)})).collect::<Vec<_>>());
                    let mut rows = Vec::new();
                    let mut bytes = 4096;
                    for (index, r) in data.rows.iter().enumerate().skip(*offset).take(100) {
                        let value = json!({"id":index.to_string(),"text":"","role":"ordinary","cells":columns.iter().map(|&i|json!({"text":short(&r[i].display(),512),"role":if r[i].text.is_none(){"muted"}else{"ordinary"}})).collect::<Vec<_>>()});
                        bytes += value.to_string().len();
                        if bytes > 700_000 {
                            break;
                        }
                        rows.push(value);
                    }
                    model["rows"] = json!(rows);
                }
                model
            }
        }
    }
}
