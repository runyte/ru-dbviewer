// SPDX-License-Identifier: MPL-2.0
use super::*;
impl App {
    pub(super) async fn release(&mut self, name: &str) {
        let lease = self.connections.get_mut(name).and_then(|c| {
            c.expiry = None;
            c.started = None;
            c.summary.clear();
            c.affected = None;
            c.lease.take()
        });
        if let Some(lease) = lease {
            self.rpc.untrack(&lease);
            let _ = self
                .rpc
                .request("activity.release", json!({"lease":lease}))
                .await;
        }
    }
    pub(super) async fn complete(&mut self, done: Completion) -> Result<()> {
        let done = match done {
            Completion {
                result: Work::Path(path),
                ..
            } => return self.complete_path(path).await,
            other => other,
        };
        let mut state = "succeeded";
        let mut failure = None;

        let changed = match &done.result {
            Work::Path(_) => unreachable!(),
            Work::Connect(p, ..) => p.name().to_owned(),
            Work::Catalog(n, ..)
            | Work::Data(n, ..)
            | Work::Settled(n, ..)
            | Work::Failed(n, ..) => n.clone(),
        };
        match done.result {
            Work::Path(_) => unreachable!(),
            Work::Connect(p, write, result) => {
                let was_cancelled = self
                    .connecting
                    .remove(p.name())
                    .is_some_and(|(_, t)| t.is_cancelled());
                let result = if was_cancelled {
                    Err("Connection cancelled".into())
                } else {
                    result
                };
                match result {
                    Ok(db) => {
                        let name = p.name().to_owned();
                        self.active = Some(name.clone());
                        self.generation += 1;
                        self.connections.insert(
                            name.clone(),
                            Connection {
                                generation: self.generation,
                                db: Arc::new(db),
                                writable: write,
                                busy: false,
                                pending: false,
                                job: None,
                                cancel: CancellationToken::new(),
                                lease: None,
                                expiry: None,
                                started: None,
                                summary: String::new(),
                                affected: None,
                            },
                        );
                        if let Some(v) = self.views.get_mut(&done.view) {
                            v.generation = Some(self.generation);
                        }
                        self.publish(
                            &done.view,
                            Content::Catalog {
                                name,
                                tables: vec![],
                                system: false,
                            },
                        )
                        .await?;
                        // Catalog loads are explicit background work without foreground acquisition.
                        if let Some(view) = self.views.get(&done.view) {
                            let ctx = json!({"view":done.view,"model_revision":view.revision});
                            let name = self.active.clone().unwrap();
                            let _ = self.load(&ctx, name, None, 0, false, false).await?;
                        }
                    }
                    Err(e) => {
                        state = "failed";
                        failure = Some(e);
                    }
                }
            }
            Work::Catalog(name, tables, system) => {
                if let Some(c) = self.connections.get_mut(&name) {
                    c.busy = false;
                    c.job = None;
                    if c.cancel.is_cancelled() {
                        state = "cancelled";
                    }
                }
                self.publish(
                    &done.view,
                    Content::Catalog {
                        name,
                        tables,
                        system,
                    },
                )
                .await?;
            }
            Work::Data(name, data, table, page, source) => {
                let cancelled = self
                    .connections
                    .get(&name)
                    .is_some_and(|c| c.cancel.is_cancelled());
                if cancelled {
                    let settled = if let Some(c) = self.connections.get(&name) {
                        c.db.settle(false).await
                    } else {
                        Err("Disconnected".into())
                    };
                    let write = self
                        .connections
                        .get(&name)
                        .is_some_and(|c| c.lease.is_some());
                    self.release(&name).await;
                    self.connections.remove(&name);
                    if write && settled.is_ok() {
                        self.saved.uncertain.retain(|n| n != &name);
                        self.save().await?;
                    }
                    state = if write && settled.is_err() {
                        "outcome_unknown"
                    } else {
                        "cancelled"
                    };
                } else if let Some(c) = self.connections.get_mut(&name) {
                    c.busy = false;
                    c.job = None;
                    c.pending = c.lease.is_some();
                    if c.pending {
                        c.affected = data.affected;
                        c.expiry = Some(
                            (tokio::time::Instant::now() + Duration::from_secs(300))
                                .min(c.expiry.unwrap_or(tokio::time::Instant::now())),
                        );
                    }
                }
                if self
                    .connections
                    .get(&name)
                    .is_some_and(|c| !c.db.usable() && !c.pending)
                {
                    self.connections.remove(&name);
                }
                if let Some(v) = self.views.get_mut(&done.view)
                    && table.is_some()
                    && v.browse.columns.is_empty()
                {
                    v.browse.columns = data.columns.clone();
                }
                if let Some(v) = self.views.get_mut(&done.view)
                    && table.is_some()
                {
                    v.browse.keys = data.order_keys.clone();
                }
                self.publish(
                    &done.view,
                    Content::Result {
                        name: name.clone(),
                        data: Arc::new(data),
                        table,
                        page,
                        offset: 0,
                        columns: self
                            .views
                            .get(&done.view)
                            .map(|v| v.browse.selected.clone())
                            .unwrap_or_default(),
                        record: None,
                        source,
                    },
                )
                .await?;
            }
            Work::Settled(name, commit) => {
                if let Some(c) = self.connections.get_mut(&name) {
                    c.pending = false;
                    c.busy = false;
                    c.job = None;
                }
                self.release(&name).await;
                self.saved.uncertain.retain(|n| n != &name);
                self.save().await?;
                let _ = commit;
                self.refresh_status(&name).await?;
            }
            Work::Failed(name, error, uncertain) => {
                let write = self
                    .connections
                    .get(&name)
                    .is_some_and(|c| c.lease.is_some());
                state = if uncertain && write {
                    "outcome_unknown"
                } else if self
                    .connections
                    .get(&name)
                    .is_some_and(|c| c.cancel.is_cancelled())
                {
                    "cancelled"
                } else {
                    "failed"
                };
                // Conservatively retire every failed connection. A dropped response or
                // interrupted operation is never followed by a query on uncertain state.
                self.release(&name).await;
                self.connections.remove(&name);
                if write && !uncertain {
                    self.saved.uncertain.retain(|n| n != &name);
                    self.save().await?;
                }
                failure = Some(error);
            }
        }
        self.refresh_status(&changed).await?;
        if let Some(error) = failure
            && let Some(view) = self.views.get(&done.view)
        {
            tokio::time::sleep_until(view.published + Duration::from_millis(110)).await;
            let mut model = self.model(&view.content);
            model["status"] = json!({"text":views::short(&error,1000),"role":"error"});
            let result = self
                .rpc
                .request(
                    "view.publish",
                    json!({"view":done.view,"expected_revision":view.revision,"model":model}),
                )
                .await;
            if let Ok(r) = result
                && let Some(v) = self.views.get_mut(&done.view)
            {
                v.revision = string(&r, "revision")?;
                v.published = tokio::time::Instant::now();
            }
        }
        let result = self
            .rpc
            .request("job.finish", json!({"job":done.job,"state":state}))
            .await;
        if let Err(e) = result {
            if e.contains("cancelled") {
                let _=self.rpc.request("job.finish",json!({"job":done.job,"state":if state=="outcome_unknown"{"outcome_unknown"}else{"cancelled"}})).await;
            } else {
                return Err(e);
            }
        }
        self.rpc.untrack(&done.job);
        Ok(())
    }
    pub(super) async fn refresh_status(&mut self, name: &str) -> Result<()> {
        let views = self
            .views
            .iter()
            .filter(|(_, v)| {
                v.content.name() == Some(name)
                    || matches!(
                        v.content,
                        Content::Connections | Content::Transactions { .. }
                    )
            })
            .map(|(id, v)| (id.clone(), v.content.clone()))
            .collect::<Vec<_>>();
        for (id, c) in views {
            let c = if matches!(c, Content::Transactions { .. }) {
                Content::Transactions {
                    entries: self.transaction_entries(),
                }
            } else {
                c
            };
            self.publish(&id, c).await?;
        }
        Ok(())
    }
    pub(super) async fn cancel_lease(&mut self, lease: &str) -> Result<()> {
        let name = self
            .connections
            .iter()
            .find(|(_, c)| c.lease.as_deref() == Some(lease))
            .map(|(n, _)| n.clone());
        if let Some(name) = name {
            let c = self.connections.get_mut(&name).unwrap();
            c.cancel.cancel();
            if c.busy {
                return Ok(());
            }
            let result = c.db.settle(false).await;
            self.release(&name).await;
            self.connections.remove(&name);
            if result.is_ok() {
                self.saved.uncertain.retain(|n| n != &name);
                self.save().await?;
            }
            self.refresh_status(&name).await?;
        }
        Ok(())
    }
    pub(super) async fn expire(&mut self) -> Result<()> {
        let names = self
            .connections
            .iter()
            .filter(|(_, c)| c.expiry.is_some_and(|t| t <= tokio::time::Instant::now()))
            .map(|(n, _)| n.clone())
            .collect::<Vec<_>>();
        for name in names {
            let c = self.connections.get(&name).unwrap();
            if c.busy {
                continue;
            }
            let result = c.db.settle(false).await;
            self.release(&name).await;
            self.connections.remove(&name);
            if result.is_ok() {
                self.saved.uncertain.retain(|n| n != &name);
                self.save().await?;
            }
            self.refresh_status(&name).await?;
        }
        Ok(())
    }
}
