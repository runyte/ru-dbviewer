// SPDX-License-Identifier: MPL-2.0
//! One native browser buffer; pages and their captured results stay in bounded memory.
use super::*;

pub(super) struct Browser {
    pub view: String,
    pub revision: String,
    pub page: String,
    published: tokio::time::Instant,
}

impl App {
    pub(super) fn resolve_browser_context(&self, ctx: &mut Value) {
        if let Some(browser) = &self.browser
            && ctx["view"] == browser.view
            && ctx["model_revision"] == browser.revision
        {
            ctx["view"] = json!(browser.page);
        }
    }

    pub(super) fn close_browser(&mut self, host_view: &str) {
        if self.browser.as_ref().is_some_and(|b| b.view == host_view) {
            self.browser = None;
            self.views.clear();
            for (_, cancel) in self.full_jobs.values() {
                cancel.cancel();
            }
        }
    }

    pub(super) fn prune_pages(&mut self, parent: Option<&str>) {
        let mut ancestors = Vec::new();
        let mut current = parent;
        while let Some(id) = current {
            if ancestors.len() == 12 || ancestors.contains(&id) {
                break;
            }
            ancestors.push(id);
            current = self.views.get(id).and_then(|v| v.parent.as_deref());
        }
        let retained = ancestors.into_iter().map(str::to_owned).collect::<Vec<_>>();
        self.views.retain(|id, _| retained.contains(id));
        for (id, (_, cancel)) in &self.full_jobs {
            if !self.views.contains_key(id) {
                cancel.cancel();
            }
        }
    }

    pub(super) async fn capture_page_position(&mut self, ctx: &Value) {
        if let Some(browser) = &self.browser
            && ctx["view"] == browser.page
            && ctx["pane"].is_string()
            && let Ok(selection) = self
                .rpc
                .request("selection.get", json!({"pane":ctx["pane"]}))
                .await
            && selection["ranges"].is_array()
            && let Some(page) = self.views.get_mut(&browser.page)
        {
            page.position = Some((page.revision.clone(), selection));
        }
    }

    pub(super) async fn show_page(&mut self, ctx: &Value, id: &str) -> Result<()> {
        // A staged publication may already have won at the host while its completion
        // is queued here. Cancel and drain it before using the shared host revision.
        // Transport cancellation remains independent of this foreground request.
        if let Some(page) = self.browser.as_ref().map(|b| b.page.clone())
            && let Some((_, cancel)) = self.full_jobs.get(&page)
        {
            cancel.cancel();
            while self.full_jobs.contains_key(&page) {
                let done = self.receiver.recv().await.ok_or("Display worker stopped")?;
                Box::pin(self.complete(done)).await?;
            }
        }
        self.capture_page_position(ctx).await;
        let page = self.views.get(id).ok_or("Page closed")?;
        let model = self.model_for_view(id, &page.content);
        // Offsets are reusable only while the saved page's exact model is unchanged.
        let position = page
            .position
            .as_ref()
            .filter(|(revision, _)| revision == &page.revision && page.model == model)
            .map(|(_, selection)| selection.clone());
        if let Some(browser) = &self.browser {
            let previous = browser.page.clone();
            // Validate foreground authority before replacing the shared buffer.
            self.rpc
                .request(
                    "pane.show",
                    json!({"invocation":ctx["invocation"],"view":browser.view}),
                )
                .await?;
            let revision = self.publish_browser_model(model.clone()).await?;
            let browser = self.browser.as_mut().unwrap();
            browser.page = id.to_owned();
            if previous != id
                && let Some((_, cancel)) = self.full_jobs.get(&previous)
            {
                cancel.cancel();
            }
            self.views.get_mut(id).unwrap().revision = revision;
        } else {
            let result = self
                .rpc
                .request("view.create", json!({"model":model}))
                .await?;
            let revision = string(&result, "revision")?;
            self.browser = Some(Browser {
                view: string(&result, "view")?,
                revision: revision.clone(),
                page: id.to_owned(),
                published: tokio::time::Instant::now(),
            });
            self.views.get_mut(id).unwrap().revision = revision;
            let view = self.browser.as_ref().unwrap().view.clone();
            if let Err(error) = self
                .rpc
                .request(
                    "pane.show",
                    json!({"invocation":ctx["invocation"],"view":view}),
                )
                .await
            {
                let _ = self.rpc.request("view.close", json!({"view":view})).await;
                self.close_browser(&view);
                return Err(error);
            }
        }
        self.views.get_mut(id).unwrap().model = model;
        if let Some(position) = position
            && ctx["pane"].is_string()
            && let Ok(current) = self
                .rpc
                .request("selection.get", json!({"pane":ctx["pane"]}))
                .await
            && current["buffer"] == position["buffer"]
        {
            // The buffer and live selection revision fence this to the invoking pane.
            let _ = self
                .rpc
                .request(
                    "selection.set",
                    json!({
                        "pane":ctx["pane"], "buffer":current["buffer"],
                        "expected_revision":current["revision"],
                        "ranges":position["ranges"], "primary":position["primary"]
                    }),
                )
                .await;
        }
        Ok(())
    }

    pub(super) async fn publish_page_model(&mut self, id: &str, model: Value) -> Result<String> {
        if self.browser.as_ref().is_some_and(|b| b.page == id) {
            self.publish_browser_model(model).await
        } else {
            // Background results update their captured page, never the page now displayed.
            self.page_serial += 1;
            Ok(format!("hidden:{}", self.page_serial))
        }
    }

    async fn publish_browser_model(&mut self, model: Value) -> Result<String> {
        let browser = self.browser.as_ref().ok_or("Browser closed")?;
        tokio::time::sleep_until(browser.published + Duration::from_millis(110)).await;
        let encoded = serde_json::to_string(&model).map_err(|_| "Cannot encode browser page")?;
        let revision = if encoded.len() > crate::protocol::FRAME - 1024 {
            let cancel = CancellationToken::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            let guard = crate::result_storage::WorkGuard::new(cancel.clone(), deadline);
            full_value::stage_document(
                &self.rpc,
                &browser.view,
                &browser.revision,
                encoded,
                &guard,
                &cancel,
                deadline,
            )
            .await?
        } else {
            let result = self
                .rpc
                .request(
                    "view.publish",
                    json!({
                        "view":browser.view, "expected_revision":browser.revision, "model":model
                    }),
                )
                .await?;
            string(&result, "revision")?
        };
        let browser = self.browser.as_mut().unwrap();
        browser.revision = revision.clone();
        browser.published = tokio::time::Instant::now();
        Ok(revision)
    }
}
