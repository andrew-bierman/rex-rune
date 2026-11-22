use crate::{
    input_prompt::InputPrompt,
    items,
    mouse::{Menu, Target},
    selection::{Grid, Selection},
    settings::{Action, Chrome, PaneDefaultColors, Panes, Settings, SidebarSide},
    targeted, text,
};
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
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

impl RowId<'_> {
    fn owned(&self) -> (String, String, String) {
        (self.0.to_owned(), self.1.to_owned(), self.2.to_owned())
    }
}

#[derive(PartialEq, Eq)]
struct SidebarTarget {
    id: (String, String, String),
    epoch: Value,
    disclosure: bool,
}

pub(crate) enum SidebarMouse {
    Ignore,
    Handled(Option<Value>),
}

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

    fn open(&self) -> Value {
        let (action, args) = match self.session {
            Some(session) => (
                "navigate",
                json!({"session": session["id"], "window": self.tab.map_or("", |tab| text(tab, "id"))}),
            ),
            None => ("activate", json!({})),
        };
        json!({"op":"host", "action":action, "args":args, "host":self.host["id"], "epoch":self.host["epoch"]})
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
    pub ansi_palette_started: bool,
    pub settings_open: bool,
    pub prompt: Option<InputPrompt>,
    pub dirty: bool,
    pub pane_sizes: Vec<(String, u16, u16)>,
    pub pane_grids: Vec<Grid>,
    pane_area: Rect,
    pub selection: Selection,
    pub pane_menu: Option<Menu>,
    selected: usize,
    selected_id: Option<(String, String, String)>,
    sidebar_offset: usize,
    sidebar_scrolled: bool,
    sidebar_inner: Rect,
    sidebar_press: Option<SidebarTarget>,
    folded: HashMap<String, HashSet<String>>,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            sidebar: true,
            settings: Settings::default(),
            ansi_palette_started: false,
            settings_open: false,
            prompt: None,
            dirty: true,
            pane_sizes: Vec::new(),
            pane_grids: Vec::new(),
            pane_area: Rect::ZERO,
            selection: Selection::default(),
            pane_menu: None,
            selected: 0,
            selected_id: None,
            sidebar_offset: 0,
            sidebar_scrolled: false,
            sidebar_inner: Rect::ZERO,
            sidebar_press: None,
            folded: HashMap::new(),
        }
    }
}

impl Screen {
    pub fn follow_active_row(&mut self) {
        self.selected_id = None;
        self.sidebar_scrolled = false;
    }

    fn remember_selection(&mut self, rows: &[Row<'_>]) {
        self.selected_id = rows.get(self.selected).map(|row| row.id().owned());
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
        if matches!(
            key,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown
                | KeyCode::Enter
                | KeyCode::Char('j' | 'k')
        ) {
            self.sidebar_scrolled = false;
            self.sidebar_press = None;
        }
        match key {
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = last,
            KeyCode::PageUp => {
                self.selected = self
                    .selected
                    .saturating_sub(usize::from(self.sidebar_inner.height.max(1)))
            }
            KeyCode::PageDown => {
                self.selected =
                    (self.selected + usize::from(self.sidebar_inner.height.max(1))).min(last)
            }
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
                self.dirty = true;
                return rows.get(self.selected).map(Row::open);
            }
            _ => return None,
        }
        self.dirty = true;
        self.remember_selection(&rows);
        None
    }

    pub fn cancel_sidebar_mouse(&mut self) {
        self.sidebar_press = None;
    }

