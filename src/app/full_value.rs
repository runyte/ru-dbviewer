// SPDX-License-Identifier: MPL-2.0
use super::*;
use crate::full_value::Document;

pub(super) struct Finished {
    pub expected: String,
    pub title: String,
    pub previous: Content,
    pub content: Content,
    pub result: Result<String>,
}

impl App {
    pub(super) async fn show_full_value(
        &mut self,
        ctx: &Value,
        toggle: bool,
    ) -> Result<Option<String>> {
        if !self.document_views {
            return Err("This Runyte host does not support full-value documents".into());
        }
        if !self.full_jobs.is_empty() {
            return Err("A full value is already loading; cancel it or wait for completion".into());
        }
        let (source, content) = self.context_view(ctx)?;
        let (name, data, row, column, raw) = match &content {
            Content::Result {
                name,
                data,
                record: Some(row),
                ..
            } if !toggle => (
                name.clone(),
                data.clone(),
                *row,
                Self::selected(ctx)?,
                false,
            ),
            Content::Value {
                name,
                data,
                row,
                column,
                ..
            } if !toggle => (name.clone(), data.clone(), *row, *column, false),
            Content::FullValue {
                name,
                data,
                row,
                column,
                raw,
                document: Some(_),
                ..
            } if toggle => (name.clone(), data.clone(), *row, *column, !*raw),
            _ => return Err("Select one field or open a value preview first".into()),
        };
        let cell = data
            .rows
            .get(row)
            .and_then(|r| r.get(column))
            .ok_or("Field is unavailable")?
            .clone();
        if !cell.full_available() {
            return Err(cell
                .unavailable_reason()
                .unwrap_or("Complete value unavailable")
                .into());
        }
        let permit = self
            .full_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Full-value worker is still stopping")?;
        let title = match &content {
            Content::FullValue { title, .. } => title.clone(),
            Content::Value { .. } => {
                let title = self.views[&source].model["title"]
                    .as_str()
                    .unwrap_or("[value]");
                if cell
                    .text
                    .as_ref()
                    .is_some_and(|text| text.trim_start().starts_with(['{', '[']))
                {
                    title.strip_suffix(" · JSON").unwrap_or(title).to_owned()
                } else {
                    title.to_owned()
                }
            }
            _ => {
                let location = self.views[&source].model["title"]
                    .as_str()
                    .unwrap_or("")
                    .strip_prefix("[record] ")
                    .unwrap_or(&name);
                let field = data
                    .columns
                    .get(column)
                    .map_or("(unnamed)", |c| c.name.as_str());
                format!("[value] {location} › {field}")
            }
        };
        let parent = if matches!(content, Content::Value { .. }) {
            self.views[&source].parent.clone()
        } else {
            Some(source.clone())
        };
        let previous = if toggle {
            content
        } else {
            Content::Value {
                name: name.clone(),
                data: data.clone(),
                row,
                column,
                raw: false,
                collapsed: Default::default(),
            }
        };
        let loading = Content::FullValue {
            title: title.clone(),
            name,
            data,
            row,
            column,
            raw,
            document: None,
        };
        let response = self
            .rpc
            .request(
                "job.create",
                json!({"title":"Load full value", "deadline_seconds":60}),
            )
            .await?;
        let job = string(&response, "job")?;
        let cancel = self.rpc.track(&job);
        let view = if toggle {
            source
        } else {
            match self.create(ctx, loading.clone()).await {
                Ok(view) => view,
                Err(error) => {
                    let _ = self.rpc.request("job.finish", json!({"job":job,"state":"failed","message":views::short(&crate::results::escape(&error),1024)})).await;
                    self.rpc.untrack(&job);
                    return Err(error);
                }
            }
        };
        if !toggle {
            self.views.get_mut(&view).unwrap().parent = parent;
        }
        let expected = self.views[&view].revision.clone();
        self.full_jobs
            .insert(view.clone(), (job.clone(), cancel.clone()));
        let rpc = self.rpc.clone();
        let tx = self.sender.clone();
        let j = job.clone();
        let presentation = self.action_presentation;
        let metadata = self.view_metadata;
        tokio::spawn(async move {
            // The slot stays owned through the blocking read, even after cancellation.
            let _permit = permit;
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            let guard = crate::result_storage::WorkGuard::new(cancel.clone(), deadline);
            let worker_guard = guard.clone();
            let loaded =
                tokio::task::spawn_blocking(move || -> Result<(Arc<Document>, String, String)> {
                    let document = Arc::new(Document::load(&cell, raw, &worker_guard)?);
                    let model = document.model(&title, presentation, metadata);
                    let encoded = serde_json::to_string(&model)
                        .map_err(|_| "Cannot encode complete value")?;
                    if encoded.len() > crate::full_value::MAX_MODEL {
                        return Err(
                            "Complete value exceeds the 16 MiB encoded document limit".into()
                        );
                    }
                    worker_guard.check()?;
                    let final_title = model["title"].as_str().unwrap_or("[value]").to_owned();
                    Ok((document, encoded, final_title))
                })
                .await
                .map_err(|_| "Full-value worker stopped".to_owned())
                .and_then(|r| r);
            let (content, title, result) = match loaded {
                Ok((document, encoded, title)) => {
                    let result =
                        stage_document(&rpc, &view, &expected, encoded, &guard, &cancel, deadline)
                            .await;
                    let mut content = loading;
                    if let Content::FullValue {
                        document: value, ..
                    } = &mut content
                    {
                        *value = Some(document);
                    }
                    (content, title, result)
                }
                Err(error) => (loading, String::new(), Err(error)),
            };
            let _ = tx
                .send(Completion {
                    job: j,
                    view,
                    result: Work::Full(Finished {
                        expected,
                        title,
                        previous,
                        content,
                        result,
                    }),
                })
                .await;
        });
        Ok(Some(job))
    }

