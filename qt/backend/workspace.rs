use anyhow::{Context, Result, bail, ensure};
use rex_rune::{
    connection::{DEFAULT_TAB_LABEL, HostCommand, HostModel},
    layout::{self, Side},
    settings::{MutedTab, Settings},
};
use rexrs::api::session::{LayerBlock, Window};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Deserialize)]
struct Target {
    epoch: u64,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    window: Option<String>,
    #[serde(default)]
    pane: Option<String>,
}
impl Target {
    fn check_session(&self, model: &HostModel) -> Result<()> {
        ensure!(
            self.epoch == model.connection_epoch,
            "Connection changed; action discarded."
        );
        ensure!(
            self.session.is_some()
                && self.session == model.selected
                && model
                    .sessions
                    .iter()
                    .any(|s| s.session_id == self.session()),
            "Session changed; action discarded."
        );
        Ok(())
    }
    fn check<'a>(&self, model: &'a HostModel, pane: bool) -> Result<&'a Window> {
        self.check_session(model)?;
        let window = model.window().context("That tab is no longer available.")?;
        ensure!(
            self.window.is_some() && self.window == window.window_id,
            "Tab changed; action discarded."
        );
        if pane {
            ensure!(
                self.pane
                    .as_deref()
                    .is_some_and(|id| blocks(window).any(|b| b.block_id.as_deref() == Some(id))),
                "That pane is no longer available."
            );
        }
        Ok(window)
    }
    fn session(&self) -> &str {
        self.session.as_deref().unwrap_or_default()
    }
    fn pane(&self) -> &str {
        self.pane.as_deref().unwrap_or_default()
    }
}
fn blocks(window: &Window) -> impl Iterator<Item = &LayerBlock> {
    window.layers.iter().flat_map(|l| &l.blocks)
}
fn scoped(target: &Target, method: &str, mut payload: Value) -> HostCommand {
    payload["session_id"] = json!(target.session);
    HostCommand::Request {
        method: method.into(),
        payload,
    }
}
fn divider(target: &Target, d: layout::Divider) -> HostCommand {
    scoped(
        target,
        "session.resize_pane",
        json!({"direction":if d.horizontal {"horizontal"} else {"vertical"}, "before_block_id":d.before,"after_block_id":d.after,"ratio":d.ratio}),
    )
}

#[derive(Clone, Copy, Deserialize)]
enum Action {
    #[serde(rename = "client.close")]
    CloseFocused,
    #[serde(rename = "session.new")]
    NewSession,
    #[serde(rename = "session.rename")]
    RenameSession,
    #[serde(rename = "session.close")]
    CloseSession,
    #[serde(rename = "client.tab.new")]
    NewTab,
    #[serde(rename = "client.tab.rename")]
    RenameTab,
    #[serde(rename = "client.tab.close")]
    CloseTab,
    #[serde(rename = "client.tab.close_others")]
    CloseOtherTabs,
    #[serde(rename = "client.tab.move")]
    MoveTab,
    #[serde(rename = "client.tab.mute")]
    MuteTab,
    #[serde(rename = "pane.split")]
    Split,
    #[serde(rename = "pane.focus")]
    Focus,
    #[serde(rename = "pane.focus_next")]
    FocusNext,
    #[serde(rename = "pane.focus_previous")]
    FocusPrevious,
    #[serde(rename = "pane.zoom")]
    Zoom,
    #[serde(rename = "pane.move")]
    Move,
    #[serde(rename = "pane.move_to_tab")]
    MoveToTab,
    #[serde(rename = "pane.move_to_new_tab")]
    MoveToNewTab,
    #[serde(rename = "pane.resize")]
    Resize,
    #[serde(rename = "pane.resize_divider")]
    ResizeDivider,
    #[serde(rename = "pane.balance")]
    Balance,
    #[serde(rename = "pane.rename")]
    RenamePane,
    #[serde(rename = "pane.close")]
    ClosePane,
    #[serde(rename = "pane.close_others")]
    CloseOtherPanes,
    #[serde(rename = "confirm.accept")]
    Confirm,
    #[serde(rename = "confirm.cancel")]
    Cancel,
}
#[derive(Clone)]
enum Closing {
    Session,
    Tab,
    Tabs(Vec<String>),
    Pane,
    Panes(Vec<String>),
}
#[derive(Clone)]
struct Confirmation {
    id: u64,
    target: Target,
    endpoint: String,
    closing: Closing,
    title: String,
    description: String,
}
impl Confirmation {
    fn needs_pane(&self) -> bool {
        matches!(self.closing, Closing::Pane | Closing::Panes(_))
    }
    fn valid_target(&self, model: &HostModel) -> bool {
        if matches!(self.closing, Closing::Session) {
            self.target.check_session(model).is_ok()
        } else {
            self.target.check(model, self.needs_pane()).is_ok()
        }
    }
    fn valid(&self, model: &HostModel, endpoint: &str) -> bool {
        endpoint == self.endpoint && self.valid_target(model)
    }
}
#[derive(Default)]
pub struct Workspace {
    pending: Option<Confirmation>,
    next_question: u64,
}
#[derive(Default)]
pub struct Outcome {
    pub commands: Vec<HostCommand>,
    pub settings_changed: bool,
}

