use crate::{
    composer::Composer,
    items,
    settings::{Action, Chrome, Panes, Settings, SidebarSide},
    targeted, text,
};
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use unicode_width::UnicodeWidthChar;

const DEFAULT_COLOR: Color = Color::Reset;

#[derive(PartialEq, Eq)]
struct RowId<'a>(&'a str, &'a str, &'a str);

struct Row<'a> {
    host: &'a Value,
    session: Option<&'a Value>,
    tab: Option<&'a Value>,
    expanded: Option<bool>,
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

fn rows<'a>(state: &'a Value, folded: &HashMap<String, HashSet<String>>) -> Vec<Row<'a>> {
    let mut result = Vec::new();
    for host in items(&state["hosts"]["rows"]) {
        let folded = folded.get(text(host, "id"));
        let sessions = items(&host["sessions"]);
        let host_expanded = !folded.is_some_and(|ids| ids.contains(""));
        result.push(Row {
            host,
            session: None,
            tab: None,
            expanded: (!sessions.is_empty()).then_some(host_expanded),
        });
        if !host_expanded {
            continue;
        }
        for session in sessions {
            let tabs = if session["opened"] == true
                || (host["active"] == true && session["selected"] == true)
            {
                items(&session["tabs"])
            } else {
                &[]
            };
            let session_expanded = !folded.is_some_and(|ids| ids.contains(text(session, "id")));
            result.push(Row {
                host,
                session: Some(session),
                tab: None,
                expanded: (!tabs.is_empty()).then_some(session_expanded),
            });
            if session_expanded {
                for tab in tabs {
                    result.push(Row {
                        host,
                        session: Some(session),
                        tab: Some(tab),
                        expanded: None,
                    });
                }
            }
        }
    }
    result
}

