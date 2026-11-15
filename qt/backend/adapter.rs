//! Transport, terminal parsing and ownership stay in the existing host workers.
//! Every terminal request names its complete target; local selection is not a
//! license to deliver an old input event to a newly selected pane.
use anyhow::{Context, Result, bail, ensure};
use rex_rune::{
    connection::{Command, Connection, HostCommand, HostConnection, HostModel},
    terminal::{CursorShape, Frame},
};
use rexrs::api::session;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub const VERSION: u64 = 2;

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Target {
    session: String,
    window: String,
    pane: String,
}

/// Selection belongs to this frontend; the server still decides the active tab.
#[derive(Default)]
pub struct Adapter {
    pane: Option<String>,
    pending_session: Option<String>,
    pending_window: Option<String>,
    pending_navigation: Option<u64>,
    next_navigation: u64,
}

fn visible_terminals(window: &session::Window) -> impl Iterator<Item = &session::LayerBlock> {
    window
        .layers
        .iter()
        .filter(|layer| layer.state.as_deref() != Some("minimized"))
        .flat_map(|layer| &layer.blocks)
        .filter(|block| block.creator_name.as_deref() == Some("com.superlogical.terminal"))
        .filter(|block| {
            window
                .zoomed_block_id
                .as_ref()
                .is_none_or(|id| block.block_id.as_ref() == Some(id))
        })
}

impl Adapter {
    fn target(&self, model: &HostModel) -> Option<Target> {
        let session = model.selected.as_ref()?;
        if self
            .pending_session
            .as_ref()
            .is_some_and(|id| id != session)
        {
            return None;
        }
        let window = model.window()?;
        let window_id = window.window_id.as_ref()?;
        if self
            .pending_window
            .as_ref()
            .is_some_and(|id| id != window_id)
        {
            return None;
        }
        let pane = self
            .pane
            .as_deref()
            .or(window.zoomed_block_id.as_deref())
            .or(window.focused_block_id.as_deref())?;
        let visible =
            visible_terminals(window).any(|block| block.block_id.as_deref() == Some(pane));
        visible.then(|| Target {
            session: session.clone(),
            window: window_id.clone(),
            pane: pane.to_owned(),
        })
    }

    fn visible_target(&self, model: &HostModel, target: &Target) -> bool {
        self.target(model).is_some()
            && model.selected.as_ref() == Some(&target.session)
            && model.window().is_some_and(|window| {
                window.window_id.as_ref() == Some(&target.window)
                    && visible_terminals(window).any(|b| b.block_id.as_ref() == Some(&target.pane))
            })
    }

    fn connection(&self, model: &HostModel, request: &Value, resize: bool) -> Result<Connection> {
        self.connection_scope(model, request, resize, resize, false)
    }

    fn connection_scope(
        &self,
        model: &HostModel,
        request: &Value,
        visible: bool,
        owner: bool,
        local: bool,
    ) -> Result<Connection> {
        let target: Target =
            serde_json::from_value(request.clone()).context("missing terminal target")?;
        ensure!(
            if visible {
                self.visible_target(model, &target)
            } else {
                self.target(model).as_ref() == Some(&target)
            },
            "terminal target is no longer selected or visible"
        );
        ensure!(local || model.connected, "connection lost; input paused");
        ensure!(
            request["epoch"].as_u64() == Some(model.connection_epoch),
            "terminal connection changed; input paused"
        );
        let connection = model
            .terminals
            .get(&target.pane)
            .context("terminal not attached")?;
        {
            let terminal = connection.model.lock().expect("terminal model");
            ensure!(
                local || terminal.connected && terminal.control_connected,
                "terminal disconnected; input paused"
            );
            ensure!(
                !owner || terminal.owner,
                "another client owns terminal size"
            );
        }
        Ok(connection.clone())
    }

