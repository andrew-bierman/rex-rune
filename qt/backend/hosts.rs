use anyhow::{Context, Result, ensure};
use rex_rune::{
    connection::{HostCommand, HostConnection, HostModel},
    settings::{Host, Settings, validate_host},
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Visit {
    pub host: String,
    pub session: String,
    pub window: Option<String>,
}

#[derive(Default)]
pub struct History {
    entries: Vec<Visit>,
    cursor: usize,
}
impl History {
    pub fn current(&self) -> Option<&Visit> {
        self.entries.get(self.cursor)
    }
    pub fn observe(&mut self, visit: Visit) {
        if self.current() == Some(&visit) {
            return;
        }
        self.entries.truncate(self.cursor + 1);
        self.entries.push(visit);
        if self.entries.len() > 128 {
            self.entries.remove(0);
        }
        self.cursor = self.entries.len().saturating_sub(1);
    }
    pub fn step(&mut self, direction: isize) -> Option<Visit> {
        let next = self.cursor.checked_add_signed(direction)?;
        let visit = self.entries.get(next)?.clone();
        self.cursor = next;
        Some(visit)
    }
    pub fn remove_host(&mut self, host: &str) {
        let before = self
            .entries
            .iter()
            .take(self.cursor + 1)
            .filter(|v| v.host != host)
            .count();
        self.entries.retain(|v| v.host != host);
        self.cursor = before.saturating_sub(1);
    }
    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }
    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }
}

pub struct Slot {
    pub id: String,
    pub connection: Option<HostConnection>,
    pub model: HostModel,
}

struct Pending {
    visit: Visit,
    request: u64,
    deadline: Instant,
    origin: usize,
    dispatch_at: Option<Instant>,
}

pub struct Hosts {
    pub slots: Vec<Slot>,
    pub active: String,
    pub history: History,
    serial: u64,
    navigation: u64,
    pending: Option<Pending>,
    visible: bool,
    prefetch: bool,
}

