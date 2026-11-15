//! Private version-one JSON-lines adapter for the standalone Qt frontend.
//!
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

pub const VERSION: u64 = 1;

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

    fn connection(&self, model: &HostModel, request: &Value, resize: bool) -> Result<Connection> {
        let target: Target =
            serde_json::from_value(request.clone()).context("missing terminal target")?;
        ensure!(
            self.target(model).as_ref() == Some(&target),
            "terminal target is no longer selected"
        );
        ensure!(model.connected, "connection lost; input paused");
        let connection = model
            .terminals
            .get(&target.pane)
            .context("terminal not attached")?;
        {
            let terminal = connection.model.lock().expect("terminal model");
            ensure!(
                terminal.connected && terminal.control_connected,
                "terminal disconnected; input paused"
            );
            ensure!(
                !resize || terminal.owner,
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
        match request["op"].as_str().context("missing op")? {
            "shutdown" => return Ok(false),
            "refresh" => host.commands.send(HostCommand::RefreshSessions)?,
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
            "key" | "paste" | "resize" | "claim" => {
                let op = request["op"].as_str().expect("checked op");
                let connection = self.connection(&model, request, op == "resize")?;
                let command = match op {
                    "key" => {
                        ensure!(request["payload"].is_object(), "missing key payload");
                        Command::Key(request["payload"].clone())
                    }
                    "paste" => Command::Paste(field("text")?),
                    "claim" => Command::TakeControl,
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
        let terminal = target.as_ref().and_then(|t| model.terminals.get(&t.pane));
        let (terminal_state, frame) = terminal.map_or((Value::Null, Value::Null), |connection| {
            let (state, frame) = {
                let m = connection.model.lock().expect("terminal model");
                (
                    json!({"connected":model.connected && m.connected && m.control_connected,
                    "owner":m.owner,"status":m.status,"revision":m.revision}),
                    m.frame.clone(),
                )
            };
            (
                state,
                frame
                    .as_ref()
                    .map_or(Value::Null, |frame| frame_value(frame)),
            )
        });
        let windows = model.view().map_or_else(Vec::new, |v| {
            v.windows.iter().filter_map(|w| {
            Some(json!({"id":w.window_id.as_ref()?,"label":w.label.as_deref().unwrap_or("Shell")}))
        }).collect()
        });
        let panes = model.window().map_or_else(Vec::new, |w| visible_terminals(w)
            .filter_map(|b| Some(json!({"id":b.block_id.as_ref()?,"label":b.label.as_deref().unwrap_or("Terminal")}))).collect());
        json!({"type":"state","version":VERSION,"connected":model.connected,"status":model.status,
            "session":model.selected,"window":model.window().and_then(|w| w.window_id.as_ref()),
            "pane":target.as_ref().map(|t| &t.pane),
            "sessions":model.sessions.iter().map(|s| json!({"id":s.session_id,"label":s.label})).collect::<Vec<_>>(),
            "windows":windows,"panes":panes,"terminal":terminal_state,"frame":frame})
    }
}

fn frame_value(frame: &Frame) -> Value {
    let cells = frame.cells.iter().take(usize::from(frame.rows)).map(|row| row.iter().map(|c| {
        json!({"text":c.text,"spacer_tail":c.spacer_tail,"fg":c.fg,"bg":c.bg,
            "bold":c.style.bold,"italic":c.style.italic,"faint":c.style.faint,
            "invisible":c.style.invisible,"underline":format!("{:?}",c.style.underline).to_lowercase(),
            "strikethrough":c.style.strikethrough,"overline":c.style.overline,
            "underline_color":c.underline_color})
    }).collect::<Vec<_>>()).collect::<Vec<_>>();
    json!({"cols":frame.cols,"rows":frame.rows,"fg":frame.fg,"bg":frame.bg,
        "cursor":frame.cursor,"cursor_color":frame.cursor_color,
        "cursor_shape":match frame.cursor_shape {
            CursorShape::Block => "block", CursorShape::Bar => "bar",
            CursorShape::Underline => "underline", CursorShape::Hollow => "hollow",
        },"cells":cells,"wrapped":frame.wrapped})
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
            adapter.handle(&host, &json!({"version":1,"op":"select_window","session":"session-a","window":window})).unwrap();
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
                &json!({"version":1,"op":"select_session","session":"session-b"}),
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
        adapter.handle(&host, &json!({"version":1,"op":"select_window","session":"session-a","window":"window-b"})).unwrap();
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
        let request = json!({"session":"session-a","window":"window-a","pane":"pane-a"});
        assert!(adapter.connection(&model, &request, false).is_ok());
        assert!(
            adapter
                .connection(
                    &model,
                    &json!({"session":"session-a","window":"window-a","pane":"removed"}),
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
        let request = json!({"session":"session-a","window":"window-a","pane":"pane-a"});
        assert!(adapter.connection(&model, &request, true).is_err());
        model.terminals["pane-a"].model.lock().unwrap().owner = true;
        assert!(adapter.connection(&model, &request, true).is_ok());
        assert!(
            adapter
                .connection(
                    &model,
                    &json!({"session":"session-a","window":"hidden","pane":"pane-a"}),
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
}