/// The shared keymap registry may use this table; every entry remains remote-denied.
pub const ACTIONS: &[(&str, &str, &str)] = &[
    ("client.close", "Close pane or tab…", "Pane"),
    ("session.new", "New session…", "Session"),
    ("session.rename", "Rename session…", "Session"),
    ("session.close", "Close session…", "Session"),
    ("client.tab.new", "New tab", "Tab"),
    ("client.tab.rename", "Rename tab…", "Tab"),
    ("client.tab.move", "Move tab", "Tab"),
    ("client.tab.mute", "Mute or unmute tab", "Tab"),
    ("client.tab.close_others", "Close other tabs…", "Tab"),
    ("client.tab.close", "Close tab…", "Tab"),
    ("pane.split", "Split pane", "Pane"),
    ("pane.focus", "Focus pane", "Pane"),
    ("pane.focus_next", "Focus next pane", "Pane"),
    ("pane.focus_previous", "Focus previous pane", "Pane"),
    ("pane.zoom", "Zoom or restore pane", "Pane"),
    ("pane.move", "Move pane", "Pane"),
    ("pane.move_to_tab", "Move pane to tab…", "Pane"),
    (
        "pane.move_to_new_tab",
        "Move pane to new tab (not supported)",
        "Pane",
    ),
    ("pane.resize", "Resize pane", "Pane"),
    ("pane.balance", "Balance splits", "Pane"),
    ("pane.rename", "Rename pane…", "Pane"),
    ("pane.close_others", "Close other panes…", "Pane"),
    ("pane.close", "Close pane…", "Pane"),
];