impl Hosts {
    /// Catalog construction is inert. `connect_all` starts attachment-only workers explicitly.
    pub fn new(settings: &Settings) -> Self {
        let slots: Vec<_> = settings
            .hosts
            .iter()
            .enumerate()
            .map(|(i, _)| Slot {
                id: format!("host-{}", i + 1),
                connection: None,
                model: HostModel::default(),
            })
            .collect();
        let selected = settings
            .last_host
            .as_ref()
            .and_then(|endpoint| settings.hosts.iter().position(|h| &h.endpoint == endpoint))
            .unwrap_or(0);
        Self {
            active: slots
                .get(selected)
                .map(|s| s.id.clone())
                .unwrap_or_default(),
            serial: slots.len() as u64,
            slots,
            history: History::default(),
            navigation: 0,
            pending: None,
            visible: true,
            prefetch: settings.sidebar_open,
        }
    }
    pub fn ids(&self) -> Vec<String> {
        self.slots.iter().map(|s| s.id.clone()).collect()
    }
    pub fn index(&self, id: &str) -> Result<usize> {
        self.slots
            .iter()
            .position(|s| s.id == id)
            .context("host was removed or replaced")
    }
    pub fn require_active(&self, id: &str) -> Result<usize> {
        ensure!(id == self.active, "active host changed; request discarded");
        self.index(id)
    }
    pub fn require_target(&self, id: &str, epoch: u64) -> Result<usize> {
        let index = self.require_active(id)?;
        ensure!(self.pending.is_none(), "navigation is still pending");
        ensure!(
            self.slots[index].model.connection_epoch == epoch,
            "connection changed; request discarded"
        );
        Ok(index)
    }
    pub fn active_slot(&self) -> Option<&Slot> {
        self.slots.iter().find(|s| s.id == self.active)
    }
    pub fn active_connection(&self) -> Result<&HostConnection> {
        self.active_slot()
            .and_then(|s| s.connection.as_ref())
            .context("host is disconnected")
    }
    pub fn connect_all(&mut self, settings: &Settings) {
        for i in 0..self.slots.len() {
            self.connect_at(settings, i);
        }
    }
    fn connect_at(&mut self, settings: &Settings, index: usize) {
        if self.slots[index].connection.is_some() {
            return;
        }
        let config = &settings.hosts[index];
        let connection = HostConnection::spawn_labeled(
            config.endpoint.clone(),
            settings.sessions.get(&config.endpoint).cloned(),
            None,
            None,
            self.visible && self.slots[index].id == self.active,
            None,
            "Rex Qt Quick".into(),
        );
        self.slots[index].connection = Some(connection);
    }
    pub fn disconnect(&mut self, id: &str) -> Result<()> {
        let i = self.index(id)?;
        self.slots[i].connection.take();
        self.slots[i].model.connected = false;
        self.slots[i].model.status = "Disconnected by you".into();
        if self.pending.as_ref().is_some_and(|p| p.visit.host == id) {
            self.cancel_navigation();
        }
        Ok(())
    }
    pub fn reconnect(&mut self, settings: &Settings, id: &str) -> Result<()> {
        let i = self.index(id)?;
        if let Some(connection) = &self.slots[i].connection {
            connection.commands.send(HostCommand::Reconnect)?;
        } else {
            self.cancel_navigation();
            let fresh = self.next_id();
            for visit in &mut self.history.entries {
                if visit.host == id {
                    visit.host = fresh.clone();
                }
            }
            if self.active == id {
                self.active = fresh.clone();
            }
            self.slots[i] = Slot {
                id: fresh,
                connection: None,
                model: HostModel::default(),
            };
            self.connect_at(settings, i);
            self.update_visibility();
        }
        Ok(())
    }
    fn next_id(&mut self) -> String {
        self.serial += 1;
        format!("host-{}", self.serial)
    }
    fn clean(settings: &mut Settings, endpoint: &str) {
        settings.sessions.remove(endpoint);
        settings.recent_paths.remove(endpoint);
        settings.muted_tabs.retain(|tab| tab.endpoint != endpoint);
        if settings.last_host.as_deref() == Some(endpoint) {
            settings.last_host = None;
        }
    }
    pub fn add(
        &mut self,
        settings: &mut Settings,
        path: &Path,
        label: &str,
        endpoint: &str,
        shell: Option<String>,
    ) -> Result<String> {
        validate_host(label.trim(), endpoint.trim())?;
        ensure!(
            !settings.hosts.iter().any(|h| h.endpoint == endpoint.trim()),
            "that endpoint is already saved"
        );
        let mut next = settings.clone();
        next.hosts.push(Host {
            label: label.trim().into(),
            endpoint: endpoint.trim().into(),
            shell: normalize_shell(shell)?,
            collapsed: false,
            remote_control: false,
        });
        next.save(path)?;
        *settings = next;
        let id = self.next_id();
        self.slots.push(Slot {
            id: id.clone(),
            connection: None,
            model: HostModel::default(),
        });
        self.connect_at(settings, self.slots.len() - 1);
        Ok(id)
    }
    pub fn edit(
        &mut self,
        settings: &mut Settings,
        path: &Path,
        id: &str,
        label: &str,
        endpoint: &str,
        shell: Option<String>,
    ) -> Result<String> {
        let i = self.index(id)?;
        let label = label.trim();
        let endpoint = endpoint.trim();
        validate_host(label, endpoint)?;
        ensure!(
            !settings
                .hosts
                .iter()
                .enumerate()
                .any(|(j, h)| i != j && h.endpoint == endpoint),
            "that endpoint is already saved"
        );
        let mut next = settings.clone();
        let old = next.hosts[i].endpoint.clone();
        let replaced = old != endpoint;
        next.hosts[i].label = label.into();
        next.hosts[i].shell = normalize_shell(shell)?;
        if replaced {
            next.hosts[i].remote_control = false;
            Self::clean(&mut next, &old);
            next.hosts[i].endpoint = endpoint.into();
        }
        next.save(path)?;
        *settings = next;
        if replaced {
            let connected = self.slots[i].connection.is_some();
            let fresh = self.next_id();
            self.slots[i] = Slot {
                id: fresh.clone(),
                connection: None,
                model: HostModel::default(),
            };
            self.cancel_navigation();
            self.history.remove_host(id);
            if self.active == id {
                self.active = fresh;
            }
            if connected {
                self.connect_at(settings, i);
            }
        }
        Ok(self.slots[i].id.clone())
    }
    pub fn remove(&mut self, settings: &mut Settings, path: &Path, id: &str) -> Result<()> {
        let i = self.index(id)?;
        ensure!(self.slots.len() > 1, "keep at least one saved host");
        let mut next = settings.clone();
        let removed = next.hosts.remove(i);
        Self::clean(&mut next, &removed.endpoint);
        next.save(path)?;
        *settings = next;
        self.slots.remove(i);
        self.cancel_navigation();
        self.history.remove_host(id);
        if self.active == id {
            self.active = self.slots[0].id.clone();
        }
        self.update_visibility();
        Ok(())
    }
    pub fn activate(&mut self, settings: &mut Settings, path: &Path, id: &str) -> Result<()> {
        let i = self.index(id)?;
        let mut next = settings.clone();
        next.last_host = Some(next.hosts[i].endpoint.clone());
        next.save(path)?;
        *settings = next;
        self.cancel_navigation();
        self.active = id.into();
        self.update_visibility();
        Ok(())
    }
    pub fn set_collapsed(
        &mut self,
        settings: &mut Settings,
        path: &Path,
        id: &str,
        collapsed: bool,
    ) -> Result<()> {
        let i = self.index(id)?;
        let mut next = settings.clone();
        next.hosts[i].collapsed = collapsed;
        next.save(path)?;
        *settings = next;
        Ok(())
    }
    pub fn set_sidebar(&mut self, settings: &mut Settings, path: &Path, open: bool) -> Result<()> {
        let mut next = settings.clone();
        next.sidebar_open = open;
        next.save(path)?;
        *settings = next;
        self.set_prefetch(open);
        Ok(())
    }
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        self.update_visibility();
    }
    fn update_visibility(&self) {
        for slot in &self.slots {
            if let Some(c) = &slot.connection {
                let _ = c.commands.send(HostCommand::Visibility(
                    self.visible && slot.id == self.active,
                ));
            }
        }
    }
    pub fn set_prefetch(&mut self, enabled: bool) {
        self.prefetch = enabled;
        self.update_prefetch();
    }
    fn update_prefetch(&self) {
        for slot in &self.slots {
            if let Some(c) = &slot.connection {
                let sessions = if self.prefetch {
                    slot.model
                        .sessions
                        .iter()
                        .map(|s| s.session_id.clone())
                        .collect()
                } else {
                    HashSet::new()
                };
                let _ = c.commands.send(HostCommand::PrefetchViews(sessions));
            }
        }
    }
    fn valid_visit(&self, visit: &Visit) -> bool {
        self.slots
            .iter()
            .find(|s| s.id == visit.host)
            .is_some_and(|s| {
                s.model
                    .sessions
                    .iter()
                    .any(|v| v.session_id == visit.session)
                    && visit.window.as_ref().is_none_or(|w| {
                        s.model.views.get(&visit.session).is_none_or(|v| {
                            v.windows.iter().any(|v| v.window_id.as_ref() == Some(w))
                        })
                    })
            })
    }
    pub fn navigate(&mut self, visit: Visit) -> Result<()> {
        self.begin_navigation(visit, false)
    }
    pub fn navigate_remote(&mut self, visit: Visit, deadline: Instant) -> Result<u64> {
        self.begin_navigation_until(visit, false, deadline)?;
        Ok(self.navigation)
    }
    pub fn navigation_pending(&self) -> bool {
        self.pending.is_some()
    }
    fn begin_navigation(&mut self, visit: Visit, defer: bool) -> Result<()> {
        self.begin_navigation_until(visit, defer, Instant::now() + Duration::from_secs(30))
    }
    fn begin_navigation_until(
        &mut self,
        visit: Visit,
        defer: bool,
        deadline: Instant,
    ) -> Result<()> {
        ensure!(self.valid_visit(&visit), "navigation target disappeared");
        let i = self.index(&visit.host)?;
        ensure!(self.slots[i].connection.is_some(), "host is disconnected");
        if !self.slots[i].model.connected {
            self.slots[i]
                .connection
                .as_ref()
                .expect("checked connection")
                .commands
                .send(HostCommand::Select(visit.session))?;
            self.cancel_navigation();
            self.active = visit.host;
            self.update_visibility();
            return Ok(());
        }
        self.navigation = self.navigation.wrapping_add(1);
        let now = Instant::now();
        let origin = self
            .pending
            .as_ref()
            .map_or(self.history.cursor, |p| p.origin);
        self.pending = Some(Pending {
            visit,
            request: self.navigation,
            deadline,
            origin,
            dispatch_at: Some(
                now + if defer {
                    Duration::from_millis(150)
                } else {
                    Duration::ZERO
                },
            ),
        });
        if !defer {
            self.dispatch_navigation()?;
        }
        Ok(())
    }
    fn dispatch_navigation(&mut self) -> Result<()> {
        let Some(p) = &self.pending else {
            return Ok(());
        };
        let i = self.index(&p.visit.host)?;
        self.slots[i]
            .connection
            .as_ref()
            .context("host disconnected during navigation")?
            .commands
            .send(HostCommand::Navigate {
                id: p.request,
                deadline: p.deadline,
                session: p.visit.session.clone(),
                window: p.visit.window.clone(),
            })?;
        self.active = p.visit.host.clone();
        if let Some(p) = &mut self.pending {
            p.dispatch_at = None;
        }
        self.update_visibility();
        Ok(())
    }
    pub fn stops(&self, settings: &Settings, tabs: bool) -> Vec<Visit> {
        let mut stops = Vec::new();
        for (slot, host) in self
            .slots
            .iter()
            .zip(&settings.hosts)
            .filter(|(slot, _)| slot.model.connected)
        {
            for session in &slot.model.sessions {
                let listed = settings.sidebar_open && settings.vertical_tabs && !host.collapsed;
                let view = slot.model.views.get(&session.session_id).filter(|_| {
                    tabs && (listed || !slot.model.background_views.contains(&session.session_id))
                });
                if let Some(view) = view {
                    stops.extend(view.windows.iter().filter_map(|w| {
                        Some(Visit {
                            host: slot.id.clone(),
                            session: session.session_id.clone(),
                            window: Some(w.window_id.clone()?),
                        })
                    }));
                } else {
                    stops.push(Visit {
                        host: slot.id.clone(),
                        session: session.session_id.clone(),
                        window: None,
                    });
                }
            }
        }
        stops
    }
    pub fn cycle(&mut self, settings: &Settings, tabs: bool, direction: isize) -> Result<()> {
        ensure!(matches!(direction, -1 | 1), "invalid navigation direction");
        let stops = self.stops(settings, tabs);
        if stops.is_empty() {
            return Ok(());
        };
        let current = self.pending.as_ref().map(|p| p.visit.clone()).or_else(|| {
            self.active_slot().and_then(|s| {
                Some(Visit {
                    host: s.id.clone(),
                    session: s.model.selected.clone()?,
                    window: s.model.window().and_then(|w| w.window_id.clone()),
                })
            })
        });
        let index = current.as_ref().and_then(|c| {
            stops.iter().position(|v| {
                v == c
                    || v.host == c.host && v.session == c.session && (!tabs || v.window.is_none())
            })
        });
        let next = index.map_or_else(
            || if direction < 0 { stops.len() - 1 } else { 0 },
            |i| (i as isize + direction).rem_euclid(stops.len() as isize) as usize,
        );
        if index == Some(next) {
            return Ok(());
        };
        let visit = stops[next].clone();
        let defer = current
            .as_ref()
            .is_some_and(|c| c.host != visit.host || c.session != visit.session);
        self.begin_navigation(visit, defer)
    }
    pub fn history_step(&mut self, direction: isize) -> Result<()> {
        ensure!(matches!(direction, -1 | 1), "invalid history direction");
        let origin = self.history.cursor;
        let mut target = None;
        while let Some(visit) = self.history.step(direction) {
            if self.valid_visit(&visit)
                && self
                    .slots
                    .iter()
                    .find(|s| s.id == visit.host)
                    .is_some_and(|s| s.model.connected)
            {
                target = Some(visit);
                break;
            }
        }
        let Some(target) = target else {
            self.history.cursor = origin;
            return Ok(());
        };
        if let Err(error) = self.navigate(target) {
            self.history.cursor = origin;
            return Err(error);
        }
        if let Some(pending) = &mut self.pending {
            pending.origin = origin;
        }
        Ok(())
    }
    pub fn cancel_navigation(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.history.cursor = pending.origin;
        }
    }
    /// Polls only already-running core workers. It never creates sessions or retries input.
    pub fn poll(&mut self, settings: &mut Settings) -> bool {
        let mut dirty = false;
        let mut inventory_changed = false;
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if let Some(c) = &slot.connection {
                let woke = c.wake.drain().next().is_some();
                if woke || slot.model.status.is_empty() {
                    let next = c.model.lock().expect("host model").clone();
                    inventory_changed |= next.sessions.iter().map(|s| &s.session_id).ne(slot
                        .model
                        .sessions
                        .iter()
                        .map(|s| &s.session_id));
                    slot.model = next;
                    dirty = true;
                }
                for t in slot.model.terminals.values() {
                    dirty |= t.wake.drain().next().is_some();
                }
                if let Some(session) = &slot.model.selected {
                    settings
                        .sessions
                        .insert(settings.hosts[i].endpoint.clone(), session.clone());
                }
            }
        }
        if inventory_changed {
            self.update_prefetch();
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.dispatch_at.is_some_and(|at| at <= Instant::now()))
        {
            if let Err(error) = self.dispatch_navigation() {
                self.cancel_navigation();
                if let Some(slot) = self.slots.iter_mut().find(|s| s.id == self.active) {
                    slot.model.notice = Some(error.to_string());
                }
            }
            dirty = true;
        }
        if let Some(p) = &self.pending {
            let result = self
                .slots
                .iter()
                .find(|s| s.id == p.visit.host)
                .and_then(|s| s.model.navigation_result);
            if result == Some((p.request, false))
                || p.deadline <= Instant::now()
                || !self.valid_visit(&p.visit)
            {
                self.cancel_navigation();
                dirty = true;
            } else if p.dispatch_at.is_none() && result == Some((p.request, true)) {
                self.pending = None;
                dirty = true;
            }
        }
        if self.pending.is_none()
            && let Some(slot) = self.active_slot()
            && let Some(session) = &slot.model.selected
            && slot.model.view().is_some()
        {
            let visit = Visit {
                host: slot.id.clone(),
                session: session.clone(),
                window: slot.model.window().and_then(|w| w.window_id.clone()),
            };
            if self.history.current() != Some(&visit) {
                self.history.observe(visit);
                dirty = true;
            }
        }
        if let Some(index) = self.slots.iter().position(|slot| slot.id == self.active) {
            settings.last_host = Some(settings.hosts[index].endpoint.clone());
        }
        dirty
    }
    pub fn state(&self, settings: &Settings) -> Value {
        let rows: Vec<_> = self.slots.iter().zip(&settings.hosts).map(|(slot, config)| {
            let sessions:Vec<_> = slot.model.sessions.iter().map(|session| {
                let view=slot.model.views.get(&session.session_id);
                let tabs:Vec<_> = view.into_iter().flat_map(|v| &v.windows).filter_map(|window| {
                    let id = window.window_id.as_ref()?;
                    Some(json!({"id":id,"label":tab_title(&slot.model,window,settings.generated_titles),"selected":view.is_some_and(|v|v.active_window_id.as_ref()==Some(id))}))
                }).collect();
                json!({"id":session.session_id,"label":session.label,"selected":slot.model.selected.as_ref()==Some(&session.session_id),
                    "opened":view.is_some() && !slot.model.background_views.contains(&session.session_id),"tabs":tabs})
            }).collect();
            json!({"id":slot.id,"label":config.label,"endpoint":config.endpoint,"shell":config.shell,"collapsed":config.collapsed,"remote_control":config.remote_control,
                "active":slot.id==self.active,"connected":slot.model.connected,"enabled":slot.connection.is_some(),"status":slot.model.status,
                "ssh_server_missing":slot.model.ssh_server_missing,"epoch":slot.model.connection_epoch,"sessions":sessions})
        }).collect();
        json!({"active":self.active,"rows":rows,"back":self.history.can_back(),"forward":self.history.can_forward(),"pending":self.pending.is_some(),"sidebar_open":settings.sidebar_open,"vertical_tabs":settings.vertical_tabs})
    }
}

