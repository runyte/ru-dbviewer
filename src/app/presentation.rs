// SPDX-License-Identifier: MPL-2.0
//! Readable command discovery and view identity, separate from action admission.
use super::*;

fn action(name: &str) -> (&str, &str, u16) {
    match name.trim_start_matches("global-") {
        "activate" => ("Open selected item", "Navigation", 0),
        "show-full" => ("Show full value", "Inspect", 10),
        "raw" => ("Show raw value", "Inspect", 20),
        "back" | "return" => ("Back", "Navigation", 30),
        "previous" => ("Previous page", "Navigation", 31),
        "next" => ("Next page", "Navigation", 32),
        "filters" => ("Edit filters", "Filter and sort", 40),
        "sort" => ("Sort rows", "Filter and sort", 41),
        "add-filter" => ("Add filter", "Filter and sort", 42),
        "edit-filter" => ("Edit filter", "Filter and sort", 43),
        "toggle-filter" => ("Enable or disable filter", "Filter and sort", 44),
        "remove-filter" => ("Remove filter", "Filter and sort", 45),
        "clear-filters" => ("Clear filters", "Filter and sort", 46),
        "match" => ("Match all or any", "Filter and sort", 47),
        "apply-filters" => ("Apply filters", "Filter and sort", 48),
        "columns" => ("Choose columns", "Columns and paging", 50),
        "page-size" => ("Page size", "Columns and paging", 51),
        "schema" => ("Inspect schema", "Table", 60),
        "refresh" => ("Refresh", "Table", 61),
        "system" => ("Show or hide system tables", "Table", 62),
        "query" => ("New query", "SQL", 70),
        "browse-sql" => ("Open current browse as SQL", "SQL", 71),
        "run" => ("Execute SQL buffer", "SQL", 72),
        "run-selection" => ("Execute selected SQL", "SQL", 73),
        "use" => ("Choose database for buffer", "SQL", 74),
        "commit" => ("Commit", "Pending changes", 5),
        "rollback" => ("Roll back", "Pending changes", 6),
        "open" => ("Databases", "Database", 80),
        "connect-new" => ("Add database", "Database", 81),
        "connect" => ("Connect", "Database", 82),
        "profile-actions" => ("Database actions", "Database", 83),
        "mode" => ("Access mode", "Database", 84),
        "transactions" => ("Transactions", "Database", 85),
        "disconnect" => ("Disconnect", "Database", 86),
        "acknowledge" => ("Acknowledge uncertain outcome", "Database", 87),
        "cancel" => ("Cancel operation", "Database", 88),
        _ => (name, "Database", 100),
    }
}
fn presentation(name: &str) -> Value {
    let (label, group, order) = action(name);
    json!({"label":label,"group":group,"order":order,"listed":name!="activate"})
}
pub(super) fn commands(enabled: bool, document: bool) -> Value {
    let mut commands = super::commands();
    if document {
        commands.as_array_mut().unwrap().push(json!({"name":"show-full","context":"view","description":"Load the complete captured value"}));
    }
    if enabled {
        for command in commands.as_array_mut().unwrap() {
            command["presentation"] = presentation(command["name"].as_str().unwrap());
            if command["name"] == "global-connect" {
                command["presentation"]["label"] = json!("Add database");
            }
        }
    }
    commands
}
fn metadata(entries: &mut Vec<Value>, label: &str, value: impl AsRef<str>) {
    entries.push(
        json!({"label":label,"value":views::short(&crate::results::escape(value.as_ref()),1024)}),
    );
}
fn suffix(title: &str) -> &str {
    title.split_once("] ").map_or(title, |(_, tail)| tail)
}
fn can_show_full(cell: &crate::results::Cell) -> bool {
    cell.full_available() && cell.text.as_ref().is_some_and(|text| !text.is_empty())
}
impl App {
    pub(super) fn model(&self, content: &Content) -> Value {
        self.model_context(content, None, None)
    }
    pub(super) fn model_context(
        &self,
        content: &Content,
        parent: Option<&View>,
        browse: Option<&crate::browse::Browse>,
    ) -> Value {
        let mut model = self.base_model(
            content,
            browse.map_or(
                crate::browse::DEFAULT_PAGE_SIZE,
                crate::browse::Browse::size,
            ),
        );
        model.as_object_mut().unwrap().remove("detail");
        let name = content.name().unwrap_or("");
        let mut entries = Vec::new();
        if let Some(connection) = self.connections.get(name) {
            metadata(
                &mut entries,
                "Access",
                if connection.writable {
                    "Read and write"
                } else {
                    "Read only"
                },
            );
        }
        let mut primary = "Open selected item";
        let title = match content {
            Content::FullValue { title, .. } => title.clone(),
            Content::Connections => {
                primary = "Connect or open tables";
                "[databases]".to_owned()
            }
            Content::Catalog { name, .. } => {
                primary = "Open rows";
                if let Some(Profile::Sqlite { path, .. }) =
                    self.saved.profiles.iter().find(|p| p.name() == name)
                {
                    metadata(&mut entries, "Database path", path);
                }
                format!("[tables] {name}")
            }
            Content::Transactions { .. } => "[transactions]".into(),
            Content::Filters {
                name,
                source,
                draft,
            } => {
                metadata(&mut entries, "Filters", draft.summary());
                let location = self
                    .views
                    .get(source)
                    .and_then(|view| view.model["title"].as_str())
                    .map(suffix)
                    .unwrap_or(name);
                format!("[filters] {location}")
            }
            Content::Review { name, .. } => {
                primary = "Confirm execution";
                format!("[review] {name} › captured SQL")
            }
            Content::Result {
                name,
                data,
                table,
                page,
                offset,
                record,
                source,
                ..
            } => {
                let size = browse.map_or(
                    crate::browse::DEFAULT_PAGE_SIZE,
                    crate::browse::Browse::size,
                );
                let location = table.as_ref().map_or_else(
                    || {
                        if let Some(table) = source.strip_prefix("schema:") {
                            table.to_owned()
                        } else {
                            source
                                .strip_prefix("buffer ")
                                .and_then(|source| source.split_whitespace().next())
                                .and_then(|buffer| self.query_labels.get(buffer))
                                .cloned()
                                .unwrap_or_else(|| "SQL result".into())
                        }
                    },
                    |table| {
                        if self
                            .saved
                            .profiles
                            .iter()
                            .any(|p| p.name() == name && p.postgres())
                        {
                            format!("{}.{}", table.schema, table.name)
                        } else {
                            table.name.clone()
                        }
                    },
                );
                let state = self.state(name);
                model["status"] = json!({"text":views::short(&state,1000),"role":"muted"});
                if data.truncated {
                    model["status"] = json!({"text":format!("{state} · Result incomplete: row or result-size limit reached"),"role":"warning"});
                }
                if let Some(count) = data.affected {
                    metadata(&mut entries, "Affected rows", count.to_string());
                }
                if let Some(record) = record {
                    primary = "Open value";
                    let number = if table.is_some() {
                        page * size + record + 1
                    } else {
                        record + 1
                    };
                    model["actions"] = json!(["activate", "back"]);
                    if self.document_views && self.row_actions {
                        for row in model["rows"].as_array_mut().unwrap() {
                            let i = row["id"].as_str().and_then(|id| id.parse::<usize>().ok());
                            if i.and_then(|i| data.rows.get(*record)?.get(i))
                                .is_some_and(can_show_full)
                            {
                                row["actions"] = json!(["activate", "back", "show-full"]);
                            }
                        }
                    } else if self.document_views
                        && data
                            .rows
                            .get(*record)
                            .is_some_and(|row| row.iter().any(can_show_full))
                    {
                        // Legacy row-action hosts still validate the exact field in dispatch.
                        model["actions"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!("show-full"));
                    }
                    format!("[record] {name} › {location} › row {number}")
                } else {
                    primary = "Open row";
                    let count = model["rows"].as_array().map_or(0, Vec::len);
                    let first = if count == 0 {
                        0
                    } else if table.is_some() {
                        page * size + 1
                    } else {
                        offset + 1
                    };
                    let last = if count == 0 { 0 } else { first + count - 1 };
                    metadata(&mut entries, "Rows", format!("{first}–{last}"));
                    if table.is_some() {
                        metadata(&mut entries, "Page size", size.to_string());
                        if let Some(browse) = browse {
                            metadata(&mut entries, "Filters", browse.summary());
                            let sort = browse
                                .sort
                                .and_then(|(i, desc)| {
                                    browse.columns.get(i).map(|c| {
                                        format!(
                                            "{} {}",
                                            c.name,
                                            if desc { "descending" } else { "ascending" }
                                        )
                                    })
                                })
                                .unwrap_or_else(|| "Default order".into());
                            metadata(&mut entries, "Sort", sort);
                        }
                    }
                    if table.is_some() && self.connections.contains_key(name) {
                        model["actions"].as_array_mut().unwrap().push(json!("mode"));
                    }
                    if let Some(actions) = model["actions"].as_array_mut() {
                        actions.retain(|action| match action.as_str() {
                            Some("previous") => *page > 0,
                            Some("next") if table.is_some() => count == size,
                            Some("next") => offset + count < data.rows.len(),
                            _ => true,
                        });
                    }
                    format!(
                        "[{}] {name} › {location}",
                        if source.starts_with("schema:") {
                            "schema"
                        } else if table.is_some() {
                            "rows"
                        } else {
                            "results"
                        }
                    )
                }
            }
            Content::Value {
                name,
                data,
                row,
                column,
                raw,
                ..
            } => {
                primary = "Expand/collapse";
                let cell = &data.rows[*row][*column];
                let field = data.columns.get(*column);
                metadata(
                    &mut entries,
                    "Database type",
                    field.map_or("unknown", |c| c.kind.as_str()),
                );
                metadata(
                    &mut entries,
                    "Display",
                    if cell.truncated {
                        "Preview"
                    } else {
                        "Complete preview"
                    },
                );
                if let Some(size) = cell.complete_size {
                    metadata(&mut entries, "Value size", format!("{size} bytes"));
                }
                if let Some(reason) = cell.unavailable_reason() {
                    metadata(&mut entries, "Full value unavailable", reason);
                }
                model["actions"] = json!(["activate", "raw", "back"]);
                if self.document_views && can_show_full(cell) {
                    model["actions"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("show-full"));
                }
                let location = parent
                    .and_then(|parent| parent.model["title"].as_str())
                    .map(suffix)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{name} › row {}", row + 1));
                let field_name = field.map_or("(unnamed)", |c| {
                    if c.name.is_empty() {
                        "(unnamed)"
                    } else {
                        &c.name
                    }
                });
                let json = cell.representation == crate::results::Representation::Text
                    && cell.text.as_ref().is_some_and(|text| {
                        text.trim_start().starts_with(['{', '['])
                            && (cell.truncated
                                || serde_json::from_str::<&serde_json::value::RawValue>(text)
                                    .is_ok())
                    });
                if self.action_presentation {
                    let mut raw_action = presentation("raw");
                    raw_action["label"] = json!(if *raw {
                        "Show formatted value"
                    } else {
                        "Show raw value"
                    });
                    model["action_presentation"] = json!({"raw":raw_action});
                }
                let field_path = if parent.is_some_and(|parent| {
                    matches!(
                        parent.content,
                        Content::Value { .. } | Content::FullValue { .. }
                    )
                }) {
                    if json {
                        location
                            .strip_suffix(" · JSON")
                            .unwrap_or(&location)
                            .to_owned()
                    } else {
                        location
                    }
                } else {
                    format!("{location} › {field_name}")
                };
                format!("[value] {field_path}{}", if json { " · JSON" } else { "" })
            }
        };
        model["title"] = json!(views::short(&crate::results::escape(&title), 150));
        if self.action_presentation {
            let mut primary_action = presentation("activate");
            primary_action["label"] = json!(primary);
            if !model["action_presentation"].is_object() {
                model["action_presentation"] = json!({});
            }
            model["action_presentation"]["activate"] = primary_action;
        }
        if self.view_help {
            model["help"] = json!(super::help::topic(content));
        }
        if self.view_metadata {
            model["metadata"] = json!(entries);
        } else if !entries.is_empty() {
            let status = model["status"]["text"].as_str().unwrap_or("");
            let summary = entries
                .iter()
                .map(|entry| {
                    format!(
                        "{}: {}",
                        entry["label"].as_str().unwrap(),
                        entry["value"].as_str().unwrap()
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ");
            model["status"]["text"] = json!(views::short(&format!("{status} · {summary}"), 1000));
        }
        model
    }
}
