// SPDX-License-Identifier: MPL-2.0
mod commands;
mod input;
mod lifecycle;
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
            "connect",
            "db-connect",
            "workspace",
            "Connect to a database",
        ),
        (
            "disconnect",
            "db-disconnect",
            "workspace",
            "Disconnect the selected database",
        ),
        ("query", "db-query", "workspace", "Create an SQL document"),
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
            "commit",
            "db-commit",
            "workspace",
            "Commit the pending transaction",
        ),
        (
            "rollback",
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
        ("next", "Next page"),
        ("previous", "Previous page"),
        ("columns", "Choose visible column ordinals"),
        ("back", "Return to rows or catalog"),
        (
            "acknowledge",
            "Acknowledge an uncertain previous operation after review",
        ),
    ] {
        commands.push(json!({"name":name,"context":"view","description":description,"primary":name=="activate"}));
    }
    for (name, description) in [
        ("connect", "Connect database"),
        ("query", "New SQL document"),
        ("disconnect", "Disconnect database"),
        ("commit", "Commit transaction"),
        ("rollback", "Roll back transaction"),
    ] {
        commands.push(
            json!({"name":format!("view-{name}"),"context":"view","description":description}),
        );
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
    Postgres,
    Password(Profile, bool),
    Use(String),
    Mode(String, bool),
    Run(Intent),
    Columns(String),
    Acknowledge(String),
    Disconnect(String),
}
enum Work {
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
    generation: u64,
    input: HashMap<String, Input>,
    connecting: HashMap<String, (String, CancellationToken)>,
    active: Option<String>,
    sender: mpsc::Sender<Completion>,
    receiver: mpsc::Receiver<Completion>,
    seconds: u64,
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
        rpc.send(json!({"type":"register","version":"runyte-1","name":"Database viewer","runyte":HOST_RANGE,"commands":commands(),"required_capabilities":CAPABILITIES,"optional_capabilities":[],"required_features":[],"optional_features":[]}))?;
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
            generation: 0,
            input: HashMap::new(),
            connecting: HashMap::new(),
            active: None,
            sender: tx,
            receiver: rx,
            seconds,
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
              if message["type"]=="event" {if message["event"]=="view.closed" {app.views.remove(message["data"]["view"].as_str().unwrap_or(""));}else if message["event"]=="activity.cancel_requested" {app.cancel_lease(message["data"]["lease"].as_str().unwrap_or("")).await?;}continue;}
              if message["type"]!="request"{continue;}
              let id=string(&message,"id")?;let mut ctx=message["params"].clone();ctx["invocation"]=json!(id);
              let result=if message["method"]=="ui.submit"{app.submit(ctx).await}else if message["method"]=="command.invoke"{app.command(ctx).await}else{Err("Unsupported host method".into())};
              rpc.reply(&id,result)?;
             }
             done=app.receiver.recv()=>{if let Some(done)=done {app.complete(done).await?;}}
             _=tokio::time::sleep_until(deadline),if expiry.is_some()=>{app.expire().await?;}
            }
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
    fn model(&self, content: &Content) -> Value {
        if matches!(content, Content::Connections) {
            views::text_model(
                "Databases",
                "Enter: connect/browse · Tab: actions",
                self.saved
                    .profiles
                    .iter()
                    .enumerate()
                    .map(|(i, p)| views::row(i, format!("{} · {}", p.name(), self.state(p.name()))))
                    .collect(),
                &["activate", "connect", "acknowledge"],
            )
        } else {
            content.model(&self.state(content.name().unwrap_or("")))
        }
    }
    async fn create(&mut self, ctx: &Value, content: Content) -> Result<String> {
        if self.views.len() >= 12 {
            return Err("Close a database view before opening another".into());
        }
        let result = self
            .rpc
            .request("view.create", json!({"model":self.model(&content)}))
            .await?;
        let id = string(&result, "view")?;
        self.views.insert(
            id.clone(),
            View {
                published: tokio::time::Instant::now(),
                revision: string(&result, "revision")?,
                content,
            },
        );
        self.rpc
            .request(
                "pane.show",
                json!({"invocation":ctx["invocation"],"view":id}),
            )
            .await?;
        Ok(id)
    }
    async fn publish(&mut self, id: &str, content: Content) -> Result<()> {
        let Some(view) = self.views.get(id) else {
            return Ok(());
        };
        tokio::time::sleep_until(view.published + Duration::from_millis(110)).await;
        let result = self
            .rpc
            .request(
                "view.publish",
                json!({"view":id,"expected_revision":view.revision,"model":self.model(&content)}),
            )
            .await;
        match result {
            Ok(v) => {
                if let Some(view) = self.views.get_mut(id) {
                    view.revision = string(&v, "revision")?;
                    view.published = tokio::time::Instant::now();
                    view.content = content;
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
            return Ok(name.0.clone());
        }
        if let Some(v) = ctx["view"].as_str().and_then(|id| self.views.get(id))
            && let Some(name) = v.content.name()
        {
            return Ok(name.to_owned());
        }
        self.active.clone().ok_or("Choose a database first".into())
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