fn normalize_shell(shell: Option<String>) -> Result<Option<String>> {
    let shell = shell.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    ensure!(
        shell
            .as_ref()
            .is_none_or(|s| !s.chars().any(char::is_control)),
        "shell contains control characters"
    );
    Ok(shell)
}

pub fn tab_title(
    model: &HostModel,
    window: &rexrs::api::session::Window,
    generated: bool,
) -> String {
    let label = window.label.as_deref().filter(|s| !s.is_empty());
    let server_default = label
        .and_then(|s| s.strip_prefix("Window "))
        .is_some_and(|suffix| {
            suffix.len() == 8
                && window
                    .window_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with(suffix))
        });
    let default =
        server_default || label.is_none_or(|s| s == rex_rune::connection::DEFAULT_TAB_LABEL);
    let focused = window
        .focused_block_id
        .as_deref()
        .filter(|id| {
            window
                .layers
                .iter()
                .flat_map(|l| &l.blocks)
                .any(|b| b.block_id.as_deref() == Some(id))
        })
        .or_else(|| {
            window
                .layers
                .iter()
                .flat_map(|l| &l.blocks)
                .find_map(|b| b.block_id.as_deref())
        });
    default
        .then(|| focused.and_then(|id| model.shown_title(id, generated)))
        .flatten()
        .map(|s| s.into_owned())
        .or_else(|| label.map(str::to_owned))
        .unwrap_or_else(|| "Terminal".into())
}

