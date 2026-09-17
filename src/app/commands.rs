// SPDX-License-Identifier: MPL-2.0
use super::*;
impl App {
    pub(super) async fn command(&mut self, ctx: Value) -> Result<Option<String>> {
        let authored = string(&ctx, "command")?;
        let command = authored
            .strip_prefix("view-")
            .unwrap_or(&authored)
            .to_owned();
        // Even workspace commands invoked from a view must use its current revision.
        if ctx["view"].is_string() {
            self.context_view(&ctx)?;
        }
        match command.as_str() {
            "open" => {
                self.create(&ctx, Content::Connections).await?;
            }
            "connect" => {
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
                        Input::Disconnect(name),
                    )
                    .await?;
                } else {
                    self.connections.remove(&name);
                }
            }
            "activate" => {
                let (_, content) = self.context_view(&ctx)?;
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
                        let id = string(&ctx, "view")?;
                        if let Some(r) = record {
                            if self.views.len() >= 12 {
                                return Err("Close a database view before opening another".into());
                            }
                            let cell = data
                                .rows
                                .get(r)
                                .and_then(|r| r.get(i))
                                .ok_or("Value no longer exists")?;
                            let model = views::text_model(
                                &format!("{name} · value {}", i + 1),
                                "Retained value; truncation is explicit",
                                vec![json!({"id":"value","text":cell.display(),"role":"ordinary"})],
                                &["back"],
                            );
                            let v = self
                                .rpc
                                .request("view.create", json!({"model":model}))
                                .await?;
                            let new = string(&v, "view")?;
                            self.views.insert(
                                new.clone(),
                                View {
                                    published: tokio::time::Instant::now(),
                                    revision: string(&v, "revision")?,
                                    content: Content::Result {
                                        name,
                                        data,
                                        table,
                                        page,
                                        offset,
                                        columns,
                                        record,
                                        source,
                                    },
                                },
                            );
                            self.rpc
                                .request(
                                    "pane.show",
                                    json!({"invocation":ctx["invocation"],"view":new}),
                                )
                                .await?;
                        } else {
                            if i >= data.rows.len() {
                                return Err("Row no longer exists".into());
                            }
                            self.publish(
                                &id,
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
            "mode" => {
                let name = self.target(&ctx)?;
                self.ready(&name)?;
                let write = !self.connections[&name].writable;
                self.confirm(
                    &ctx,
                    &format!("Change access for {name}?"),
                    if write {
                        "Reconnect in writable mode? Every SQL execution will require confirmation."
                    } else {
                        "Reconnect in read-only mode?"
                    },
                    Input::Mode(name, write),
                )
                .await?;
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
                    self.target(&ctx)?
                };
                if self
                    .connections
                    .get(&name)
                    .is_some_and(|c| c.busy || c.pending)
                {
                    return Err("Resolve the live transaction first".into());
                }
                self.confirm(&ctx,"Acknowledge previous outcome?","Only continue after independently reviewing the database. This does not undo or replay SQL.",Input::Acknowledge(name)).await?;
            }
            "columns" => {
                let (id, content) = self.context_view(&ctx)?;
                if let Content::Result { data, .. } = content {
                    self.form(
                        &ctx,
                        &format!("Visible columns: 1–{} (up to eight)", data.columns.len()),
                        vec![field(
                            "columns",
                            "Comma-separated column numbers",
                            "text",
                            true,
                        )],
                        Input::Columns(id),
                    )
                    .await?;
                } else {
                    return Err("Open query results first".into());
                }
            }
            "refresh" | "schema" | "system" | "next" | "previous" | "back" => {
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