pub fn known(action: &str) -> bool {
    ACTIONS.iter().any(|(id, _, _)| *id == action)
}
pub fn schema(action: &str) -> Option<Value> {
    let side = json!({"type":"string","enum":["left","right","up","down"]});
    let properties = match action {
        "pane.split" => {
            json!({"direction":{"type":"string","enum":["left","right","up","down","best_fit"]}})
        }
        "pane.focus" | "pane.move" => json!({"direction":side}),
        "pane.resize" => {
            json!({"direction":side,"amount":{"type":"number","exclusiveMinimum":0,"default":5}})
        }
        "client.tab.move" => json!({"direction":{"type":"string","enum":["left","right"]}}),
        "pane.move_to_tab" => json!({"window_id":{"type":"string","minLength":1}}),
        "session.new" => json!({"label":{"type":"string"}}),
        "session.rename" | "client.tab.rename" | "pane.rename" => {
            json!({"label":{"type":"string"}})
        }
        _ => return None,
    };
    let required = if matches!(
        action,
        "pane.split" | "pane.focus" | "pane.move" | "pane.resize" | "client.tab.move"
    ) {
        json!(["direction"])
    } else {
        json!([])
    };
    Some(
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    )
}
pub fn validate(action: &str, args: &Value) -> Result<()> {
    ensure!(known(action), "Unknown workspace action.");
    let args = if args.is_null() {
        json!({})
    } else {
        args.clone()
    };
    let object = args
        .as_object()
        .context("Action arguments must be an object.")?;
    let allowed: &[&str] = match action {
        "pane.split" | "pane.focus" | "pane.move" | "client.tab.move" => &["direction"],
        "pane.resize" => &["direction", "amount"],
        "pane.move_to_tab" => &["window_id"],
        "session.new" | "session.rename" | "client.tab.rename" | "pane.rename" => &["label"],
        _ => &[],
    };
    ensure!(
        object.keys().all(|k| allowed.contains(&k.as_str())),
        "Unknown workspace argument."
    );
    match action {
        "pane.split" => ensure!(
            matches!(
                args["direction"].as_str(),
                Some("left" | "right" | "up" | "down" | "best_fit")
            ),
            "Choose a split direction."
        ),
        "pane.focus" | "pane.move" | "pane.resize" => ensure!(
            args["direction"].as_str().and_then(Side::parse).is_some(),
            "Choose a pane direction."
        ),
        "client.tab.move" => ensure!(
            matches!(args["direction"].as_str(), Some("left" | "right")),
            "Choose a tab direction."
        ),
        _ => {}
    }
    if let Some(amount) = object.get("amount") {
        ensure!(
            amount.as_f64().is_some_and(|n| n.is_finite() && n > 0.),
            "Resize amount must be positive."
        );
    }
    for field in ["label", "window_id"] {
        if let Some(value) = object.get(field) {
            ensure!(value.is_string(), "{field} must be text.");
        }
    }
    Ok(())
}

