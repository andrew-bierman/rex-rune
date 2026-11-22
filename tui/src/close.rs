use crate::{items, targeted, text};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Clear, Paragraph, Wrap},
};
use serde_json::{Value, json};

const TARGET_FIELDS: [&str; 5] = ["host", "epoch", "session", "window", "pane"];

pub(crate) enum CloseTarget {
    Session,
    Tab,
    FocusedPane,
}

pub(crate) struct CloseDialog {
    target: Value,
    title: Option<String>,
    request_id: u64,
    previous_question: Value,
    cancel_requested: bool,
    submitted: bool,
}

impl CloseDialog {
    pub(crate) fn start(
        state: &Value,
        kind: CloseTarget,
        request_id: u64,
    ) -> Result<(Self, Value), &'static str> {
        if state["hosts"]["pending"] == true || state["connected"] != true {
            return Err("Open a connected workspace first");
        }
        let (action, label_source, field, missing) = match kind {
            CloseTarget::Session => (
                "session.close",
                Some(("session", "sessions")),
                "session",
                "Open the session you want to close",
            ),
            CloseTarget::Tab => (
                "client.tab.close",
                Some(("tab", "windows")),
                "window",
                "Open the tab you want to close",
            ),
            // Rex chooses whether closing the focused pane means closing its tab.
            CloseTarget::FocusedPane => (
                "client.close",
                None,
                "pane",
                "Focus the pane you want to close",
            ),
        };
        let id = state[field].as_str().ok_or(missing)?;
        let title = label_source.map(|(kind, rows)| {
            let name = items(&state[rows])
                .iter()
                .find(|row| row["id"] == id)
                .map(|row| text(row, "label"))
                .filter(|label| !label.trim().is_empty())
                .unwrap_or(id);
            format!("Close {kind} {name}?")
        });
        let target = targeted(state, "workspace");
        let mut request = target.clone();
        request["action"] = json!(action);
        request["args"] = json!({});
        request["request_id"] = json!(request_id);
        Ok((
            Self {
                target,
                title,
                request_id,
                previous_question: state["workspace"]["id"].clone(),
                cancel_requested: false,
                submitted: false,
            },
            request,
        ))
    }

    fn question<'a>(&self, state: &'a Value) -> Option<(&'a Value, u64)> {
        let question = &state["workspace"];
        let id = question["id"].as_u64()?;
        (question["id"] != self.previous_question
            && TARGET_FIELDS
                .iter()
                .all(|field| question[*field] == self.target[*field]))
        .then_some((question, id))
    }

    pub(crate) fn active(&self, state: &Value, rejected_request: Option<u64>) -> bool {
        rejected_request != Some(self.request_id)
            && state["connected"] == true
            && state["hosts"]["pending"] != true
            && TARGET_FIELDS
                .iter()
                .all(|field| state[*field] == self.target[*field])
            && (!self.submitted || self.question(state).is_some())
    }

    pub(crate) fn key(
        &mut self,
        state: &Value,
        key: KeyEvent,
        popup_visible: bool,
    ) -> Option<Value> {
        if key.kind != KeyEventKind::Press || self.submitted || !self.active(state, None) {
            return None;
        }
        if key.modifiers.is_empty() && matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            self.cancel_requested = true;
            return self.cancel(state);
        }
        let accept = matches!(key.code, KeyCode::Char('y' | 'Y'))
            && key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
            && popup_visible
            && !self.cancel_requested;
        if accept {
            let (_, id) = self.question(state)?;
            return Some(self.reply(id, "confirm.accept"));
        }
        None
    }

    pub(crate) fn cancel(&mut self, state: &Value) -> Option<Value> {
        if !self.cancel_requested || self.submitted || !self.active(state, None) {
            return None;
        }
        let (_, id) = self.question(state)?;
        Some(self.reply(id, "confirm.cancel"))
    }

    fn reply(&mut self, id: u64, action: &str) -> Value {
        self.submitted = true;
        let mut request = self.target.clone();
        request["action"] = json!(action);
        request["args"] = json!({"id": id});
        request
    }

    pub(crate) fn draw(&self, frame: &mut Frame<'_>, state: &Value) {
        let area = frame.area();
        let width = area.width.min(74);
        let height = area.height.min(9);
        let popup = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let safe_text =
            |source: &str| -> String { source.chars().filter(|ch| !ch.is_control()).collect() };
        let lines = if self.cancel_requested || self.submitted {
            vec![Line::raw("Finishing close request…")]
        } else if let Some((question, _)) = self.question(state) {
            vec![
                Line::styled(
                    safe_text(
                        self.title
                            .as_deref()
                            .unwrap_or_else(|| text(question, "title")),
                    ),
                    Style::new().bold(),
                ),
                Line::raw("Enter / Esc Cancel    Y Confirm"),
                Line::raw(""),
                Line::raw(safe_text(text(question, "description"))),
            ]
        } else {
            vec![Line::raw("Waiting for Rex…    Enter / Esc Cancel")]
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().border_style(Style::new().fg(Color::Yellow)))
                .wrap(Wrap { trim: true }),
            popup,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Value {
        json!({
            "host": "local", "epoch": 7, "connected": true,
            "session": "session", "window": "tab", "pane": "shell"
        })
    }

    fn question(state: &Value, id: u64) -> Value {
        let mut question = targeted(state, "workspace");
        question["id"] = json!(id);
        question["title"] = json!("Close Work?");
        question["description"] = json!("Ends programs on Local.");
        question
    }

    #[test]
    fn close_tab_requires_explicit_confirmation_of_the_captured_question() {
        let mut state = state();
        let (mut dialog, request) = CloseDialog::start(&state, CloseTarget::Tab, 21).unwrap();
        assert_eq!(request["action"], "client.tab.close");
        assert_eq!(request["window"], "tab");
        assert_eq!(request["request_id"], 21);
        let accept = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        assert!(dialog.key(&state, accept, true).is_none());
        state["workspace"] = question(&state, 6);
        assert!(dialog.key(&state, accept, false).is_none());
        let reply = dialog.key(&state, accept, true).unwrap();
        assert_eq!(reply["action"], "confirm.accept");
        assert_eq!(reply["args"]["id"], 6);
        assert_eq!(reply["host"], "local");
        assert_eq!(reply["epoch"], 7);
        assert!(dialog.key(&state, accept, true).is_none());
        state["workspace"] = Value::Null;
        assert!(!dialog.active(&state, None));
    }

    #[test]
    fn enter_cancels_even_before_the_backend_question_arrives() {
        let mut state = state();
        let (mut dialog, request) = CloseDialog::start(&state, CloseTarget::Session, 21).unwrap();
        assert_eq!(request["action"], "session.close");
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert!(dialog.key(&state, enter, true).is_none());
        state["workspace"] = question(&state, 6);
        let accept = KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT);
        assert!(dialog.key(&state, accept, true).is_none());
        assert_eq!(dialog.cancel(&state).unwrap()["action"], "confirm.cancel");
        assert!(dialog.cancel(&state).is_none());
    }

    #[test]
    fn stale_or_switched_targets_are_not_replied_to() {
        let mut state = state();
        state["workspace"] = question(&state, 6);
        let (mut dialog, _) = CloseDialog::start(&state, CloseTarget::Tab, 21).unwrap();
        let accept = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        assert!(dialog.key(&state, accept, true).is_none());
        state["workspace"]["id"] = json!(7);
        state["workspace"]["epoch"] = json!(8);
        assert!(dialog.key(&state, accept, true).is_none());
        state["workspace"]["epoch"] = json!(7);
        state["window"] = json!("other tab");
        assert!(dialog.key(&state, accept, true).is_none());
        assert!(!dialog.active(&state, None));
        assert!(!dialog.active(&state, Some(21)));
    }

    #[test]
    fn pane_close_shows_the_backend_scope_and_requires_confirmation() {
        use ratatui::{Terminal, backend::TestBackend};

        for title in ["Close split shell in Work?", "Close Work?"] {
            let mut state = state();
            let (mut dialog, request) =
                CloseDialog::start(&state, CloseTarget::FocusedPane, 21).unwrap();
            assert_eq!(request["action"], "client.close");
            assert_eq!(request["pane"], "shell");
            assert_eq!(request["window"], "tab");
            assert_eq!(request["request_id"], 21);

            let accept = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
            assert!(dialog.key(&state, accept, true).is_none());
            state["workspace"] = question(&state, 6);
            state["workspace"]["title"] = json!(title);
            let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
            terminal.draw(|frame| dialog.draw(frame, &state)).unwrap();
            let screen: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(screen.contains(title));

            let mut changed = state.clone();
            changed["pane"] = json!("other pane");
            assert!(dialog.key(&changed, accept, true).is_none());
            assert!(!dialog.active(&changed, None));
            let reply = dialog.key(&state, accept, true).unwrap();
            assert_eq!(reply["action"], "confirm.accept");
            assert_eq!(reply["pane"], "shell");
            assert_eq!(reply["args"]["id"], 6);
        }
    }

    #[test]
    fn close_requires_the_active_target() {
        let mut state = state();
        state["pane"] = Value::Null;
        assert!(CloseDialog::start(&state, CloseTarget::FocusedPane, 21).is_err());
        state["window"] = Value::Null;
        assert!(CloseDialog::start(&state, CloseTarget::Tab, 21).is_err());
        assert!(CloseDialog::start(&state, CloseTarget::Session, 21).is_ok());
        state["hosts"]["pending"] = json!(true);
        assert!(CloseDialog::start(&state, CloseTarget::Session, 21).is_err());
    }
}
