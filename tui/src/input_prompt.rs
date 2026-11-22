use crate::{items, settings::Chrome, targeted, text};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Position, Rect, Size},
    style::Style,
    text::Line,
    widgets::{Block, Clear, Paragraph},
};
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const TARGET_FIELDS: [&str; 5] = ["host", "epoch", "session", "window", "pane"];

pub(crate) struct InputPrompt {
    target: Value,
    purpose: Purpose,
    label: String,
    draft: String,
    cursor: usize,
    stale: bool,
    sent: bool,
    error: Option<String>,
}

enum Purpose {
    Compose { generation: Value },
    Rename { kind: RenameTarget, request_id: u64 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenameTarget {
    Session,
    Tab,
    Pane,
}

impl RenameTarget {
    fn title(self) -> &'static str {
        match self {
            Self::Session => " Rename session ",
            Self::Tab => " Rename tab ",
            Self::Pane => " Rename pane ",
        }
    }
}

pub(crate) enum Edit {
    Continue,
    Cancel,
    Send(Value),
}

impl InputPrompt {
    pub(crate) fn compose(state: &Value) -> Result<Self, &'static str> {
        if !connected(state)
            || state["epoch"].as_u64().is_none()
            || ["host", "session", "window", "pane"]
                .iter()
                .any(|field| text(state, field).is_empty())
        {
            return Err("Select a connected shell pane to compose input");
        }
        let label = items(&state["panes"])
            .iter()
            .find(|pane| pane["pane"] == state["pane"])
            .map(|pane| text(pane, "label"))
            .filter(|label| !label.is_empty())
            .unwrap_or("Selected pane");
        Ok(Self {
            target: targeted(state, "paste"),
            purpose: Purpose::Compose {
                generation: state["terminal"]["generation"].clone(),
            },
            label: single_line(label),
            draft: String::new(),
            cursor: 0,
            stale: false,
            sent: false,
            error: None,
        })
    }

