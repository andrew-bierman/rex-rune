use crate::{items, targeted, text};
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use serde_json::{Value, json};
use unicode_width::UnicodeWidthChar;

const DEFAULT_COLOR: Color = Color::Reset;
const MUTED: Color = Color::DarkGray;
const ACCENT: Color = Color::Cyan;
const WARNING: Color = Color::Yellow;

#[derive(PartialEq, Eq)]
struct RowId<'a>(&'a str, &'a str, &'a str);

struct Row<'a> {
    host: &'a Value,
    session: Option<&'a Value>,
    tab: Option<&'a Value>,
}

impl Row<'_> {
    fn id(&self) -> RowId<'_> {
        RowId(
            text(self.host, "id"),
            self.session.map_or("", |session| text(session, "id")),
            self.tab.map_or("", |tab| text(tab, "id")),
        )
    }

    fn active(&self, state: &Value) -> bool {
        self.host["active"] == true
            && self
                .session
                .is_none_or(|session| session["id"] == state["session"])
            && self.tab.is_none_or(|tab| tab["id"] == state["window"])
    }
}

fn rows(state: &Value) -> Vec<Row<'_>> {
    let mut result = Vec::new();
    for host in items(&state["hosts"]["rows"]) {
        result.push(Row {
            host,
            session: None,
            tab: None,
        });
        for session in items(&host["sessions"]) {
            result.push(Row {
                host,
                session: Some(session),
                tab: None,
            });
            if session["opened"] == true || (host["active"] == true && session["selected"] == true)
            {
                for tab in items(&session["tabs"]) {
                    result.push(Row {
                        host,
                        session: Some(session),
                        tab: Some(tab),
                    });
                }
            }
        }
    }
    result
}

pub struct Screen {
    pub sidebar: bool,
    pub dirty: bool,
    pub pane_sizes: Vec<(String, u16, u16)>,
    selected: usize,
    selected_id: Option<(String, String, String)>,
    sidebar_offset: usize,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            sidebar: true,
            dirty: true,
            pane_sizes: Vec::new(),
            selected: 0,
            selected_id: None,
            sidebar_offset: 0,
        }
    }
}

impl Screen {
    pub fn follow_active_row(&mut self) {
        self.selected_id = None;
    }

