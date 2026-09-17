// SPDX-License-Identifier: MPL-2.0
use super::*;
impl App {
    pub(super) async fn form(
        &mut self,
        ctx: &Value,
        title: &str,
        fields: Vec<Value>,
        next: Input,
    ) -> Result<()> {
        let v = self
            .rpc
            .request(
                "ui.form",
                json!({"invocation":ctx["invocation"],"title":title,"fields":fields}),
            )
            .await?;
        self.input.clear();
        self.input.insert(string(&v, "surface")?, next);
        Ok(())
    }
    pub(super) async fn confirm(
        &mut self,
        ctx: &Value,
        title: &str,
        message: &str,
        next: Input,
    ) -> Result<()> {
        let v=self.rpc.request("ui.confirm",json!({"invocation":ctx["invocation"],"title":views::short(title,150),"message":views::short(message,150)})).await?;
        self.input.clear();
        self.input.insert(string(&v, "surface")?, next);
        Ok(())
    }
    pub(super) async fn pick(
        &mut self,
        ctx: &Value,
        title: &str,
        choices: Vec<String>,
        next: Input,
    ) -> Result<()> {
        if choices.is_empty() {
            return Err("No databases available; use ::db-connect".into());
        }
        let v = self
            .rpc
            .request(
                "ui.pick",
                json!({"invocation":ctx["invocation"],"title":title,"choices":choices}),
            )
            .await?;
        self.input.clear();
        self.input.insert(string(&v, "surface")?, next);
        Ok(())
    }
    pub(super) async fn submit(&mut self, ctx: Value) -> Result<Option<String>> {
        let surface = string(&ctx, "surface")?;
        let next = self.input.remove(&surface).ok_or("Input expired")?;
        if ctx["accepted"] != true {
            return Ok(None);
        }
        let v = &ctx["values"];
        if matches!(
            next,
            Input::Run(_) | Input::Mode(..) | Input::Acknowledge(_) | Input::Disconnect(_)
        ) && v["confirmed"] != true
        {
            return Err("Physical confirmation was not accepted".into());
        }
        let value = |key: &str| v[key].as_str().unwrap_or("").to_owned();
        match next {
            Input::Backend => {
                let choice = value("choice");
                if choice == "SQLite" {
                    self.form(
                        &ctx,
                        "SQLite connection",
                        vec![
                            field("name", "Profile name", "text", true),
                            field("path", "Existing database path", "text", true),
                        ],
                        Input::Sqlite,
                    )
                    .await?;
                } else if choice == "PostgreSQL" {
                    self.form(
                        &ctx,
                        "PostgreSQL connection",
                        vec![
                            field("name", "Profile name", "text", true),
                            field("host", "Host or Unix socket directory", "text", true),
                            field("port", "Port (empty: 5432)", "text", false),
                            field("database", "Database", "text", true),
                            field("user", "User", "text", true),
                            field(
                                "password_env",
                                "Password environment variable (optional)",
                                "text",
                                false,
                            ),
                            field(
                                "plaintext",
                                "Allow plaintext TCP (default: verified TLS)",
                                "boolean",
                                false,
                            ),
                            field("ca", "Custom CA PEM path (optional)", "text", false),
                            field(
                                "certificate",
                                "Client certificate PEM path (optional)",
                                "text",
                                false,
                            ),
                            field("key", "Client key PEM path (optional)", "text", false),
                        ],
                        Input::Postgres,
                    )
                    .await?;
                } else {
                    return Err("Choose SQLite or PostgreSQL".into());
                }
            }
            Input::Sqlite | Input::Postgres => {
                let p = if matches!(next, Input::Sqlite) {
                    Profile::Sqlite {
                        name: value("name"),
                        path: value("path"),
                    }
                } else {
                    Profile::Postgres {
                        name: value("name"),
                        host: value("host"),
                        port: if value("port").is_empty() {
                            5432
                        } else {
                            value("port").parse().map_err(|_| "Invalid port")?
                        },
                        database: value("database"),
                        user: value("user"),
                        password_env: value("password_env"),
                        plaintext: v["plaintext"].as_bool().unwrap_or(false),
                        ca: value("ca"),
                        certificate: value("certificate"),
                        key: value("key"),
                    }
                };
                p.validate()?;
                if self.saved.profiles.len() >= 32 {
                    return Err("At most 32 profiles".into());
                }
                if self.saved.profiles.iter().any(|old| old.name() == p.name()) {
                    return Err("Profile name already exists".into());
                }
                self.saved.profiles.push(p.clone());
                if let Err(e) = self.save().await {
                    self.saved.profiles.pop();
                    return Err(e);
                }
                return self.password(&ctx, p, false).await;
            }
            Input::Password(p, write) => {
                return self.connect(&ctx, p, write, value("password")).await;
            }
            Input::Use(buffer) => {
                let name = value("choice");
                self.ready(&name)?;
                self.buffers
                    .insert(buffer, (name.clone(), self.connections[&name].generation));
                self.active = Some(name);
            }
            Input::Mode(name, write) => {
                let p = self
                    .saved
                    .profiles
                    .iter()
                    .find(|p| p.name() == name)
                    .cloned()
                    .ok_or("Profile missing")?;
                self.ready(&name)?;
                return self.password(&ctx, p, write).await;
            }
            Input::Run(intent) => return self.execute(&ctx, intent).await,
            Input::Columns(id) => {
                let mut content = self.views.get(&id).ok_or("View closed")?.content.clone();
                if let Content::Result { data, columns, .. } = &mut content {
                    let values = value("columns")
                        .split(',')
                        .map(|s| {
                            s.trim()
                                .parse::<usize>()
                                .map_err(|_| "Use column numbers separated by commas")
                        })
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    if values.is_empty()
                        || values.len() > 8
                        || values.iter().any(|&n| n == 0 || n > data.columns.len())
                    {
                        return Err("Choose one to eight valid column numbers".into());
                    }
                    let mut sorted = values.clone();
                    sorted.sort_unstable();
                    sorted.dedup();
                    if sorted.len() != values.len() {
                        return Err("Choose each column once".into());
                    }
                    *columns = values.into_iter().map(|n| n - 1).collect();
                    self.publish(&id, content).await?;
                }
            }
            Input::Acknowledge(name) => {
                if self
                    .connections
                    .get(&name)
                    .is_some_and(|c| c.pending || c.busy)
                {
                    return Err("Live transaction must be resolved first".into());
                }
                self.saved.uncertain.retain(|n| n != &name);
                self.save().await?;
            }
            Input::Disconnect(name) => {
                let c = self.connections.get(&name).ok_or("Disconnected")?;
                if c.busy {
                    return Err("Operation still running".into());
                }
                let result = c.db.settle(false).await;
                self.release(&name).await;
                self.connections.remove(&name);
                if result.is_ok() {
                    self.saved.uncertain.retain(|n| n != &name);
                    self.save().await?;
                }
                result?;
            }
        }
        Ok(None)
    }
}
