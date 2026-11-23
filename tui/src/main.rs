mod attention;
mod attention_notifications;
mod close;
mod input_prompt;
mod mouse;
mod selection;
mod settings;
mod ui;

use anyhow::{Context, Result, bail};
use close::{CloseDialog, CloseTarget};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use input_prompt::{Edit, InputPrompt, RenameTarget};
use mouse::{Choice, Focus, FocusStatus, MenuAction, Pointer, PointerAction};
use serde_json::{Value, json};
use settings::{Action, PaneDefaultColors, Settings, StartupFocus};
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct Inbox {
    state: Option<Value>,
    problem: Option<String>,
    rejected_request: Option<(u64, String)>,
    closed: bool,
}

struct Backend {
    child: Child,
    input: ChildStdin,
    inbox: Arc<Mutex<Inbox>>,
}

impl Backend {
    fn start(executable: PathBuf, profile: PathBuf, ansi_palette: bool) -> Result<Self> {
        let mut child = Command::new(&executable)
            .env("REX_BACKEND_COLOR_ROLES", "1")
            .env(
                "REX_BACKEND_ANSI_ROLES",
                if ansi_palette { "1" } else { "0" },
            )
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
                    Some("error") => {
                        let message = text(&value, "message").to_owned();
                        if let Some(request_id) = value["request_id"].as_u64() {
                            slot.rejected_request = Some((request_id, message.clone()));
                        }
                        slot.problem = Some(message);
                    }
                    Some("notice") => slot.problem = Some(text(&value, "message").into()),
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

fn create_workspace_item(state: &Value, action: Action) -> Result<Value, &'static str> {
    if state["hosts"]["pending"] == true {
        return Err("Wait for the workspace to open");
    }
    if state["connected"] != true {
        return Err("Connect to a host first");
    }
    let (action, args) = match action {
        Action::NewSession => ("session.new", json!({})),
        Action::NewTab if state["session"].is_string() => ("client.tab.new", json!({})),
        Action::NewTab => return Err("Open a session before creating a tab"),
        Action::SplitRight if state["pane"].is_string() => {
            ("pane.split", json!({"direction": "right"}))
        }
        Action::SplitDown if state["pane"].is_string() => {
            ("pane.split", json!({"direction": "down"}))
        }
        _ => return Err("Select a shell pane to split"),
    };
    let mut request = targeted(state, "workspace");
    request["action"] = json!(action);
    request["args"] = args;
    Ok(request)
}

fn move_active_tab(state: &Value, action: Action) -> Option<Value> {
    if state["connected"] != true
        || state["hosts"]["pending"] == true
        || !state["session"].is_string()
        || !state["window"].is_string()
    {
        return None;
    }
    let tabs = items(&state["windows"]);
    let active = tabs.iter().position(|tab| tab["id"] == state["window"])?;
    let direction = match action {
        Action::MoveTabLeft if active > 0 => "left",
        Action::MoveTabRight if active + 1 < tabs.len() => "right",
        _ => return None,
    };
    let mut request = targeted(state, "workspace");
    request["action"] = json!("client.tab.move");
    request["args"] = json!({"direction": direction});
    Some(request)
}

fn scroll_active_pane(state: &Value, action: Action) -> Option<Value> {
    if state["connected"] != true
        || state["hosts"]["pending"] == true
        || state["terminal"]["connected"] != true
        || !state["pane"].is_string()
    {
        return None;
    }
    let mut request = targeted(state, "scroll");
    request["lines"] = json!(if action == Action::ScrollUp { -3 } else { 3 });
    Some(request)
}

fn resize_split(state: &Value, action: Action, kind: KeyEventKind) -> Option<Value> {
    if kind == KeyEventKind::Release
        || state["connected"] != true
        || state["hosts"]["pending"] == true
        || !state["pane"].is_string()
        || items(&state["dividers"]).is_empty()
    {
        return None;
    }
    let direction = match action {
        Action::ResizeLeft => "left",
        Action::ResizeRight => "right",
        Action::ResizeUp => "up",
        Action::ResizeDown => "down",
        _ => return None,
    };
    let mut request = targeted(state, "workspace");
    request["action"] = json!("pane.resize");
    request["args"] = json!({"direction": direction, "amount": 5});
    Some(request)
}

fn consume_modal_key(keys: &mut HashSet<KeyCode>, key: KeyEvent, modal: bool) -> bool {
    match key.kind {
        KeyEventKind::Press | KeyEventKind::Repeat if modal => {
            keys.insert(key.code);
            true
        }
        KeyEventKind::Press => {
            keys.remove(&key.code);
            false
        }
        KeyEventKind::Repeat => keys.contains(&key.code),
        KeyEventKind::Release => keys.remove(&key.code) || modal,
    }
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

fn edit_settings(terminal: &mut ratatui::DefaultTerminal, path: &Path) -> Result<Result<()>> {
    use crossterm::{
        cursor::Show,
        event::{DisableBracketedPaste, EnableBracketedPaste},
        execute,
        terminal::{EnterAlternateScreen, enable_raw_mode},
    };

    let mut editor = match settings::editor(path) {
        Ok(editor) => editor,
        Err(error) => return Ok(Err(error)),
    };
    execute!(std::io::stdout(), DisableBracketedPaste, Show)?;
    ratatui::try_restore()?;
    let result = editor
        .status()
        .context("Run the settings editor")
        .and_then(|status| {
            anyhow::ensure!(status.success(), "Settings editor exited with {status}");
            Ok(())
        });

    enable_raw_mode()?;
    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste
    )?;
    terminal.clear()?;
    Ok(result)
}

fn reload_settings(screen: &mut ui::Screen, path: &Path) -> String {
    match Settings::load(path) {
        Ok(settings) => {
            screen.settings = settings;
            String::new()
        }
        Err(error) => format!("{error:#}"),
    }
}

fn focus_pane(
    state: &Value,
    screen: &mut ui::Screen,
    backend: &mut Backend,
    request_id: &mut u64,
    target: mouse::Target,
) -> Result<Option<Focus>> {
    if target.selected(state) {
        screen.sidebar = false;
        return Ok(None);
    }
    *request_id = request_id.wrapping_add(1);
    let (focus, request) = Focus::start(target, *request_id);
    backend.send(request)?;
    Ok(Some(focus))
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    backend: &mut Backend,
    settings_path: &Path,
    settings: Settings,
) -> Result<()> {
    let mut focus_terminal_on_attach = matches!(settings.startup_focus, StartupFocus::Terminal);
    let mut screen = ui::Screen::default();
    screen.ansi_palette_started = settings.panes.ansi_palette == PaneDefaultColors::Terminal;
    screen.settings = settings;
    let mut state = Value::Null;
    let mut problem = String::new();
    let mut sizes = HashMap::new();
    let mut resize_scope: Option<(String, Option<u64>, String, String)> = None;
    let mut last_claim = None;
    let mut close_dialog: Option<CloseDialog> = None;
    let mut request_id = 0u64;
    let mut modal_keys = HashSet::new();
    let mut mouse_capture = selection::MouseCapture::default();
    let mut pointer = Pointer::default();
    let mut pending_focus: Option<Focus> = None;
    let foot = std::env::var("TERM")
        .is_ok_and(|term| matches!(term.as_str(), "foot" | "foot-extra" | "foot-direct"));
    let mut notifications = attention_notifications::Notifications::default();
    let mut notifications_enabled = false;
    loop {
        mouse_capture.set(screen.settings.mouse_selection)?;
        let (update, message, rejection, closed) = {
            let mut inbox = backend.inbox.lock().unwrap();
            (
                inbox.state.take(),
                inbox.problem.take(),
                inbox.rejected_request.take(),
                inbox.closed,
            )
        };
        let updated = update.is_some();
        let changed = updated || message.is_some();
        if let Some(update) = update {
            state = update;
            if let Some(prompt) = &mut screen.prompt {
                prompt.observe(&state);
            }
            if focus_terminal_on_attach && state["terminal"]["connected"] == true {
                screen.sidebar = false;
                focus_terminal_on_attach = false;
            }
        }
        if let Some(message) = message {
            problem = message;
        }
        if let (Some((id, message)), Some(prompt)) = (&rejection, &mut screen.prompt) {
            prompt.reject(*id, message);
        }
        let rejected_request = rejection.map(|(id, _)| id);
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
        let enabled = foot && screen.settings.attention_notifications;
        let notification_setting_changed = enabled != notifications_enabled;
        if notification_setting_changed {
            notifications.clear();
            notifications_enabled = enabled;
        }
        if enabled && (updated || notification_setting_changed) {
            let _ = notifications.observe(&state, &mut std::io::stdout());
        }
        if let Some(focus) = &pending_focus {
            match focus.status(&state, rejected_request) {
                FocusStatus::Pending => {}
                FocusStatus::Selected => {
                    pending_focus = None;
                    screen.sidebar = false;
                    problem.clear();
                    screen.dirty = true;
                }
                FocusStatus::Rejected => {
                    pending_focus = None;
                    problem = "The pane could not be focused".into();
                    screen.dirty = true;
                }
            }
        }
        let pointer_enabled = screen.settings.mouse_actions
            && !screen.settings_open
            && screen.prompt.is_none()
            && close_dialog.is_none();
        if !pointer_enabled {
            pointer.cancel();
            screen.cancel_sidebar_mouse();
        }
        if close_dialog
            .as_ref()
            .is_some_and(|dialog| !dialog.active(&state, rejected_request))
        {
            close_dialog = None;
            screen.dirty = true;
        }
        if let Some(request) = close_dialog
            .as_mut()
            .and_then(|dialog| dialog.cancel(&state))
        {
            backend.send(request)?;
            screen.dirty = true;
        }
        if changed || screen.dirty {
            terminal.draw(|frame| {
                screen.draw(frame, &state, &problem, settings_path);
                if let Some(dialog) = &close_dialog {
                    dialog.draw(frame, &state);
                }
            })?;
            screen.dirty = false;
        }
        if screen.pane_menu.as_ref().is_some_and(|menu| {
            !pointer_enabled || !menu.target.visible(&state, &screen.pane_grids)
        }) {
            screen.pane_menu = None;
            screen.dirty = true;
        }
        let selection_enabled = screen.settings.mouse_selection
            && !screen.settings_open
            && screen.prompt.is_none()
            && close_dialog.is_none();
        if let Some(request) =
            screen
                .selection
                .reconcile(&state, &screen.pane_grids, selection_enabled)
        {
            backend.send(request)?;
            screen.dirty = true;
        }
        if let Some(result) = screen.selection.take_copy(&state) {
            problem = match result {
                Ok(text) => {
                    selection::write_clipboard(&mut std::io::stdout(), text)?;
                    String::new()
                }
                Err(message) => message.into(),
            };
            screen.dirty = true;
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
                && !screen.settings_open
                && screen.prompt.is_none()
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
        let input = event::read()?;
        if let Some(prompt) = &mut screen.prompt {
            match input {
                Event::Key(key) => {
                    consume_modal_key(&mut modal_keys, key, true);
                    // A new target snapshot can arrive while waiting for input.
                    if let Some(update) = backend.inbox.lock().unwrap().state.as_ref() {
                        prompt.observe(update);
                    }
                    match prompt.key(&state, key, InputPrompt::visible(terminal.size()?)) {
                        Edit::Continue => {}
                        Edit::Cancel => screen.prompt = None,
                        // Keep the prompt open so repeated Enter presses stay out of the shell.
                        Edit::Send(request) => backend.send(request)?,
                    }
                }
                Event::Paste(paste) => prompt.insert(&paste),
                _ => {}
            }
            screen.dirty = true;
            continue;
        }
        if pending_focus.is_some() {
            if let Event::Key(key) = input {
                consume_modal_key(&mut modal_keys, key, true);
            }
            if matches!(input, Event::Resize(..)) {
                screen.dirty = true;
            }
            continue;
        }
        if let Some(menu) = screen.pane_menu.as_mut() {
            let focused = menu.target.selected(&state);
            let can_copy = screen.selection.can_copy(menu.target.id());
            let choice = match input {
                Event::Key(key) => {
                    consume_modal_key(&mut modal_keys, key, true);
                    menu.key(key, focused, can_copy)
                }
                Event::Mouse(mouse) => {
                    let size = terminal.size()?;
                    menu.mouse(
                        mouse,
                        ratatui::layout::Rect::new(0, 0, size.width, size.height),
                        focused,
                        can_copy,
                    )
                }
                _ => MenuAction::Keep,
            };
            match choice {
                MenuAction::Keep => {}
                MenuAction::Dismiss => screen.pane_menu = None,
                MenuAction::Choose(choice) => {
                    if let Some(menu) = screen.pane_menu.take() {
                        match choice {
                            Choice::Focus => {
                                pending_focus = focus_pane(
                                    &state,
                                    &mut screen,
                                    backend,
                                    &mut request_id,
                                    menu.target,
                                )?;
                            }
                            Choice::Copy => {
                                problem = screen
                                    .selection
                                    .request_copy()
                                    .err()
                                    .unwrap_or_default()
                                    .into();
                            }
                            Choice::SplitRight | Choice::SplitDown => {
                                let action = if choice == Choice::SplitRight {
                                    Action::SplitRight
                                } else {
                                    Action::SplitDown
                                };
                                if let Ok(request) = create_workspace_item(&state, action) {
                                    backend.send(request)?;
                                }
                            }
                        }
                    }
                }
            }
            screen.dirty = true;
            continue;
        }
        if let Event::Key(key) = &input {
            let swallowed = consume_modal_key(&mut modal_keys, *key, close_dialog.is_some());
            if let Some(dialog) = &mut close_dialog {
                let size = terminal.size()?;
                let popup_visible = size.width >= 38 && size.height >= 8;
                if let Some(request) = dialog.key(&state, *key, popup_visible) {
                    backend.send(request)?;
                }
                screen.dirty = true;
            }
            if swallowed {
                continue;
            }
        }
        match input {
            Event::Resize(..) => screen.dirty = true,
            Event::Mouse(event) if selection_enabled => {
                if pointer_enabled
                    && let ui::SidebarMouse::Handled(request) = screen.sidebar_mouse(&state, event)
                {
                    if matches!(
                        event.kind,
                        event::MouseEventKind::Down(event::MouseButton::Left)
                    ) {
                        pointer.cancel();
                        focus_terminal_on_attach = false;
                    }
                    if let Some(request) = request {
                        backend.send(request)?;
                    }
                    continue;
                }
                if let Some(request) = mouse::wheel(&state, &screen.pane_grids, event) {
                    backend.send(request)?;
                    continue;
                }
                if !pointer_enabled {
                    if let Some(request) = screen.selection.mouse(&state, &screen.pane_grids, event)
                    {
                        backend.send(request)?;
                        screen.dirty = true;
                    }
                    continue;
                }
                match pointer.event(&state, &screen.pane_grids, event) {
                    PointerAction::None => {}
                    PointerAction::Selection(event) => {
                        if selection_enabled
                            && let Some(request) =
                                screen.selection.mouse(&state, &screen.pane_grids, event)
                        {
                            backend.send(request)?;
                        }
                    }
                    PointerAction::BeginDrag(down, drag) => {
                        if selection_enabled {
                            for event in [down, drag] {
                                if let Some(request) =
                                    screen.selection.mouse(&state, &screen.pane_grids, event)
                                {
                                    backend.send(request)?;
                                }
                            }
                        }
                    }
                    PointerAction::Focus(target) => {
                        if let Some(request) =
                            screen
                                .selection
                                .reconcile(&state, &screen.pane_grids, false)
                        {
                            backend.send(request)?;
                        }
                        pending_focus =
                            focus_pane(&state, &mut screen, backend, &mut request_id, target)?;
                    }
                    PointerAction::Menu(menu) => screen.pane_menu = Some(menu),
                }
                screen.dirty = true;
            }
            Event::Paste(paste)
                if close_dialog.is_none()
                    && !screen.sidebar
                    && !screen.settings_open
                    && state["terminal"]["connected"] == true
                    && state["hosts"]["pending"] != true =>
            {
                let mut request = targeted(&state, "paste");
                request["text"] = json!(paste);
                backend.send(request)?;
            }
            Event::Paste(_) if close_dialog.is_none() && !screen.settings_open => {
                problem = if screen.sidebar {
                    match screen.settings.shortcut(Action::ToggleFocus) {
                        Some(key) => format!("Press {key} to focus the shell before pasting"),
                        None => "Enable the focus shortcut in Settings before pasting".into(),
                    }
                } else {
                    "Wait for the shell to connect before pasting".into()
                };
                screen.dirty = true;
            }
            Event::Key(key) => {
                let down = key.kind != KeyEventKind::Release;
                let action = screen.settings.action(key);
                if screen.settings_open {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match (key.code, key.modifiers) {
                        (KeyCode::Esc, KeyModifiers::NONE) => screen.settings_open = false,
                        (KeyCode::Char('e'), KeyModifiers::NONE) => {
                            mouse_capture.set(false)?;
                            problem = match edit_settings(terminal, settings_path)? {
                                Ok(()) => reload_settings(&mut screen, settings_path),
                                Err(error) => format!("{error:#}"),
                            };
                        }
                        (KeyCode::Char('r'), KeyModifiers::NONE) => {
                            problem = reload_settings(&mut screen, settings_path);
                        }
                        _ if action == Some(Action::Settings) => screen.settings_open = false,
                        _ => {}
                    }
                    screen.dirty = true;
                    continue;
                }
                if down {
                    focus_terminal_on_attach = false;
                }
                match action {
                    Some(
                        action @ (Action::RenameSession | Action::RenameTab | Action::RenamePane),
                    ) => {
                        if key.kind == KeyEventKind::Press {
                            request_id = request_id.wrapping_add(1);
                            let kind = match action {
                                Action::RenameSession => RenameTarget::Session,
                                Action::RenameTab => RenameTarget::Tab,
                                _ => RenameTarget::Pane,
                            };
                            match InputPrompt::rename(&state, kind, request_id) {
                                Ok(prompt) => {
                                    screen.prompt = Some(prompt);
                                    modal_keys.insert(key.code);
                                    problem.clear();
                                }
                                Err(message) => problem = message.into(),
                            }
                            screen.dirty = true;
                        }
                    }
                    Some(action @ (Action::CloseSession | Action::CloseTab)) => {
                        if key.kind == KeyEventKind::Press {
                            request_id = request_id.wrapping_add(1);
                            let kind = if action == Action::CloseSession {
                                CloseTarget::Session
                            } else {
                                CloseTarget::Tab
                            };
                            match CloseDialog::start(&state, kind, request_id) {
                                Ok((dialog, request)) => {
                                    backend.send(request)?;
                                    close_dialog = Some(dialog);
                                    problem.clear();
                                }
                                Err(message) => problem = message.into(),
                            }
                            screen.dirty = true;
                        }
                    }
                    Some(
                        action @ (Action::ResizeLeft
                        | Action::ResizeRight
                        | Action::ResizeUp
                        | Action::ResizeDown),
                    ) => {
                        if let Some(request) = resize_split(&state, action, key.kind) {
                            backend.send(request)?;
                        }
                    }
                    Some(
                        action @ (Action::FocusLeft
                        | Action::FocusRight
                        | Action::FocusUp
                        | Action::FocusDown),
                    ) => {
                        if key.kind == KeyEventKind::Press
                            && let Some(target) = screen.directional_pane(&state, action)
                        {
                            pending_focus =
                                focus_pane(&state, &mut screen, backend, &mut request_id, target)?;
                            screen.dirty = true;
                        }
                    }
                    Some(action @ (Action::MoveTabLeft | Action::MoveTabRight)) => {
                        if key.kind == KeyEventKind::Press
                            && let Some(request) = move_active_tab(&state, action)
                        {
                            backend.send(request)?;
                        }
                    }
                    Some(Action::NextAttention) => {
                        if key.kind == KeyEventKind::Press && state["hosts"]["pending"] != true {
                            let attention = attention::Attention::from_state(&state);
                            if let Some(request) = attention.next(&state) {
                                backend.send(request)?;
                                problem.clear();
                            } else {
                                problem = if !attention.is_empty() {
                                    "You’re on the only tab needing attention"
                                } else if state["activity"]["reading"] == true {
                                    "Still checking Rex activity"
                                } else if state["activity"]["incomplete"] == true {
                                    "No reported attention. Some Rex activity is unavailable."
                                } else {
                                    "No tabs report needing attention"
                                }
                                .into();
                            }
                            screen.dirty = true;
                        }
                    }
                    Some(action @ (Action::ScrollUp | Action::ScrollDown)) => {
                        if down && let Some(request) = scroll_active_pane(&state, action) {
                            backend.send(request)?;
                        }
                    }
                    Some(Action::CopySelection) => {
                        if key.kind == KeyEventKind::Press {
                            problem = screen
                                .selection
                                .request_copy()
                                .err()
                                .unwrap_or_default()
                                .into();
                            screen.dirty = true;
                        }
                    }
                    Some(Action::ComposeInput) => {
                        if key.kind == KeyEventKind::Press {
                            match InputPrompt::compose(&state) {
                                Ok(prompt) => {
                                    screen.prompt = Some(prompt);
                                    modal_keys.insert(key.code);
                                    problem.clear();
                                }
                                Err(message) => problem = message.into(),
                            }
                            screen.dirty = true;
                        }
                    }
                    Some(Action::Settings) => {
                        if key.kind == KeyEventKind::Press {
                            screen.settings_open = true;
                            screen.dirty = true;
                        }
                    }
                    Some(
                        action @ (Action::NewSession
                        | Action::NewTab
                        | Action::SplitRight
                        | Action::SplitDown),
                    ) => {
                        if key.kind == KeyEventKind::Press {
                            match create_workspace_item(&state, action) {
                                Ok(request) => {
                                    backend.send(request)?;
                                    problem.clear();
                                }
                                Err(message) => problem = message.into(),
                            }
                            screen.dirty = true;
                        }
                    }
                    Some(Action::Quit) => {
                        if down {
                            return Ok(());
                        }
                    }
                    Some(Action::ToggleFocus) => {
                        if key.kind == KeyEventKind::Press
                            && (!screen.sidebar || state["pane"].is_string())
                        {
                            screen.sidebar = !screen.sidebar;
                            if screen.sidebar {
                                screen.follow_active_row();
                            }
                            problem.clear();
                            screen.dirty = true;
                            last_claim = None;
                        }
                    }
                    Some(action @ (Action::PreviousTab | Action::NextTab | Action::NextPane)) => {
                        if down && let Some(request) = screen.cycle(&state, action) {
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
    let first = args
        .next()
        .context("usage: rex-tui BACKEND PROFILE [--settings PATH]")?;
    if first == "--print-default-settings" {
        anyhow::ensure!(args.next().is_none(), "Unexpected argument");
        print!("{}", settings::DEFAULT_FILE);
        return Ok(());
    }
    let check = first == "--check-settings";
    let profile = if check {
        None
    } else {
        Some(PathBuf::from(
            args.next().context("Expected a workspace profile")?,
        ))
    };
    let explicit_path = match args.next() {
        Some(flag) if flag == "--settings" => Some(PathBuf::from(
            args.next().context("Expected --settings PATH")?,
        )),
        Some(_) => bail!("Expected --settings PATH"),
        None => None,
    };
    anyhow::ensure!(args.next().is_none(), "Unexpected argument");
    let settings_path = match explicit_path {
        None => settings::default_path()?,
        Some(path) => {
            if check {
                anyhow::ensure!(
                    path.try_exists()?,
                    "Settings file does not exist: {}",
                    path.display()
                );
            }
            path
        }
    };
    let settings = Settings::load(&settings_path)?;
    if let Some(profile) = profile {
        let mut backend = Backend::start(
            PathBuf::from(first),
            profile,
            settings.panes.ansi_palette == PaneDefaultColors::Terminal,
        )?;
        let mut terminal = ratatui::try_init()?;
        let result = crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste)
            .map_err(Into::into)
            .and_then(|()| run(&mut terminal, &mut backend, &settings_path, settings));
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
        ratatui::restore();
        result
    } else {
        println!("Settings valid: {}", settings_path.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_reordering_targets_the_active_tab_and_stops_at_the_boundaries() {
        let mut state = json!({
            "connected":true,"host":"h","epoch":7,"session":"s","window":"b","pane":"p",
            "windows":[{"id":"a"},{"id":"b"},{"id":"c"}]
        });
        let request = move_active_tab(&state, Action::MoveTabLeft).unwrap();
        assert_eq!(
            request,
            json!({
                "op":"workspace","host":"h","epoch":7,"session":"s","window":"b","pane":"p",
                "action":"client.tab.move","args":{"direction":"left"}
            })
        );
        assert_eq!(
            move_active_tab(&state, Action::MoveTabRight).unwrap()["args"]["direction"],
            "right"
        );
        state["window"] = json!("a");
        assert!(move_active_tab(&state, Action::MoveTabLeft).is_none());
        state["window"] = json!("c");
        assert!(move_active_tab(&state, Action::MoveTabRight).is_none());
        state["window"] = json!("missing");
        assert!(move_active_tab(&state, Action::MoveTabLeft).is_none());
        state["window"] = json!("b");
        state["hosts"]["pending"] = json!(true);
        assert!(move_active_tab(&state, Action::MoveTabLeft).is_none());
    }

    #[test]
    fn keyboard_scrollback_uses_local_scroll_on_the_active_pane() {
        let mut state = json!({"host":"host","epoch":7,"session":"s","window":"w","pane":"p",
            "connected":true,"terminal":{"connected":true}});
        let request = scroll_active_pane(&state, Action::ScrollUp).unwrap();
        assert_eq!(
            request,
            json!({"op":"scroll","host":"host","epoch":7,"session":"s",
            "window":"w","pane":"p","lines":-3})
        );
        assert_eq!(
            scroll_active_pane(&state, Action::ScrollDown).unwrap()["lines"],
            3
        );
        state["terminal"]["connected"] = json!(false);
        assert!(scroll_active_pane(&state, Action::ScrollUp).is_none());
    }

    #[test]
    fn reload_applies_settings_together_and_preserves_last_good_on_error() {
        let fixture = settings::tests::Fixture::new();
        let path = fixture.0.join("settings.toml");
        let mut screen = ui::Screen::default();
        std::fs::write(&path, "[sidebar]\nwidth = 34\n[keys]\nquit = \"F12\"").unwrap();
        assert!(reload_settings(&mut screen, &path).is_empty());
        assert_eq!(screen.settings.sidebar.width, Some(34));
        assert_eq!(screen.settings.shortcut(Action::Quit), Some("F12"));

        std::fs::write(&path, "[sidebar]\nwidth = 40\n[keys]\nquit = \"F1\"").unwrap();
        assert!(reload_settings(&mut screen, &path).contains("Duplicate shortcut"));
        assert_eq!(screen.settings.sidebar.width, Some(34));
        assert_eq!(screen.settings.shortcut(Action::Quit), Some("F12"));
    }

    #[test]
    fn new_session_uses_the_connected_host_without_a_session() {
        let state = json!({"host": "local", "epoch": 7, "connected": true});
        let request = create_workspace_item(&state, Action::NewSession).unwrap();
        assert_eq!(
            request,
            json!({
                "op": "workspace", "action": "session.new", "args": {},
                "host": "local", "epoch": 7, "session": null, "window": null, "pane": null
            })
        );
        assert!(create_workspace_item(&state, Action::NewTab).is_err());
        assert!(create_workspace_item(&state, Action::SplitRight).is_err());
    }

    #[test]
    fn splits_target_the_selected_pane_and_wait_for_navigation() {
        let mut state = json!({
            "host": "local", "epoch": 7, "connected": true,
            "session": "session", "window": "window", "pane": "shell"
        });
        let request = create_workspace_item(&state, Action::SplitDown).unwrap();
        assert_eq!(request["action"], "pane.split");
        assert_eq!(request["pane"], "shell");
        assert_eq!(request["args"]["direction"], "down");
        state["hosts"] = json!({"pending": true});
        assert!(create_workspace_item(&state, Action::SplitDown).is_err());
    }

    #[test]
    fn resize_uses_five_point_steps_in_each_direction() {
        let mut state = json!({
            "host": "local", "epoch": 7, "connected": true, "session": "session",
            "window": "window", "pane": "shell", "dividers": [{"ratio": 0.5}]
        });
        for (action, direction) in [
            (Action::ResizeLeft, "left"),
            (Action::ResizeRight, "right"),
            (Action::ResizeUp, "up"),
            (Action::ResizeDown, "down"),
        ] {
            let request = resize_split(&state, action, KeyEventKind::Press).unwrap();
            assert_eq!(request["action"], "pane.resize");
            assert_eq!(request["pane"], "shell");
            assert_eq!(
                request["args"],
                json!({"direction": direction, "amount": 5})
            );
        }
        let down = Action::ResizeDown;
        assert!(resize_split(&state, down, KeyEventKind::Repeat).is_some());
        assert!(resize_split(&state, down, KeyEventKind::Release).is_none());
        state["dividers"] = json!([]);
        assert!(resize_split(&state, down, KeyEventKind::Press).is_none());
        state["dividers"] = json!([{"ratio": 0.5}]);
        state["hosts"]["pending"] = json!(true);
        assert!(resize_split(&state, down, KeyEventKind::Press).is_none());
    }

    #[test]
    fn modal_press_and_its_trailing_events_do_not_reach_the_shell() {
        let mut keys = HashSet::new();
        let event = |kind| KeyEvent::new_with_kind(KeyCode::Char('y'), KeyModifiers::NONE, kind);
        assert!(consume_modal_key(
            &mut keys,
            event(KeyEventKind::Press),
            true
        ));
        assert!(consume_modal_key(
            &mut keys,
            event(KeyEventKind::Repeat),
            false
        ));
        assert!(consume_modal_key(
            &mut keys,
            event(KeyEventKind::Release),
            false
        ));
        assert!(!consume_modal_key(
            &mut keys,
            event(KeyEventKind::Press),
            false
        ));
    }
}