    pub fn sidebar_mouse(&mut self, state: &Value, event: MouseEvent) -> SidebarMouse {
        if !event.modifiers.is_empty() {
            self.cancel_sidebar_mouse();
            return SidebarMouse::Ignore;
        }
        let at = Position::new(event.column, event.row);
        let inside = self.sidebar_inner.contains(at);
        if inside
            && matches!(
                event.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            )
        {
            self.cancel_sidebar_mouse();
            self.sidebar_scrolled = true;
            self.sidebar_offset = if event.kind == MouseEventKind::ScrollUp {
                self.sidebar_offset.saturating_sub(3)
            } else {
                self.sidebar_offset.saturating_add(3)
            };
            self.dirty = true;
            return SidebarMouse::Handled(None);
        }
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {}
            MouseEventKind::Drag(MouseButton::Left) if self.sidebar_press.is_some() => {
                self.cancel_sidebar_mouse();
                return SidebarMouse::Handled(None);
            }
            _ => return SidebarMouse::Ignore,
        }
        if event.kind == MouseEventKind::Up(MouseButton::Left) && self.sidebar_press.is_none() {
            return SidebarMouse::Ignore;
        }
        if !inside && self.sidebar_press.is_none() {
            return SidebarMouse::Ignore;
        }
        let rows = rows(state, &self.folded);
        let row = inside
            .then(|| {
                let index = self.sidebar_offset + usize::from(at.y - self.sidebar_inner.y);
                rows.get(index).map(|row| {
                    let indent = if row.session.is_some() { 4 } else { 2 };
                    let disclosure =
                        row.expanded.is_some() && at.x == self.sidebar_inner.x + indent;
                    (
                        index,
                        row,
                        SidebarTarget {
                            id: row.id().owned(),
                            epoch: row.host["epoch"].clone(),
                            disclosure,
                        },
                    )
                })
            })
            .flatten();

        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            self.sidebar_press = row.map(|(_, _, target)| target);
            return SidebarMouse::Handled(None);
        }
        let pressed = self.sidebar_press.take();
        if let Some((index, row, target)) = row
            && pressed.as_ref() == Some(&target)
        {
            self.sidebar = true;
            self.selected = index;
            self.selected_id = Some(target.id);
            self.sidebar_scrolled = false;
            self.dirty = true;
            if target.disclosure {
                return SidebarMouse::Handled(self.navigate(
                    state,
                    if row.expanded == Some(true) {
                        KeyCode::Left
                    } else {
                        KeyCode::Right
                    },
                ));
            }
            return SidebarMouse::Handled(Some(row.open()));
        }
        SidebarMouse::Handled(None)
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

    pub fn directional_pane(&self, state: &Value, action: Action) -> Option<Target> {
        let panes = items(&state["panes"]);
        let current = panes.iter().find(|pane| pane["pane"] == state["pane"])?;
        let source = pane_rectangle(self.pane_area, &current["rect"])?;
        panes
            .iter()
            .filter(|pane| pane["pane"] != state["pane"])
            .filter_map(|pane| {
                let area = pane_rectangle(self.pane_area, &pane["rect"])?;
                let rank = neighbor_rank(source, area, action)?;
                let target = Target::for_grid(state, Grid::new(pane, area)?)?;
                Some((rank, target))
            })
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, target)| target)
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
        self.pane_grids.clear();
        self.pane_area = Rect::ZERO;
        self.sidebar_inner = Rect::ZERO;
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
        self.pane_area = terminal;
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
                active
                    && !self.sidebar
                    && !self.settings_open
                    && self.prompt.is_none()
                    && self.pane_menu.is_none(),
                self.settings.panes,
                self.settings.chrome,
                PaneHighlights {
                    selection: self.settings.mouse_selection
                        && self.selection.highlights(state, pane),
                    search: active
                        && self
                            .prompt
                            .as_ref()
                            .is_some_and(InputPrompt::showing_matches),
                },
            );
            if self.settings.mouse_selection
                && let Some(grid) = Grid::new(pane, inner)
            {
                self.pane_grids.push(grid);
            }
            if pane["terminal"]["owner"] == true
                && pane["terminal"]["connected"] == true
                && inner.width > 1
                && inner.height > 1
            {
                self.pane_sizes
                    .push((text(pane, "pane").to_owned(), inner.width, inner.height));
            }
        }
        if let Some(prompt) = &self.prompt
            && prompt.is_find()
        {
            prompt.draw_find(
                frame,
                Rect {
                    height: 1,
                    ..footer
                },
                state,
                self.settings.chrome,
            );
        } else {
            frame.render_widget(Paragraph::new(footer_lines), footer);
        }
        if self.settings_open {
            self.draw_settings(frame, settings_path, problem);
        }
        if let Some(prompt) = &self.prompt
            && !prompt.is_find()
        {
            prompt.draw(frame, self.settings.chrome);
        }
        if let Some(menu) = &self.pane_menu
            && menu.target.visible(state, &self.pane_grids)
        {
            menu.draw(
                frame,
                state,
                self.settings.chrome,
                self.selection.can_copy(menu.target.id()),
            );
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
            Line::from(match self.settings.panes.ansi_palette {
                PaneDefaultColors::Server => "ANSI palette  Server",
                PaneDefaultColors::Terminal if self.ansi_palette_started => {
                    "ANSI palette  Terminal"
                }
                PaneDefaultColors::Terminal => "ANSI palette  Terminal (restart required)",
            }),
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
                self.hint(Action::CopySelection, "Copy"),
                if !self.sidebar && state["pane"].is_string() {
                    self.hint(Action::Find, "Find")
                } else {
                    String::new()
                },
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
        self.sidebar_inner = inner;
        self.sidebar_offset = self.sidebar_offset.min(rows.len().saturating_sub(capacity));
        if !self.sidebar_scrolled {
            self.sidebar_offset = self
                .sidebar_offset
                .min(self.selected)
                .max(self.selected.saturating_add(1).saturating_sub(capacity));
        }
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

fn neighbor_rank(
    source: Rect,
    candidate: Rect,
    action: Action,
) -> Option<(u16, u32, std::cmp::Reverse<u16>)> {
    let (gap, from, to) = match action {
        Action::FocusLeft => (
            source.left().checked_sub(candidate.right())?,
            (source.top(), source.bottom()),
            (candidate.top(), candidate.bottom()),
        ),
        Action::FocusRight => (
            candidate.left().checked_sub(source.right())?,
            (source.top(), source.bottom()),
            (candidate.top(), candidate.bottom()),
        ),
        Action::FocusUp => (
            source.top().checked_sub(candidate.bottom())?,
            (source.left(), source.right()),
            (candidate.left(), candidate.right()),
        ),
        Action::FocusDown => (
            candidate.top().checked_sub(source.bottom())?,
            (source.left(), source.right()),
            (candidate.left(), candidate.right()),
        ),
        _ => return None,
    };
    let overlap = from.1.min(to.1).checked_sub(from.0.max(to.0))?;
    if overlap == 0 {
        return None;
    }
    // Compare doubled centers to keep half-cell positions exact.
    let center_distance =
        (u32::from(from.0) + u32::from(from.1)).abs_diff(u32::from(to.0) + u32::from(to.1));
    Some((gap, center_distance, std::cmp::Reverse(overlap)))
}

fn rgb(value: &Value, fallback: Color) -> Color {
    value.as_u64().map_or(fallback, |n| {
        Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
    })
}

fn ansi_color(code: Option<&u8>) -> Option<Color> {
    let index = char::from(*code?).to_digit(16)?;
    Some(Color::Indexed(index as u8))
}

#[derive(Default)]
struct PaneHighlights {
    selection: bool,
    search: bool,
}

fn search_span(hit: &Value) -> Option<std::ops::RangeInclusive<(u64, u64)>> {
    let position = |point: &Value| match items(point) {
        [x, y] => Some((y.as_u64()?, x.as_u64()?)),
        _ => None,
    };
    Some(position(&hit["start"])?..=position(&hit["end"])?)
}

fn draw_pane(
    frame: &mut Frame<'_>,
    area: Rect,
    pane: &Value,
    focused: bool,
    options: Panes,
    chrome: Chrome,
    highlights: PaneHighlights,
) -> Rect {
    let snapshot = &pane["frame"];
    let roles = &snapshot["color_roles"];
    let use_terminal_colors = options.default_colors == PaneDefaultColors::Terminal;
    let foreground = if use_terminal_colors && roles["inherit"][0] == true {
        DEFAULT_COLOR
    } else {
        rgb(&snapshot["fg"], DEFAULT_COLOR)
    };
    let background = if use_terminal_colors && roles["inherit"][1] == true {
        DEFAULT_COLOR
    } else {
        rgb(&snapshot["bg"], DEFAULT_COLOR)
    };
    let ansi_rows = if options.ansi_palette == PaneDefaultColors::Terminal {
        items(&roles["ansi_rows"])
    } else {
        &[]
    };
    let role_rows = if use_terminal_colors || !ansi_rows.is_empty() {
        items(&roles["rows"])
    } else {
        &[]
    };
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
    let selected_match = if highlights.search {
        search_span(&snapshot["search"]["selected"])
    } else {
        None
    };
    let buffer = frame.buffer_mut();
    buffer.set_style(inner, Style::new().bg(background));
    for (y, row) in items(&snapshot["cells"])
        .iter()
        .take(inner.height.into())
        .enumerate()
    {
        let row_roles = role_rows
            .get(y)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .as_bytes();
        let ansi = ansi_rows
            .get(y)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .as_bytes();
        let selection = highlights
            .selection
            .then(|| items(&snapshot["selection_rows"]).get(y))
            .flatten()
            .and_then(|row| {
                let [start, end] = items(row) else {
                    return None;
                };
                Some(start.as_u64()?..=end.as_u64()?)
            });
        for (x, cell) in items(row).iter().take(inner.width.into()).enumerate() {
            let wide = items(row)
                .get(x + 1)
                .is_some_and(|next| next["spacer_tail"] == true);
            if cell["spacer_tail"] == true {
                continue;
            }
            // The legacy RGB fields already have inverse applied. Undo that
            // swap before asking the outer terminal to reverse logical roles.
            let roles = row_roles
                .get(x)
                .filter(|byte| (b'0'..=b'7').contains(byte))
                .map_or(0, |byte| byte - b'0');
            let reversed = roles & 4 != 0;
            let (fg, bg) = if reversed {
                (&cell["bg"], &cell["fg"])
            } else {
                (&cell["fg"], &cell["bg"])
            };
            let mut style = Style::new()
                .fg(if roles & 1 != 0 {
                    foreground
                } else {
                    rgb(fg, foreground)
                })
                .bg(if roles & 2 != 0 {
                    background
                } else {
                    rgb(bg, background)
                });
            if let Some(index) = ansi_color(ansi.get(2 * x)) {
                style = style.fg(index);
            }
            if let Some(index) = ansi_color(ansi.get(2 * x + 1)) {
                style = style.bg(index);
            }
            if reversed {
                style = style.add_modifier(Modifier::REVERSED);
            }
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
            if selected_match
                .as_ref()
                .is_some_and(|hit| hit.contains(&(y as u64, x as u64)))
            {
                style = if reversed {
                    style.remove_modifier(Modifier::REVERSED)
                } else {
                    style.add_modifier(Modifier::REVERSED)
                }
                .add_modifier(Modifier::BOLD);
            } else if selection
                .as_ref()
                .is_some_and(|range| range.contains(&(x as u64)))
            {
                style = match (chrome.selection_text, chrome.selection_background) {
                    (None, None) if reversed => style.remove_modifier(Modifier::REVERSED),
                    (None, None) => style.add_modifier(Modifier::REVERSED),
                    (text, background) => {
                        let (text, background) = if reversed {
                            (background, text)
                        } else {
                            (text, background)
                        };
                        if let Some(color) = text {
                            style = style.fg(color);
                        }
                        if let Some(color) = background {
                            style = style.bg(color);
                        }
                        style
                    }
                };
            }
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

    fn sidebar_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    #[test]
    fn opt_in_ansi_preserves_rgb_and_inverse_and_supports_old_backends() {
        let pane = json!({"label":"Shell","frame":{
            "fg":0xaabbcc,"bg":0x112233,
            "cells":[[
                {"text":"A","fg":0xaa0000,"bg":0x112233,"bold":true},
                {"text":"B","fg":0xaabbcc,"bg":0x0000aa},
                {"text":"R","fg":0xaa0000,"bg":0x112233},
                {"text":"I","fg":0x112233,"bg":0xff0000}
            ]],
            "color_roles":{"inherit":[true,true],"rows":["2104"],"ansi_rows":["1..4..9."]}
        }});
        let server = Panes {
            borders: false,
            headers: false,
            default_colors: PaneDefaultColors::Server,
            ..Panes::default()
        };
        let terminal = Panes {
            ansi_palette: PaneDefaultColors::Terminal,
            ..server
        };
        let mut old = pane.clone();
        old["frame"]["color_roles"]
            .as_object_mut()
            .unwrap()
            .remove("ansi_rows");
        let mut screen = ratatui::Terminal::new(ratatui::backend::TestBackend::new(10, 3)).unwrap();
        for (fixture, options, inherited) in [
            (&pane, server, false),
            (&old, terminal, false),
            (&pane, terminal, true),
        ] {
            let drawn = screen
                .draw(|frame| {
                    draw_pane(
                        frame,
                        frame.area(),
                        fixture,
                        false,
                        options,
                        Chrome::default(),
                        PaneHighlights::default(),
                    );
                })
                .unwrap();
            let cells = drawn.buffer.content();
            let red = Color::Rgb(0xaa, 0, 0);
            assert_eq!(cells[0].fg, if inherited { Color::Indexed(1) } else { red });
            assert!(cells[0].modifier.contains(Modifier::BOLD));
            assert_eq!(
                cells[1].bg,
                if inherited {
                    Color::Indexed(4)
                } else {
                    Color::Rgb(0, 0, 0xaa)
                }
            );
            assert_eq!(cells[2].fg, red);
            assert_eq!(cells[3].modifier.contains(Modifier::REVERSED), inherited);
            if inherited {
                assert_eq!(cells[3].fg, Color::Indexed(9));
            }
        }
    }

    #[test]
    fn directional_focus_uses_visible_nested_split_neighbors_and_does_not_wrap() {
        let mut state = json!({
            "host":"h","epoch":1,"session":"s","window":"w","pane":"bottom","connected":true,
            "panes":[
                {"pane":"left","rect":[0,0,0.5,1],"terminal":{"connected":true,"generation":1},"frame":{"cols":30,"rows":20}},
                {"pane":"top","rect":[0.5,0,0.5,0.4],"terminal":{"connected":true,"generation":1},"frame":{"cols":30,"rows":8}},
                {"pane":"bottom","rect":[0.5,0.4,0.5,0.6],"terminal":{"connected":true,"generation":1},"frame":{"cols":30,"rows":12}}
            ]
        });
        let screen = Screen {
            pane_area: Rect::new(20, 3, 61, 25),
            ..Screen::default()
        };
        assert_eq!(
            screen
                .directional_pane(&state, Action::FocusLeft)
                .unwrap()
                .id(),
            "left"
        );
        assert_eq!(
            screen
                .directional_pane(&state, Action::FocusUp)
                .unwrap()
                .id(),
            "top"
        );
        assert!(
            screen
                .directional_pane(&state, Action::FocusRight)
                .is_none()
        );
        assert!(screen.directional_pane(&state, Action::FocusDown).is_none());
        state["pane"] = json!("left");
        assert_eq!(
            screen
                .directional_pane(&state, Action::FocusRight)
                .unwrap()
                .id(),
            "bottom"
        );
        state["panes"][2]["terminal"]["connected"] = json!(false);
        assert_eq!(
            screen
                .directional_pane(&state, Action::FocusRight)
                .unwrap()
                .id(),
            "top"
        );
        state["hosts"]["pending"] = json!(true);
        assert!(
            screen
                .directional_pane(&state, Action::FocusRight)
                .is_none()
        );
    }

    #[test]
    fn a_touching_corner_is_not_a_directional_neighbor() {
        let current = Rect::new(0, 0, 10, 10);
        let diagonal = Rect::new(10, 10, 10, 10);
        assert!(neighbor_rank(current, diagonal, Action::FocusRight).is_none());
        assert!(neighbor_rank(current, diagonal, Action::FocusDown).is_none());
    }

    #[test]
    fn sidebar_click_uses_the_same_row_as_keyboard_and_cancels_on_reorder() {
        let mut state = json!({
            "hosts":{"rows":[{"id":"h","epoch":1,"active":true,"sessions":[
                {"id":"s","opened":true,"tabs":[{"id":"a"},{"id":"b"}]}
            ]}]}
        });
        let mut screen = Screen {
            sidebar_inner: Rect::new(1, 1, 20, 12),
            sidebar: false,
            ..Screen::default()
        };
        let down = sidebar_event(MouseEventKind::Down(MouseButton::Left), 12, 3);
        let up = sidebar_event(MouseEventKind::Up(MouseButton::Left), 12, 3);
        assert!(matches!(
            screen.sidebar_mouse(&state, down),
            SidebarMouse::Handled(None)
        ));
        state["hosts"]["rows"][0]["sessions"][0]["tabs"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert!(matches!(
            screen.sidebar_mouse(&state, up),
            SidebarMouse::Handled(None)
        ));
        assert!(!screen.sidebar);
        screen.sidebar_mouse(&state, down);
        let SidebarMouse::Handled(Some(request)) = screen.sidebar_mouse(&state, up) else {
            panic!("Click should open its captured tab");
        };
        assert_eq!(request["args"]["window"], "b");
        assert_eq!(screen.navigate(&state, KeyCode::Enter), Some(request));
        assert!(screen.sidebar);
    }

    #[test]
    fn sidebar_disclosure_folds_and_wheel_leaves_typing_focus_unchanged() {
        let state = json!({"hosts":{"rows":[
            {"id":"h","epoch":2,"sessions":[{"id":"s","opened":true,"tabs":[{"id":"a"}]}]}
        ]}});
        let mut screen = Screen {
            sidebar_inner: Rect::new(1, 1, 20, 2),
            sidebar: false,
            ..Screen::default()
        };
        let wheel = sidebar_event(MouseEventKind::ScrollDown, 5, 1);
        assert!(matches!(
            screen.sidebar_mouse(&state, wheel),
            SidebarMouse::Handled(None)
        ));
        assert!(!screen.sidebar);
        assert_eq!(screen.sidebar_offset, 3);
        screen.sidebar_offset = 0;
        let down = sidebar_event(MouseEventKind::Down(MouseButton::Left), 3, 1);
        let up = sidebar_event(MouseEventKind::Up(MouseButton::Left), 3, 1);
        for expected in [true, false] {
            screen.sidebar_mouse(&state, down);
            assert!(matches!(
                screen.sidebar_mouse(&state, up),
                SidebarMouse::Handled(None)
            ));
            assert_eq!(
                screen.folded.get("h").is_some_and(|ids| ids.contains("")),
                expected
            );
        }
        assert!(matches!(
            screen.sidebar_mouse(&state, up),
            SidebarMouse::Ignore
        ));
    }

    #[test]
    fn pane_renders_native_colors_explicit_colors_and_inverted_cells() {
        use ratatui::backend::TestBackend;
        let (fg, bg) = (0x112233, 0x445566);
        let pane = json!({"frame": {
            "fg":fg, "bg":bg, "cells":[[
                {"text":"D", "fg":fg, "bg":bg},
                {"text":"E", "fg":fg, "bg":bg},
                {"text":"I", "fg":bg, "bg":fg},
                {"text":"M", "fg":bg, "bg":0x010203},
                {"text":"B", "fg":fg, "bg":bg}
            ]],
            "color_roles":{"inherit":[true,true],"rows":["32761"]}
        }});
        let mut terminal = ratatui::Terminal::new(TestBackend::new(7, 3)).unwrap();
        let options = Panes {
            borders: false,
            headers: false,
            ..Panes::default()
        };
        let colors = |cell: &ratatui::buffer::Cell| (cell.fg, cell.bg);
        let explicit_fg = Color::Rgb(17, 34, 51);
        let explicit_bg = Color::Rgb(68, 85, 102);
        let rendered = terminal
            .draw(|frame| {
                draw_pane(
                    frame,
                    frame.area(),
                    &pane,
                    false,
                    options,
                    Chrome::default(),
                    PaneHighlights::default(),
                );
            })
            .unwrap();
        let cells = rendered.buffer.content();
        assert_eq!(colors(&cells[0]), (Color::Reset, Color::Reset));
        assert_eq!(colors(&cells[1]), (explicit_fg, Color::Reset));
        assert_eq!(colors(&cells[2]), (Color::Reset, Color::Reset));
        assert!(cells[2].modifier.contains(Modifier::REVERSED));
        assert_eq!(colors(&cells[3]), (Color::Rgb(1, 2, 3), Color::Reset));
        assert!(cells[3].modifier.contains(Modifier::REVERSED));
        assert_eq!(colors(&cells[4]), (Color::Reset, explicit_bg));
        assert_eq!(
            cells[6].bg,
            Color::Reset,
            "untouched space inherits the outer background"
        );

        let mut selected = pane.clone();
        selected["frame"]["selection_rows"] = json!([[1, 2]]);
        let rendered = terminal
            .draw(|frame| {
                draw_pane(
                    frame,
                    frame.area(),
                    &selected,
                    false,
                    options,
                    Chrome::default(),
                    PaneHighlights {
                        selection: true,
                        search: false,
                    },
                );
            })
            .unwrap();
        let cells = rendered.buffer.content();
        assert!(cells[1].modifier.contains(Modifier::REVERSED));
        assert!(!cells[2].modifier.contains(Modifier::REVERSED));

        let mut found = pane.clone();
        found["frame"]["search"]["selected"] = json!({"start":[1,0], "end":[2,0]});
        let rendered = terminal
            .draw(|frame| {
                draw_pane(
                    frame,
                    frame.area(),
                    &found,
                    false,
                    options,
                    Chrome::default(),
                    PaneHighlights {
                        search: true,
                        selection: false,
                    },
                );
            })
            .unwrap();
        let cells = rendered.buffer.content();
        assert!(
            cells[1]
                .modifier
                .contains(Modifier::REVERSED | Modifier::BOLD)
        );
        assert!(!cells[2].modifier.contains(Modifier::REVERSED));
        assert!(cells[2].modifier.contains(Modifier::BOLD));
        assert!(!cells[3].modifier.contains(Modifier::BOLD));

        let mut overridden = pane.clone();
        overridden["frame"]["color_roles"]["inherit"][1] = json!(false);
        let rendered = terminal
            .draw(|frame| {
                draw_pane(
                    frame,
                    frame.area(),
                    &overridden,
                    false,
                    options,
                    Chrome::default(),
                    PaneHighlights::default(),
                );
            })
            .unwrap();
        let cells = rendered.buffer.content();
        assert_eq!(colors(&cells[0]), (Color::Reset, explicit_bg));
        assert_eq!(colors(&cells[2]), (Color::Reset, explicit_bg));
        assert!(cells[2].modifier.contains(Modifier::REVERSED));

        let server = Panes {
            default_colors: PaneDefaultColors::Server,
            ..options
        };
        let mut old_backend = pane.clone();
        old_backend["frame"]
            .as_object_mut()
            .unwrap()
            .remove("color_roles");
        for (fixture, options) in [(&pane, server), (&old_backend, options)] {
            let rendered = terminal
                .draw(|frame| {
                    draw_pane(
                        frame,
                        frame.area(),
                        fixture,
                        false,
                        options,
                        Chrome::default(),
                        PaneHighlights::default(),
                    );
                })
                .unwrap();
            let cells = rendered.buffer.content();
            assert_eq!(colors(&cells[0]), (explicit_fg, explicit_bg));
            assert_eq!(colors(&cells[2]), (explicit_bg, explicit_fg));
            assert!(!cells[2].modifier.contains(Modifier::REVERSED));
        }
    }

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