    /// Apply one local frontend request. Returns false only for clean shutdown.
    pub fn handle(&mut self, host: &HostConnection, request: &Value) -> Result<bool> {
        ensure!(
            request["version"].as_u64() == Some(VERSION),
            "unsupported adapter protocol version"
        );
        let model = host.model.lock().expect("host model").clone();
        let field = |name: &str| -> Result<String> {
            request[name]
                .as_str()
                .map(str::to_owned)
                .with_context(|| format!("missing {name}"))
        };
        let op = request["op"].as_str().context("missing op")?;
        if !matches!(op, "shutdown" | "refresh") {
            ensure!(
                request["epoch"].as_u64() == Some(model.connection_epoch),
                "connection changed; request discarded"
            );
        }
        match op {
            "shutdown" => return Ok(false),
            "refresh" => host.commands.send(HostCommand::RefreshSessions)?,
            "visibility" => host.commands.send(HostCommand::Visibility(
                request["visible"].as_bool().context("invalid visibility")?,
            ))?,
            "select_session" => {
                let session = field("session")?;
                ensure!(
                    model.sessions.iter().any(|s| s.session_id == session),
                    "session no longer exists"
                );
                self.pending_session = Some(session.clone());
                self.pending_window = None;
                self.pane = None;
                self.navigate(host, session, None)?;
            }
            "select_window" => {
                let session = field("session")?;
                let window = field("window")?;
                ensure!(
                    model.selected.as_ref() == Some(&session),
                    "session is no longer selected"
                );
                ensure!(
                    model.view().is_some_and(|v| v
                        .windows
                        .iter()
                        .any(|w| w.window_id.as_ref() == Some(&window))),
                    "tab no longer exists"
                );
                self.pending_window = Some(window.clone());
                self.pane = None;
                self.navigate(host, session, Some(window))?;
            }
            "select_pane" => {
                let target: Target = serde_json::from_value(request.clone())?;
                let previous = self.pane.replace(target.pane.clone());
                if self.target(&model).as_ref() != Some(&target) {
                    self.pane = previous;
                    bail!("pane no longer exists in the selected tab");
                }
                let connection = model
                    .terminals
                    .get(&target.pane)
                    .context("terminal not attached")?;
                connection.commands.focus()?;
            }
            "claim" => {
                let target: Target = serde_json::from_value(request.clone())?;
                ensure!(
                    model.connected
                        && request["epoch"].as_u64() == Some(model.connection_epoch)
                        && self.visible_target(&model, &target),
                    "pane is no longer visible"
                );
                host.commands.send(HostCommand::Claim {
                    session: target.session,
                    blocks: vec![target.pane],
                })?;
            }
            "scroll" | "search" | "search_step" | "prompt_step" | "selection" | "mouse"
            | "wheel" => {
                let local = !matches!(op, "mouse" | "wheel");
                let connection = self.connection_scope(&model, request, local, false, local)?;
                connection
                    .commands
                    .try_send(crate::terminal::command(request)?)?;
            }
            "key" | "paste" | "resize" => {
                let op = request["op"].as_str().expect("checked op");
                let connection = self.connection(&model, request, op == "resize")?;
                let command = match op {
                    "key" => {
                        ensure!(request["payload"].is_object(), "missing key payload");
                        Command::Key(request["payload"].clone())
                    }
                    "paste" => Command::Paste(field("text")?),
                    _ => {
                        let number = |name: &str| -> Result<u16> {
                            request[name]
                                .as_u64()
                                .filter(|n| (1..=65535).contains(n))
                                .map(|n| n as u16)
                                .with_context(|| format!("invalid {name}"))
                        };
                        Command::Resize(
                            number("cols")?,
                            number("rows")?,
                            number("cell_width")?,
                            number("cell_height")?,
                        )
                    }
                };
                connection.commands.try_send(command)?;
            }
            _ => bail!("unknown adapter operation"),
        }
        Ok(true)
    }

    fn navigate(
        &mut self,
        host: &HostConnection,
        session: String,
        window: Option<String>,
    ) -> Result<()> {
        self.next_navigation = self.next_navigation.wrapping_add(1);
        self.pending_navigation = Some(self.next_navigation);
        host.commands.send(HostCommand::Navigate {
            id: self.next_navigation,
            deadline: Instant::now() + Duration::from_secs(5),
            session,
            window,
        })?;
        Ok(())
    }

