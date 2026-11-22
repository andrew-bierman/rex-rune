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

pub(crate) struct Composer {
    target: Value,
    generation: Value,
    label: String,
    draft: String,
    cursor: usize,
    stale: bool,
    sent: bool,
}

pub(crate) enum Edit {
    Continue,
    Cancel,
    Send(Value),
}

impl Composer {
    pub(crate) fn open(state: &Value) -> Result<Self, &'static str> {
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
            generation: state["terminal"]["generation"].clone(),
            label: single_line(label),
            draft: String::new(),
            cursor: 0,
            stale: false,
            sent: false,
        })
    }

    pub(crate) fn observe(&mut self, state: &Value) {
        self.stale |= !connected(state)
            || self.generation != state["terminal"]["generation"]
            || TARGET_FIELDS
                .iter()
                .any(|field| self.target[*field] != state[*field]);
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
                if key.kind == KeyEventKind::Press
                    && key.modifiers.is_empty()
                    && visible
                    && !self.draft.is_empty() =>
            {
                let mut request = self.target.clone();
                request["text"] = json!(std::mem::take(&mut self.draft));
                self.cursor = 0;
                self.sent = true;
                return Edit::Send(request);
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
        let block = Block::bordered()
            .title(" Input composer · Experimental ")
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
                if self.sent {
                    Line::raw("Paste requested. Esc returns.")
                } else if self.stale {
                    Line::styled(
                        "Pane changed. Esc discards draft.",
                        Style::new().fg(chrome.warning),
                    )
                } else {
                    Line::raw("Enter Paste only    Esc Discard")
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
            "panes": [{"pane": "p", "label": "Build shell"}]
        })
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn edits_extended_graphemes_and_scrolls_in_display_columns() {
        let state = state();
        let mut composer = Composer::open(&state).unwrap();
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
        let mut composer = Composer::open(&state).unwrap();
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
            let mut composer = Composer::open(&original).unwrap();
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
        assert!(Composer::open(&pending).is_err());
    }

    #[test]
    fn draws_the_captured_label_draft_and_stale_notice() {
        let state = state();
        let mut composer = Composer::open(&state).unwrap();
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
}