pub struct Screen {
    pub sidebar: bool,
    pub settings: Settings,
    pub settings_open: bool,
    pub composer: Option<Composer>,
    pub dirty: bool,
    pub pane_sizes: Vec<(String, u16, u16)>,
    selected: usize,
    selected_id: Option<(String, String, String)>,
    sidebar_offset: usize,
    folded: HashMap<String, HashSet<String>>,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            sidebar: true,
            settings: Settings::default(),
            settings_open: false,
            composer: None,
            dirty: true,
            pane_sizes: Vec::new(),
            selected: 0,
            selected_id: None,
            sidebar_offset: 0,
            folded: HashMap::new(),
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
        let rows = rows(state, &self.folded);
        let last = rows.len().saturating_sub(1);
        self.restore_selection(&rows, state);
        match key {
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = last,
            KeyCode::Left | KeyCode::Right => {
                let row = rows.get(self.selected)?;
                let RowId(host, session, tab) = row.id();
                match (key, row.expanded) {
                    (KeyCode::Left, Some(true)) => {
                        self.folded
                            .entry(host.into())
                            .or_default()
                            .insert(session.into());
                    }
                    (KeyCode::Right, Some(false)) => {
                        if let Some(ids) = self.folded.get_mut(host) {
                            ids.remove(session);
                        }
                    }
                    (KeyCode::Right, Some(true)) => self.selected += 1,
                    (KeyCode::Left, _) if !session.is_empty() => {
                        let parent = RowId(host, if tab.is_empty() { "" } else { session }, "");
                        if let Some(index) = rows.iter().position(|row| row.id() == parent) {
                            self.selected = index;
                        }
                    }
                    _ => return None,
                }
            }
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

    pub fn cycle(&self, state: &Value, action: Action) -> Option<Value> {
        let (entries, current, field, op) = if action == Action::NextPane {
            (&state["panes"], &state["pane"], "pane", "select_pane")
        } else {
            (&state["windows"], &state["window"], "id", "select_window")
        };
        let entries = items(entries);
        if entries.len() < 2 || state["hosts"]["pending"] == true {
            return None;
        }
        let index = entries.iter().position(|entry| entry[field] == *current)?;
        let step = if action == Action::PreviousTab {
            entries.len() - 1
        } else {
            1
        };
        let next = &entries[(index + step) % entries.len()][field];
        let mut message = targeted(state, op);
        message[if field == "pane" { "pane" } else { "window" }] = next.clone();
        Some(message)
    }

    pub fn draw(
        &mut self,
        frame: &mut Frame<'_>,
        state: &Value,
        problem: &str,
        settings_path: &Path,
    ) {
        let area = frame.area();
        self.pane_sizes.clear();
        frame.render_widget(Block::new().style(self.settings.chrome.style()), area);
        if area.width < 38 || area.height < 8 {
            frame.render_widget(Paragraph::new("Enlarge the terminal to use rex-tui."), area);
            return;
        }
        let footer_problem = if self.settings_open { "" } else { problem };
        let footer_lines = self.footer(state, footer_problem, area.width);
        let [body, footer] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(footer_lines.len() as u16),
        ])
        .areas(area);
        let sidebar_width = self
            .settings
            .sidebar
            .width
            .unwrap_or_else(|| (area.width / 4).clamp(18, 33))
            .min(area.width - 19);
        let sidebar_constraint = Constraint::Length(sidebar_width);
        let main_constraint = Constraint::Min(0);
        let (sidebar, work) = match self.settings.sidebar.side {
            SidebarSide::Left => {
                let [sidebar, work] =
                    Layout::horizontal([sidebar_constraint, main_constraint]).areas(body);
                (sidebar, work)
            }
            SidebarSide::Right => {
                let [work, sidebar] =
                    Layout::horizontal([main_constraint, sidebar_constraint]).areas(body);
                (sidebar, work)
            }
        };
        self.draw_sidebar(frame, sidebar, state);
        let [tabs, terminal] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(work);
        draw_tabs(frame, tabs, state, self.settings.chrome);
        let panes = items(&state["panes"]);
        if panes.is_empty() {
            let (empty, help) = if state["connected"] != true {
                ("Waiting for Rex", "↑ ↓ Navigate  ·  Enter Open".into())
            } else if items(&state["sessions"]).is_empty() {
                (
                    "No sessions on this host",
                    self.hint(Action::NewSession, "Create session"),
                )
            } else {
                ("Choose a session from the sidebar", "Enter Open".into())
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
            let inner = draw_pane(
                frame,
                rect,
                pane,
                active && !self.sidebar && !self.settings_open && self.composer.is_none(),
                self.settings.panes,
                self.settings.chrome,
            );
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
        if self.settings_open {
            self.draw_settings(frame, settings_path, problem);
        }
        if let Some(composer) = &self.composer {
            composer.draw(frame, self.settings.chrome);
        }
    }

    fn hint(&self, action: Action, label: &str) -> String {
        self.settings
            .shortcut(action)
            .map_or_else(String::new, |key| format!("{key} {label}"))
    }

    fn draw_settings(&self, frame: &mut Frame<'_>, path: &Path, problem: &str) {
        let area = frame.area();
        let width = area.width.min(70);
        let height = area.height.min(12);
        let bounds = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let chrome = self.settings.chrome;
        let width = self
            .settings
            .sidebar
            .width
            .map_or_else(|| "auto".into(), |width| width.to_string());
        let mut lines = vec![
            Line::raw("E Edit    R Reload    Esc Close"),
            Line::from(path.display().to_string()),
            Line::raw(""),
            Line::from(format!("Theme  {}", self.settings.theme)),
            Line::from(format!(
                "Sidebar  {:?}, {width}",
                self.settings.sidebar.side
            )),
            Line::from(format!("Startup focus  {:?}", self.settings.startup_focus)),
            Line::raw(""),
        ];
        if !problem.is_empty() {
            lines.push(Line::styled(problem, Style::new().fg(chrome.warning)));
        }
        frame.render_widget(Clear, bounds);
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: true }).block(
                Block::bordered()
                    .title(" Settings ")
                    .style(chrome.style())
                    .border_style(Style::new().fg(chrome.accent)),
            ),
            bounds,
        );
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
                Style::new().fg(self.settings.chrome.accent).bold(),
            ),
            Span::raw(if self.sidebar {
                format!(
                    "↑↓ Move  ←→ Fold  ↵ Open  {}",
                    self.hint(Action::ToggleFocus, "Type")
                )
            } else {
                [
                    self.hint(Action::ToggleFocus, "Browse"),
                    self.hint(Action::PreviousTab, "Prev"),
                    self.hint(Action::NextTab, "Next"),
                    self.hint(Action::NextPane, "Pane"),
                ]
                .into_iter()
                .filter(|hint| !hint.is_empty())
                .collect::<Vec<_>>()
                .join("  ")
            }),
        ]);
        let actions = Span::raw(
            [
                self.hint(Action::Settings, "Settings"),
                self.hint(Action::ComposeInput, "Compose"),
                self.hint(Action::NewSession, "Session"),
                self.hint(Action::NewTab, "Tab"),
                self.hint(Action::SplitRight, "Split →"),
                self.hint(Action::SplitDown, "↓"),
                self.hint(Action::Quit, "Exit"),
            ]
            .into_iter()
            .filter(|hint| !hint.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        );
        let status = Span::styled(
            status,
            if problem.is_empty() {
                Style::new()
            } else {
                Style::new().fg(self.settings.chrome.warning)
            },
        );
        let separator = Span::raw("  ");
        if focus.width() + actions.width() + separator.width() <= usize::from(width) {
            if problem.is_empty() {
                focus.spans.extend([separator.clone(), actions]);
            }
            focus.spans.extend([separator, status]);
            vec![focus]
        } else if !problem.is_empty() {
            vec![focus, Line::from(status)]
        } else {
            vec![focus, Line::from(vec![actions, separator, status])]
        }
    }

    fn draw_sidebar(&mut self, frame: &mut Frame<'_>, area: Rect, state: &Value) {
        let chrome = self.settings.chrome;
        let block = Block::default()
            .title(Line::styled(
                " rex-tui ",
                Style::new().fg(chrome.accent).bold(),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(if self.sidebar {
                chrome.accent
            } else {
                chrome.border
            }));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = rows(state, &self.folded);
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
                chrome.selection()
            } else if selected {
                Style::new().fg(chrome.accent).bold()
            } else {
                Style::new()
            };
            let cursor_marker = if cursor { "› " } else { "  " };
            let branch = match row.expanded {
                Some(true) => "▾ ",
                Some(false) => "▸ ",
                None => "  ",
            };
            let line = if let Some(tab) = row.tab {
                Line::styled(
                    format!(
                        "{cursor_marker}      {} {}",
                        if selected { "▰" } else { "▫" },
                        label(tab)
                    ),
                    row_style,
                )
            } else if let Some(session) = row.session {
                Line::styled(
                    format!("{cursor_marker}  {branch}{}", label(session)),
                    row_style,
                )
            } else {
                Line::from(vec![
                    Span::raw(cursor_marker),
                    Span::raw(branch),
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
                                chrome.accent
                            } else {
                                chrome.muted
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

fn draw_tabs(frame: &mut Frame<'_>, area: Rect, state: &Value, chrome: Chrome) {
    let active = items(&state["hosts"]["rows"])
        .iter()
        .find(|host| host["active"] == true);
    let title = active.map(label).unwrap_or_else(|| "Workspace".into());
    let block = Block::default()
        .title(Line::raw(format!(" {title} ")))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(chrome.border));
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
                        chrome.selection()
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

fn draw_pane(
    frame: &mut Frame<'_>,
    area: Rect,
    pane: &Value,
    focused: bool,
    options: Panes,
    chrome: Chrome,
) -> Rect {
    let snapshot = &pane["frame"];
    let background = rgb(&snapshot["bg"], DEFAULT_COLOR);
    let foreground = rgb(&snapshot["fg"], DEFAULT_COLOR);
    let mut block = Block::default().style(Style::new().bg(background));
    if options.borders {
        block = block
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(if focused {
                chrome.accent
            } else {
                chrome.border
            }));
    }
    if options.headers {
        block = block.title(Line::styled(
            format!(" {} {} ", if focused { "▸" } else { "·" }, label(pane)),
            Style::new().fg(if focused { chrome.accent } else { chrome.muted }),
        ));
    }
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
    fn settings_panel_shows_effective_config_and_an_error() {
        let fixture = crate::settings::tests::Fixture::new();
        let path = fixture.0.join("settings.toml");
        std::fs::write(
            &path,
            "[sidebar]\nside = \"right\"\nwidth = 27\n[keys]\nsettings = \"F12\"",
        )
        .unwrap();
        let mut screen = Screen {
            settings: Settings::load(&path).unwrap(),
            settings_open: true,
            ..Screen::default()
        };
        let backend = ratatui::backend::TestBackend::new(100, 25);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let rendered = terminal
            .draw(|frame| {
                screen.draw(frame, &Value::Null, "Invalid color", &path);
            })
            .unwrap();
        let buffer = rendered.buffer;
        let display: String = buffer
            .content()
            .chunks(100)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>() + "\n")
            .collect();
        assert!(display.contains("Theme  terminal"));
        assert!(display.contains("Sidebar  Right, 27"));
        assert!(display.contains("Invalid color"));
        assert!(display.contains("F12 Settings"));
    }

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
        let rows = rows(&state, &screen.folded);
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

    #[test]
    fn folding_keeps_navigation_on_a_visible_parent() {
        let state = json!({
            "session": "s", "window": "a", "hosts": {"rows": [{
                "id": "h", "epoch": 1, "active": true, "sessions": [{
                    "id": "s", "opened": true, "tabs": [{"id": "a"}]
                }]
            }]}
        });
        let mut screen = Screen::default();
        assert!(screen.navigate(&state, KeyCode::Left).is_none());
        assert_eq!(screen.selected, 1);
        assert!(screen.navigate(&state, KeyCode::Left).is_none());
        assert_eq!(rows(&state, &screen.folded).len(), 2);
        screen.navigate(&state, KeyCode::Left);
        screen.navigate(&state, KeyCode::Left);
        assert_eq!(rows(&state, &screen.folded).len(), 1);
        screen.navigate(&state, KeyCode::Right);
        screen.navigate(&state, KeyCode::Right);
        screen.navigate(&state, KeyCode::Right);
        assert_eq!(rows(&state, &screen.folded).len(), 3);
        assert_eq!(screen.selected, 1);
    }
}
