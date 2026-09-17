// SPDX-License-Identifier: MPL-2.0
use super::*;
impl App {
    pub(super) async fn connect(
        &mut self,
        ctx: &Value,
        p: Profile,
        writable: bool,
        password: String,
    ) -> Result<Option<String>> {
        if self.connections.len() + self.connecting.len() >= 2
            && !self.connections.contains_key(p.name())
        {
            return Err("At most two live databases; disconnect one first".into());
        }
        if self.connecting.contains_key(p.name()) {
            return Err("Connection attempt already running".into());
        }
        if self
            .connections
            .get(p.name())
            .is_some_and(|c| c.busy || c.pending)
        {
            return Err("Resolve the current operation first".into());
        }
        if self.saved.uncertain.iter().any(|n| n == p.name()) {
            return Err("Acknowledge the previous uncertain operation after review".into());
        }
        self.active = Some(p.name().to_owned());
        let view = self
            .create(
                ctx,
                Content::Catalog {
                    name: p.name().into(),
                    tables: vec![],
                    system: false,
                },
            )
            .await?;
        let (job, cancel) = self.job("Connect to database").await?;
        let tx = self.sender.clone();
        let j = job.clone();
        self.connections.remove(p.name());
        self.connecting
            .insert(p.name().into(), (job.clone(), cancel.clone()));
        tokio::spawn(async move {
            let result = tokio::select! {biased;_=cancel.cancelled()=>Err("Connection cancelled".into()),r=Database::open(&p,writable,password)=>r};
            let _ = tx
                .send(Completion {
                    job: j,
                    view,
                    result: Work::Connect(p, writable, result),
                })
                .await;
        });
        Ok(Some(job))
    }
    pub(super) async fn password(
        &mut self,
        ctx: &Value,
        p: Profile,
        writable: bool,
    ) -> Result<Option<String>> {
        if let Profile::Postgres { password_env, .. } = &p {
            if !password_env.is_empty() {
                let password = std::env::var(password_env)
                    .map_err(|_| "Password environment variable is unavailable")?;
                return self.connect(ctx, p, writable, password).await;
            }
            self.form(
                ctx,
                "PostgreSQL password (empty for socket/certificate auth)",
                vec![field("password", "Password", "secret", false)],
                Input::Password(p, writable),
            )
            .await?;
            Ok(None)
        } else {
            self.connect(ctx, p, writable, String::new()).await
        }
    }
    pub(super) async fn query_document(&mut self, ctx: &Value, name: String) -> Result<()> {
        self.ready(&name)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "Clock unavailable")?
            .as_nanos();
        let result=self.rpc.request("buffer.create",json!({"invocation":ctx["invocation"],"path":format!("db-query-{stamp}.sql"),"text":"SELECT 1;\n"})).await?;
        let generation = self.connections[&name].generation;
        self.buffers
            .insert(string(&result, "buffer")?, (name, generation));
        Ok(())
    }
    pub(super) async fn capture(&mut self, ctx: &Value, selection: bool) -> Result<Intent> {
        let buffer = string(ctx, "buffer")?;
        let (name, generation) = self
            .buffers
            .get(&buffer)
            .cloned()
            .ok_or("Associate this SQL buffer using ::db-use")?;
        self.ready(&name)?;
        if self.connections[&name].generation != generation {
            return Err("Connection changed; associate this buffer again with ::db-use".into());
        }
        let mut span = None;
        if selection {
            let value = self
                .rpc
                .request("selection.get", json!({"pane":ctx["pane"]}))
                .await?;
            if value["revision"] != ctx["selection_revision"] || value["buffer"] != ctx["buffer"] {
                return Err("Selection changed".into());
            }
            let spans = value["spans"].as_array().ok_or("Missing selection spans")?;
            if spans.len() != 1 {
                return Err("Select exactly one nonempty SQL range".into());
            }
            let from = spans[0]["from"].as_u64().ok_or("Invalid selection")?;
            let to = spans[0]["to"].as_u64().ok_or("Invalid selection")?;
            if from >= to {
                return Err("Select nonempty SQL".into());
            }
            span = Some((from, to));
        }
        let snapshot = self
            .rpc
            .request(
                "buffer.snapshot.open",
                json!({"buffer":buffer,"expected_revision":ctx["buffer_revision"]}),
            )
            .await?;
        let id = string(&snapshot, "snapshot")?;
        let result = async {
            let (from, to) = span.unwrap_or((
                0,
                snapshot["chars"]
                    .as_u64()
                    .ok_or("Missing snapshot length")?,
            ));
            if to.saturating_sub(from) > crate::query::MAX_SQL as u64 {
                return Err("SQL input limit exceeded".into());
            }
            let mut sql = String::new();
            let mut start = from;
            while start < to {
                let end = (start + 16384).min(to);
                let v = self
                    .rpc
                    .request(
                        "buffer.snapshot.read",
                        json!({"snapshot":id,"from":start,"to":end}),
                    )
                    .await?;
                sql.push_str(v["text"].as_str().ok_or("Missing snapshot text")?);
                if sql.len() > crate::query::MAX_SQL {
                    return Err("SQL input limit exceeded".into());
                }
                start = end;
            }
            let pg = self
                .saved
                .profiles
                .iter()
                .find(|p| p.name() == name)
                .is_some_and(Profile::postgres);
            crate::query::validate(&sql, pg)?;
            Ok(Intent {
                generation,
                name,
                sql,
                source: format!(
                    "buffer {} · {}",
                    buffer,
                    ctx["buffer_revision"].as_str().unwrap_or("")
                ),
            })
        }
        .await;
        let _ = self
            .rpc
            .request("buffer.snapshot.close", json!({"snapshot":id}))
            .await;
        result
    }
    pub(super) async fn execute(&mut self, ctx: &Value, intent: Intent) -> Result<Option<String>> {
        let db = self.ready(&intent.name)?;
        if self.connections[&intent.name].generation != intent.generation {
            return Err("Connection changed; capture SQL again".into());
        }
        let write = self.connections[&intent.name].writable;
        let view = self
            .create(
                ctx,
                Content::Result {
                    name: intent.name.clone(),
                    data: Arc::new(Data::default()),
                    table: None,
                    page: 0,
                    offset: 0,
                    columns: vec![],
                    record: None,
                    source: intent.source.clone(),
                },
            )
            .await?;
        if write {
            self.saved.uncertain.push(intent.name.clone());
            if let Err(error) = self.save().await {
                self.saved.uncertain.retain(|n| n != &intent.name);
                return Err(error);
            }
            let lease=match self.rpc.request("activity.acquire",json!({"title":format!("Database transaction: {}",intent.name),"duration_seconds":600})).await {
                Ok(lease)=>lease,
                Err(error)=>{self.saved.uncertain.retain(|n|n!=&intent.name);self.save().await?;return Err(error);}
            };
            let lease = string(&lease, "lease")?;
            self.rpc.track(&lease);
            self.connections.get_mut(&intent.name).unwrap().lease = Some(lease);
            self.connections.get_mut(&intent.name).unwrap().expiry =
                Some(tokio::time::Instant::now() + Duration::from_secs(590));
        }
        let (job, cancel) = match self.job("Execute SQL").await {
            Ok(job) => job,
            Err(error) => {
                if write {
                    self.release(&intent.name).await;
                    self.saved.uncertain.retain(|n| n != &intent.name);
                    self.save().await?;
                }
                return Err(error);
            }
        };
        let c = self.connections.get_mut(&intent.name).unwrap();
        c.busy = true;
        c.cancel = cancel.clone();
        c.job = Some(job.clone());
        let tx = self.sender.clone();
        let j = job.clone();
        let seconds = self.seconds;
        tokio::spawn(async move {
            let result = db.execute(intent.sql, write, cancel.clone(), seconds).await;
            let result = match result {
                Ok(data) if !cancel.is_cancelled() => {
                    Work::Data(intent.name, data, None, 0, intent.source)
                }
                Ok(_) => {
                    let settled = db.settle(false).await;
                    Work::Failed(
                        intent.name,
                        if settled.is_ok() {
                            "Query cancelled; transaction rolled back".into()
                        } else {
                            "Transaction outcome unknown after cancellation".into()
                        },
                        settled.is_err(),
                    )
                }
                Err(e) => {
                    let uncertain = e.starts_with("Transaction outcome unknown")
                        || e.starts_with("PostgreSQL connection failed")
                        || e.contains("worker stopped")
                        || e.contains("worker failed");
                    Work::Failed(intent.name, e, uncertain)
                }
            };
            let _ = tx
                .send(Completion {
                    job: j,
                    view,
                    result,
                })
                .await;
        });
        Ok(Some(job))
    }
    pub(super) async fn load(
        &mut self,
        ctx: &Value,
        name: String,
        table: Option<Table>,
        page: usize,
        schema: bool,
        system: bool,
    ) -> Result<Option<String>> {
        let db = self.ready(&name)?;
        let view = if let Some(id) = ctx["view"].as_str() {
            self.context_view(ctx)?;
            id.to_owned()
        } else {
            self.create(
                ctx,
                Content::Catalog {
                    name: name.clone(),
                    tables: vec![],
                    system,
                },
            )
            .await?
        };
        let (job, cancel) = self.job("Read database").await?;
        let c = self.connections.get_mut(&name).unwrap();
        c.busy = true;
        c.cancel = cancel.clone();
        c.job = Some(job.clone());
        let tx = self.sender.clone();
        let j = job.clone();
        tokio::spawn(async move {
            let result = if let Some(table) = table {
                let result = if schema {
                    db.schema(&table, cancel).await
                } else {
                    db.browse(&table, page, cancel).await
                };
                result.map(|d| {
                    Work::Data(
                        name.clone(),
                        d,
                        if schema { None } else { Some(table) },
                        if schema { 0 } else { page },
                        if schema {
                            "schema"
                        } else {
                            "browse: offset pages; external writes can change ordering"
                        }
                        .into(),
                    )
                })
            } else {
                db.catalog(system, cancel)
                    .await
                    .map(|t| Work::Catalog(name.clone(), t, system))
            };
            let _ = tx
                .send(Completion {
                    job: j,
                    view,
                    result: result.unwrap_or_else(|e| Work::Failed(name, e, false)),
                })
                .await;
        });
        Ok(Some(job))
    }
    pub(super) async fn settle(
        &mut self,
        ctx: &Value,
        name: String,
        commit: bool,
    ) -> Result<Option<String>> {
        let c = self.connections.get(&name).ok_or("Database disconnected")?;
        if c.busy || !c.pending {
            return Err("No pending transaction, or operation still running".into());
        }
        let db = c.db.clone();
        let view = ctx["view"].as_str().unwrap_or("").to_owned();
        let (job, _) = self
            .job(if commit {
                "Commit transaction"
            } else {
                "Roll back transaction"
            })
            .await?;
        self.connections.get_mut(&name).unwrap().busy = true;
        let tx = self.sender.clone();
        let j = job.clone();
        tokio::spawn(async move {
            let result = match db.settle(commit).await {
                Ok(()) => Work::Settled(name, commit),
                Err(e) => Work::Failed(name, e, true),
            };
            let _ = tx
                .send(Completion {
                    job: j,
                    view,
                    result,
                })
                .await;
        });
        Ok(Some(job))
    }
}
