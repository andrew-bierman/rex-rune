//! Child process owned by the Qt frontend. Standard output is JSON lines only.
mod adapter;
mod hosts;
mod keymap;
mod preferences;
mod terminal;
mod workspace;

use adapter::{Adapter, VERSION};
use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use hosts::{Discovery, Hosts, Visit};
use rex_rune::{
    connection::{Command, HostCommand, wait_for_hosts},
    settings::{Host, Settings, validate_host},
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

struct Backend {
    settings: Settings,
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
            preferences: preferences::snapshot(&settings),
            settings,
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
    fn synchronize(&mut self) -> Result<()> {
        self.preferences = preferences::snapshot(&self.settings);
        let ids = self.hosts.ids();
        self.adapters.retain(|id, _| ids.contains(id));
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
    fn handle(&mut self, request: &Value) -> Result<bool> {
        ensure!(
            request["version"].as_u64() == Some(VERSION),
            "unsupported adapter protocol version"
        );
        let op = request["op"].as_str().context("missing op")?;
        if op == "shutdown" {
            return Ok(false);
        }
        if op == "host" {
            self.host_request(request)?;
            return Ok(true);
        }
        if op == "visibility" {
            let visible = request["visible"].as_bool().context("missing visibility")?;
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
        let index = self.hosts.require_target(&id, epoch)?;
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
        let dirty = self.hosts.poll(&mut self.settings) | self.discovery.poll();
        if previous_host != self.hosts.active {
            if let Some(adapter) = self.adapters.get_mut(&previous_host) {
                adapter.clear_captures();
            }
            self.after_state.retain(|event| event["type"] != "action");
        }
        let mut events = Vec::new();
        let mut declared = false;
        let mut cursor_targets = HashMap::new();
        for slot in &self.hosts.slots {
            let Some(connection) = &slot.connection else {
                continue;
            };
            let mut model = connection.model.lock().expect("host model");
            let adapter = self
                .adapters
                .get(&slot.id)
                .context("host adapter unavailable")?;
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
        self.cursor_targets = cursor_targets;
        if declared && !self.settings.terminal_theme_declared {
            self.settings.terminal_theme_declared = true;
            self.settings.save(&self.path)?;
        }
        Ok((dirty || !self.after_state.is_empty(), events))
    }
    fn state(&mut self) -> Value {
        let mut state = self.hosts.active_slot().and_then(|slot| self.adapters.get_mut(&slot.id).map(|adapter| adapter.state(&slot.model)))
            .unwrap_or_else(|| json!({"type":"state","version":VERSION,"connected":false,"status":"Add a host","epoch":0,"panes":[],"sessions":[],"windows":[],"keymap":keymap::state(&Default::default(),&self.settings,self.generation)}));
        state["host"] = json!(self.hosts.active);
        if state["workspace"].is_object() {
            state["workspace"]["host"] = json!(self.hosts.active);
        }
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
        if let Some(actions) = state["keymap"]["actions"].as_array_mut() {
            for action in actions {
                match action["id"].as_str() {
                    Some("client.history.back") => {
                        action["enabled"] = json!(self.hosts.history.can_back())
                    }
                    Some("client.history.forward") => {
                        action["enabled"] = json!(self.hosts.history.can_forward())
                    }
                    Some("client.host.remove") => {
                        action["enabled"] = json!(self.hosts.slots.len() > 1)
                    }
                    _ => {}
                }
            }
        }
        state["preferences"] = self.preferences.clone();
        state["hosts"] = catalog;
        state
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
