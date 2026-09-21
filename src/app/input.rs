// SPDX-License-Identifier: MPL-2.0
use super::*;
impl App {
    pub(super) fn restore_input_source(&self, ctx: &mut Value) {
        if ctx["surface"]
            .as_str()
            .is_some_and(|id| self.input.contains_key(id))
            && let Some(source) = &self.input_source
            && let Some(view) = self.views.get(source)
        {
            ctx["view"] = json!(source);
            ctx["model_revision"] = json!(view.revision);
        }
    }
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
        self.input_epoch = self.input_epoch.wrapping_add(1);
        self.input_source = ctx["view"].as_str().map(str::to_owned);
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
        self.input_epoch = self.input_epoch.wrapping_add(1);
        self.input_source = ctx["view"].as_str().map(str::to_owned);
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
        self.input_epoch = self.input_epoch.wrapping_add(1);
        self.input_source = ctx["view"].as_str().map(str::to_owned);
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
            Input::Run(_) | Input::Mode(..) | Input::Acknowledge(_) | Input::Disconnect(..)
        ) && v["confirmed"] != true
        {
            return Err("Physical confirmation was not accepted".into());
        }
        let value = |key: &str| v[key].as_str().unwrap_or("").to_owned();
        if matches!(
            next,
            Input::ColumnPick { .. }
                | Input::ColumnSearch { .. }
                | Input::FieldChoice { .. }
                | Input::FieldSearch { .. }
                | Input::FilterOp { .. }
                | Input::FilterValue { .. }
                | Input::Match(_)
                | Input::SortDirection(..)
                | Input::PageSize(_)
        ) {
            return self.browse_submit(&ctx, next).await;
        }
        match next {
            Input::ColumnPick { .. }
            | Input::ColumnSearch { .. }
            | Input::FieldChoice { .. }
            | Input::FieldSearch { .. }
            | Input::FilterOp { .. }
            | Input::FilterValue { .. }
            | Input::Match(_)
            | Input::SortDirection(..)
            | Input::PageSize(_) => unreachable!(),
            Input::Backend => {
                let choice = value("choice");
                if choice == "SQLite" {
                    let mut path = json!({"id":"path","label":"Local SQLite file (required)","kind":"text","required":true,"maximum_length":4096,"validate":true,"validation_message":"Use an existing local SQLite file or directory/prefix; web URLs are not supported"});
                    if self.path_completion {
                        path["completion"] = json!("local-path");
                    }
                    self.form(
                        &ctx,
                        "SQLite form · local file",
                        vec![
                            json!({"id":"name","label":"Profile name (required)","kind":"text","required":true,"maximum_length":64,"validate":true,"validation_message":"Use a unique, nonempty profile name (maximum 64 bytes)"}),
                            path,
                        ],
                        Input::Sqlite,
                    )
                    .await?;
                } else if choice == "PostgreSQL" {
                    self.form(
                        &ctx,
                        "PostgreSQL connection · local or remote server",
                        vec![
                            field("name", "Profile name", "text", true),
                            field("host", "Host or Unix socket directory", "text", true),
                            field("port", "Port (empty: 5432)", "text", false),
                            field("database", "Database", "text", true),
                            field("user", "User", "text", true),
                            field(
                                "password-env",
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
            Input::SqlitePath { .. } | Input::Sqlite => {
                return Err("Path submission was not captured".into());
            }
            Input::Postgres => {
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
                        password_env: value("password-env"),
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
            Input::ProfileActions {
                source,
                name,
                generation,
                choices,
            } => {
                let choice = value("choice");
                if !choices.contains(&choice) {
                    return Err("Profile action expired".into());
                }
                if let Some(generation) = generation {
                    self.check_generation(&name, generation)?;
                } else if self.connections.contains_key(&name) {
                    return Err("Connection changed; choose the profile again".into());
                }
                let mut source_ctx = ctx.clone();
                if let Some(view) = self.views.get(&source) {
                    source_ctx["view"] = json!(source);
                    source_ctx["model_revision"] = json!(view.revision);
                }
                match choice.as_str() {
                    "query" => self.query_document(&source_ctx, name).await?,
                    "mode" => self.mode_picker(&ctx, name).await?,
                    "transactions" => {
                        self.create(
                            &source_ctx,
                            Content::Transactions {
                                entries: self.transaction_entries(),
                            },
                        )
                        .await?;
                    }
                    "commit" | "rollback" => {
                        return self.settle(&ctx, name, choice == "commit").await;
                    }
                    "disconnect" => {
                        let c = self.connections.get(&name).ok_or("Database disconnected")?;
                        if c.busy {
                            return Err("Operation running; use ::db-cancel first".into());
                        }
                        if c.pending {
                            self.confirm(
                                &ctx,
                                "Disconnect database?",
                                "Roll back pending changes and disconnect?",
                                Input::Disconnect(name, c.generation),
                            )
                            .await?;
                        } else {
                            self.connections.remove(&name);
                            self.refresh_status(&name).await?;
                        }
                    }
                    "connect" => {
                        let p = self
                            .saved
                            .profiles
                            .iter()
                            .find(|p| p.name() == name)
                            .cloned()
                            .ok_or("Profile missing")?;
                        return self.password(&source_ctx, p, false).await;
                    }
                    "Acknowledge uncertain outcome" => {
                        if !self.saved.uncertain.contains(&name) {
                            return Err("Uncertain outcome already acknowledged".into());
                        }
                        self.confirm(&ctx,"Acknowledge uncertain outcome?","Independently review the database first. Acknowledgement does not commit, roll back, or replay SQL.",Input::Acknowledge(name)).await?;
                    }
                    _ => return Err("Profile action expired".into()),
                }
            }
            Input::ModeChoice(name, generation) => {
                self.check_generation(&name, generation)?;
                let write = match value("choice").as_str() {
                    "READ ONLY" => false,
                    "READ AND WRITE" => true,
                    _ => return Err("Choose an access mode".into()),
                };
                let c = &self.connections[&name];
                if c.writable == write {
                    return Err("This access mode is already selected; connection unchanged".into());
                }
                if c.pending {
                    return Err("Commit or roll back the pending transaction, then choose the mode again; connection unchanged".into());
                }
                self.ready(&name)?;
                if write {
                    self.confirm(&ctx,"Enable READ AND WRITE?","Reconnect writable? SQL requires review and explicit settlement. Existing SQL buffers will need ::db-use.",Input::Mode(name,generation,write)).await?;
                } else {
                    let p = self
                        .saved
                        .profiles
                        .iter()
                        .find(|p| p.name() == name)
                        .cloned()
                        .ok_or("Profile missing")?;
                    return self.password(&ctx, p, false).await;
                }
            }
            Input::Mode(name, generation, write) => {
                self.check_generation(&name, generation)?;
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
            Input::Disconnect(name, generation) => {
                self.check_generation(&name, generation)?;
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
                self.refresh_status(&name).await?;
                result?;
            }
        }
        Ok(None)
    }
}
impl App {
    pub(super) async fn mode_picker(&mut self, ctx: &Value, name: String) -> Result<()> {
        let c = self.connections.get(&name).ok_or("Database disconnected")?;
        if c.busy {
            return Err("Operation running; use ::db-cancel first".into());
        }
        let generation = c.generation;
        let current = if c.writable {
            "READ AND WRITE"
        } else {
            "READ ONLY"
        };
        self.pick(
            ctx,
            &format!("Access mode · current: {current}"),
            vec!["READ ONLY".into(), "READ AND WRITE".into()],
            Input::ModeChoice(name, generation),
        )
        .await
    }
    pub(super) fn validate_form(&self, message: &Value) {
        let ctx = &message["params"];
        let sqlite = ctx["surface"]
            .as_str()
            .and_then(|s| self.input.get(s))
            .is_some_and(|i| matches!(i, Input::Sqlite));
        let names = self
            .saved
            .profiles
            .iter()
            .map(|p| p.name().to_owned())
            .collect::<Vec<_>>();
        let rpc = self.rpc.clone();
        let permit = self.path_slots.clone().try_acquire_owned().ok();
        let message = message.clone();
        tokio::spawn(async move {
            let reply=tokio::task::spawn_blocking(move||{
                let available=permit.is_some();let _permit=permit;
                let ctx=&message["params"];
                let name=ctx["values"]["name"].as_str().unwrap_or("");
                let path=ctx["values"]["path"].as_str().unwrap_or("");
                let valid_name=!name.trim().is_empty()&&name.len()<=64&&!name.chars().any(char::is_control)&&!names.iter().any(|n|n==name);
                let valid_path=available&&std::env::current_dir().ok().is_some_and(|root|crate::paths::resolve(&root,path).is_ok());
                let fields=ctx["fields"].as_array().map(|a|a.iter().map(|f|json!({"field":f,"status":if !sqlite || !available {"unavailable"}else if (f=="name"&&valid_name)||(f=="path"&&valid_path){"valid"}else{"invalid"}})).collect::<Vec<_>>()).unwrap_or_default();
                json!({"type":"response","id":message["id"],"result":{"kind":"validation","surface":ctx["surface"],"revision":ctx["revision"],"fields":fields}})
            }).await;
            if let Ok(reply) = reply {
                let _ = rpc.send(reply);
            }
        });
    }
    pub(super) fn defer_path(&mut self, ctx: &Value) -> Result<bool> {
        let surface = ctx["surface"].as_str().unwrap_or("");
        if ctx["accepted"] != true
            || !self
                .input
                .get(surface)
                .is_some_and(|i| matches!(i, Input::Sqlite | Input::SqlitePath { .. }))
        {
            return Ok(false);
        }
        if self.path_pending {
            return Err("Path completion is still running; try again after it finishes".into());
        }
        let (name, path) = match self.input.remove(surface).unwrap() {
            Input::Sqlite => (
                string(&ctx["values"], "name")?,
                string(&ctx["values"], "path")?,
            ),
            Input::SqlitePath { name, choices } => {
                let choice = ctx["values"]["choice"].as_str().unwrap_or("");
                let path = choices
                    .iter()
                    .find(|(label, _)| label == choice)
                    .ok_or("Path choice expired")?
                    .1
                    .clone();
                (name, path)
            }
            _ => unreachable!(),
        };
        Profile::Sqlite {
            name: name.clone(),
            path: path.clone(),
        }
        .validate()?;
        if self.saved.profiles.iter().any(|p| p.name() == name) {
            return Err("Profile name already exists".into());
        }
        let permit = self
            .path_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Path workers are busy; try again later")?;
        self.path_pending = true;
        let epoch = self.input_epoch;
        let ctx = ctx.clone();
        let tx = self.sender.clone();
        tokio::spawn(async move {
            let worker = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                crate::paths::resolve(
                    &std::env::current_dir().map_err(|_| "Workspace unavailable")?,
                    &path,
                )
            });
            let result = match tokio::time::timeout(Duration::from_secs(8), worker).await {
                Ok(Ok(result)) => result,
                _ => Err("Path completion unavailable; retry with a local existing path".into()),
            };
            let _ = tx
                .send(Completion {
                    job: String::new(),
                    view: String::new(),
                    result: Work::Path(PathCompletion {
                        ctx,
                        name,
                        result,
                        epoch,
                    }),
                })
                .await;
        });
        Ok(true)
    }
    pub(super) async fn complete_path(&mut self, done: PathCompletion) -> Result<()> {
        self.path_pending = false;
        let id = string(&done.ctx, "invocation")?;
        let result = if self.input_epoch != done.epoch {
            Err("Path completion superseded by newer input".into())
        } else {
            match done.result {
                Ok(destination) => {
                    self.sqlite_destination(&done.ctx, done.name, destination)
                        .await
                }
                Err(e) => Err(e),
            }
        };
        self.rpc.reply(&id, result)
    }
    async fn sqlite_destination(
        &mut self,
        ctx: &Value,
        name: String,
        destination: crate::paths::Destination,
    ) -> Result<Option<String>> {
        match destination {
            crate::paths::Destination::Choices(paths) => {
                let choices = paths
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        (
                            format!(
                                "{}: {}",
                                i + 1,
                                views::short(&crate::results::escape(&p.to_string_lossy()), 145)
                            ),
                            p.to_string_lossy().into_owned(),
                        )
                    })
                    .collect::<Vec<_>>();
                self.pick(
                    ctx,
                    "Resolved paths · choose a directory to continue or a file to connect",
                    choices.iter().map(|(s, _)| s.clone()).collect(),
                    Input::SqlitePath { name, choices },
                )
                .await?;
                Ok(None)
            }
            crate::paths::Destination::File(path) => {
                if self.saved.profiles.len() >= 32 {
                    return Err("At most 32 profiles".into());
                }
                let p = Profile::Sqlite {
                    name,
                    path: path.to_str().ok_or("Path must be UTF-8")?.into(),
                };
                self.saved.profiles.push(p.clone());
                if let Err(e) = self.save().await {
                    self.saved.profiles.pop();
                    return Err(e);
                }
                self.password(ctx, p, false).await
            }
        }
    }
}
