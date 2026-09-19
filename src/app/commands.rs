// SPDX-License-Identifier: MPL-2.0
use super::*;
impl App {
    pub(super) async fn command(&mut self, ctx: Value) -> Result<Option<String>> {
        let authored = string(&ctx, "command")?;
        let command = authored
            .strip_prefix("global-")
            .unwrap_or(&authored)
            .to_owned();
        // Even workspace commands invoked from a view must use its current revision.
        if ctx["view"].is_string() {
            self.context_view(&ctx)?;
        }
        if matches!(
            command.as_str(),
            "filters"
                | "add-filter"
                | "edit-filter"
                | "remove-filter"
                | "toggle-filter"
                | "clear-filters"
                | "match"
                | "apply-filters"
                | "sort"
                | "page-size"
                | "browse-sql"
        ) {
            return self.browse_command(&ctx, &command).await;
        }
        match command.as_str() {
            "open" => {
                self.create(&ctx, Content::Connections).await?;
            }
            "connect" if authored == "connect" && ctx["view"].is_string() => {
                let (_, content) = self.context_view(&ctx)?;
                if !matches!(content, Content::Connections) {
                    return Err("Select a saved database profile".into());
                }
                let name = self.target(&ctx)?;
                if self.connections.contains_key(&name) {
                    return Err("Database is already connected".into());
                }
                let profile = self
                    .saved
                    .profiles
                    .iter()
                    .find(|p| p.name() == name)
                    .cloned()
                    .ok_or("Profile no longer exists")?;
                return self.password(&ctx, profile, false).await;
            }
            "connect" | "connect-new" => {
                self.pick(
                    &ctx,
                    "Database type",
                    vec!["SQLite".into(), "PostgreSQL".into()],
                    Input::Backend,
                )
                .await?;
            }
            "use" => {
                self.pick(
                    &ctx,
                    "Database for this SQL buffer",
                    self.connections.keys().cloned().collect(),
                    Input::Use(string(&ctx, "buffer")?),
                )
                .await?;
            }
            "return" => {
                let parent = ctx["buffer"]
                    .as_str()
                    .and_then(|b| self.sources.get(b))
                    .cloned();
                self.back(&ctx, parent).await?;
            }
            "transactions" => {
                self.create(
                    &ctx,
                    Content::Transactions {
                        entries: self.transaction_entries(),
                    },
                )
                .await?;
            }
            "back" => {
                let (id, _) = self.context_view(&ctx)?;
                let parent = self.views[&id].parent.clone();
                self.back(&ctx, parent).await?;
            }
            "raw" => {
                let (id, mut content) = self.context_view(&ctx)?;
                if let Content::Value { raw, .. } = &mut content {
                    *raw = !*raw;
                    self.publish(&id, content).await?;
                }
            }
            "query" => {
                let name = self.target(&ctx)?;
                self.query_document(&ctx, name).await?;
            }
            "run" | "run-selection" => {
                let intent = self.capture(&ctx, command == "run-selection").await?;
                if self.connections[&intent.name].writable {
                    self.create(
                        &ctx,
                        Content::Review {
                            generation: intent.generation,
                            name: intent.name,
                            sql: intent.sql,
                            source: intent.source,
                        },
                    )
                    .await?;
                } else {
                    return self.execute(&ctx, intent).await;
                }
            }
            "cancel" => {
                let name = self.target(&ctx)?;
                if let Some((job, cancel)) = self.connecting.get(&name) {
                    cancel.cancel();
                    self.rpc.request("job.cancel", json!({"job":job})).await?;
                }
                if let Some(c) = self.connections.get(&name) {
                    c.cancel.cancel();
                    if let Some(job) = &c.job {
                        self.rpc.request("job.cancel", json!({"job":job})).await?;
                    }
                }
            }
            "commit" | "rollback" => {
                let name = self.target(&ctx)?;
                return self.settle(&ctx, name, command == "commit").await;
            }
            "disconnect" => {
                let name = self.target(&ctx)?;
                let c = self.connections.get(&name).ok_or("Database disconnected")?;
                if c.busy {
                    return Err("Cancel the operation first".into());
                }
                if c.pending {
                    self.confirm(
                        &ctx,
                        "Disconnect database?",
                        "Roll back the pending transaction and disconnect?",
                        Input::Disconnect(name, c.generation),
                    )
                    .await?;
                } else {
                    self.connections.remove(&name);
                    self.refresh_status(&name).await?;
                }
            }
            "activate" => {
                let (_, content) = self.context_view(&ctx)?;
                if let Content::Value {
                    mut collapsed,
                    name,
                    data,
                    row,
                    column,
                    raw,
                } = content
                {
                    let path = ctx["rows"]
                        .as_array()
                        .filter(|a| a.len() == 1)
                        .and_then(|a| a[0].as_str())
                        .ok_or("Select one JSON node")?
                        .to_owned();
                    if path.len() > 256 || !path.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
                        return Err("Invalid JSON node".into());
                    }
                    if !collapsed.remove(&path) && collapsed.len() < 4096 {
                        collapsed.insert(path);
                    }
                    self.publish(
                        ctx["view"].as_str().unwrap(),
                        Content::Value {
                            collapsed,
                            name,
                            data,
                            row,
                            column,
                            raw,
                        },
                    )
                    .await?;
                    return Ok(None);
                }
                let i = Self::selected(&ctx)?;
                match content {
                    Content::Connections => {
                        let p = self
                            .saved
                            .profiles
                            .get(i)
                            .cloned()
                            .ok_or("Profile no longer exists")?;
                        self.active = Some(p.name().into());
                        if self.connections.contains_key(p.name()) {
                            return self
                                .load(&ctx, p.name().into(), None, 0, false, false)
                                .await;
                        }
                        return self.password(&ctx, p, false).await;
                    }
                    Content::Filters { .. }
                    | Content::Transactions { .. }
                    | Content::Value { .. } => {
                        return Err("Use Tab for contextual actions".into());
                    }
                    Content::Catalog { name, tables, .. } => {
                        let t = tables.get(i).cloned().ok_or("Table no longer exists")?;
                        return self.load(&ctx, name, Some(t), 0, false, false).await;
                    }
                    Content::Review {
                        name,
                        sql,
                        source,
                        generation,
                    } => {
                        self.ready(&name)?;
                        self.confirm(&ctx,&format!("Execute reviewed SQL on {name}?"),"Execute the exact SQL in this review? Changes await Commit or Rollback.",Input::Run(Intent{name,sql,source,generation})).await?;
                    }
                    Content::Result {
                        name,
                        data,
                        table,
                        page,
                        offset,
                        columns,
                        record,
                        source,
                    } => {
                        if let Some(r) = record {
                            data.rows
                                .get(r)
                                .and_then(|r| r.get(i))
                                .ok_or("Value no longer exists")?;
                            self.create(
                                &ctx,
                                Content::Value {
                                    name,
                                    data,
                                    row: r,
                                    column: i,
                                    raw: false,
                                    collapsed: Default::default(),
                                },
                            )
                            .await?;
                        } else {
                            if i >= data.rows.len() {
                                return Err("Row no longer exists".into());
                            }
                            self.create(
                                &ctx,
                                Content::Result {
                                    name,
                                    data,
                                    table,
                                    page,
                                    offset,
                                    columns,
                                    record: Some(i),
                                    source,
                                },
                            )
                            .await?;
                        }
                    }
                }
            }
            "profile-actions" => {
                let name = self.target(&ctx)?;
                let generation = self.connections.get(&name).map(|c| c.generation);
                let choices = self.profile_choices(&name);
                self.pick(
                    &ctx,
                    &format!(
                        "{} · {}",
                        views::short(&name, 60),
                        views::short(&self.state(&name), 80)
                    ),
                    choices.clone(),
                    Input::ProfileActions {
                        source: string(&ctx, "view")?,
                        name,
                        generation,
                        choices,
                    },
                )
                .await?;
            }
            "mode" => {
                let name = self.target(&ctx)?;
                self.mode_picker(&ctx, name).await?;
            }

            "acknowledge" => {
                let (_, content) = self.context_view(&ctx)?;
                let name = if matches!(content, Content::Connections) {
                    self.saved
                        .profiles
                        .get(Self::selected(&ctx)?)
                        .ok_or("Select a profile")?
                        .name()
                        .to_owned()
                } else {
                    content
                        .name()
                        .ok_or("Select an uncertain database")?
                        .to_owned()
                };
                if self
                    .connections
                    .get(&name)
                    .is_some_and(|c| c.busy || c.pending)
                {
                    return Err("Resolve the live transaction first".into());
                }
                if !self.saved.uncertain.contains(&name) {
                    return Err("No uncertain outcome to acknowledge".into());
                }
                self.confirm(&ctx,"Acknowledge uncertain outcome?","Only continue after independently reviewing the database. This does not undo or replay SQL.",Input::Acknowledge(name)).await?;
            }
            "columns" => {
                let (id, content) = self.context_view(&ctx)?;
                if let Content::Result { data, columns, .. } = content {
                    let selected = if columns.is_empty() {
                        (0..data.columns.len().min(8)).collect()
                    } else {
                        columns
                    };
                    self.column_picker(&ctx, id, selected, String::new(), 0)
                        .await?;
                } else {
                    return Err("Open rows first".into());
                }
            }
            "refresh" | "schema" | "system" | "next" | "previous" => {
                let (id, content) = self.context_view(&ctx)?;
                match content {
                    Content::Catalog {
                        name,
                        tables,
                        system,
                    } => {
                        let table = if command == "schema" {
                            Some(
                                tables
                                    .get(Self::selected(&ctx)?)
                                    .cloned()
                                    .ok_or("Select a table")?,
                            )
                        } else {
                            None
                        };
                        return self
                            .load(
                                &ctx,
                                name,
                                table,
                                0,
                                command == "schema",
                                if command == "system" { !system } else { system },
                            )
                            .await;
                    }
                    Content::Result {
                        name,
                        data,
                        table,
                        page,
                        offset,
                        columns,
                        record,
                        source,
                    } => {
                        if command == "back" && record.is_some() {
                            self.publish(
                                &id,
                                Content::Result {
                                    name,
                                    data,
                                    table,
                                    page,
                                    offset,
                                    columns,
                                    record: None,
                                    source,
                                },
                            )
                            .await?;
                        } else if command == "back" {
                            return self.load(&ctx, name, None, 0, false, false).await;
                        } else if let Some(table) = table {
                            let page = if command == "next" {
                                page + 1
                            } else if command == "previous" {
                                page.saturating_sub(1)
                            } else {
                                page
                            };
                            return self
                                .load(&ctx, name, Some(table), page, command == "schema", false)
                                .await;
                        } else if command == "refresh" {
                            return Err(
                                "SQL results are retained; use ::db-run for explicit re-execution"
                                    .into(),
                            );
                        } else {
                            let offset = if command == "next" {
                                (offset + 100).min(data.rows.len().saturating_sub(1) / 100 * 100)
                            } else {
                                offset.saturating_sub(100)
                            };
                            self.publish(
                                &id,
                                Content::Result {
                                    name,
                                    data,
                                    table: None,
                                    page: offset / 100,
                                    offset,
                                    columns,
                                    record: None,
                                    source,
                                },
                            )
                            .await?;
                        }
                    }
                    Content::Filters { .. } => return Err("Apply filters or return to rows".into()),
                    Content::Transactions { .. } => {
                        self.publish(
                            &id,
                            Content::Transactions {
                                entries: self.transaction_entries(),
                            },
                        )
                        .await?;
                    }
                    Content::Value { .. } => {
                        return Err("Retained value; use back to return".into());
                    }
                    Content::Review { .. } => {
                        return Err(
                            "Return to your SQL buffer to edit or execute another query".into()
                        );
                    }
                    Content::Connections => {
                        self.publish(&id, Content::Connections).await?;
                    }
                }
            }
            _ => return Err("Unknown database command".into()),
        }
        Ok(None)
    }
}