/// Explicit discovery is read-only and runs outside the UI thread. Receivers may discard
/// late rows when a dialog closes; discovery never saves a host or attaches to a session.
pub fn discover() -> flume::Receiver<Value> {
    let (send, receive) = flume::unbounded();
    std::thread::spawn(move || {
        let result = rex_rune::tailnet::status();
        let machines = match result {
            Ok(m) => m,
            Err(error) => {
                let _ = send.send(json!({"type":"problem","message":error}));
                return;
            }
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                let _ = send.send(json!({"type":"problem","message":e.to_string()}));
                return;
            }
        };
        runtime.block_on(async {
            use rex_rune::tailnet::Probe;
            for machine in machines.into_iter().filter(|m|!m.is_ios()) {
                let probe=if machine.online {rex_rune::tailnet::probe(&machine).await} else {Probe::Nothing};
                let (endpoint,problem)=match probe {Probe::Rex {endpoint,reason,..}=>(Some(endpoint),reason), Probe::Nothing=>(None,Some(if machine.online {"No direct Rex listener found"} else {"Offline"}.into())),Probe::Unknown(reason)=>(None,Some(reason))};
                let _=send.send(json!({"type":"machine","label":machine.name,"os":machine.os_name(),"online":machine.online,"endpoint":endpoint,
                    "ssh_endpoint":machine.ssh_endpoint("",""),"problem":problem}));
            }
            let _=send.send(json!({"type":"done"}));
        });
    });
    receive
}