    /// Snapshot a bounded copy of host state, without serializing under its lock.
    pub fn state(&mut self, model: &HostModel) -> Value {
        if self.pane.as_deref() == model.focused() {
            self.pane = None;
        }
        if let Some((id, _)) = model.navigation_result
            && self.pending_navigation == Some(id)
        {
            self.pending_navigation = None;
            self.pending_session = None;
            self.pending_window = None;
        }
        if self.pending_session.as_ref() == model.selected.as_ref() {
            self.pending_session = None;
        }
        if self
            .pending_window
            .as_ref()
            .is_some_and(|id| model.window().and_then(|w| w.window_id.as_ref()) == Some(id))
        {
            self.pending_window = None;
        }
        if self.pane.is_some() && self.target(model).is_none() {
            self.pane = None;
        }
        let target = self.target(model);
        let (terminal_state, frame) = target.as_ref().map_or((Value::Null, Value::Null), |t| {
            terminal_value(model, &t.pane)
        });
        let windows = model.view().map_or_else(Vec::new, |v| {
            v.windows.iter().filter_map(|w| {
            Some(json!({"id":w.window_id.as_ref()?,"label":w.label.as_deref().unwrap_or("Shell")}))
        }).collect()
        });
        let (panes, dividers) = model.window().map_or_else(|| (Vec::new(), Vec::new()), |window| {
            let (placements, dividers) = rex_rune::layout::placements(window, None);
            let panes = placements.into_iter().filter_map(|placement| {
                let block = visible_terminals(window).find(|b| b.block_id.as_ref() == Some(&placement.block))?;
                let (terminal, frame) = terminal_value(model, &placement.block);
                Some(json!({"id":placement.block,"label":block.label.as_deref().unwrap_or("Terminal"),
                    "session":model.selected,"window":window.window_id,"pane":placement.block,
                    "rect":placement.rect,"terminal":terminal,"frame":frame}))
            }).collect();
            let dividers = dividers.into_iter().map(|d| json!({"before":d.before,"after":d.after,
                "horizontal":d.horizontal,"rect":d.rect,"ratio":d.ratio})).collect();
            (panes, dividers)
        });
        json!({"type":"state","version":VERSION,"connected":model.connected,"status":model.status,"epoch":model.connection_epoch,
            "session":model.selected,"window":model.window().and_then(|w| w.window_id.as_ref()),
            "pane":target.as_ref().map(|t| &t.pane),
            "sessions":model.sessions.iter().map(|s| json!({"id":s.session_id,"label":s.label})).collect::<Vec<_>>(),
            "windows":windows,"panes":panes,"dividers":dividers,"terminal":terminal_state,"frame":frame})
    }
}

fn terminal_value(model: &HostModel, pane: &str) -> (Value, Value) {
    let Some(connection) = model.terminals.get(pane) else {
        return (Value::Null, Value::Null);
    };
    let (state, frame) = {
        let m = connection.model.lock().expect("terminal model");
        (
            json!({"connected":model.connected && m.connected && m.control_connected,
            "owner":m.owner,"status":m.status,"revision":m.revision,"generation":m.restore_generation}),
            m.frame.clone(),
        )
    };
    (
        state,
        frame
            .as_ref()
            .map_or(Value::Null, |frame| frame_value(frame)),
    )
}