impl Workspace {
    pub fn state(&mut self, model: &HostModel) -> Value {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| !p.valid_target(model))
        {
            self.pending = None;
        }
        self.pending.as_ref().map_or(Value::Null, |p| json!({"id":p.id,"epoch":p.target.epoch,"session":p.target.session,"window":p.target.window,"pane":p.target.pane,"title":p.title,"description":p.description,"default":"cancel"}))
    }
    pub fn handle(
        &mut self,
        model: &HostModel,
        settings: &mut Settings,
        endpoint: &str,
        request: &Value,
    ) -> Result<Outcome> {
        let action: Action = serde_json::from_value(request["action"].clone())
            .context("Unknown workspace action.")?;
        let action = if matches!(action, Action::CloseFocused) {
            if model
                .window()
                .is_some_and(|window| blocks(window).count() > 1)
            {
                Action::ClosePane
            } else {
                Action::CloseTab
            }
        } else {
            action
        };
        let args = &request["args"];
        let mut out = Outcome::default();
        if matches!(action, Action::Cancel) {
            if self
                .pending
                .as_ref()
                .is_some_and(|p| args["id"].as_u64() == Some(p.id))
            {
                self.pending = None;
            }
            return Ok(out);
        }
        ensure!(
            model.connected || matches!(action, Action::MuteTab),
            "Connection lost; action discarded."
        );
        if matches!(action, Action::Confirm) {
            let pending = self
                .pending
                .take()
                .context("That question is no longer available.")?;
            ensure!(
                args["id"].as_u64() == Some(pending.id),
                "That question is no longer available."
            );
            ensure!(
                request["epoch"].as_u64() == Some(pending.target.epoch)
                    && pending.valid(model, endpoint),
                "Target or connection changed; nothing was closed."
            );
            let target = &pending.target;
            let window = if matches!(pending.closing, Closing::Session) {
                None
            } else {
                Some(target.check(model, pending.needs_pane())?)
            };
            let one = |method: &str, payload: Value| scoped(target, method, payload);
            match pending.closing {
                Closing::Session => out.commands.push(one("session.destroy", json!({}))),
                Closing::Tab => out.commands.push(one(
                    "session.close_window",
                    json!({"window_id":target.window}),
                )),
                Closing::Pane => out
                    .commands
                    .push(one("block.close", json!({"block_id":target.pane}))),
                Closing::Tabs(ids) => {
                    for id in ids {
                        if model.view().is_some_and(|v| {
                            v.windows
                                .iter()
                                .any(|w| w.window_id.as_deref() == Some(&id))
                        }) {
                            out.commands
                                .push(one("session.close_window", json!({"window_id":id})));
                        }
                    }
                }
                Closing::Panes(ids) => {
                    for id in ids {
                        if blocks(window.context("That tab is no longer available.")?)
                            .any(|b| b.block_id.as_deref() == Some(&id))
                        {
                            out.commands
                                .push(one("block.close", json!({"block_id":id})));
                        }
                    }
                }
            }
            return Ok(out);
        }
        let target: Target =
            serde_json::from_value(request.clone()).context("Missing workspace target.")?;
        ensure!(
            target.epoch == model.connection_epoch,
            "Connection changed; action discarded."
        );
        if !matches!(action, Action::ResizeDivider) {
            validate(request["action"].as_str().unwrap_or_default(), args)?;
        }
        let shell = settings
            .hosts
            .iter()
            .find(|h| h.endpoint == endpoint)
            .and_then(|h| h.shell.clone());
        if matches!(action, Action::NewSession) {
            out.commands.push(HostCommand::CreateSession {
                label: args["label"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_owned),
                shell,
            });
            return Ok(out);
        }
        if matches!(action, Action::NewTab) && model.window().is_none() {
            target.check_session(model)?;
            ensure!(
                target.window.is_none() && target.pane.is_none(),
                "Tab changed; action discarded."
            );
            out.commands.push(scoped(
                &target,
                "session.new_window",
                json!({
                    "window_label":DEFAULT_TAB_LABEL,
                    "layout":layout::shell_layout(shell.as_deref(),None),"focus":true
                }),
            ));
            return Ok(out);
        }
        if matches!(action, Action::RenameSession | Action::CloseSession) {
            target.check_session(model)?;
            if matches!(action, Action::RenameSession) {
                let label = args["label"].as_str().context("Enter a session name.")?;
                out.commands
                    .push(scoped(&target, "session.set_label", json!({"label":label})));
            } else {
                let name = model
                    .sessions
                    .iter()
                    .find(|s| s.session_id == target.session())
                    .map_or("this session", |s| s.label.as_str());
                let host = settings
                    .hosts
                    .iter()
                    .find(|h| h.endpoint == endpoint)
                    .map_or("this host", |h| h.label.as_str());
                self.next_question = self.next_question.wrapping_add(1);
                self.pending = Some(Confirmation {
                    id: self.next_question,
                    target,
                    endpoint: endpoint.into(),
                    closing: Closing::Session,
                    title: format!("Close {name}?"),
                    description: format!(
                        "Ends every program in {name} on {host}. Closing the app itself leaves programs running."
                    ),
                });
            }
            return Ok(out);
        }
        let pane = matches!(
            action,
            Action::Split
                | Action::Focus
                | Action::FocusNext
                | Action::FocusPrevious
                | Action::Zoom
                | Action::Move
                | Action::MoveToTab
                | Action::MoveToNewTab
                | Action::Resize
                | Action::RenamePane
                | Action::ClosePane
                | Action::CloseOtherPanes
        );
        let window = target.check(model, pane)?;
        let view = model
            .view()
            .context("That session is no longer available.")?;
        let one = |method: &str, payload: Value| scoped(&target, method, payload);
        let name = window
            .label
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("this tab");
        let host = settings
            .hosts
            .iter()
            .find(|h| h.endpoint == endpoint)
            .map_or("this host", |h| h.label.as_str());
        match action {
            Action::NewTab | Action::Split => {
                let layout = layout::shell_layout(shell.as_deref(), None);
                let (method, payload) = if matches!(action, Action::NewTab) {
                    (
                        "session.new_window",
                        json!({"session_id":target.session,"window_label":DEFAULT_TAB_LABEL,"layout":layout,"focus":true}),
                    )
                } else {
                    let direction = args["direction"].as_str().unwrap();
                    let (direction, side) = match direction {
                        "left" => ("horizontal", "before"),
                        "right" => ("horizontal", "after"),
                        "up" => ("vertical", "before"),
                        "down" => ("vertical", "after"),
                        _ => {
                            let rect = layout::placements(window, None)
                                .0
                                .into_iter()
                                .find(|p| p.block == target.pane())
                                .map_or([0., 0., 1., 1.], |p| p.rect);
                            let pane_height = request["height"].as_f64().unwrap_or(0.) * rect[3];
                            let pane_width = request["width"].as_f64().unwrap_or(0.) * rect[2];
                            if pane_height > pane_width {
                                ("vertical", "after")
                            } else {
                                ("horizontal", "after")
                            }
                        }
                    };
                    (
                        "session.new_split",
                        json!({"session_id":target.session,"anchor_id":target.pane,"direction":direction,"side":side,"layout":layout,"focus":true}),
                    )
                };
                if let Some(from) = target
                    .pane
                    .as_ref()
                    .filter(|p| blocks(window).any(|b| b.block_id.as_ref() == Some(p)))
                {
                    out.commands.push(HostCommand::NewTerminal {
                        method: method.into(),
                        payload,
                        cwd_from: from.clone(),
                    });
                } else {
                    out.commands.push(HostCommand::Request {
                        method: method.into(),
                        payload,
                    });
                }
            }
            Action::RenameTab | Action::RenamePane => {
                let label = args["label"]
                    .as_str()
                    .context("Enter a name, or use the default title.")?;
                let (method, payload) = match action {
                    Action::RenameTab => (
                        "session.set_window_label",
                        json!({"window_id":target.window,"label":label}),
                    ),
                    _ => (
                        "session.set_block_label",
                        json!({"block_id":target.pane,"label":label}),
                    ),
                };
                out.commands.push(one(method, payload));
            }
            Action::MoveTab => {
                let index = view
                    .windows
                    .iter()
                    .position(|w| w.window_id == target.window)
                    .context("That tab is no longer available.")?;
                let before = if args["direction"] == "left" {
                    index.checked_sub(1)
                } else if index + 1 < view.windows.len() {
                    Some(index + 2)
                } else {
                    None
                };
                if let Some(before) = before {
                    let mut payload = json!({"window_id":target.window});
                    if let Some(id) = view.windows.get(before).and_then(|w| w.window_id.as_ref()) {
                        payload["before_window_id"] = json!(id);
                    }
                    out.commands.push(one("session.move_window", payload));
                }
            }
            Action::MuteTab => {
                let muted = MutedTab {
                    endpoint: endpoint.into(),
                    session: target.session().into(),
                    window: target.window.clone().unwrap(),
                };
                let was = settings.muted_tabs.contains(&muted);
                settings.muted_tabs.retain(|m| m != &muted);
                if !was {
                    settings.muted_tabs.push(muted);
                    let extra = settings.muted_tabs.len().saturating_sub(256);
                    settings.muted_tabs.drain(..extra);
                }
                out.settings_changed = true;
            }
            Action::Focus | Action::Move => {
                let side = args["direction"].as_str().and_then(Side::parse).unwrap();
                if let Some(other) = layout::neighbour(window, target.pane(), side) {
                    out.commands.push(if matches!(action, Action::Focus) {
                        one("session.focus_block", json!({"block_id":other}))
                    } else {
                        one(
                            "session.swap_blocks",
                            json!({"block_id":target.pane,"other_block_id":other}),
                        )
                    });
                }
            }
            Action::FocusNext | Action::FocusPrevious => out.commands.push(one(
                if matches!(action, Action::FocusNext) {
                    "session.focus_next_pane"
                } else {
                    "session.focus_previous_pane"
                },
                json!({"wrap":true}),
            )),
            Action::Zoom => {
                out.commands
                    .push(one("session.focus_block", json!({"block_id":target.pane})));
                out.commands
                    .push(one("session.zoom_block", json!({"block_id":target.pane})));
            }
            Action::Resize => {
                if let Some(d) = layout::resized_by(
                    window,
                    target.pane(),
                    args["direction"].as_str().and_then(Side::parse).unwrap(),
                    args["amount"].as_f64().unwrap_or(5.) / 100.,
                ) {
                    out.commands.push(divider(&target, d));
                }
            }
            Action::ResizeDivider => {
                let before = args["before"].as_str().context("Missing divider.")?;
                let after = args["after"].as_str().context("Missing divider.")?;
                let mut d = layout::placements(window, None)
                    .1
                    .into_iter()
                    .find(|d| d.before == before && d.after == after)
                    .context("That divider is no longer available.")?;
                let ratio = args["ratio"]
                    .as_f64()
                    .filter(|r| r.is_finite())
                    .context("Invalid divider ratio.")?;
                d.ratio = ratio.clamp(layout::RATIO_RANGE.0, layout::RATIO_RANGE.1);
                out.commands.push(divider(&target, d));
            }
            Action::Balance => out.commands.extend(
                layout::balanced(window)
                    .into_iter()
                    .map(|d| divider(&target, d)),
            ),
            Action::MoveToTab => {
                let destination = args["window_id"].as_str().context("Choose another tab.")?;
                let other = view
                    .windows
                    .iter()
                    .find(|w| {
                        w.window_id.as_deref() == Some(destination) && w.window_id != target.window
                    })
                    .context("Choose another tab in this session.")?;
                let anchor = other
                    .focused_block_id
                    .as_deref()
                    .or_else(|| blocks(other).find_map(|b| b.block_id.as_deref()))
                    .context("The destination tab has no pane.")?;
                out.commands.push(one("session.move_block",json!({"block_id":target.pane,"anchor_block_id":anchor,"direction":"horizontal","side":"after"})));
            }
            Action::MoveToNewTab => bail!(
                "Move pane to new tab is not supported by the Rex server API; choose an existing tab instead."
            ),
            Action::CloseTab
            | Action::CloseOtherTabs
            | Action::ClosePane
            | Action::CloseOtherPanes => {
                let pane_name = blocks(window)
                    .find(|b| b.block_id.as_deref() == Some(target.pane()))
                    .and_then(|b| b.label.as_deref())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("this pane");
                let session_name = model
                    .sessions
                    .iter()
                    .find(|s| s.session_id == target.session())
                    .map_or("this session", |s| s.label.as_str());
                let (closing, title, what) = match action {
                    Action::CloseTab => (
                        Closing::Tab,
                        format!("Close {name}?"),
                        format!("every program in {name}"),
                    ),
                    Action::ClosePane => (
                        Closing::Pane,
                        format!("Close {pane_name} in {name}?"),
                        format!("the program in {pane_name}"),
                    ),
                    Action::CloseOtherTabs => {
                        let ids: Vec<_> = view
                            .windows
                            .iter()
                            .filter_map(|w| w.window_id.as_ref())
                            .filter(|id| Some(*id) != target.window.as_ref())
                            .cloned()
                            .collect();
                        if ids.is_empty() {
                            return Ok(out);
                        }
                        let n = ids.len();
                        (
                            Closing::Tabs(ids),
                            format!("Close {n} other tabs?"),
                            format!(
                                "every program in the {n} other tabs of {session_name}; {name} stays open"
                            ),
                        )
                    }
                    _ => {
                        let ids: Vec<_> = blocks(window)
                            .filter_map(|b| b.block_id.as_ref())
                            .filter(|id| Some(*id) != target.pane.as_ref())
                            .cloned()
                            .collect();
                        if ids.is_empty() {
                            return Ok(out);
                        }
                        let n = ids.len();
                        (
                            Closing::Panes(ids),
                            format!("Close {n} other panes in {name}?"),
                            format!("the programs in the {n} other panes; {pane_name} stays open"),
                        )
                    }
                };
                self.next_question = self.next_question.wrapping_add(1);
                self.pending = Some(Confirmation {
                    id: self.next_question,
                    target,
                    endpoint: endpoint.into(),
                    closing,
                    title,
                    description: format!(
                        "Ends {what} on {host}. Closing the app itself leaves programs running."
                    ),
                });
            }
            Action::CloseFocused
            | Action::NewSession
            | Action::RenameSession
            | Action::CloseSession
            | Action::Confirm
            | Action::Cancel => unreachable!(),
        }
        Ok(out)
    }
}