#[derive(Default)]
pub struct Discovery {
    receiver: Option<flume::Receiver<Value>>,
    rows: Vec<Value>,
    problem: Option<String>,
}
impl Discovery {
    pub fn start(&mut self) {
        if self.receiver.is_some() {
            return;
        }
        self.rows.clear();
        self.problem = None;
        self.receiver = Some(discover());
    }
    pub fn cancel(&mut self) {
        self.receiver = None;
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let mut done = false;
        if let Some(receiver) = &self.receiver {
            for message in receiver.try_iter() {
                changed = true;
                match message["type"].as_str() {
                    Some("machine") => self.rows.push(message),
                    Some("problem") => {
                        self.problem = message["message"].as_str().map(str::to_owned);
                        done = true;
                    }
                    Some("done") => done = true,
                    _ => {}
                }
            }
        }
        if done {
            self.receiver = None;
        }
        changed
    }
    pub fn state(&self) -> Value {
        json!({"reading":self.receiver.is_some(),"rows":self.rows,"problem":self.problem})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> tempfile::TempDir {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".build-tools/qt-host-tests");
        std::fs::create_dir_all(&root).unwrap();
        tempfile::tempdir_in(root).unwrap()
    }
    fn configured() -> (Settings, Hosts) {
        let settings = Settings {
            hosts: ["a", "b"]
                .into_iter()
                .map(|name| Host {
                    label: name.into(),
                    endpoint: format!("unix:///tmp/qt-host-contract-{name}.sock"),
                    remote_control: false,
                    collapsed: false,
                    shell: None,
                })
                .collect(),
            ..Settings::default()
        };
        let hosts = Hosts::new(&settings);
        (settings, hosts)
    }
    fn visit(host: &str, session: &str) -> Visit {
        Visit {
            host: host.into(),
            session: session.into(),
            window: None,
        }
    }