fn frame_value(frame: &Frame) -> Value {
    let cells = frame.cells.iter().take(usize::from(frame.rows)).map(|row| row.iter().map(|c| {
        json!({"text":c.text,"spacer_tail":c.spacer_tail,"fg":c.fg,"bg":c.bg,
            "bold":c.style.bold,"italic":c.style.italic,"faint":c.style.faint,
            "invisible":c.style.invisible,"underline":format!("{:?}",c.style.underline).to_lowercase(),
            "strikethrough":c.style.strikethrough,"overline":c.style.overline,
            "underline_color":c.underline_color})
    }).collect::<Vec<_>>()).collect::<Vec<_>>();
    let mut links = Vec::new();
    let mut index = 0;
    while index < usize::from(frame.cols) * usize::from(frame.rows) {
        if let Some(link) = frame.link_at(index) {
            index = link.end.saturating_add(1).max(index + 1);
            links.push(json!({"uri":link.uri,"start":link.start,"end":link.end}));
        } else {
            index += 1;
        }
    }
    json!({"links":links,"cols":frame.cols,"rows":frame.rows,"fg":frame.fg,"bg":frame.bg,
        "cursor":frame.cursor,"cursor_color":frame.cursor_color,
        "cursor_shape":match frame.cursor_shape {
            CursorShape::Block => "block", CursorShape::Bar => "bar",
            CursorShape::Underline => "underline", CursorShape::Hollow => "hollow",
        },"cells":cells,"wrapped":frame.wrapped,
        "mouse_tracking":frame.mouse_tracking,"selection_rows":frame.selection_rows,
        "selection_text":frame.selection_text.as_deref(),"selection_error":frame.selection_error,
        "selection_revision":frame.selection_revision,
        "search":frame.search.as_ref().map(|s| json!({"total":s.total,"index":s.index,"selected":s.selected.map(|m| json!({"start":m.start,"end":m.end})),
            "matches":s.matches.iter().map(|m| json!({"start":m.start,"end":m.end})).collect::<Vec<_>>()}))})
}

#[cfg(test)]
mod tests {
    use super::*;
    use rexrs::api::session;

