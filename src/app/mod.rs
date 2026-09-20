// SPDX-License-Identifier: MPL-2.0
mod browsing;
mod commands;
mod full_value;
mod input;
mod lifecycle;
mod presentation;
mod work;
use crate::{
    CAPABILITIES, HOST_RANGE, Result,
    db::{Database, Table},
    profiles::{Profile, Saved},
    protocol::Rpc,
    results::Data,
    views::{self, Content, View},
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub fn commands() -> Value {
    let mut commands = Vec::new();
    for (name, alias, context, description) in [
        ("open", "db", "workspace", "Open database connections"),
        (
            "return",
            "db-return",
            "buffer",
            "Return to the source browsing view",
        ),
        (
            "global-transactions",
            "db-transactions",
            "workspace",
            "Inspect pending transactions",
        ),
        (
            "global-connect",
            "db-connect",
            "workspace",
            "Connect to a database",
        ),
        (
            "global-disconnect",
            "db-disconnect",
            "workspace",
            "Disconnect the selected database",
        ),
        (
            "global-query",
            "db-query",
            "workspace",
            "Create an SQL document",
        ),
        (
            "use",
            "db-use",
            "buffer",
            "Associate this buffer with a database",
        ),
        (
            "run",
            "db-run",
            "buffer",
            "Execute the entire captured SQL buffer",
        ),
        (
            "run-selection",
            "db-run-selection",
            "buffer",
            "Execute one captured selection",
        ),
        (
            "cancel",
            "db-cancel",
            "workspace",
            "Cancel the selected database operation",
        ),
        (
            "global-commit",
            "db-commit",
            "workspace",
            "Commit the pending transaction",
        ),
        (
            "global-rollback",
            "db-rollback",
            "workspace",
            "Roll back the pending transaction",
        ),
    ] {
        commands
            .push(json!({"name":name,"alias":alias,"context":context,"description":description}));
    }
    for (name, description) in [
        ("activate", "Open selected item or inspect value"),
        ("schema", "Inspect table columns, keys and indexes"),
        ("refresh", "Refresh database data"),
        ("system", "Toggle system schemas"),
        ("mode", "Change read-only/writable mode"),
        (
            "profile-actions",
            "Actions for the selected database profile",
        ),
        ("next", "Next page"),
        ("previous", "Previous page"),
        ("columns", "Choose visible columns"),
        ("filters", "Edit row filters"),
        ("add-filter", "Add filter"),
        ("edit-filter", "Edit selected filter"),
        ("remove-filter", "Remove selected filter"),
        ("toggle-filter", "Enable or disable selected filter"),
        ("clear-filters", "Clear filters"),
        ("match", "Choose Match ALL or ANY"),
        ("apply-filters", "Apply filters and return to page one"),
        ("sort", "Sort rows"),
        ("page-size", "Set browse page size"),
        ("browse-sql", "Open current browse as SQL"),
        ("back", "Return to parent"),
        ("transactions", "Inspect pending transactions"),
        ("raw", "Toggle raw and formatted JSON"),
        (
            "acknowledge",
            "Acknowledge an uncertain previous operation after review",
        ),
    ] {
        commands.push(json!({"name":name,"context":"view","description":description,"primary":name=="activate"}));
    }
    for (name, description) in [
        ("connect", "Connect the selected saved profile"),
        ("connect-new", "Connect to a new database"),
        ("query", "New SQL document"),
        ("disconnect", "Disconnect database"),
        ("commit", "Commit transaction"),
        ("rollback", "Roll back transaction"),
    ] {
        commands.push(json!({"name":name,"context":"view","description":description}));
    }
    json!(commands)
}
struct Connection {
    generation: u64,
    db: Arc<Database>,
    writable: bool,
    busy: bool,
    pending: bool,
    job: Option<String>,
    cancel: CancellationToken,
    lease: Option<String>,
    expiry: Option<tokio::time::Instant>,
    started: Option<tokio::time::Instant>,
    summary: String,
    affected: Option<u64>,
}
#[derive(Clone)]
struct Intent {
    generation: u64,
    name: String,
    sql: String,
    source: String,
}
enum Input {
    Backend,
    Sqlite,
    SqlitePath {
        name: String,
        choices: Vec<(String, String)>,
    },
    Postgres,
    Password(Profile, bool),
    Use(String),
    ProfileActions {
        source: String,
        name: String,
        generation: Option<u64>,
        choices: Vec<String>,
    },
    ModeChoice(String, u64),
    Mode(String, u64, bool),
    Run(Intent),
    ColumnPick {
        view: String,
        selected: Vec<usize>,
        search: String,
        start: usize,
        choices: Vec<(String, usize)>,
    },
    ColumnSearch {
        view: String,
        selected: Vec<usize>,
    },
    FieldChoice {
        view: String,
        index: Option<usize>,
        sorting: bool,
        search: String,
        start: usize,
        choices: Vec<(String, usize)>,
    },
    FieldSearch {
        view: String,
        index: Option<usize>,
        sorting: bool,
    },
    FilterOp {
        view: String,
        index: Option<usize>,
        column: usize,
    },
    FilterValue {
        view: String,
        index: Option<usize>,
        column: usize,
        op: crate::browse::Operator,
    },
    Match(String),
    SortDirection(String, usize),
    PageSize(String),
    Acknowledge(String),
    Disconnect(String, u64),
}
struct PathCompletion {
    ctx: Value,
    name: String,
    result: Result<crate::paths::Destination>,
    epoch: u64,
}
enum Work {
    Full(full_value::Finished),
    Path(PathCompletion),
    Connect(Profile, bool, Result<Database>),
    Catalog(String, Vec<Table>, bool),
    Data(String, Data, Option<Table>, usize, String),
    Settled(String, bool),
    Failed(String, String, bool),
}
struct Completion {
    job: String,
    view: String,
    result: Work,
}
pub struct App {
    rpc: Rpc,
    saved: Saved,
    state_revision: String,
    connections: HashMap<String, Connection>,
    views: HashMap<String, View>,
    buffers: HashMap<String, (String, u64)>,
    sources: HashMap<String, String>,
    query_labels: HashMap<String, String>,
    query_serial: u64,
    generation: u64,
    input: HashMap<String, Input>,
    input_epoch: u64,
    input_source: Option<String>,
    path_pending: bool,
    path_slots: Arc<tokio::sync::Semaphore>,
    connecting: HashMap<String, (String, CancellationToken)>,
    active: Option<String>,
    sender: mpsc::Sender<Completion>,
    receiver: mpsc::Receiver<Completion>,
    seconds: u64,
    row_actions: bool,
    action_presentation: bool,
    view_metadata: bool,
    path_completion: bool,
    document_views: bool,
    full_slots: Arc<tokio::sync::Semaphore>,
    full_jobs: HashMap<String, (String, CancellationToken)>,
}
fn string(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Missing {key}"))
}
fn field(id: &str, label: &str, kind: &str, required: bool) -> Value {
    json!({"id":id,"label":label,"kind":kind,"required":required,"maximum_length":4096})
}
impl App {
    pub async fn run(rpc: Rpc, mut input: mpsc::Receiver<Value>) -> Result<()> {
        let hello = tokio::time::timeout(Duration::from_secs(8), input.recv())
            .await
            .map_err(|_| "Host hello timed out")?
            .ok_or("Missing host hello")?;
        let version = hello["host_version"].as_str().unwrap_or("");
        if hello["type"] != "hello" || hello["version"] != "runyte-1" || !supported(version) {
            return Err("Runyte >=0.3.0, <0.4.0 is required".into());
        }
        let supported = hello["features"]
            .as_array()
            .ok_or("Invalid host features")?;
        let known = [
            "view-row-actions",
            "view-action-presentation",
            "view-metadata",
            "view-document",
            "job-feedback",
            "input-path-completion",
        ];
        let requested = known
            .iter()
            .filter(|name| supported.iter().any(|value| value == **name))
            .copied()
            .collect::<Vec<_>>();
        let presentation = requested.contains(&"view-action-presentation");
        let document = requested.contains(&"view-document") && requested.contains(&"job-feedback");
        rpc.send(json!({"type":"register","version":"runyte-1","name":"Database viewer","runyte":HOST_RANGE,"commands":presentation::commands(presentation, document),"required_capabilities":CAPABILITIES,"optional_capabilities":[],"required_features":[],"optional_features":requested}))?;
        let registered = tokio::time::timeout(Duration::from_secs(8), input.recv())
            .await
            .map_err(|_| "Registration timed out")?
            .ok_or("Registration failed")?;
        if registered["type"] != "registered"
            || !CAPABILITIES.iter().all(|c| {
                registered["capabilities"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == c))
            })
            || registered["limits"]["line_bytes"].as_u64() != Some(crate::protocol::FRAME as u64)
        {
            return Err("Plugin registration refused".into());
        }
        let features = registered["features"]
            .as_array()
            .ok_or("Invalid negotiated features")?;
        if features.len() > known.len()
            || features.iter().enumerate().any(|(i, f)| {
                !f.as_str().is_some_and(|f| requested.contains(&f)) || features[..i].contains(f)
            })
            || (presentation && !features.iter().any(|f| f == "view-action-presentation"))
        {
            return Err("Invalid negotiated features".into());
        }
        let row_actions = features.iter().any(|f| f == "view-row-actions");
        let action_presentation = features.iter().any(|f| f == "view-action-presentation");
        let view_metadata = features.iter().any(|f| f == "view-metadata");
        let path_completion = features.iter().any(|f| f == "input-path-completion");
        let document_views = features.iter().any(|f| f == "view-document")
            && features.iter().any(|f| f == "job-feedback");
        let saved = rpc.request("state.get", json!({})).await?;
        let document = &saved["document"];
        let state: Saved = if document.is_null() {
            Saved::default()
        } else {
            if document["version"] != 1 {
                return Err("Unsupported saved profile version".into());
            }
            serde_json::from_value(document["data"].clone())
                .map_err(|_| "Invalid saved profiles")?
        };
        state.validate()?;
        let settings = rpc.request("settings.get", json!({})).await?;
        let seconds = settings["settings"]["query_timeout_seconds"]
            .as_u64()
            .unwrap_or(30)
            .clamp(1, 300);
        let (tx, rx) = mpsc::channel(4);
        let mut app = Self {
            rpc: rpc.clone(),
            saved: state,
            state_revision: string(&saved, "revision")?,
            connections: HashMap::new(),
            views: HashMap::new(),
            buffers: HashMap::new(),
            sources: HashMap::new(),
            query_labels: HashMap::new(),
            query_serial: 0,
            generation: 0,
            input: HashMap::new(),
            input_epoch: 0,
            input_source: None,
            path_pending: false,
            path_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            connecting: HashMap::new(),
            active: None,
            sender: tx,
            receiver: rx,
            seconds,
            row_actions,
            action_presentation,
            view_metadata,
            path_completion,
            document_views,
            full_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            full_jobs: HashMap::new(),
        };
        loop {
            if rpc.closed() {
                break;
            }
            let expiry = app
                .connections
                .values()
                .filter(|c| !c.busy)
                .filter_map(|c| c.expiry)
                .min();
            let deadline =
                expiry.unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(86400));
            tokio::select! {
             message=input.recv()=>{
              let Some(message)=message else{break;};
              if message["type"]=="event" {if message["event"]=="view.closed" {let id=message["data"]["view"].as_str().unwrap_or("");app.views.remove(id);if let Some((_,cancel))=app.full_jobs.get(id){cancel.cancel();}}else if message["event"]=="activity.cancel_requested" {app.cancel_lease(message["data"]["lease"].as_str().unwrap_or("")).await?;}continue;}
              if message["type"]!="request"{continue;}
              if message["method"]=="ui.validate" { app.validate_form(&message); continue; }
              let id=string(&message,"id")?;let mut ctx=message["params"].clone();ctx["invocation"]=json!(id);
              if message["method"]=="ui.submit" {
                app.restore_input_source(&mut ctx);
                match app.defer_path(&ctx) {Ok(true)=>continue,Ok(false)=>{},Err(e)=>{rpc.reply(&id,Err(e))?;continue;}}
              }
              let result=if message["method"]=="ui.submit"{app.submit(ctx).await}else if message["method"]=="command.invoke"{app.command(ctx).await}else{Err("Unsupported host method".into())};
              rpc.reply(&id,result)?;
             }
             done=app.receiver.recv()=>{if let Some(done)=done {app.complete(done).await?;}}
             _=tokio::time::sleep_until(deadline),if expiry.is_some()=>{app.expire().await?;}
            }
        }
        for (_, cancel) in app.full_jobs.values() {
            cancel.cancel();
        }
        for c in app.connections.values() {
            c.cancel.cancel();
        }
        for c in app.connections.values() {
            if !c.busy {
                let _ = tokio::time::timeout(Duration::from_secs(1), c.db.settle(false)).await;
            }
        }
        rpc.stop();
        Ok(())
    }
    async fn save(&mut self) -> Result<()> {
        let result=self.rpc.request("state.set",json!({"expected_revision":self.state_revision,"document":{"version":1,"data":self.saved}})).await?;
        self.state_revision = string(&result, "revision")?;
        Ok(())
    }
    fn state(&self, name: &str) -> String {
        if let Some(c) = self.connections.get(name) {
            format!(
                "{} · {}",
                if c.writable { "WRITABLE" } else { "READ ONLY" },
                if c.busy {
                    "running"
                } else if c.pending {
                    "PENDING COMMIT — commit or roll back within 5 minutes"
                } else {
                    "ready"
                }
            )
        } else if self.saved.uncertain.iter().any(|n| n == name) {
            "OUTCOME UNKNOWN — review database before acknowledgement".into()
        } else {
            "disconnected".into()
        }
    }
    fn profile_choices(&self, name: &str) -> Vec<String> {
        let connection = self.connections.get(name);
        if self.saved.uncertain.iter().any(|n| n == name)
            && !connection.is_some_and(|c| c.pending || c.busy)
        {
            return vec!["Acknowledge uncertain outcome".into()];
        }
        let mut choices: Vec<String> = if connection.is_some() {
            ["query", "mode", "disconnect", "transactions"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        } else {
            vec!["connect".into()]
        };
        if connection.is_some_and(|c| c.pending) {
            choices.extend(["commit".into(), "rollback".into()]);
        }
        choices
    }
    fn base_model(&self, content: &Content, page_size: usize) -> Value {
        let mut model = match content {
            Content::Connections => views::text_model(
                "Databases",
                "Enter: connect/browse · Tab: actions",
                self.saved
                    .profiles
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let mut row =
                            views::row(i, format!("{} · {}", p.name(), self.state(p.name())));
                        if self.row_actions {
                            let choices = self.profile_choices(p.name());
                            let mut actions = vec!["connect-new".to_owned(), "transactions".into()];
                            if !choices.iter().any(|c| c == "Acknowledge uncertain outcome") {
                                actions.push("activate".into());
                            }
                            for choice in choices {
                                let action = if choice == "Acknowledge uncertain outcome" {
                                    "acknowledge".into()
                                } else {
                                    choice
                                };
                                if !actions.contains(&action) {
                                    actions.push(action);
                                }
                            }
                            row["actions"] = json!(actions);
                        }
                        row
                    })
                    .collect(),
                if self.row_actions {
                    &["connect-new", "transactions"]
                } else {
                    &["activate", "connect-new", "profile-actions", "transactions"]
                },
            ),
            Content::Filters { draft, .. } => views::text_model(
                "Row filters",
                &draft.summary(),
                draft
                    .filters
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        views::row(
                            i,
                            format!(
                                "[{}] {} {} {}",
                                if f.enabled { "x" } else { " " },
                                draft.columns.get(f.column).map_or("?", |c| c.name.as_str()),
                                f.op.name(),
                                f.value
                            ),
                        )
                    })
                    .collect(),
                &[
                    "add-filter",
                    "edit-filter",
                    "remove-filter",
                    "toggle-filter",
                    "clear-filters",
                    "match",
                    "apply-filters",
                    "back",
                ],
            ),
            Content::Transactions { entries } => views::text_model(
                "Transactions",
                "Select a transaction · Tab commit / rollback / disconnect · age at refresh",
                entries
                    .iter()
                    .enumerate()
                    .map(|(i, (name, generation))| {
                        let detail = self
                            .connections
                            .get(name)
                            .filter(|c| c.generation == *generation && c.pending)
                            .map(|c| {
                                format!(
                                    "{} · age {}s · {} affected · {}",
                                    self.state(name),
                                    c.started.map_or(0, |t| t.elapsed().as_secs()),
                                    c.affected.map_or("unknown".into(), |n| n.to_string()),
                                    c.summary
                                )
                            })
                            .unwrap_or_else(|| "settled or disconnected".into());
                        views::row(i, format!("{name} · {detail}"))
                    })
                    .collect(),
                &["commit", "rollback", "disconnect", "refresh", "back"],
            ),
            _ => content.model(&self.state(content.name().unwrap_or("")), page_size),
        };
        let name = content.name();
        let live = name.and_then(|n| self.connections.get(n));
        if let Some(actions) = model["actions"].as_array_mut() {
            actions.retain(|a| match a.as_str().unwrap_or("") {
                "disconnect"|"query"|"mode" if matches!(content,Content::Connections)=>!self.connections.is_empty(),
                "disconnect"|"commit"|"rollback" if matches!(content,Content::Transactions{entries} if entries.is_empty())=>false,
                "acknowledge" => name.map_or_else(
                    || {
                        self.saved
                            .uncertain
                            .iter()
                            .any(|n| !self.connections.get(n).is_some_and(|c| c.pending || c.busy))
                    },
                    |n| {
                        self.saved.uncertain.iter().any(|x| x == n)
                            && !live.is_some_and(|c| c.pending || c.busy)
                    },
                ),
                "disconnect" | "mode" | "query" if name.is_some() => live.is_some(),
                "commit" | "rollback" if name.is_some() => live.is_some_and(|c| c.pending),
                _ => true,
            });
        }
        model
    }
    fn model_for_view(&self, id: &str, content: &Content) -> Value {
        let view = &self.views[id];
        let parent = view.parent.as_ref().and_then(|id| self.views.get(id));
        let mut model = self.model_context(content, parent, Some(&view.browse));
        if matches!(content, Content::Value { .. }) && parent.is_none() {
            model["title"] = view.model["title"].clone();
        }
        if let Some(name) = content.name()
            && let Some(generation) = self.views[id].generation
            && self
                .connections
                .get(name)
                .is_some_and(|c| c.generation != generation)
        {
            model["status"] = json!({"text":"Connection changed · retained data; return to Databases and reopen this connection","role":"warning"});
            model["actions"]
                .as_array_mut()
                .unwrap()
                .retain(|a| matches!(a.as_str(), Some("back" | "activate" | "raw" | "show-full")));
        }
        model
    }
    async fn create(&mut self, ctx: &Value, content: Content) -> Result<String> {
        if self.views.len() >= 12 {
            return Err("Close a database view before opening another".into());
        }
        let parent = ctx["view"].as_str().and_then(|id| self.views.get(id));
        let model = self.model_context(&content, parent, parent.map(|v| &v.browse));
        let inherited_browse = if matches!(
            content,
            Content::Result {
                record: Some(_),
                ..
            } | Content::Value { .. }
                | Content::FullValue { .. }
                | Content::Filters { .. }
        ) {
            parent.map(|v| v.browse.clone()).unwrap_or_default()
        } else {
            Default::default()
        };
        let result = self
            .rpc
            .request("view.create", json!({"model":model}))
            .await?;
        let id = string(&result, "view")?;
        let connected_content = content.name().is_some();
        self.views.insert(
            id.clone(),
            View {
                published: tokio::time::Instant::now(),
                revision: string(&result, "revision")?,
                model,
                content,
                parent: ctx["view"].as_str().map(str::to_owned),
                browse: inherited_browse,
                generation: connected_content
                    .then(|| {
                        ctx["view"]
                            .as_str()
                            .and_then(|id| self.views.get(id))
                            .and_then(|v| v.generation)
                            .or_else(|| {
                                ctx["buffer"]
                                    .as_str()
                                    .and_then(|id| self.buffers.get(id))
                                    .map(|(_, g)| *g)
                            })
                            .or_else(|| {
                                self.target(ctx)
                                    .ok()
                                    .and_then(|n| self.connections.get(&n).map(|c| c.generation))
                            })
                    })
                    .flatten(),
            },
        );
        if let Err(error) = self
            .rpc
            .request(
                "pane.show",
                json!({"invocation":ctx["invocation"],"view":id}),
            )
            .await
        {
            self.views.remove(&id);
            let _ = self.rpc.request("view.close", json!({"view":id})).await;
            return Err(error);
        }
        Ok(id)
    }
    async fn publish(&mut self, id: &str, content: Content) -> Result<()> {
        let Some(view) = self.views.get(id) else {
            return Ok(());
        };
        let model = self.model_for_view(id, &content);
        if model == view.model {
            if let Some(v) = self.views.get_mut(id) {
                v.content = content;
            }
            return Ok(());
        }
        tokio::time::sleep_until(view.published + Duration::from_millis(110)).await;
        let result = self
            .rpc
            .request(
                "view.publish",
                json!({"view":id,"expected_revision":view.revision,"model":model}),
            )
            .await;
        match result {
            Ok(v) => {
                if let Some(view) = self.views.get_mut(id) {
                    view.revision = string(&v, "revision")?;
                    view.published = tokio::time::Instant::now();
                    view.content = content;
                    view.model = model;
                }
                Ok(())
            }
            Err(e)
                if matches!(
                    e.as_str(),
                    "Host refused operation: not_found"
                        | "Host refused operation: closed"
                        | "Host refused operation: cancelled"
                ) =>
            {
                self.views.remove(id);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
    fn context_view(&self, ctx: &Value) -> Result<(String, Content)> {
        let id = string(ctx, "view")?;
        let v = self.views.get(&id).ok_or("View is closed")?;
        if ctx["model_revision"] != v.revision {
            return Err("View changed; invoke the action again".into());
        }
        Ok((id, v.content.clone()))
    }
    fn target(&self, ctx: &Value) -> Result<String> {
        if let Some(name) = ctx["buffer"].as_str().and_then(|b| self.buffers.get(b)) {
            self.check_generation(&name.0, name.1)?;
            return Ok(name.0.clone());
        }
        if ctx["view"].is_string() {
            let (_, content) = self.context_view(ctx)?;
            match content {
                Content::Connections => {
                    return self
                        .saved
                        .profiles
                        .get(Self::selected(ctx)?)
                        .map(|p| p.name().to_owned())
                        .ok_or("Select a database profile".into());
                }
                Content::Transactions { entries } => {
                    let (name, generation) = entries
                        .get(Self::selected(ctx)?)
                        .ok_or("Select a pending transaction")?;
                    self.check_generation(name, *generation)?;
                    return Ok(name.clone());
                }
                _ => {
                    if let Some(name) = content.name() {
                        if let Some(generation) = ctx["view"]
                            .as_str()
                            .and_then(|id| self.views.get(id))
                            .and_then(|v| v.generation)
                        {
                            self.check_generation(name, generation)?;
                        }
                        return Ok(name.to_owned());
                    }
                }
            }
        }
        self.active.clone().ok_or("Choose a database first".into())
    }
    fn check_generation(&self, name: &str, generation: u64) -> Result<()> {
        if self
            .connections
            .get(name)
            .is_some_and(|c| c.generation == generation)
        {
            Ok(())
        } else {
            Err("Connection changed or disconnected; invoke the action again".into())
        }
    }
    async fn back(&mut self, ctx: &Value, parent: Option<String>) -> Result<()> {
        if let Some(id) = parent.filter(|id| self.views.contains_key(id)) {
            self.rpc
                .request(
                    "pane.show",
                    json!({"invocation":ctx["invocation"],"view":id}),
                )
                .await?;
        } else {
            self.create(ctx, Content::Connections).await?;
        }
        Ok(())
    }
    fn transaction_entries(&self) -> Vec<(String, u64)> {
        let mut entries = self
            .connections
            .iter()
            .filter(|(_, c)| c.pending)
            .map(|(n, c)| (n.clone(), c.generation))
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }
    fn selected(ctx: &Value) -> Result<usize> {
        let rows = ctx["rows"].as_array().ok_or("Select one row")?;
        if rows.len() != 1 {
            return Err("Select exactly one row".into());
        }
        rows[0]
            .as_str()
            .ok_or("Invalid row")?
            .parse()
            .map_err(|_| "Invalid row".into())
    }
    fn ready(&self, name: &str) -> Result<Arc<Database>> {
        if self.connecting.contains_key(name) {
            return Err("Connection attempt in progress".into());
        }
        if self.saved.uncertain.iter().any(|n| n == name)
            && !self.connections.get(name).is_some_and(|c| c.pending)
        {
            return Err("Review and acknowledge the previous uncertain operation first".into());
        }
        let c = self
            .connections
            .get(name)
            .ok_or("Database is disconnected")?;
        if c.busy || c.pending {
            return Err("Database is busy or awaiting Commit/Rollback".into());
        }
        Ok(c.db.clone())
    }
    async fn job(&self, title: &str) -> Result<(String, CancellationToken)> {
        let v = self
            .rpc
            .request(
                "job.create",
                json!({"title":views::short(title,150),"deadline_seconds":self.seconds+15}),
            )
            .await?;
        let id = string(&v, "job")?;
        let token = self.rpc.track(&id);
        Ok((id, token))
    }
}
fn supported(version: &str) -> bool {
    if version.len() > 256 {
        return false;
    }
    let (core, build) = version
        .split_once('+')
        .map_or((version, None), |(c, b)| (c, Some(b)));
    if build.is_some_and(|b| {
        b.split('.')
            .any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
    }) {
        return false;
    }
    let Some(patch) = core.strip_prefix("0.3.") else {
        return false;
    };
    !patch.is_empty()
        && (patch == "0" || !patch.starts_with('0'))
        && patch.bytes().all(|b| b.is_ascii_digit())
        && patch.parse::<u64>().is_ok()
}