    #[test]
    fn editing_same_endpoint_preserves_consent_but_replacement_drops_it() {
        let (mut settings, mut hosts) = configured();
        settings.hosts[0].remote_control = true;
        settings.hosts[1].remote_control = true;
        let id = hosts.ids()[0].clone();
        let endpoint = settings.hosts[0].endpoint.clone();
        let dir = scratch();
        let profile = dir.path().join("workspace.json");
        let edited = hosts
            .edit(
                &mut settings,
                &profile,
                &id,
                "Renamed",
                &endpoint,
                Some("fish".into()),
            )
            .unwrap();
        assert_eq!(edited, id);
        assert!(settings.hosts[0].remote_control);
        assert_eq!(settings.hosts[0].shell.as_deref(), Some("fish"));
        let saved: Settings = serde_json::from_slice(&std::fs::read(&profile).unwrap()).unwrap();
        assert!(saved.hosts[0].remote_control);

        let replacement = format!("unix://{}", dir.path().join("replacement.sock").display());
        let replaced = hosts
            .edit(&mut settings, &profile, &id, "Renamed", &replacement, None)
            .unwrap();
        assert_ne!(replaced, id);
        assert!(!settings.hosts[0].remote_control);
        assert!(settings.hosts[1].remote_control);
        let saved: Settings = serde_json::from_slice(&std::fs::read(&profile).unwrap()).unwrap();
        assert!(!saved.hosts[0].remote_control);
        assert!(saved.hosts[1].remote_control);
    }