    fn fixture() -> HostModel {
        let mut model = HostModel {
            connected: true,
            selected: Some("session-a".into()),
            ..Default::default()
        };
        model.views.insert(
            "session-a".into(),
            session::ViewResponse {
                active_window_id: Some("window-a".into()),
                windows: vec![session::Window {
                    window_id: Some("window-a".into()),
                    focused_block_id: Some("pane-a".into()),
                    layers: vec![session::Layer {
                        blocks: vec![session::LayerBlock {
                            block_id: Some("pane-a".into()),
                            creator_name: Some("com.superlogical.terminal".into()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        model
    }

    #[test]
    fn current_visible_pane_is_targeted() {
        let adapter = Adapter::default();
        assert_eq!(adapter.target(&fixture()).unwrap().pane, "pane-a");
    }

    #[test]
    fn pending_navigation_cannot_target_previous_session_or_window() {
        let mut adapter = Adapter {
            pending_session: Some("session-b".into()),
            ..Default::default()
        };
        assert!(adapter.target(&fixture()).is_none());
        adapter.pending_session = None;
        adapter.pending_window = Some("window-b".into());
        assert!(adapter.target(&fixture()).is_none());
    }

    #[test]
    fn removed_pane_does_not_retarget_old_input() {
        let adapter = Adapter {
            pane: Some("removed-pane".into()),
            ..Default::default()
        };
        assert!(adapter.target(&fixture()).is_none());
    }

    fn host_fixture(mut model: HostModel) -> (HostConnection, flume::Receiver<HostCommand>) {
        use std::sync::{Arc, Mutex};
        let view = model.views.get_mut("session-a").unwrap();
        for id in ["window-b", "window-c"] {
            view.windows.push(session::Window {
                window_id: Some(id.into()),
                ..Default::default()
            });
        }
        let (commands, receive) = flume::unbounded();
        let (_, wake) = flume::bounded(1);
        (
            HostConnection {
                model: Arc::new(Mutex::new(model)),
                commands,
                wake,
            },
            receive,
        )
    }

    #[test]
    fn failed_navigation_restores_the_current_pane_without_retargeting_a_new_request() {
        let (host, requests) = host_fixture(fixture());
        let mut adapter = Adapter::default();
        let mut ids = Vec::new();
        for window in ["window-b", "window-c"] {
            adapter.handle(&host, &json!({"version":2,"epoch":0,"op":"select_window","session":"session-a","window":window})).unwrap();
            match requests.recv().unwrap() {
                HostCommand::Navigate { id, .. } => ids.push(id),
                _ => panic!("navigation needs a correlated outcome"),
            }
        }
        assert_ne!(ids[0], ids[1]);
        let mut model = host.model.lock().unwrap();
        model.navigation_result = Some((ids[0], false));
        assert!(adapter.state(&model)["pane"].is_null());
        model.navigation_result = Some((ids[1], false));
        assert_eq!(adapter.state(&model)["pane"], "pane-a");
    }

    #[test]
    fn failed_session_navigation_restores_the_current_session_pane() {
        let mut model = fixture();
        model.sessions.push(session::Session {
            session_id: "session-b".into(),
            label: "Other".into(),
        });
        let (host, requests) = host_fixture(model);
        let mut adapter = Adapter::default();
        adapter
            .handle(
                &host,
                &json!({"version":2,"epoch":0,"op":"select_session","session":"session-b"}),
            )
            .unwrap();
        let id = match requests.recv().unwrap() {
            HostCommand::Navigate { id, .. } => id,
            _ => panic!("session navigation needs a correlated outcome"),
        };
        let mut model = host.model.lock().unwrap();
        assert!(adapter.state(&model)["pane"].is_null());
        model.navigation_result = Some((id, false));
        assert_eq!(adapter.state(&model)["pane"], "pane-a");
    }

    #[test]
    fn completed_navigation_follows_server_focus_even_if_it_has_moved_again() {
        let (host, requests) = host_fixture(fixture());
        let mut adapter = Adapter::default();
        adapter.handle(&host, &json!({"version":2,"epoch":0,"op":"select_window","session":"session-a","window":"window-b"})).unwrap();
        let id = match requests.recv().unwrap() {
            HostCommand::Navigate { id, .. } => id,
            _ => panic!("navigation needs a correlated outcome"),
        };
        let mut model = host.model.lock().unwrap();
        model.navigation_result = Some((id, true));
        assert_eq!(adapter.state(&model)["pane"], "pane-a");
    }

    #[test]
    fn zoomed_pane_is_the_only_selectable_terminal_even_with_an_old_local_selection() {
        let mut model = fixture();
        let window = &mut model.views.get_mut("session-a").unwrap().windows[0];
        window.layers[0].blocks.push(session::LayerBlock {
            block_id: Some("pane-b".into()),
            creator_name: Some("com.superlogical.terminal".into()),
            ..Default::default()
        });
        window.zoomed_block_id = Some("pane-b".into());
        let mut adapter = Adapter {
            pane: Some("pane-a".into()),
            ..Default::default()
        };
        let state = adapter.state(&model);
        assert_eq!(state["panes"].as_array().unwrap().len(), 1);
        assert_eq!(state["panes"][0]["id"], "pane-b");
        assert_eq!(state["pane"], "pane-b");
    }

    fn attached_fixture() -> HostModel {
        use rex_rune::connection::{Model, TerminalSender};
        use std::sync::{Arc, Mutex};
        let mut model = fixture();
        let (commands, _) = flume::unbounded();
        let (_, wake) = flume::bounded(1);
        model.terminals.insert(
            "pane-a".into(),
            Connection {
                model: Arc::new(Mutex::new(Model {
                    connected: true,
                    control_connected: true,
                    ..Default::default()
                })),
                commands: TerminalSender::new(commands, "session-a".into(), "pane-a".into()),
                wake,
            },
        );
        model
    }

    #[test]
    fn input_refuses_disconnected_and_stale_targets() {
        let adapter = Adapter::default();
        let mut model = attached_fixture();
        let request = json!({"epoch":0,"session":"session-a","window":"window-a","pane":"pane-a"});
        assert!(adapter.connection(&model, &request, false).is_ok());
        assert!(
            adapter
                .connection(
                    &model,
                    &json!({"epoch":0,"session":"session-a","window":"window-a","pane":"removed"}),
                    false
                )
                .is_err()
        );
        model.connected = false;
        assert!(adapter.connection(&model, &request, false).is_err());
        model.connected = true;
        model.terminals["pane-a"]
            .model
            .lock()
            .unwrap()
            .control_connected = false;
        assert!(adapter.connection(&model, &request, false).is_err());
    }

    #[test]
    fn only_the_selected_owned_pane_can_be_resized() {
        let adapter = Adapter::default();
        let model = attached_fixture();
        let request = json!({"epoch":0,"session":"session-a","window":"window-a","pane":"pane-a"});
        assert!(adapter.connection(&model, &request, true).is_err());
        model.terminals["pane-a"].model.lock().unwrap().owner = true;
        assert!(adapter.connection(&model, &request, true).is_ok());
        assert!(
            adapter
                .connection(
                    &model,
                    &json!({"epoch":0,"session":"session-a","window":"hidden","pane":"pane-a"}),
                    true
                )
                .is_err()
        );
    }

    #[test]
    fn serialization_preserves_ghostty_cells_styles_and_wide_tails() {
        let mut terminal = ghostty_vt::Terminal::new(ghostty_vt::TerminalOptions {
            cols: 12,
            rows: 3,
            max_scrollback: 1024,
        })
        .unwrap();
        terminal.vt_write("\x1b[38;2;1;2;3;48;2;4;5;6;1m界e\u{301}".as_bytes());
        let frame = rex_rune::terminal::Renderer::new()
            .unwrap()
            .frame(&terminal)
            .unwrap();
        let value = frame_value(&frame);
        assert_eq!(value["cells"][0][0]["text"], "界");
        assert_eq!(value["cells"][0][0]["fg"], 0x010203);
        assert_eq!(value["cells"][0][0]["bg"], 0x040506);
        assert_eq!(value["cells"][0][0]["bold"], true);
        assert_eq!(value["cells"][0][1]["spacer_tail"], true);
        assert_eq!(value["cells"][0][2]["text"], "e\u{301}");
        assert_eq!(value["cells"].as_array().unwrap().len(), 3);
    }
    #[test]
    fn frame_links_use_the_core_safe_scheme_policy() {
        let mut terminal = ghostty_vt::Terminal::new(ghostty_vt::TerminalOptions {
            cols: 40,
            rows: 3,
            max_scrollback: 1024,
        })
        .unwrap();
        terminal.vt_write(b"\x1b]8;;https://example.com\x1b\\Safe\x1b]8;;\x1b\\ \x1b]8;;file:///tmp/private\x1b\\Unsafe\x1b]8;;\x1b\\");
        let frame = rex_rune::terminal::Renderer::new()
            .unwrap()
            .frame(&terminal)
            .unwrap();
        let value = frame_value(&frame);
        let links = value["links"].as_array().unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0]["uri"], "https://example.com/");
        assert_eq!(links[0]["start"], 0);
        assert_eq!(links[0]["end"], 3);
    }

    #[test]
    fn local_history_navigation_can_cancel_a_visible_unfocused_pane() {
        let model = split_fixture();
        let adapter = Adapter::default();
        let request = json!({"epoch":0,"session":"session-a","window":"window-a","pane":"pane-b"});
        assert!(
            adapter
                .connection_scope(&model, &request, true, false, true)
                .is_ok()
        );
        assert!(
            adapter
                .connection_scope(&model, &request, false, false, false)
                .is_err()
        );
    }

    fn split_fixture() -> HostModel {
        let mut model = attached_fixture();
        let window = &mut model.views.get_mut("session-a").unwrap().windows[0];
        window.layers[0].layout = Some(
            serde_json::from_value(json!({"split":{
                "direction":"horizontal","ratio":0.4,
                "before":{"block_id":"pane-a"},"after":{"block_id":"pane-b"}
            }}))
            .unwrap(),
        );
        window.layers[0].blocks.push(session::LayerBlock {
            block_id: Some("pane-b".into()),
            creator_name: Some("com.superlogical.terminal".into()),
            ..Default::default()
        });
        let second = model.terminals["pane-a"].clone();
        model.terminals.insert("pane-b".into(), second);
        model
    }

    #[test]
    fn scene_publishes_authoritative_rectangles_and_all_visible_terminal_states() {
        let model = split_fixture();
        let state = Adapter::default().state(&model);
        assert_eq!(state["panes"][0]["rect"], json!([0., 0., 0.4, 1.]));
        assert_eq!(state["panes"][1]["rect"], json!([0.4, 0., 0.6, 1.]));
        assert_eq!(state["panes"][1]["terminal"]["connected"], true);
        assert_eq!(state["panes"][1]["pane"], "pane-b");
        assert_eq!(state["dividers"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn visible_owned_unselected_pane_can_resize_but_cannot_receive_typing() {
        let model = split_fixture();
        model.terminals["pane-b"].model.lock().unwrap().owner = true;
        let adapter = Adapter::default();
        let request = json!({"epoch":0,"session":"session-a","window":"window-a","pane":"pane-b"});
        assert!(adapter.connection(&model, &request, true).is_ok());
        assert!(adapter.connection(&model, &request, false).is_err());
    }

    #[test]
    fn focus_acknowledgment_restores_server_focus_authority() {
        let mut model = split_fixture();
        let mut adapter = Adapter {
            pane: Some("pane-a".into()),
            ..Default::default()
        };
        assert_eq!(adapter.state(&model)["pane"], "pane-a");
        model.views.get_mut("session-a").unwrap().windows[0].focused_block_id =
            Some("pane-b".into());
        assert_eq!(adapter.state(&model)["pane"], "pane-b");
    }
    #[test]
    fn old_connection_epoch_cannot_send_to_the_same_reconnected_pane() {
        let mut model = attached_fixture();
        model.connection_epoch = 7;
        let adapter = Adapter::default();
        assert!(
            adapter
                .connection(
                    &model,
                    &json!({"epoch":6,"session":"session-a","window":"window-a","pane":"pane-a"}),
                    false
                )
                .is_err()
        );
        assert!(
            adapter
                .connection(
                    &model,
                    &json!({"epoch":7,"session":"session-a","window":"window-a","pane":"pane-a"}),
                    false
                )
                .is_ok()
        );
    }

    #[test]
    fn scene_preserves_nested_and_floating_layer_order_and_omits_minimized_layers() {
        let mut model = fixture();
        let block = |id: &str| json!({"block_id":id,"creator_name":"com.superlogical.terminal"});
        model.views.get_mut("session-a").unwrap().windows[0] = serde_json::from_value(json!({
            "window_id":"window-a","focused_block_id":"pane-a","layers":[
                {"kind":"tiled","bounds":[0,0,1,1],"blocks":[block("pane-a"),block("pane-b"),block("pane-c")],
                 "layout":{"split":{"direction":"horizontal","ratio":0.4,"before":{"block_id":"pane-a"},
                  "after":{"split":{"direction":"vertical","ratio":0.5,"before":{"block_id":"pane-b"},"after":{"block_id":"pane-c"}}}}}},
                {"kind":"floating","bounds":[0.2,0.3,0.4,0.5],"blocks":[block("pane-d")],"layout":{"block_id":"pane-d"}},
                {"kind":"floating","state":"minimized","bounds":[0,0,1,1],"blocks":[block("pane-e")],"layout":{"block_id":"pane-e"}}
            ]})).unwrap();
        let state = Adapter::default().state(&model);
        assert_eq!(state["panes"].as_array().unwrap().len(), 4);
        assert_eq!(state["panes"][2]["rect"], json!([0.4, 0.5, 0.6, 0.5]));
        assert_eq!(state["panes"][3]["id"], "pane-d");
        assert_eq!(state["panes"][3]["rect"], json!([0.2, 0.3, 0.4, 0.5]));
    }
    #[test]
    fn local_history_remains_usable_while_terminal_input_is_paused() {
        let mut model = attached_fixture();
        model.connected = false;
        model.terminals["pane-a"]
            .model
            .lock()
            .unwrap()
            .control_connected = false;
        let request = json!({"epoch":0,"session":"session-a","window":"window-a","pane":"pane-a"});
        let adapter = Adapter::default();
        assert!(
            adapter
                .connection_scope(&model, &request, true, false, true)
                .is_ok()
        );
        assert!(adapter.connection(&model, &request, false).is_err());
    }
}
