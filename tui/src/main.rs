mod ui;

use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct Inbox {
    state: Option<Value>,
    problem: Option<String>,
    closed: bool,
}

struct Backend {
    child: Child,
    input: ChildStdin,
    inbox: Arc<Mutex<Inbox>>,
}

impl Backend {
    fn start(executable: PathBuf, profile: PathBuf) -> Result<Self> {
        let mut child = Command::new(&executable)
            .arg("--config")
            .arg(profile)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("start {}", executable.display()))?;
        let input = child.stdin.take().context("backend stdin")?;
        let output = child.stdout.take().context("backend stdout")?;
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let sender = Arc::clone(&inbox);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if value["type"] == "state" {
                    let previous = sender.lock().unwrap().state.replace(value);
                    drop(previous);
                    continue;
                }
                let mut slot = sender.lock().unwrap();
                match value["type"].as_str() {
                    Some("hello") if value["version"] != 3 => {
                        slot.problem = Some("Unsupported Rex backend protocol".into());
                        break;
                    }
                    Some("error" | "notice") => slot.problem = Some(text(&value, "message").into()),
                    _ => {}
                }
            }
            sender.lock().unwrap().closed = true;
        });
        Ok(Self {
            child,
            input,
            inbox,
        })
    }

    fn send(&mut self, mut message: Value) -> Result<()> {
        message["version"] = json!(3);
        let mut bytes = serde_json::to_vec(&message)?;
        bytes.push(b'\n');
        self.input.write_all(&bytes)?;
        Ok(())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

fn items(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

fn targeted(state: &Value, op: &str) -> Value {
    json!({
        "op": op, "host": state["host"], "epoch": state["epoch"],
        "session": state["session"], "window": state["window"], "pane": state["pane"]
    })
}

fn create_workspace_item(state: &Value, key: KeyCode) -> Result<Value, &'static str> {
    if state["hosts"]["pending"] == true {
        return Err("Wait for the workspace to open");
    }
    if state["connected"] != true {
        return Err("Connect to a host first");
    }
    let (action, args) = match key {
        KeyCode::F(2) => ("session.new", json!({})),
        KeyCode::F(5) if state["session"].is_string() => ("client.tab.new", json!({})),
        KeyCode::F(5) => return Err("Open a session before creating a tab"),
        KeyCode::F(8) if state["pane"].is_string() => ("pane.split", json!({"direction": "right"})),
        KeyCode::F(9) if state["pane"].is_string() => ("pane.split", json!({"direction": "down"})),
        _ => return Err("Select a shell pane to split"),
    };
    let mut request = targeted(state, "workspace");
    request["action"] = json!(action);
    request["args"] = args;
    Ok(request)
}

fn terminal_key(event: KeyEvent) -> Option<Value> {
    use KeyCode::*;
    let name = match event.code {
        Char(' ') => "space".into(),
        Char(c) if c.is_ascii_alphabetic() => format!("key_{}", c.to_ascii_lowercase()),
        Char(c) if c.is_ascii_digit() => format!("digit_{c}"),
        Char('-' | '_') => "minus".into(),
        Char('=' | '+') => "equal".into(),
        Char('[' | '{') => "bracket_left".into(),
        Char(']' | '}') => "bracket_right".into(),
        Char('\\' | '|') => "backslash".into(),
        Char(';' | ':') => "semicolon".into(),
        Char('\'' | '"') => "quote".into(),
        Char(',' | '<') => "comma".into(),
        Char('.' | '>') => "period".into(),
        Char('/' | '?') => "slash".into(),
        Char('`' | '~') => "backquote".into(),
        Char(c) if "!@#$%^&*()".contains(c) => format!(
            "digit_{}",
            ("1234567890".as_bytes()["!@#$%^&*()".find(c)?]) as char
        ),
        Char(_) => "unidentified".into(),
        F(n) => format!("f{n}"),
        Enter => "enter".into(),
        Tab | BackTab => "tab".into(),
        Backspace => "backspace".into(),
        Delete => "delete".into(),
        Insert => "insert".into(),
        Esc => "escape".into(),
        Left => "arrow_left".into(),
        Right => "arrow_right".into(),
        Up => "arrow_up".into(),
        Down => "arrow_down".into(),
        Home => "home".into(),
        End => "end".into(),
        PageUp => "page_up".into(),
        PageDown => "page_down".into(),
        _ => return None,
    };
    let modifiers = event.modifiers;
    let typed = if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
        || event.kind == KeyEventKind::Release
    {
        String::new()
    } else {
        match event.code {
            Char(c) => c.to_string(),
            Enter => "\n".into(),
            Tab | BackTab => "\t".into(),
            _ => String::new(),
        }
    };
    let mods = [
        (KeyModifiers::SHIFT, "shift"),
        (KeyModifiers::CONTROL, "ctrl"),
        (KeyModifiers::ALT, "alt"),
        (KeyModifiers::SUPER, "super"),
    ]
    .into_iter()
    .filter_map(|(flag, name)| modifiers.contains(flag).then_some(name))
    .collect::<Vec<_>>()
    .join("+");
    Some(json!({
        "key": name, "mods": mods, "text": typed,
        "consumed_mods": if modifiers.contains(KeyModifiers::SHIFT) && matches!(event.code, Char(c) if c != ' ') { "shift" } else { "" },
        "unshifted_codepoint": match event.code { Char(c) => c.to_ascii_lowercase() as u32, _ => 0 },
        "action": match event.kind { KeyEventKind::Press => "press", KeyEventKind::Repeat => "repeat", KeyEventKind::Release => "release" },
        "composing": false,
    }))
}

fn run(terminal: &mut ratatui::DefaultTerminal, backend: &mut Backend) -> Result<()> {
    let mut screen = ui::Screen::default();
    let mut state = Value::Null;
    let mut problem = String::new();
    let mut sizes = HashMap::new();
    let mut resize_scope: Option<(String, Option<u64>, String, String)> = None;
    let mut last_claim = None;
    loop {
        let (update, message, closed) = {
            let mut inbox = backend.inbox.lock().unwrap();
            (inbox.state.take(), inbox.problem.take(), inbox.closed)
        };
        let changed = update.is_some() || message.is_some();
        if let Some(update) = update {
            state = update;
        }
        if let Some(message) = message {
            problem = message;
        }
        if closed {
            bail!(
                "{}",
                if problem.is_empty() {
                    "Rex backend exited"
                } else {
                    &problem
                }
            );
        }
        if changed || screen.dirty {
            terminal.draw(|frame| screen.draw(frame, &state, &problem))?;
            screen.dirty = false;
        }
        if state["hosts"]["pending"] != true {
            let scope = (
                text(&state, "host"),
                state["epoch"].as_u64(),
                text(&state, "session"),
                text(&state, "window"),
            );
            if resize_scope
                .as_ref()
                .is_none_or(|old| (old.0.as_str(), old.1, old.2.as_str(), old.3.as_str()) != scope)
            {
                sizes.clear();
                resize_scope = Some((
                    scope.0.to_owned(),
                    scope.1,
                    scope.2.to_owned(),
                    scope.3.to_owned(),
                ));
            }
            if !screen.sidebar
                && state["terminal"]["connected"] == true
                && state["terminal"]["owner"] != true
            {
                let claim = (scope.0.to_owned(), scope.1, text(&state, "pane").to_owned());
                if last_claim.as_ref() != Some(&claim) {
                    backend.send(targeted(&state, "claim"))?;
                    last_claim = Some(claim);
                }
            } else {
                last_claim = None;
            }
            sizes.retain(|pane, _| screen.pane_sizes.iter().any(|(id, _, _)| id == pane));
            for (pane, cols, rows) in &screen.pane_sizes {
                if sizes.get(pane) != Some(&(*cols, *rows)) {
                    let mut request = targeted(&state, "resize");
                    request["pane"] = json!(pane);
                    request["cols"] = json!(cols);
                    request["rows"] = json!(rows);
                    request["cell_width"] = json!(8);
                    request["cell_height"] = json!(16);
                    backend.send(request)?;
                    sizes.insert(pane.clone(), (*cols, *rows));
                }
            }
        }
        if !event::poll(Duration::from_millis(25))? {
            continue;
        }
        match event::read()? {
            Event::Resize(..) => screen.dirty = true,
            Event::Paste(paste)
                if !screen.sidebar
                    && state["terminal"]["connected"] == true
                    && state["hosts"]["pending"] != true =>
            {
                let mut request = targeted(&state, "paste");
                request["text"] = json!(paste);
                backend.send(request)?;
            }
            Event::Key(key) => {
                let down = key.kind != KeyEventKind::Release;
                let local = key.modifiers.is_empty();
                match key.code {
                    KeyCode::F(2 | 5 | 8 | 9) if local => {
                        if key.kind == KeyEventKind::Press {
                            match create_workspace_item(&state, key.code) {
                                Ok(request) => {
                                    backend.send(request)?;
                                    problem.clear();
                                }
                                Err(message) => problem = message.into(),
                            }
                            screen.dirty = true;
                        }
                    }
                    KeyCode::F(10) if local => {
                        if down {
                            return Ok(());
                        }
                    }
                    KeyCode::F(6) if local => {
                        if down && (!screen.sidebar || state["pane"].is_string()) {
                            screen.sidebar = !screen.sidebar;
                            problem.clear();
                            screen.dirty = true;
                            last_claim = None;
                        }
                    }
                    KeyCode::F(3 | 4 | 7) if local => {
                        if down && let Some(request) = screen.cycle(&state, key.code) {
                            backend.send(request)?;
                        }
                    }
                    _ if screen.sidebar => {
                        if down && let Some(request) = screen.navigate(&state, key.code) {
                            backend.send(request)?;
                        }
                    }
                    _ if state["terminal"]["connected"] == true
                        && state["hosts"]["pending"] != true =>
                    {
                        if let Some(payload) = terminal_key(key) {
                            let mut request = targeted(&state, "key");
                            request["payload"] = payload;
                            backend.send(request)?;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let executable = args
        .next()
        .map(PathBuf::from)
        .context("usage: rex-tui BACKEND PROFILE")?;
    let profile = args
        .next()
        .map(PathBuf::from)
        .context("usage: rex-tui BACKEND PROFILE")?;
    let mut backend = Backend::start(executable, profile)?;
    let mut terminal = ratatui::try_init()?;
    let result = crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste)
        .map_err(Into::into)
        .and_then(|()| run(&mut terminal, &mut backend));
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_uses_the_connected_host_without_a_session() {
        let state = json!({"host": "local", "epoch": 7, "connected": true});
        let request = create_workspace_item(&state, KeyCode::F(2)).unwrap();
        assert_eq!(
            request,
            json!({
                "op": "workspace", "action": "session.new", "args": {},
                "host": "local", "epoch": 7, "session": null, "window": null, "pane": null
            })
        );
        assert!(create_workspace_item(&state, KeyCode::F(5)).is_err());
        assert!(create_workspace_item(&state, KeyCode::F(8)).is_err());
    }

    #[test]
    fn splits_target_the_selected_pane_and_wait_for_navigation() {
        let mut state = json!({
            "host": "local", "epoch": 7, "connected": true,
            "session": "session", "window": "window", "pane": "shell"
        });
        let request = create_workspace_item(&state, KeyCode::F(9)).unwrap();
        assert_eq!(request["action"], "pane.split");
        assert_eq!(request["pane"], "shell");
        assert_eq!(request["args"]["direction"], "down");
        state["hosts"] = json!({"pending": true});
        assert!(create_workspace_item(&state, KeyCode::F(9)).is_err());
    }
}