    pub(super) async fn complete_full_value(
        &mut self,
        job: String,
        id: String,
        done: Finished,
    ) -> Result<()> {
        let owned = self
            .full_jobs
            .get(&id)
            .is_some_and(|(current, _)| current == &job);
        let cancelled = owned
            && self
                .full_jobs
                .remove(&id)
                .is_some_and(|(_, cancel)| cancel.is_cancelled());
        let current = owned
            && self
                .views
                .get(&id)
                .is_some_and(|view| view.revision == done.expected);
        let mut failure = None;
        let mut state = if cancelled { "cancelled" } else { "succeeded" };
        match done.result {
            Ok(revision) => {
                if current && let Some(view) = self.views.get_mut(&id) {
                    view.content = done.content;
                    view.revision = revision;
                    view.published = tokio::time::Instant::now();
                    // Full documents are managed by the staged path; do not retain
                    // a second JSON value copy or republish them on database status changes.
                    view.model = json!({"title":done.title});
                }
            }
            Err(error) => {
                failure = Some(views::short(&crate::results::escape(&error), 1024));
                if !cancelled {
                    state = "failed";
                }
                if current
                    && let Some(view) = self.views.get(&id)
                    && view.revision == done.expected
                    && matches!(view.content, Content::FullValue { document: None, .. })
                {
                    let mut model = self.model(&done.previous);
                    model["title"] = view.model["title"].clone();
                    model["status"] = json!({"text":views::short(&crate::results::escape(&error),1000), "role":if cancelled {"muted"} else {"error"}});
                    tokio::time::sleep_until(view.published + Duration::from_millis(110)).await;
                    if let Ok(result) = self
                        .rpc
                        .request(
                            "view.publish",
                            json!({"view":id,"expected_revision":done.expected,"model":model}),
                        )
                        .await
                        && let Some(view) = self.views.get_mut(&id)
                    {
                        view.revision = string(&result, "revision")?;
                        view.content = done.previous;
                        view.model = model;
                        view.published = tokio::time::Instant::now();
                    }
                }
            }
        }
        let mut finish = json!({"job":job,"state":state});
        if let Some(message) = failure {
            finish["message"] = json!(message);
        }
        if let Err(error) = self.rpc.request("job.finish", finish).await
            && error.contains("cancelled")
        {
            let _ = self
                .rpc
                .request("job.finish", json!({"job":job,"state":"cancelled"}))
                .await;
        }
        self.rpc.untrack(&job);
        Ok(())
    }
}

async fn stage_document(
    rpc: &Rpc,
    view: &str,
    expected: &str,
    encoded: String,
    guard: &crate::result_storage::WorkGuard,
    cancel: &CancellationToken,
    deadline: std::time::Instant,
) -> Result<String> {
    guard.check()?;
    let response = rpc
        .request(
            "view.stage.open",
            json!({"view":view,"expected_revision":expected,"kind":"model","bytes":encoded.len()}),
        )
        .await?;
    let stage = string(&response, "stage")?;
    let result = async {
        let mut offset = 0;
        while offset < encoded.len() {
            guard.check()?;
            let mut end = (offset + 128 * 1024).min(encoded.len());
            while !encoded.is_char_boundary(end) {
                end -= 1;
            }
            rpc.request(
                "view.stage.write",
                json!({"stage":stage,"offset":offset,"text":&encoded[offset..end]}),
            )
            .await?;
            offset = end;
        }
        guard.check()?;
        let commit = rpc.request("view.stage.commit", json!({"stage":stage}));
        tokio::pin!(commit);
        let result = tokio::select! {
            biased;
            result = &mut commit => result,
            _ = cancel.cancelled() => {
                let _ = rpc.request("view.stage.close", json!({"stage":stage})).await;
                commit.await
            },
            _ = tokio::time::sleep_until(deadline.into()) => {
                let _ = rpc.request("view.stage.close", json!({"stage":stage})).await;
                commit.await
            },
        }?;
        // A successful commit is the host's atomic winner. Closing the stage
        // first cancels preparation; a commit already installed remains valid.
        string(&result, "revision")
    }
    .await;
    // Commit consumes the stage; close is idempotent and also handles every failure.
    let _ = rpc
        .request("view.stage.close", json!({"stage":stage}))
        .await;
    result
}