    #[test]
    fn removing_a_host_cancels_pending_history_before_pruning_occurrences() {
        let (mut settings, mut hosts) = configured();
        let ids = hosts.ids();
        for (host, session) in [
            (&ids[0], "a"),
            (&ids[1], "b"),
            (&ids[0], "c"),
            (&ids[1], "d"),
        ] {
            hosts.history.observe(visit(host, session));
        }
        hosts.pending = Some(Pending {
            visit: visit(&ids[0], "a"),
            request: 1,
            deadline: Instant::now() + Duration::from_secs(5),
            origin: 3,
            dispatch_at: None,
        });
        let dir = scratch();
        hosts
            .remove(&mut settings, &dir.path().join("workspace.json"), &ids[0])
            .unwrap();
        assert_eq!(hosts.history.current(), Some(&visit(&ids[1], "d")));
        assert!(hosts.state(&settings)["back"].as_bool().unwrap());
    }

    #[test]
    fn superseded_navigation_reports_do_not_release_current_input_pause() {
        let (mut settings, mut hosts) = configured();
        let id = hosts.ids()[0].clone();
        let model = &mut hosts.slots[0].model;
        model.connected = true;
        model.connection_epoch = 7;
        model.sessions =
            serde_json::from_value(json!([{"session_id":"target","label":"Target"}])).unwrap();
        hosts.pending = Some(Pending {
            visit: visit(&id, "target"),
            request: 3,
            deadline: Instant::now() + Duration::from_secs(5),
            origin: 0,
            dispatch_at: None,
        });
        for success in [true, false] {
            hosts.slots[0].model.navigation_result = Some((2, success));
            hosts.poll(&mut settings);
            assert!(hosts.require_target(&id, 7).is_err());
            assert_eq!(hosts.state(&settings)["pending"], true);
        }
        hosts.slots[0].model.navigation_result = Some((3, false));
        hosts.poll(&mut settings);
        assert!(hosts.require_target(&id, 7).is_ok());
        assert_eq!(hosts.state(&settings)["pending"], false);
    }
    #[test]
    fn acknowledged_navigation_follows_the_latest_server_focus() {
        let (mut settings, mut hosts) = configured();
        let id = hosts.ids()[0].clone();
        let model = &mut hosts.slots[0].model;
        model.connected = true;
        model.connection_epoch = 7;
        model.selected = Some("target".into());
        model.sessions =
            serde_json::from_value(json!([{"session_id":"target","label":"Target"}])).unwrap();
        model.views.insert(
            "target".into(),
            serde_json::from_value(json!({"revision":2,"active_window_id":"latest","windows":[
                {"window_id":"requested","layers":[]},{"window_id":"latest","layers":[]}
            ]}))
            .unwrap(),
        );
        model.navigation_result = Some((3, true));
        hosts.pending = Some(Pending {
            visit: Visit {
                host: id.clone(),
                session: "target".into(),
                window: Some("requested".into()),
            },
            request: 3,
            deadline: Instant::now() + Duration::from_secs(5),
            origin: 0,
            dispatch_at: None,
        });
        hosts.poll(&mut settings);
        assert_eq!(
            hosts.history.current().and_then(|v| v.window.as_deref()),
            Some("latest")
        );
        assert!(hosts.require_target(&id, 7).is_ok());
    }
    #[test]
    fn explicitly_recreated_worker_gets_new_identity_without_losing_same_machine_history() {
        let (mut settings, mut hosts) = configured();
        let old = hosts.active.clone();
        let dir = scratch();
        settings.hosts[0].endpoint = format!(
            "unix://{}",
            dir.path().join("absent-private.sock").display()
        );
        hosts.slots[0].model.connection_epoch = 1;
        hosts.history.observe(visit(&old, "saved-session"));
        hosts.reconnect(&settings, &old).unwrap();
        assert_ne!(hosts.active, old);
        assert!(hosts.require_target(&old, 1).is_err());
        assert_eq!(
            hosts.history.current().map(|v| v.host.as_str()),
            Some(hosts.active.as_str())
        );
        assert_eq!(
            hosts.history.current().map(|v| v.session.as_str()),
            Some("saved-session")
        );
        drop(hosts);
        assert!(rex_rune::connection::wait_for_hosts(Duration::from_secs(3)));
    }
}