    fn remember_selection(&mut self, rows: &[Row<'_>]) {
        self.selected_id = rows.get(self.selected).map(|row| {
            let RowId(host, session, tab) = row.id();
            (host.to_owned(), session.to_owned(), tab.to_owned())
        });
    }

    fn restore_selection(&mut self, rows: &[Row<'_>], state: &Value) {
        if let Some((host, session, tab)) = &self.selected_id {
            for id in [
                RowId(host, session, tab),
                RowId(host, session, ""),
                RowId(host, "", ""),
            ] {
                if let Some(index) = rows.iter().position(|row| row.id() == id) {
                    self.selected = index;
                    return;
                }
            }
        } else if let Some(index) = rows.iter().rposition(|row| row.active(state)) {
            self.selected = index;
            self.remember_selection(rows);
            return;
        }
        self.selected = self.selected.min(rows.len().saturating_sub(1));
    }

    pub fn navigate(&mut self, state: &Value, key: KeyCode) -> Option<Value> {
        let rows = rows(state);
        let last = rows.len().saturating_sub(1);
        self.restore_selection(&rows, state);
        match key {
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = last,
            KeyCode::Enter => {
                let row = rows.get(self.selected)?;
                let (action, args) = match row.session {
                    Some(session) => (
                        "navigate",
                        json!({"session": session["id"], "window": row.tab.map_or("", |tab| text(tab, "id"))}),
                    ),
                    None => ("activate", json!({})),
                };
                return Some(
                    json!({"op":"host", "action":action, "args":args, "host":row.host["id"], "epoch":row.host["epoch"]}),
                );
            }
            _ => return None,
        }
        self.dirty = true;
        self.remember_selection(&rows);
        None
    }

    pub fn cycle(&self, state: &Value, key: KeyCode) -> Option<Value> {
        let (entries, current, field, op) = if key == KeyCode::F(7) {
            (&state["panes"], &state["pane"], "pane", "select_pane")
        } else {
            (&state["windows"], &state["window"], "id", "select_window")
        };
        let entries = items(entries);
        if entries.len() < 2 || state["hosts"]["pending"] == true {
            return None;
        }
        let index = entries.iter().position(|entry| entry[field] == *current)?;
        let step = if key == KeyCode::F(3) {
            entries.len() - 1
        } else {
            1
        };
        let next = &entries[(index + step) % entries.len()][field];
        let mut message = targeted(state, op);
        message[if field == "pane" { "pane" } else { "window" }] = next.clone();
        Some(message)
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>, state: &Value, problem: &str) {
        let area = frame.area();
        self.pane_sizes.clear();
        if area.width < 38 || area.height < 8 {
            frame.render_widget(Paragraph::new("Enlarge the terminal to use rex-tui."), area);
            return;
        }
        let footer_lines = self.footer(state, problem, area.width);
        let [body, footer] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(footer_lines.len() as u16),
        ])
        .areas(area);
        let [sidebar, work] = Layout::horizontal([
            Constraint::Length((area.width / 4).clamp(18, 33)),
            Constraint::Min(0),
        ])
        .areas(body);
        self.draw_sidebar(frame, sidebar, state);
        let [tabs, terminal] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(work);
        draw_tabs(frame, tabs, state);
        let panes = items(&state["panes"]);
        if panes.is_empty() {
            let (empty, help) = if state["connected"] != true {
                ("Waiting for Rex", "↑ ↓ Navigate  ·  Enter Open")
            } else if items(&state["sessions"]).is_empty() {
                ("No sessions on this host", "F2 Create session")
            } else {
                ("Choose a session from the sidebar", "Enter Open  ·  F2 New")
            };
            frame.render_widget(
                Paragraph::new(vec![
                    Line::raw(""),
                    Line::styled(empty, Style::new().bold()),
                    Line::raw(help),
                ])
                .centered(),
                terminal,
            );
        }
        for pane in panes {
            let Some(rect) = pane_rectangle(terminal, &pane["rect"]) else {
                continue;
            };
            let active = pane["pane"] == state["pane"];
            let inner = draw_pane(frame, rect, pane, active && !self.sidebar);
            if pane["terminal"]["owner"] == true
                && pane["terminal"]["connected"] == true
                && inner.width > 1
                && inner.height > 1
            {
                self.pane_sizes
                    .push((text(pane, "pane").to_owned(), inner.width, inner.height));
            }
        }
        frame.render_widget(Paragraph::new(footer_lines), footer);
    }

    fn footer<'a>(&self, state: &'a Value, problem: &'a str, width: u16) -> Vec<Line<'a>> {
        let status = if !problem.is_empty() {
            problem
        } else if state["hosts"]["pending"] == true {
            "Opening workspace…"
        } else {
            text(state, "status")
        };
        let mut focus = Line::from(vec![
            Span::styled(
                if self.sidebar {
                    " Sidebar  "
                } else {
                    " Terminal  "
                },
                Style::new().fg(ACCENT).bold(),
            ),
            Span::raw(if self.sidebar {
                "↑/↓ Move  Enter Open  F6 Type"
            } else {
                "F6 Browse  F3/4 Tabs  F7 Pane"
            }),
        ]);
        let actions = Span::raw(" F2 Session  F5 Tab  F8/9 Split  F10 Exit");
        let status = Span::styled(
            status,
            if problem.is_empty() {
                Style::new()
            } else {
                Style::new().fg(WARNING)
            },
        );
        let separator = Span::raw("  ");
        if focus.width() + actions.width() + separator.width() <= usize::from(width) {
            focus
                .spans
                .extend([separator.clone(), actions, separator, status]);
            vec![focus]
        } else {
            vec![focus, Line::from(vec![actions, separator, status])]
        }
    }