    pub(crate) fn rename(
        state: &Value,
        kind: RenameTarget,
        request_id: u64,
    ) -> Result<Self, &'static str> {
        if state["connected"] != true
            || state["hosts"]["pending"] == true
            || state["epoch"].as_u64().is_none()
            || text(state, "host").is_empty()
            || text(state, "session").is_empty()
        {
            return Err("Open a connected session before renaming");
        }
        let (action, rows, target_id, row_id, missing) = match kind {
            RenameTarget::Session => (
                "session.rename",
                "sessions",
                "session",
                "id",
                "No session is selected",
            ),
            RenameTarget::Tab => (
                "client.tab.rename",
                "windows",
                "window",
                "id",
                "No tab is selected",
            ),
            RenameTarget::Pane => ("pane.rename", "panes", "pane", "pane", "No pane is focused"),
        };
        let id = text(state, target_id);
        let row = items(&state[rows])
            .iter()
            .find(|row| !id.is_empty() && row[row_id] == id)
            .ok_or(missing)?;
        let label = single_line(text(row, "label"));
        let draft = if kind == RenameTarget::Session {
            label.clone()
        } else {
            single_line(text(row, "user_label"))
        };
        let mut target = targeted(state, "workspace");
        target["action"] = json!(action);
        target["request_id"] = json!(request_id);
        Ok(Self {
            target,
            purpose: Purpose::Rename { kind, request_id },
            label,
            cursor: draft.len(),
            draft,
            stale: false,
            sent: false,
            error: None,
        })
    }

    pub(crate) fn observe(&mut self, state: &Value) {
        let unavailable = match &self.purpose {
            Purpose::Compose { generation } => {
                !connected(state) || *generation != state["terminal"]["generation"]
            }
            Purpose::Rename { .. } => {
                state["connected"] != true || state["hosts"]["pending"] == true
            }
        };
        self.stale |= unavailable
            || TARGET_FIELDS
                .iter()
                .any(|field| self.target[*field] != state[*field]);
    }

    pub(crate) fn reject(&mut self, id: u64, message: &str) {
        if matches!(self.purpose, Purpose::Rename { request_id, .. } if request_id == id) {
            self.error = Some(single_line(message));
        }
    }

    pub(crate) fn insert(&mut self, input: &str) {
        if self.stale || self.sent {
            return;
        }
        let input = single_line(input);
        self.draft.insert_str(self.cursor, &input);
        // A combining character can join the grapheme on either side of the insertion.
        self.advance_to_boundary(self.cursor + input.len());
    }

    fn advance_to_boundary(&mut self, offset: usize) {
        self.cursor = self
            .draft
            .grapheme_indices(true)
            .find_map(|(index, _)| (index >= offset).then_some(index))
            .unwrap_or(self.draft.len());
    }

    fn previous(&self) -> usize {
        self.draft[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next(&self) -> usize {
        self.cursor
            + self.draft[self.cursor..]
                .graphemes(true)
                .next()
                .map_or(0, str::len)
    }

    pub(crate) fn key(&mut self, state: &Value, key: KeyEvent, visible: bool) -> Edit {
        self.observe(state);
        if key.kind == KeyEventKind::Release {
            return Edit::Continue;
        }
        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            return Edit::Cancel;
        }
        if self.stale || self.sent {
            return Edit::Continue;
        }
        match key.code {
            KeyCode::Enter
                if key.kind == KeyEventKind::Press && key.modifiers.is_empty() && visible =>
            {
                let mut request = self.target.clone();
                match self.purpose {
                    Purpose::Compose { .. } if self.draft.is_empty() => return Edit::Continue,
                    Purpose::Compose { .. } => {
                        request["text"] = json!(std::mem::take(&mut self.draft));
                        self.cursor = 0;
                    }
                    Purpose::Rename {
                        kind: RenameTarget::Session,
                        ..
                    } if self.draft.trim().is_empty() => {
                        self.error = Some("Enter a session name".into());
                        return Edit::Continue;
                    }
                    Purpose::Rename { .. } => {
                        request["args"] = json!({"label": self.draft.trim()});
                    }
                }
                self.error = None;
                self.sent = true;
                return Edit::Send(request);
            }
            KeyCode::Char('u')
                if key.modifiers == KeyModifiers::CONTROL
                    && matches!(self.purpose, Purpose::Rename { .. }) =>
            {
                self.draft.clear();
                self.cursor = 0;
                self.error = None;
            }
            KeyCode::Char(character)
                if key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
                    && !character.is_control() =>
            {
                self.insert(character.encode_utf8(&mut [0; 4]));
            }
            _ if !key.modifiers.is_empty() => {}
            KeyCode::Left => self.cursor = self.previous(),
            KeyCode::Right => self.cursor = self.next(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.draft.len(),
            KeyCode::Backspace => {
                let start = self.previous();
                self.draft.drain(start..self.cursor);
                self.advance_to_boundary(start);
            }
            KeyCode::Delete => {
                self.draft.drain(self.cursor..self.next());
                self.advance_to_boundary(self.cursor);
            }
            _ => {}
        }
        Edit::Continue
    }

    pub(crate) fn visible(area: Size) -> bool {
        area.width >= 38 && area.height >= 8
    }

    pub(crate) fn draw(&self, frame: &mut Frame<'_>, chrome: Chrome) {
        let area = frame.area();
        if !Self::visible(area.as_size()) {
            return;
        }
        let width = area.width.min(84);
        let height = 7;
        let bounds = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let (title, sent, stale, help) = match self.purpose {
            Purpose::Compose { .. } => (
                " Input composer · Experimental ",
                "Paste requested. Esc returns.",
                "Pane changed. Esc discards draft.",
                "Enter Paste only    Esc Discard",
            ),
            Purpose::Rename { kind, .. } => (
                kind.title(),
                "Rename requested. Esc returns.",
                "Workspace changed. Esc discards draft.",
                "Enter Rename    Ctrl+U Clear    Esc Cancel",
            ),
        };
        let block = Block::bordered()
            .title(title)
            .style(chrome.style())
            .border_style(Style::new().fg(chrome.accent));
        let inner = block.inner(bounds);
        let (draft, column) = self.viewport(usize::from(inner.width));
        frame.render_widget(Clear, bounds);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(self.label.as_str(), Style::new().fg(chrome.muted)),
                Line::raw(""),
                Line::raw(draft),
                Line::raw(""),
                if let Some(error) = &self.error {
                    Line::styled(error, Style::new().fg(chrome.warning))
                } else if self.sent {
                    Line::raw(sent)
                } else if self.stale {
                    Line::styled(stale, Style::new().fg(chrome.warning))
                } else {
                    Line::raw(help)
                },
            ])
            .block(block),
            bounds,
        );
        if !self.stale && !self.sent {
            frame.set_cursor_position(Position::new(inner.x + column as u16, inner.y + 2));
        }
    }

    fn viewport(&self, width: usize) -> (&str, usize) {
        let mut left = self.cursor;
        let mut column = 0;
        for (index, grapheme) in self.draft[..self.cursor].grapheme_indices(true).rev() {
            let next = column + grapheme.width();
            if next >= width {
                break;
            }
            left = index;
            column = next;
        }
        (&self.draft[left..], column)
    }
}

