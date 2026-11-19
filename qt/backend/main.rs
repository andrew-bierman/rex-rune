//! Child process owned by the Qt frontend. Standard output is JSON lines only.
mod activity;
mod adapter;
mod files;
mod hosts;
mod keymap;
mod maintenance;
mod notification_events;
mod preferences;
mod programs;
mod remote;
#[cfg(test)]
mod remote_tests;
mod server;
mod terminal;
mod workspace;

use adapter::{Adapter, VERSION};
use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use hosts::{Discovery, Hosts, Visit};
use rex_rune::{
    connection::{Command, HostCommand, wait_for_hosts},
    settings::{ActivityView, Host, Settings, validate_host},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version, about = "Private backend for the Rex Qt Quick frontend")]
struct Options {
    #[arg(long)]
    endpoint: Option<String>,
    #[arg(long)]
    session: Option<String>,
    #[arg(long, default_value_os_t = config_path())]
    config: PathBuf,
}
fn config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".config")
        })
        .join("rex-qt/workspace.json")
}

struct SetupReply {
    review_endpoint: Option<String>,
    host: String,
    epoch: u64,
    request: u64,
    message: String,
}
struct Backend {
    maintenance: maintenance::Maintenance,
    server: server::Server,
    server_clock: Option<Instant>,
    confirmation: Value,
    setup_result: Option<SetupReply>,
    setup_rx: Option<std::sync::mpsc::Receiver<SetupReply>>,
    remote: remote::Requests,
    remote_checks: Vec<(String, rex_rune::connection::perform::Pending, Value)>,
    remote_navigation: Option<(String, rex_rune::connection::perform::Pending, u64, Visit)>,
    settings: Settings,
    activity: activity::Activity,
    program_notices: Vec<Value>,
    next_notice: u64,
    visible: bool,
    preferences: Value,
    path: PathBuf,
    hosts: Hosts,
    adapters: HashMap<String, Adapter>,
    generation: u64,
    discovery: Discovery,
    after_state: Vec<Value>,
    cursor_targets: HashMap<(String, String), (u64, rex_rune::terminal::CursorShape)>,
}
impl Backend {
    fn new(options: Options) -> Result<Self> {
        let existed = options.config.exists();
        let mut settings = Settings::load(&options.config)?;
        if let Some(endpoint) = options.endpoint {
            validate_host("Rex host", &endpoint)?;
            if !settings.hosts.iter().any(|host| host.endpoint == endpoint) {
                let host = Host {
                    label: "Rex host".into(),
                    endpoint: endpoint.clone(),
                    remote_control: false,
                    collapsed: false,
                    shell: None,
                };
                if existed {
                    settings.hosts.push(host);
                } else {
                    settings.hosts = vec![host];
                }
            } else if !existed {
                settings.hosts.retain(|host| host.endpoint == endpoint);
            }
            settings.last_host = Some(endpoint);
        }
        if let Some(session) = options.session {
            let endpoint = settings
                .last_host
                .clone()
                .or_else(|| settings.hosts.first().map(|h| h.endpoint.clone()))
                .context("add a host before selecting a session")?;
            settings.sessions.insert(endpoint, session);
        }
        let mut hosts = Hosts::new(&settings);
        hosts.connect_all(&settings);
        let mut backend = Self {
            maintenance: maintenance::Maintenance::default(),
            server: server::Server::default(),
            server_clock: None,
            confirmation: Value::Null,
            setup_result: None,
            setup_rx: None,
            remote: remote::Requests::default(),
            remote_navigation: None,
            remote_checks: Vec::new(),
            preferences: preferences::snapshot(&settings),
            settings,
            activity: activity::Activity::default(),
            program_notices: Vec::new(),
            next_notice: 0,
            visible: false,
            path: options.config,
            hosts,
            adapters: HashMap::new(),
            generation: 1,
            discovery: Discovery::default(),
            after_state: Vec::new(),
            cursor_targets: HashMap::new(),
        };
        backend.synchronize()?;
        Ok(backend)
    }
    fn retain_remote_hosts(&mut self) {
        let hosts: Vec<_> = self
            .hosts
            .slots
            .iter()
            .map(|slot| {
                (
                    slot.id.as_str(),
                    (slot.connection.is_some() && slot.model.connected)
                        .then_some(slot.model.connection_epoch),
                )
            })
            .collect();
        self.remote.retain_hosts(&hosts);
    }
    fn synchronize(&mut self) -> Result<()> {
        self.preferences = preferences::snapshot(&self.settings);
        self.retain_remote_hosts();
        let ids = self.hosts.ids();
        self.adapters.retain(|id, _| ids.contains(id));
        self.activity.retain_hosts(&ids);
        for (slot, config) in self.hosts.slots.iter().zip(&self.settings.hosts) {
            let added = !self.adapters.contains_key(&slot.id);
            let adapter = self.adapters.entry(slot.id.clone()).or_default();
            adapter.settings = self.settings.clone();
            adapter.settings_path = Some(self.path.clone());
            adapter.endpoint = config.endpoint.clone();
            if added || adapter.keymap_generation != self.generation {
                adapter.keymap_generation = self.generation;
                if let Some(connection) = &slot.connection {
                    adapter.declare_keymap(connection)?;
                    if added {
                        connection.commands.send(HostCommand::Identify)?;
                        connection
                            .commands
                            .send(HostCommand::Theme(preferences::declaration(
                                &self.settings,
                            )?))?;
                    }
                }
            }
        }
        Ok(())
    }
    fn epoch(&self, request: &Value) -> Result<usize> {
        let id = request["host"].as_str().context("missing host identity")?;
        let index = self.hosts.index(id)?;
        ensure!(
            request["epoch"].as_u64() == Some(self.hosts.slots[index].model.connection_epoch),
            "connection changed; request discarded"
        );
        Ok(index)
    }
    fn host_request(&mut self, request: &Value) -> Result<()> {
        let action = request["action"].as_str().context("missing host action")?;
        let args = &request["args"];
        let id = request["host"]
            .as_str()
            .context("missing host identity")?
            .to_owned();
        if matches!(
            action,
            "tab_step"
                | "tab_goto"
                | "history"
                | "cycle_tab"
                | "cycle_session"
                | "sidebar"
                | "discover"
        ) {
            self.hosts.require_active(&id)?;
        }
        if action != "discover_cancel" && !(action == "add" && self.hosts.slots.is_empty()) {
            self.epoch(request)?;
        }
        let old_active = self.hosts.active.clone();
        let text = |name: &str| {
            args[name]
                .as_str()
                .with_context(|| format!("missing {name}"))
        };
        match action {
            "add" => {
                let saved = self.hosts.add(
                    &mut self.settings,
                    &self.path,
                    text("label")?,
                    text("endpoint")?,
                    args["shell"].as_str().map(str::to_owned),
                )?;
                if self.hosts.active.is_empty() {
                    self.hosts
                        .activate(&mut self.settings, &self.path, &saved)?;
                }
                self.after_state
                    .push(json!({"type":"host_saved","version":VERSION,"host":saved}));
            }
            "edit" => {
                let saved = self.hosts.edit(
                    &mut self.settings,
                    &self.path,
                    &id,
                    text("label")?,
                    text("endpoint")?,
                    args["shell"].as_str().map(str::to_owned),
                )?;
                self.after_state
                    .push(json!({"type":"host_saved","version":VERSION,"host":saved}));
            }
            "remote_control" => {
                let index = self.epoch(request)?;
                let mut next = self.settings.clone();
                next.hosts[index].remote_control = args["enabled"]
                    .as_bool()
                    .context("missing remote consent")?;
                next.save(&self.path)?;
                self.settings = next;
            }
            "remove" => self.hosts.remove(&mut self.settings, &self.path, &id)?,
            "activate" => self.hosts.activate(&mut self.settings, &self.path, &id)?,
            "connect" => self.hosts.reconnect(&self.settings, &id)?,
            "disconnect" => self.hosts.disconnect(&id)?,
            "collapse" => self.hosts.set_collapsed(
                &mut self.settings,
                &self.path,
                &id,
                args["collapsed"]
                    .as_bool()
                    .context("missing collapsed state")?,
            )?,
            "sidebar" => self.hosts.set_sidebar(
                &mut self.settings,
                &self.path,
                args["open"].as_bool().context("missing sidebar state")?,
            )?,
            "navigate" => self.hosts.navigate(Visit {
                host: id.clone(),
                session: text("session")?.into(),
                window: args["window"]
                    .as_str()
                    .filter(|w| !w.is_empty())
                    .map(str::to_owned),
            })?,
            "history" => self.hosts.history_step(
                args["direction"]
                    .as_i64()
                    .context("missing history direction")? as isize,
            )?,
            "cycle_tab" | "cycle_session" => self.hosts.cycle(
                &self.settings,
                action == "cycle_tab",
                args["direction"]
                    .as_i64()
                    .context("missing navigation direction")? as isize,
            )?,
            "tab_step" | "tab_goto" => {
                let slot = self.hosts.active_slot().context("no active host")?;
                let view = slot.model.view().context("no selected session")?;
                let current = view
                    .windows
                    .iter()
                    .position(|w| w.window_id == view.active_window_id)
                    .unwrap_or(0);
                let next = if action == "tab_goto" {
                    let index = args["index"].as_i64().context("missing tab number")?;
                    if index == -1 {
                        view.windows
                            .len()
                            .checked_sub(1)
                            .context("no tabs available")?
                    } else {
                        ensure!(index > 0, "tab numbers start at one");
                        usize::try_from(index - 1)?
                    }
                } else {
                    ensure!(!view.windows.is_empty(), "no tabs available");
                    let direction = args["direction"]
                        .as_i64()
                        .context("missing tab direction")?;
                    ensure!(matches!(direction, -1 | 1), "invalid tab direction");
                    (current as i64 + direction).rem_euclid(view.windows.len() as i64) as usize
                };
                let window = view
                    .windows
                    .get(next)
                    .and_then(|w| w.window_id.clone())
                    .context("that tab is unavailable")?;
                self.hosts.navigate(Visit {
                    host: slot.id.clone(),
                    session: slot.model.selected.clone().context("no selected session")?,
                    window: Some(window),
                })?;
            }
            "discover" => self.discovery.start(),
            "discover_cancel" => self.discovery.cancel(),
            _ => bail!("unknown host action"),
        }
        if old_active != self.hosts.active {
            if let Some(adapter) = self.adapters.get_mut(&old_active) {
                adapter.clear_captures();
            }
            self.after_state.retain(|event| event["type"] != "action");
        }
        self.synchronize()?;
        if action == "connect"
            && let Some(slot) = self.hosts.slots.iter().find(|s| s.id == id)
            && let Some(connection) = &slot.connection
        {
            self.adapters[&id].declare_keymap(connection)?;
            connection.commands.send(HostCommand::Identify)?;
        }
        Ok(())
    }
    fn reload_shortcuts(&mut self) -> Result<()> {
        if self.settings.shortcut_file.reload(&self.path) {
            self.generation = self.generation.wrapping_add(1);
            self.synchronize()?;
        }
        Ok(())
    }
    fn server_request(&mut self, request: &Value) -> Result<()> {
        let index = self.epoch(request)?;
        let slot = &self.hosts.slots[index];
        self.hosts.require_active(&slot.id)?;
        let endpoint = &self.settings.hosts[index].endpoint;
        self.server.observe(&slot.id, &slot.model, endpoint);
        let op = request["op"].as_str().unwrap_or("");
        match op {
            "server_page_open" => {
                self.server_clock = Some(Instant::now() + Duration::from_secs(60));
            }
            "server_page_close" => {
                self.server_clock = None;
            }
            "server_read" => {
                slot.connection
                    .as_ref()
                    .context("host disconnected")?
                    .commands
                    .send(HostCommand::Identify)?;
            }
            "server_prepare_stop" | "server_prepare_setup" => {
                if op.ends_with("setup") {
                    ensure!(self.setup_rx.is_none(), "a setup attempt is still running");
                }
                self.confirmation = self.server.prepare(
                    if op.ends_with("setup") {
                        "setup"
                    } else {
                        "stop"
                    },
                    &slot.id,
                    &slot.model,
                    endpoint,
                    request["request_id"]
                        .as_u64()
                        .context("missing request id")?,
                )?;
            }
            "server_cancel" => {
                self.server.cancel();
                self.confirmation = Value::Null;
            }
            "server_confirm" => {
                let request_id = request["request_id"]
                    .as_u64()
                    .context("missing request id")?;
                let effect = self
                    .server
                    .confirm(&slot.id, &slot.model, endpoint, request_id);
                self.confirmation = Value::Null;
                match effect? {
                    server::Effect::Stop { epoch } => slot
                        .connection
                        .as_ref()
                        .context("host disconnected")?
                        .commands
                        .send(HostCommand::StopServer { epoch })?,
                    server::Effect::Setup { endpoint } => {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.setup_rx = Some(rx);
                        self.setup_result = None;
                        let host = slot.id.clone();
                        let epoch = slot.model.connection_epoch;
                        std::thread::Builder::new().name("qt-ssh-setup".into()).spawn(move || {
                            let result = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string()).and_then(|rt| rt.block_on(rex_rune::ssh::setup::start_personal(&endpoint)).map_err(|e| e.to_string()));
                            let (message, review_endpoint) = match result {
                                Ok(rex_rune::ssh::setup::Attachment::Automatic) => ("Personal server answered. Use Connect to attach without another start.".into(), None),
                                Ok(rex_rune::ssh::setup::Attachment::Review { reason, endpoint }) => (reason, endpoint),
                                Err(message) => (message, None),
                            };
                            let _ = tx.send(SetupReply { host, epoch, request: request_id, message, review_endpoint });
                        })?;
                    }
                }
            }
            "server_pair" => self.server.pair(&slot.id, &slot.model)?,
            "server_pair_close" => self.server.close_pair(),
            "server_copy" => {
                let text =
                    server::copy_address(request["text"].as_str().context("missing address")?)
                        .context("address is not sanitized")?;
                let snapshot = self.server.snapshot(&slot.id, &slot.model, endpoint);
                ensure!(
                    snapshot["pair"]["address"] == text
                        || snapshot["listeners"]
                            .as_array()
                            .is_some_and(|rows| rows.iter().any(|r| r["address"] == text)),
                    "address report changed"
                );
                self.after_state.push(json!({"type":"clipboard","scope":"server","version":VERSION,"host":slot.id,"epoch":slot.model.connection_epoch,"text":text}));
            }
            _ => bail!("unknown server operation"),
        }
        Ok(())
    }
    fn handle(&mut self, request: &Value) -> Result<bool> {
        ensure!(
            request["version"].as_u64() == Some(VERSION),
            "unsupported adapter protocol version"
        );
        let op = request["op"].as_str().context("missing op")?;
        if op == "maintenance" {
            self.maintenance.request(request, &self.path)?;
            return Ok(true);
        }
        if op.starts_with("server_") {
            self.server_request(request)?;
            return Ok(true);
        }
        if op == "shutdown" {
            return Ok(false);
        }
        if op == "host" {
            self.host_request(request)?;
            return Ok(true);
        }
        if op == "remote_context" {
            let Some(host) = request["host"].as_str() else {
                return Ok(true);
            };
            let Ok(index) = self.hosts.require_active(host) else {
                return Ok(true);
            };
            let epoch = self.hosts.slots[index].model.connection_epoch;
            if request["epoch"].as_u64() != Some(epoch) {
                return Ok(true);
            }
            self.remote.context.host = host.into();
            self.remote.context.epoch = epoch;
            self.remote.context.ready = true;
            self.remote.context.modal =
                request["modal"].as_bool().context("missing modal state")?;
            self.remote.context.field =
                request["field"].as_bool().context("missing field state")?;
            self.remote.context.sequence = request["sequence"]
                .as_bool()
                .context("missing sequence state")?;
            if request["keypress"] == true {
                self.remote.context.last_key = Some(Instant::now());
            }
            return Ok(true);
        }
        if op == "visibility" {
            let visible = request["visible"].as_bool().context("missing visibility")?;
            self.visible = visible;
            self.hosts.set_visible(visible);
            if visible {
                self.reload_shortcuts()?;
            }
            return Ok(true);
        }
        let id = request["host"]
            .as_str()
            .context("missing host identity")?
            .to_owned();
        if op == "remote_authorize" {
            self.authorize_remote(request)?;
            return Ok(true);
        }
        if op == "view_action" {
            let epoch = request["epoch"].as_u64().context("missing epoch")?;
            let index = self.hosts.require_active(&id)?;
            let model = &self.hosts.slots[index].model;
            let action = request["action"].as_str().context("missing action")?;
            ensure!(
                request["session"].as_str() == model.selected.as_deref()
                    && request["window"].as_str()
                        == model.window().and_then(|w| w.window_id.as_deref())
                    && (!action.starts_with("pane.")
                        || request["pane"].as_str() == model.focused()),
                "View changed; action discarded"
            );
            if let Some(visit) = self.view_action(&id, epoch, action, &request["args"])? {
                self.hosts.navigate(visit)?;
            }
            return Ok(true);
        }
        if op == "activity" {
            self.hosts.require_active(&id)?;
            self.epoch(request)?;
            match request["action"].as_str() {
                Some("refresh") => {}
                Some("view") => {
                    let view = match request["args"]["view"].as_str() {
                        Some("priority") => ActivityView::Priority,
                        Some("session") => ActivityView::Session,
                        _ => bail!("invalid activity view"),
                    };
                    if self.settings.activity_view != view {
                        let mut next = self.settings.clone();
                        next.activity_view = view;
                        next.save(&self.path)?;
                        self.settings = next;
                    }
                }
                _ => bail!("unknown activity action"),
            }
            return Ok(true);
        }
        if matches!(op, "refresh" | "keymap_reload" | "shortcut" | "preferences") {
            self.hosts.require_active(&id)?;
            self.epoch(request)?;
            match op {
                "preferences" => {
                    preferences::apply(&mut self.settings, &self.path, &request["patch"])?;
                    let theme = preferences::declaration(&self.settings)?;
                    for slot in &self.hosts.slots {
                        if let Some(connection) = &slot.connection {
                            connection
                                .commands
                                .send(HostCommand::Theme(theme.clone()))?;
                        }
                    }
                    self.synchronize()?;
                    self.after_state.push(json!({"type":"preferences_saved","version":VERSION,"request_id":request["request_id"]}));
                }
                "refresh" => {
                    self.reload_shortcuts()?;
                    self.hosts
                        .active_connection()?
                        .commands
                        .send(HostCommand::RefreshSessions)?;
                }
                "keymap_reload" => self.reload_shortcuts()?,
                "shortcut" => {
                    let mut next = self.settings.clone();
                    let slot = self.hosts.active_slot().context("no active host")?;
                    keymap::record(
                        &mut next,
                        request["action"].as_str().context("missing action")?,
                        serde_json::from_value(request["keys"].clone())
                            .context("invalid shortcut keys")?,
                        (!request["args"].is_null()).then(|| request["args"].clone()),
                        request["mode"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned),
                        request["operation"]
                            .as_str()
                            .context("missing shortcut operation")?,
                        slot.model
                            .keymap
                            .as_ref()
                            .map_or(&[], |map| map.reply.actions.as_slice()),
                    )?;
                    next.save(&self.path)?;
                    self.settings = next;
                    self.generation = self.generation.wrapping_add(1);
                    self.synchronize()?;
                }
                _ => unreachable!(),
            }
            return Ok(true);
        }
        let epoch = request["epoch"]
            .as_u64()
            .context("missing connection epoch")?;
        if matches!(op, "select_session" | "select_window") {
            self.hosts.require_active(&id)?;
            self.epoch(request)?;
            self.hosts.navigate(Visit {
                host: id,
                session: request["session"]
                    .as_str()
                    .context("missing session")?
                    .into(),
                window: (op == "select_window")
                    .then(|| request["window"].as_str().map(str::to_owned))
                    .flatten(),
            })?;
            return Ok(true);
        }
        let index = if op == "download.cancel" {
            self.epoch(request)?
        } else {
            self.hosts.require_target(&id, epoch)?
        };
        if op.starts_with("directory.") || op.starts_with("download.") || op == "files.upload" {
            let slot = &self.hosts.slots[index];
            let connection = slot
                .connection
                .as_ref()
                .context("host is disconnected; file action discarded")?;
            let adapter = self
                .adapters
                .get_mut(&id)
                .context("host adapter unavailable")?;
            let mut next = self.settings.clone();
            let changed = adapter.file_request(connection, &mut next, request)?;
            if changed {
                next.save(&self.path)?;
                self.settings = next;
                self.synchronize()?;
            }
            return Ok(true);
        }
        if op == "reconnect" && self.hosts.slots[index].connection.is_none() {
            let mut connect = request.clone();
            connect["action"] = json!("connect");
            connect["args"] = json!({});
            self.host_request(&connect)?;
            return Ok(true);
        }
        let adapter = self
            .adapters
            .get_mut(&id)
            .context("host adapter unavailable")?;
        if op == "workspace" {
            adapter.settings = self.settings.clone();
        }
        let slot = &self.hosts.slots[index];
        if let Some(connection) = &slot.connection {
            adapter.handle(connection, request)?;
        } else if op == "workspace" {
            ensure!(
                adapter.workspace_commands(&slot.model, request)?.is_empty(),
                "host is disconnected"
            );
        } else {
            bail!("host is disconnected; input paused");
        }
        if op == "workspace" {
            self.settings = adapter.settings.clone();
            self.synchronize()?;
        }
        Ok(true)
    }
    fn poll(&mut self) -> Result<(bool, Vec<Value>)> {
        let previous_host = self.hosts.active.clone();
        let mut dirty =
            self.hosts.poll(&mut self.settings) | self.discovery.poll() | self.maintenance.poll();
        if self.server_clock.is_some_and(|next| Instant::now() >= next) {
            self.server_clock = Some(Instant::now() + Duration::from_secs(60));
            dirty = true;
        }
        if let Some(reply) = self.setup_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.setup_result = Some(reply);
            self.setup_rx = None;
            dirty = true;
        }
        self.retain_remote_hosts();
        if previous_host != self.hosts.active {
            if let Some(adapter) = self.adapters.get_mut(&previous_host) {
                adapter.clear_captures();
            }
            self.after_state.retain(|event| event["type"] != "action");
        }
        let mut events = Vec::new();
        let mut declared = false;
        let mut cursor_targets = HashMap::new();
        let mut performs = Vec::new();
        for slot in &self.hosts.slots {
            let Some(connection) = &slot.connection else {
                continue;
            };
            let mut model = connection.model.lock().expect("host model");
            let adapter = self
                .adapters
                .get_mut(&slot.id)
                .context("host adapter unavailable")?;
            for notice in std::mem::take(&mut model.notices) {
                if model.connected {
                    self.next_notice = self.next_notice.wrapping_add(1);
                    self.program_notices.push(json!({"event":self.next_notice,"host":slot.id,"epoch":model.connection_epoch,"activityEpoch":model.activity_epoch,
                        "session":notice.session,"block":notice.block,"title":notice.title,"body":notice.body}));
                }
            }
            for command in adapter.reconcile_downloads(&model) {
                connection.commands.send(command)?;
            }
            for (session, block) in std::mem::take(&mut model.bells) {
                self.activity
                    .bell_for_block(&slot.id, &model, &session, &block);
            }
            declared |= model.theme_declared;
            let shape = self.settings.terminal.cursor_style.shape();
            for (pane, terminal) in &model.terminals {
                let key = (slot.id.clone(), pane.clone());
                let value = (model.connection_epoch, shape);
                if self.cursor_targets.get(&key) != Some(&value) {
                    terminal.commands.try_send(Command::CursorStyle(shape))?;
                }
                cursor_targets.insert(key, value);
            }
            performs.extend(
                std::mem::take(&mut model.performs)
                    .into_iter()
                    .map(|p| (slot.id.clone(), p)),
            );
            let active = slot.id == self.hosts.active;
            let actions = adapter.take_actions(&mut model);
            if active {
                self.after_state
                    .extend(actions.into_iter().map(|mut action| {
                        action["host"] = json!(slot.id);
                        action
                    }));
            }
            let clipboard = model.clipboard.take().filter(|copy| {
                active
                    && rex_rune::connection::clipboard::accepts(&model, &copy.session, &copy.block)
            });
            if let Some(copy) = clipboard {
                events.push(json!({"type":"clipboard","version":VERSION,"host":slot.id,"epoch":model.connection_epoch,"session":copy.session,"pane":copy.block,"text":copy.text}));
            }
            if let Some(message) = model.notice.take()
                && active
            {
                events.push(
                    json!({"type":"error","version":VERSION,"host":slot.id,"message":message}),
                );
            }
        }
        for (host, pending) in performs {
            self.remote_perform(&host, pending)?;
        }
        self.poll_remote_checks()?;
        self.poll_remote_navigation()?;
        self.cursor_targets = cursor_targets;
        if declared && !self.settings.terminal_theme_declared {
            self.settings.terminal_theme_declared = true;
            self.settings.save(&self.path)?;
        }
        Ok((
            dirty || !self.after_state.is_empty() || !self.program_notices.is_empty(),
            events,
        ))
    }
    fn remote_reply(
        &mut self,
        host: &str,
        pending: rex_rune::connection::perform::Pending,
        outcome: rex_rune::connection::perform::Outcome,
    ) -> Result<()> {
        let index = self.hosts.index(host)?;
        if self.hosts.slots[index].model.connection_epoch != pending.epoch {
            return Ok(());
        }
        if outcome == rex_rune::connection::perform::Outcome::Performed {
            self.after_state.push(json!({"type":"notice","version":VERSION,"host":host,"epoch":pending.epoch,
                "message":format!("{} performed {} on {}",pending.request.requester,pending.request.action,self.settings.hosts[index].label)}));
        }
        if let Some(connection) = &self.hosts.slots[index].connection {
            connection.commands.send(HostCommand::PerformReply {
                epoch: pending.epoch,
                call_id: pending.request.call_id,
                outcome,
            })?;
        }
        Ok(())
    }
    fn view_action(
        &mut self,
        host: &str,
        epoch: u64,
        action: &str,
        args: &Value,
    ) -> Result<Option<Visit>> {
        ensure!(
            matches!(keymap::remote(action), keymap::Remote::View { .. }),
            "This is not a view action"
        );
        keymap::validate(action, args)?;
        let index = if action == "session.select" {
            self.hosts.index(host)?
        } else {
            self.hosts.require_active(host)?
        };
        ensure!(
            self.hosts.slots[index].model.connection_epoch == epoch,
            "Connection changed; action discarded"
        );
        let model = self.hosts.slots[index].model.clone();
        let mut command = json!({"version":VERSION,"host":host,"epoch":epoch,
            "session":model.selected,"window":model.window().and_then(|w|w.window_id.as_ref()),"pane":model.focused(),"args":args});
        (|| match action {
            "session.select" => Ok(Some(Visit {
                host: host.into(),
                session: args["session_id"]
                    .as_str()
                    .context("missing session")?
                    .into(),
                window: args["window_id"].as_str().map(str::to_owned),
            })),
            "client.tab.next" | "client.tab.previous" | "client.tab.goto" => {
                let view = model.view().context("no session on screen")?;
                let current = view
                    .windows
                    .iter()
                    .position(|w| w.window_id == view.active_window_id)
                    .unwrap_or(0);
                ensure!(!view.windows.is_empty(), "no tabs available");
                let target = if action == "client.tab.goto" {
                    let n = args["index"].as_i64().context("invalid index")?;
                    if n == -1 {
                        view.windows.len() - 1
                    } else {
                        usize::try_from(n - 1)?
                    }
                } else {
                    (current as i64 + if action == "client.tab.next" { 1 } else { -1 })
                        .rem_euclid(view.windows.len() as i64) as usize
                };
                let window = view
                    .windows
                    .get(target)
                    .and_then(|w| w.window_id.clone())
                    .context("tab index out of range")?;
                Ok(Some(Visit {
                    host: host.into(),
                    session: model.selected.clone().context("no session")?,
                    window: Some(window),
                }))
            }
            "client.sidebar.toggle" => {
                command["op"] = json!("host");
                command["action"] = json!("sidebar");
                command["args"] = json!({"open":!self.settings.sidebar_open});
                self.host_request(&command)?;
                Ok(None)
            }
            "pane.focus" | "pane.zoom" | "pane.scroll" => {
                command["op"] = json!(if action == "pane.scroll" {
                    "scroll"
                } else {
                    "workspace"
                });
                command["action"] = json!(action);
                command["lines"] = args["lines"].clone();
                let connection = self.hosts.active_connection()?;
                self.adapters
                    .get_mut(host)
                    .context("missing adapter")?
                    .handle(connection, &command)?;
                Ok(None)
            }
            _ => bail!("remote action is unavailable"),
        })()
    }
    fn authorize_remote(&mut self, request: &Value) -> Result<()> {
        use rex_rune::connection::perform::Outcome;
        let Some(index) = self.remote_checks.iter().position(|(host, pending, _)| {
            request["host"] == *host
                && request["epoch"].as_u64() == Some(pending.epoch)
                && request["call_id"] == pending.request.call_id
        }) else {
            return Ok(());
        };
        let (host, pending, target) = self.remote_checks.remove(index);
        let Ok(index) = self.hosts.index(&host) else {
            return Ok(());
        };
        let model = &self.hosts.slots[index].model;
        if model.connection_epoch != pending.epoch {
            return Ok(());
        }
        let visible = &self.hosts.slots[self.hosts.index(&self.hosts.active)?].model;
        let unchanged = target["visible_host"] == self.hosts.active
            && target["visible_epoch"].as_u64() == Some(visible.connection_epoch)
            && target["visible_session"].as_str() == visible.selected.as_deref()
            && target["visible_window"].as_str()
                == visible.window().and_then(|w| w.window_id.as_deref())
            && target["visible_pane"].as_str() == visible.focused()
            && target["session"].as_str() == model.selected.as_deref()
            && target["window"].as_str() == model.window().and_then(|w| w.window_id.as_deref())
            && target["pane"].as_str() == model.focused();
        if request["accepted"] != true || !unchanged {
            return self.remote_reply(&host,pending,Outcome::error("refused","The native view changed or a panel, field or key sequence is active; nothing was done."));
        }
        self.remote_perform_stage(&host, pending, true)
    }
    fn poll_remote_checks(&mut self) -> Result<()> {
        use rex_rune::connection::perform::{MAX_WAIT, Outcome};
        for (host, pending, target) in std::mem::take(&mut self.remote_checks) {
            let Ok(index) = self.hosts.index(&host) else {
                continue;
            };
            let model = &self.hosts.slots[index].model;
            if model.connection_epoch != pending.epoch || !model.connected {
                continue;
            }
            if pending.received.elapsed() >= MAX_WAIT {
                self.remote_reply(
                    &host,
                    pending,
                    Outcome::error(
                        "internal",
                        "The native view did not authorize this request in time; nothing was done.",
                    ),
                )?;
            } else {
                self.remote_checks.push((host, pending, target));
            }
        }
        Ok(())
    }
    fn remote_perform(
        &mut self,
        host: &str,
        pending: rex_rune::connection::perform::Pending,
    ) -> Result<()> {
        self.remote_perform_stage(host, pending, false)
    }
    fn remote_perform_stage(
        &mut self,
        host: &str,
        pending: rex_rune::connection::perform::Pending,
        authorized: bool,
    ) -> Result<()> {
        use rex_rune::connection::perform::Outcome;
        let index = self.hosts.index(host)?;
        let slot = &self.hosts.slots[index];
        if !authorized && !self.remote.claim(host, &slot.model, &pending) {
            return Ok(());
        }
        let policy = self.remote.policy(
            &slot.model,
            &pending,
            self.settings.hosts[index].remote_control,
            host == self.hosts.active,
            self.visible
                && self.remote.context.host == self.hosts.active
                && self.hosts.index(&self.hosts.active).is_ok_and(|active| {
                    self.remote.context.epoch == self.hosts.slots[active].model.connection_epoch
                }),
            self.hosts.navigation_pending() || self.remote_navigation.is_some(),
        );
        if let Err(outcome) = policy {
            if self.remote.off_notice(
                host,
                &pending.request.action,
                self.settings.hosts[index].remote_control,
            ) {
                self.after_state.push(json!({"type":"notice","version":VERSION,"host":host,"epoch":pending.epoch,
                    "message":format!("{} asked for {} on {}. Remote control is off in Settings.",pending.request.requester,pending.request.action,self.settings.hosts[index].label)}));
            }
            return self.remote_reply(host, pending, outcome);
        }
        if !authorized {
            let visible = &self.hosts.slots[self.hosts.index(&self.hosts.active)?].model;
            let target = json!({"type":"remote_check","version":VERSION,"host":host,"epoch":pending.epoch,
                "visible_host":self.hosts.active,"visible_epoch":visible.connection_epoch,
                "visible_session":visible.selected,"visible_window":visible.window().and_then(|w|w.window_id.as_ref()),"visible_pane":visible.focused(),
                "call_id":pending.request.call_id,"action":pending.request.action,"session":slot.model.selected,"window":slot.model.window().and_then(|w|w.window_id.as_ref()),"pane":slot.model.focused()});
            self.after_state.push(target.clone());
            self.remote_checks.push((host.into(), pending, target));
            return Ok(());
        }
        let result = self.view_action(
            host,
            pending.epoch,
            &pending.request.action,
            &pending.request.args,
        );
        match result {
            Ok(Some(visit)) => match self.hosts.navigate_remote(
                visit.clone(),
                pending.received + rex_rune::connection::perform::MAX_WAIT,
            ) {
                Ok(id) => {
                    self.remote_navigation = Some((host.into(), pending, id, visit));
                    Ok(())
                }
                Err(error) => self.remote_reply(
                    host,
                    pending,
                    Outcome::error("not_found", error.to_string()),
                ),
            },
            Ok(None) => self.remote_reply(host, pending, Outcome::Performed),
            Err(error) => self.remote_reply(
                host,
                pending,
                Outcome::error("bad_request", error.to_string()),
            ),
        }
    }
    fn poll_remote_navigation(&mut self) -> Result<()> {
        use rex_rune::connection::perform::{MAX_WAIT, Outcome};
        let Some((host, pending, id, visit)) = self.remote_navigation.take() else {
            return Ok(());
        };
        let Ok(index) = self.hosts.index(&host) else {
            return Ok(());
        };
        let model = &self.hosts.slots[index].model;
        if model.connection_epoch != pending.epoch || !model.connected {
            return Ok(());
        }
        let outcome = if let Some((_, success)) = model.navigation_result.filter(|(n, _)| *n == id)
        {
            Some(
                if success
                    && self.hosts.active == host
                    && model.selected.as_deref() == Some(visit.session.as_str())
                    && visit.window.as_deref().is_none_or(|w| {
                        model.window().and_then(|v| v.window_id.as_deref()) == Some(w)
                    })
                {
                    Outcome::Performed
                } else {
                    Outcome::error("internal", "Navigation failed or was interrupted.")
                },
            )
        } else if pending.received.elapsed() >= MAX_WAIT || !self.hosts.navigation_pending() {
            Some(Outcome::error(
                "internal",
                "Navigation expired or was interrupted.",
            ))
        } else {
            None
        };
        if let Some(outcome) = outcome {
            self.remote_reply(&host, pending, outcome)
        } else {
            self.remote_navigation = Some((host, pending, id, visit));
            Ok(())
        }
    }
    fn state(&mut self) -> Value {
        let mut state = self.hosts.active_slot().and_then(|slot| self.adapters.get_mut(&slot.id).map(|adapter| adapter.state(&slot.model)))
            .unwrap_or_else(|| json!({"type":"state","version":VERSION,"connected":false,"status":"Add a host","epoch":0,"panes":[],"sessions":[],"windows":[],"keymap":keymap::state(&Default::default(),&self.settings,self.generation)}));
        state["host"] = json!(self.hosts.active);
        if state["workspace"].is_object() {
            state["workspace"]["host"] = json!(self.hosts.active);
        }
        if state["files"]["directory"].is_object() {
            state["files"]["directory"]["target"]["host"] = json!(self.hosts.active);
        }
        if state["files"]["download"].is_object() {
            state["files"]["download"]["target"]["host"] = json!(self.hosts.active);
        }
        let mut downloads = self
            .hosts
            .slots
            .iter()
            .zip(&self.settings.hosts)
            .flat_map(|(slot, config)| {
                let mut rows = files::download_rows(&slot.model);
                for row in &mut rows {
                    row["host"] = json!(slot.id);
                    row["label"] = json!(config.label);
                }
                rows
            })
            .collect::<Vec<_>>();
        downloads.sort_by_key(|row| {
            (
                row["running"] != true,
                std::cmp::Reverse(row["id"].as_u64().unwrap_or_default()),
            )
        });
        state["downloads"] = json!(downloads);
        let mut catalog = self.hosts.state(&self.settings);
        catalog["discovery"] = self.discovery.state();
        if catalog["pending"].as_bool() == Some(true) {
            if state["terminal"].is_object() {
                state["terminal"]["connected"] = json!(false);
            }
            if let Some(panes) = state["panes"].as_array_mut() {
                for pane in panes {
                    if pane["terminal"].is_object() {
                        pane["terminal"]["connected"] = json!(false);
                    }
                }
            }
        }
        let mut server_facts = Value::Null;
        if let Some(slot) = self.hosts.active_slot() {
            let index = self.hosts.index(&slot.id).unwrap();
            let endpoint = &self.settings.hosts[index].endpoint;
            self.server.observe(&slot.id, &slot.model, endpoint);
            if !self
                .server
                .confirmation_valid(self.confirmation["request_id"].as_u64().unwrap_or(0))
            {
                self.confirmation = Value::Null;
            }
            let mut facts = self.server.snapshot(&slot.id, &slot.model, endpoint);
            facts["confirmation"] = self.confirmation.clone();
            facts["setup_running"] = json!(self.setup_rx.is_some());
            if let Some(reply) = &self.setup_result
                && reply.host == slot.id
                && reply.epoch == slot.model.connection_epoch
            {
                facts["setup_result"] = json!({"request_id":reply.request,"message":reply.message,"endpoint":reply.review_endpoint});
            }
            server_facts = facts;
        }
        if let Some(actions) = state["keymap"]["actions"].as_array_mut() {
            for action in actions {
                match action["id"].as_str() {
                    Some("client.history.back") => {
                        action["enabled"] = json!(self.hosts.history.can_back())
                    }
                    Some("client.history.forward") => {
                        action["enabled"] = json!(self.hosts.history.can_forward())
                    }
                    Some("client.pair_device") => {
                        action["enabled"] = json!(server_facts["can_pair"] == true);
                    }
                    Some("client.server.stop") => {
                        action["enabled"] = json!(server_facts["can_stop"] == true);
                    }
                    Some("client.server.setup") => {
                        action["enabled"] = json!(
                            self.hosts
                                .active_slot()
                                .is_some_and(|s| s.model.ssh_server_missing && !s.model.connected)
                        );
                    }
                    Some("client.host.remove") => {
                        action["enabled"] = json!(self.hosts.slots.len() > 1)
                    }
                    _ => {}
                }
            }
        }
        if !server_facts.is_null() {
            state["server"] = server_facts;
        }
        state["preferences"] = self.preferences.clone();
        state["maintenance"] = self.maintenance.snapshot();
        state["hosts"] = catalog;
        self.augment_activity(&mut state);
        state
    }
    fn augment_activity(&mut self, state: &mut Value) {
        let mut rows = Vec::new();
        let mut sessions = Vec::new();
        let mut notification_sessions = Vec::new();
        let mut reading = false;
        let mut incomplete = false;
        let now = Instant::now();
        let pending = state["hosts"]["pending"].as_bool() == Some(true);
        for (slot, config) in self.hosts.slots.iter().zip(&self.settings.hosts) {
            let visible = (!pending && slot.id == self.hosts.active)
                .then(|| {
                    slot.model
                        .selected
                        .as_deref()
                        .zip(slot.model.window()?.window_id.as_deref())
                })
                .flatten();
            if slot.model.connected {
                for session in &slot.model.sessions {
                    notification_sessions.push(json!({"key":serde_json::to_string(&(slot.id.as_str(),session.session_id.as_str(),Option::<String>::None,slot.model.connection_epoch)).unwrap(),
                        "host":slot.id,"hostLabel":config.label,"epoch":slot.model.connection_epoch,"activityEpoch":slot.model.activity_epoch,
                        "session":session.session_id,"sessionLabel":session.label,"label":session.label,"window":null,
                        "seen":self.visible && slot.id==self.hosts.active && slot.model.selected.as_deref()==Some(session.session_id.as_str()),"muted":false}));
                }
            }
            let projection = self.activity.snapshot(
                &slot.id,
                &config.label,
                &slot.model,
                |session, window| {
                    self.settings.muted_tabs.iter().any(|m| {
                        m.endpoint == config.endpoint && m.session == session && m.window == window
                    })
                },
                activity::Presentation {
                    visible,
                    window_focused: self.visible,
                    generated_titles: self.settings.generated_titles,
                },
                now,
            );
            reading |= projection["reading"].as_bool().unwrap_or(false);
            incomplete |= projection["incomplete"].as_bool().unwrap_or(false);
            rows.extend(projection["rows"].as_array().into_iter().flatten().cloned());
            sessions.extend(
                projection["sessions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
        }
        let mut row_index = HashMap::new();
        for row in &rows {
            row_index
                .entry((
                    row["host"].clone(),
                    row["session"].clone(),
                    row["window"].clone(),
                ))
                .or_insert(row);
        }
        let mut session_index = HashMap::new();
        for session in &sessions {
            session_index
                .entry((session["host"].clone(), session["session"].clone()))
                .or_insert(session);
        }
        let active_host = state["host"].as_str().unwrap_or("").to_owned();
        let active_session = state["session"].as_str().unwrap_or("").to_owned();
        let active_window = state["window"].as_str().unwrap_or("").to_owned();
        if let Some(windows) = state["windows"].as_array_mut() {
            for window in windows {
                window["activity"] = row_index
                    .get(&(
                        json!(active_host),
                        json!(active_session),
                        window["id"].clone(),
                    ))
                    .copied()
                    .cloned()
                    .unwrap_or(Value::Null);
            }
        }
        if let Some(panes) = state["panes"].as_array_mut() {
            let current = row_index
                .get(&(
                    json!(active_host),
                    json!(active_session),
                    json!(active_window),
                ))
                .copied();
            for pane in panes {
                pane["activity"] = current
                    .and_then(|r| r["panes"].as_array())
                    .and_then(|p| p.iter().find(|p| p["pane"] == pane["id"]))
                    .cloned()
                    .unwrap_or(Value::Null);
            }
        }
        if let Some(hosts) = state["hosts"]["rows"].as_array_mut() {
            for host in hosts {
                let host_id = host["id"].clone();
                if let Some(items) = host["sessions"].as_array_mut() {
                    for session in items {
                        let session_id = session["id"].clone();
                        session["activity"] = session_index
                            .get(&(host_id.clone(), session_id.clone()))
                            .copied()
                            .cloned()
                            .unwrap_or(Value::Null);
                        if let Some(tabs) = session["tabs"].as_array_mut() {
                            for tab in tabs {
                                tab["activity"] = row_index
                                    .get(&(host_id.clone(), session_id.clone(), tab["id"].clone()))
                                    .copied()
                                    .cloned()
                                    .unwrap_or(Value::Null);
                            }
                        }
                    }
                }
            }
        }
        if let Some(items) = state["sessions"].as_array_mut() {
            for session in items {
                session["activity"] = session_index
                    .get(&(json!(active_host), session["id"].clone()))
                    .copied()
                    .cloned()
                    .unwrap_or(Value::Null);
            }
        }
        let notices = notification_events::project(
            &mut self.program_notices,
            &rows,
            &notification_sessions,
            |notice| {
                self.hosts.slots.iter().any(|slot| {
                    slot.id == notice["host"]
                        && slot.connection.is_some()
                        && notice["epoch"]
                            .as_u64()
                            .is_some_and(|epoch| slot.model.connection_epoch < epoch)
                })
            },
        );
        state["programNotices"] = json!(notices);
        state["notificationSessions"] = json!(notification_sessions);
        state["activity"] = json!({"rows":rows,"reading":reading,"incomplete":incomplete,"view":self.settings.activity_view});
    }
    fn after_state(&mut self) -> Vec<Value> {
        let slot = self.hosts.active_slot();
        self.after_state
            .drain(..)
            .filter(|event| {
                event["type"] != "action"
                    || slot.is_some_and(|slot| {
                        event["host"] == slot.id
                            && event["epoch"].as_u64() == Some(slot.model.connection_epoch)
                            && event["generation"].as_u64() == Some(self.generation)
                    })
            })
            .collect()
    }
}
fn main() -> Result<()> {
    let mut backend = Backend::new(Options::parse())?;
    std::thread::spawn(|| {
        rex_rune::download::store::Records::default().clean_up_leftovers();
    });
    let (send, receive) = flume::bounded(64);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut publish = |value: &Value| -> Result<()> {
        serde_json::to_writer(&mut output, value)?;
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    };
    publish(&json!({"type":"hello","version":VERSION}))?;
    let mut previous = Value::Null;
    let mut dirty = true;
    let mut next_frame = Instant::now();
    loop {
        let (changed, events) = backend.poll()?;
        dirty |= changed;
        for event in events {
            publish(&event)?;
        }
        match receive.recv_timeout(Duration::from_millis(33)) {
            Ok(line) => {
                dirty = true;
                let request: Result<Value> = line
                    .map_err(anyhow::Error::from)
                    .and_then(|line| Ok(serde_json::from_str(&line)?));
                let host_form = request.as_ref().is_ok_and(|request| {
                    request["op"] == "host"
                        && matches!(request["action"].as_str(), Some("add" | "edit"))
                });
                let preference_form = request
                    .as_ref()
                    .is_ok_and(|request| request["op"] == "preferences");
                let request_id = request
                    .as_ref()
                    .ok()
                    .map(|request| request["request_id"].clone());
                let result = request.and_then(|request| backend.handle(&request));
                match result {
                    Ok(false) => break,
                    Ok(true) => {}
                    Err(error) => publish(
                        &json!({"type":"error","version":VERSION,"message":error.to_string(),
                            "request_id":request_id,"scope":if host_form {"host_form"} else if preference_form {"preferences"} else {"application"}}),
                    )?,
                }
            }
            Err(flume::RecvTimeoutError::Disconnected) => break,
            Err(flume::RecvTimeoutError::Timeout) => {}
        }
        if dirty && Instant::now() >= next_frame {
            let next = backend.state();
            if next != previous {
                publish(&next)?;
                previous = next;
            }
            for event in backend.after_state() {
                publish(&event)?;
            }
            dirty = false;
            next_frame = Instant::now() + Duration::from_millis(33);
        }
    }
    backend.settings.save(&backend.path)?;
    drop(backend);
    wait_for_hosts(Duration::from_secs(3));
    Ok(())
}

#[cfg(test)]
mod tests;