    fn draw_sidebar(&mut self, frame: &mut Frame<'_>, area: Rect, state: &Value) {
        let block = Block::default()
            .title(Line::styled(" rex-tui ", Style::new().fg(ACCENT).bold()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(if self.sidebar { ACCENT } else { MUTED }));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = rows(state);
        self.restore_selection(&rows, state);
        let capacity = usize::from(inner.height);
        self.sidebar_offset = self
            .sidebar_offset
            .min(self.selected)
            .min(rows.len().saturating_sub(capacity))
            .max(self.selected.saturating_add(1).saturating_sub(capacity));
        let mut lines = Vec::with_capacity(capacity);
        for (index, row) in rows
            .iter()
            .enumerate()
            .skip(self.sidebar_offset)
            .take(capacity)
        {
            let cursor = self.sidebar && index == self.selected;
            let selected = row.active(state);
            let row_style = if cursor {
                Style::new().add_modifier(Modifier::REVERSED).bold()
            } else if selected {
                Style::new().fg(ACCENT).bold()
            } else {
                Style::new()
            };
            let cursor_marker = if cursor { "› " } else { "  " };
            let line = if let Some(tab) = row.tab {
                Line::styled(
                    format!(
                        "{cursor_marker}    {} {}",
                        if selected { "▰" } else { "▫" },
                        label(tab)
                    ),
                    row_style,
                )
            } else if let Some(session) = row.session {
                Line::styled(
                    format!(
                        "{cursor_marker}  {} {}",
                        if selected { "›" } else { "┊" },
                        label(session)
                    ),
                    row_style,
                )
            } else {
                Line::from(vec![
                    Span::raw(cursor_marker),
                    Span::styled(
                        if row.host["connected"] == true {
                            "● "
                        } else {
                            "○ "
                        },
                        if cursor {
                            row_style
                        } else {
                            row_style.fg(if row.host["connected"] == true {
                                ACCENT
                            } else {
                                MUTED
                            })
                        },
                    ),
                    Span::styled(label(row.host), row_style.bold()),
                ])
                .style(row_style)
            };
            lines.push(line);
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

fn draw_tabs(frame: &mut Frame<'_>, area: Rect, state: &Value) {
    let active = items(&state["hosts"]["rows"])
        .iter()
        .find(|host| host["active"] == true);
    let title = active.map(label).unwrap_or_else(|| "Workspace".into());
    let block = Block::default()
        .title(Line::raw(format!(" {title} ")))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(MUTED));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let windows = items(&state["windows"]);
    let tabs = if windows.is_empty() {
        vec![Span::raw("  No tab selected")]
    } else {
        let selected = windows
            .iter()
            .position(|tab| tab["id"] == state["window"])
            .unwrap_or_default();
        let start = selected.saturating_sub(usize::from(inner.width / 28).saturating_sub(1));
        windows
            .iter()
            .skip(start)
            .map(|tab| {
                let budget = usize::from(inner.width).min(28).saturating_sub(4);
                let mut remaining = budget;
                let name: String = label(tab)
                    .chars()
                    .take_while(|c| {
                        let width = c.width().unwrap_or(0);
                        if width > remaining {
                            return false;
                        }
                        remaining -= width;
                        true
                    })
                    .collect();
                let active = tab["id"] == state["window"];
                Span::styled(
                    format!(" {} {name} ", if active { "▰" } else { "▫" }),
                    if active {
                        Style::new().add_modifier(Modifier::REVERSED).bold()
                    } else {
                        Style::new()
                    },
                )
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(Line::from(tabs)), inner);
}

fn label(value: &Value) -> String {
    text(value, "label")
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn pane_rectangle(area: Rect, value: &Value) -> Option<Rect> {
    let [x, y, width, height] = items(value) else {
        return None;
    };
    let (x, y, width, height) = (x.as_f64()?, y.as_f64()?, width.as_f64()?, height.as_f64()?);
    let axis = |fraction: f64, size| (fraction.clamp(0.0, 1.0) * f64::from(size)).round() as u16;
    let left = axis(x, area.width);
    let top = axis(y, area.height);
    let width = axis(x + width, area.width).saturating_sub(left);
    let height = axis(y + height, area.height).saturating_sub(top);
    (width >= 3 && height >= 3).then_some(Rect::new(area.x + left, area.y + top, width, height))
}

fn rgb(value: &Value, fallback: Color) -> Color {
    value.as_u64().map_or(fallback, |n| {
        Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
    })
}

fn draw_pane(frame: &mut Frame<'_>, area: Rect, pane: &Value, focused: bool) -> Rect {
    let snapshot = &pane["frame"];
    let background = rgb(&snapshot["bg"], DEFAULT_COLOR);
    let foreground = rgb(&snapshot["fg"], DEFAULT_COLOR);
    let block = Block::default()
        .title(Line::styled(
            format!(" {} {} ", if focused { "▸" } else { "·" }, label(pane)),
            if focused {
                Style::new().fg(ACCENT).bold()
            } else {
                Style::new()
            },
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { ACCENT } else { MUTED }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if !snapshot.is_object() {
        frame.render_widget(Paragraph::new("  Waiting for terminal…"), inner);
        return inner;
    }
    let buffer = frame.buffer_mut();
    buffer.set_style(inner, Style::new().bg(background));
    for (y, row) in items(&snapshot["cells"])
        .iter()
        .take(inner.height.into())
        .enumerate()
    {
        for (x, cell) in items(row).iter().take(inner.width.into()).enumerate() {
            let wide = items(row)
                .get(x + 1)
                .is_some_and(|next| next["spacer_tail"] == true);
            if cell["spacer_tail"] == true {
                continue;
            }
            let mut style = Style::new()
                .fg(rgb(&cell["fg"], foreground))
                .bg(rgb(&cell["bg"], background));
            for (property, flag) in [
                ("bold", Modifier::BOLD),
                ("faint", Modifier::DIM),
                ("italic", Modifier::ITALIC),
                ("strikethrough", Modifier::CROSSED_OUT),
            ] {
                if cell[property] == true {
                    style = style.add_modifier(flag);
                }
            }
            if cell["underline"]
                .as_str()
                .is_some_and(|kind| kind != "none")
            {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            let symbol = text(cell, "text");
            let symbol = if cell["invisible"] == true
                || symbol.is_empty()
                || (wide && x + 1 == usize::from(inner.width))
                || symbol.chars().any(char::is_control)
            {
                " "
            } else {
                symbol
            };
            if let Some(target) = buffer.cell_mut((inner.x + x as u16, inner.y + y as u16)) {
                target.set_symbol(symbol).set_style(style);
            }
        }
    }
    if focused
        && let [x, y] = items(&snapshot["cursor"])
        && let (Some(x), Some(y)) = (x.as_u64(), y.as_u64())
        && x < u64::from(inner.width)
        && y < u64::from(inner.height)
    {
        frame.set_cursor_position(Position::new(inner.x + x as u16, inner.y + y as u16));
    }
    inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_cursor_tracks_tab_identity_across_inventory_updates() {
        let mut state = json!({
            "session": "s", "window": "a", "hosts": {"pending": true, "rows": [{
                "id": "h", "epoch": 4, "active": true, "sessions": [{
                    "id": "s", "opened": true, "tabs": [{"id": "a"}, {"id": "b"}]
                }]
            }]}
        });
        let mut screen = Screen::default();
        screen.navigate(&state, KeyCode::End);
        let host_rows = state["hosts"]["rows"].as_array_mut().unwrap();
        host_rows.insert(0, json!({"id": "new-host"}));
        let request = screen.navigate(&state, KeyCode::Enter).unwrap();

        assert_eq!(request["host"], "h");
        assert_eq!(request["args"], json!({"session": "s", "window": "b"}));
        let rows = rows(&state);
        assert!(rows[3].active(&state));
        assert!(!rows[4].active(&state));
    }

    #[test]
    fn adjacent_splits_share_a_boundary_at_odd_sizes() {
        let area = Rect::new(5, 2, 19, 9);
        let left = pane_rectangle(area, &json!([0, 0, 0.5, 1])).unwrap();
        let right = pane_rectangle(area, &json!([0.5, 0, 0.5, 1])).unwrap();
        assert_eq!(left.right(), right.left());
        assert_eq!(right.right(), area.right());
    }

    #[test]
    fn sidebar_focus_follows_the_current_tab() {
        let mut state = json!({
            "session": "s", "window": "b", "hosts": {"rows": [{
                "id": "h", "epoch": 4, "active": true, "sessions": [{
                    "id": "s", "opened": true, "tabs": [{"id": "a"}, {"id": "b"}]
                }]
            }]}
        });
        let mut screen = Screen::default();
        let request = screen.navigate(&state, KeyCode::Enter).unwrap();
        assert_eq!(request["args"]["window"], "b");

        state["window"] = json!("a");
        screen.follow_active_row();
        let request = screen.navigate(&state, KeyCode::Enter).unwrap();
        assert_eq!(request["args"]["window"], "a");
    }

    #[test]
    fn footer_uses_one_row_when_the_shortcuts_fit() {
        let screen = Screen::default();
        let state = json!({"status": "Connected"});
        assert_eq!(screen.footer(&state, "", 100).len(), 1);
        assert_eq!(screen.footer(&state, "", 60).len(), 2);
    }
}