fn connected(state: &Value) -> bool {
    state["connected"] == true
        && state["terminal"]["connected"] == true
        && state["hosts"]["pending"] != true
}

fn single_line(input: &str) -> String {
    input
        .chars()
        .filter_map(|character| match character {
            '\r' | '\n' | '\t' | '\u{2028}' | '\u{2029}' => Some(' '),
            _ if character.is_control() => None,
            _ => Some(character),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Value {
        json!({
            "host": "h", "epoch": 9, "session": "s", "window": "w", "pane": "p",
            "connected": true, "terminal": {"connected": true, "generation": 1},
            "sessions": [{"id": "s", "label": "Original session"}],
            "windows": [{"id": "w", "label": "Current tab", "user_label": "Current tab"}],
            "panes": [{"pane": "p", "label": "Build shell", "user_label": null}]
        })
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn edits_extended_graphemes_and_scrolls_in_display_columns() {
        let state = state();
        let mut composer = InputPrompt::compose(&state).unwrap();
        composer.insert("e\u{301}🧑‍🚀界");
        assert_eq!(composer.viewport(4), ("界", 2));
        composer.key(&state, press(KeyCode::Left), true);
        composer.key(&state, press(KeyCode::Backspace), true);
        assert_eq!(composer.draft, "e\u{301}界");
        composer.key(&state, press(KeyCode::Home), true);
        composer.key(&state, press(KeyCode::Delete), true);
        assert_eq!(composer.draft, "界");
        composer.key(&state, press(KeyCode::Char('λ')), true);
        assert_eq!(composer.draft, "λ界");
    }

    #[test]
    fn accept_emits_one_scoped_paste_without_submission_characters() {
        let state = state();
        let mut composer = InputPrompt::compose(&state).unwrap();
        assert!(matches!(
            composer.key(&state, press(KeyCode::Enter), true),
            Edit::Continue
        ));
        composer.insert("printf\tλ\nnow\r\u{1b}\0");
        assert!(matches!(
            composer.key(&state, press(KeyCode::Enter), false),
            Edit::Continue
        ));
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Repeat);
        assert!(matches!(composer.key(&state, repeat, true), Edit::Continue));
        let Edit::Send(request) = composer.key(&state, press(KeyCode::Enter), true) else {
            panic!("Expected one paste request");
        };
        assert_eq!(
            request,
            json!({
                "op":"paste","host":"h","epoch":9,"session":"s","window":"w","pane":"p",
                "text":"printf λ now "
            })
        );
        assert!(matches!(
            composer.key(&state, press(KeyCode::Enter), true),
            Edit::Continue
        ));
        assert!(matches!(
            composer.key(&state, press(KeyCode::Esc), true),
            Edit::Cancel
        ));
    }

    #[test]
    fn scope_change_or_disconnect_invalidates_the_draft_even_if_the_target_returns() {
        let original = state();
        for path in [
            "/host",
            "/epoch",
            "/session",
            "/window",
            "/pane",
            "/terminal/generation",
            "/terminal/connected",
            "/connected",
        ] {
            let mut composer = InputPrompt::compose(&original).unwrap();
            composer.insert("draft");
            let mut changed = original.clone();
            *changed.pointer_mut(path).unwrap() = Value::Null;
            composer.observe(&changed);
            assert!(
                matches!(
                    composer.key(&original, press(KeyCode::Enter), true),
                    Edit::Continue
                ),
                "{path}"
            );
            assert!(
                matches!(
                    composer.key(&original, press(KeyCode::Esc), true),
                    Edit::Cancel
                ),
                "{path}"
            );
        }
        let mut pending = original.clone();
        pending["hosts"]["pending"] = json!(true);
        assert!(InputPrompt::compose(&pending).is_err());
    }

    #[test]
    fn draws_the_captured_label_draft_and_stale_notice() {
        let state = state();
        let mut composer = InputPrompt::compose(&state).unwrap();
        composer.insert("echo café");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(65, 12)).unwrap();
        let display = |buffer: &ratatui::buffer::Buffer| {
            buffer
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };
        let frame = terminal
            .draw(|frame| composer.draw(frame, Chrome::default()))
            .unwrap();
        let rendered = display(frame.buffer);
        assert!(rendered.contains("Build shell") && rendered.contains("echo café"));
        composer.observe(&Value::Null);
        let frame = terminal
            .draw(|frame| composer.draw(frame, Chrome::default()))
            .unwrap();
        assert!(display(frame.buffer).contains("Esc discards draft"));
    }

    #[test]
    fn rename_submits_each_captured_target_once() {
        let state = state();
        for (kind, action) in [
            (RenameTarget::Session, "session.rename"),
            (RenameTarget::Tab, "client.tab.rename"),
            (RenameTarget::Pane, "pane.rename"),
        ] {
            let mut prompt = InputPrompt::rename(&state, kind, 17).unwrap();
            prompt.key(
                &state,
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
                true,
            );
            prompt.insert("München\nBuild");
            let Edit::Send(request) = prompt.key(&state, press(KeyCode::Enter), true) else {
                panic!("Expected a rename request");
            };
            assert_eq!(
                request,
                json!({
                    "op":"workspace","action":action,"request_id":17,
                    "host":"h","epoch":9,"session":"s","window":"w","pane":"p",
                    "args":{"label":"München Build"}
                })
            );
            assert!(matches!(
                prompt.key(&state, press(KeyCode::Enter), true),
                Edit::Continue
            ));
            assert!(matches!(
                prompt.key(&state, press(KeyCode::Esc), true),
                Edit::Cancel
            ));
        }
    }

    #[test]
    fn empty_labels_restore_automatic_titles_only_for_tabs_and_panes() {
        let state = state();
        for kind in [RenameTarget::Session, RenameTarget::Tab, RenameTarget::Pane] {
            let mut prompt = InputPrompt::rename(&state, kind, 17).unwrap();
            prompt.key(
                &state,
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
                true,
            );
            let edit = prompt.key(&state, press(KeyCode::Enter), true);
            if kind == RenameTarget::Session {
                assert!(matches!(edit, Edit::Continue));
                assert_eq!(prompt.error.as_deref(), Some("Enter a session name"));
            } else {
                let Edit::Send(request) = edit else {
                    panic!("Expected title reset")
                };
                assert_eq!(request["args"]["label"], "");
            }
        }
    }

    #[test]
    fn rename_never_retargets_a_draft_and_reports_correlated_errors() {
        let state = state();
        let mut prompt = InputPrompt::rename(&state, RenameTarget::Tab, 25).unwrap();
        let mut changed = state.clone();
        changed["window"] = json!("other");
        prompt.observe(&changed);
        assert!(matches!(
            prompt.key(&state, press(KeyCode::Enter), true),
            Edit::Continue
        ));
        assert!(matches!(
            prompt.key(&state, press(KeyCode::Esc), true),
            Edit::Cancel
        ));

        let mut prompt = InputPrompt::rename(&state, RenameTarget::Pane, 26).unwrap();
        prompt.insert("Fixture pane");
        assert!(matches!(
            prompt.key(&state, press(KeyCode::Enter), true),
            Edit::Send(_)
        ));
        prompt.reject(25, "Other request failed");
        assert!(prompt.error.is_none());
        prompt.reject(26, "Pane no longer exists");
        assert_eq!(prompt.error.as_deref(), Some("Pane no longer exists"));
    }
}
