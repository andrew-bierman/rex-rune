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

const INK: Color = Color::Rgb(213, 225, 237);
const MUTED: Color = Color::Rgb(135, 154, 177);
const BORDER: Color = Color::Rgb(48, 66, 88);
const BASE: Color = Color::Rgb(12, 18, 29);
const PANEL: Color = Color::Rgb(19, 29, 45);
const SELECTED: Color = Color::Rgb(33, 63, 78);
const MINT: Color = Color::Rgb(121, 232, 199);
const GOLD: Color = Color::Rgb(246, 196, 126);

struct Row<'a> {
    host: &'a Value,
    session: Option<&'a Value>,
    tab: Option<&'a Value>,
}

fn rows(state: &Value) -> Vec<Row<'_>> {
    let mut result = Vec::new();
    for host in items(&state["hosts"]["rows"]) {
        result.push(Row {
            host,
            session: None,
            tab: None,
        });
        if host["active"] == true {
            for session in items(&host["sessions"]) {
                result.push(Row {
                    host,
                    session: Some(session),
                    tab: None,
                });
                if session["selected"] == true {
                    for tab in items(&state["windows"]) {
                        result.push(Row {
                            host,
                            session: Some(session),
                            tab: Some(tab),
                        });
                    }
                }
            }
        }
    }
    result
}

pub struct Screen {
    pub sidebar: bool,
    pub dirty: bool,
    pub terminal_size: (u16, u16),
    selected: usize,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            sidebar: true,
            dirty: true,
            terminal_size: (0, 0),
            selected: 0,
        }
    }
}

impl Screen {
    pub fn navigate(&mut self, state: &Value, key: KeyCode) -> Option<Value> {
        let rows = rows(state);
        let last = rows.len().saturating_sub(1);
        self.selected = self.selected.min(last);
        self.dirty = true;
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
            _ => {}
        }
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
        frame.render_widget(Block::default().style(Style::new().bg(BASE)), area);
        self.terminal_size = (0, 0);
        if area.width < 38 || area.height < 8 {
            frame.render_widget(
                Paragraph::new("Enlarge the terminal to use Rex.").style(Style::new().fg(MUTED)),
                area,
            );
            return;
        }
        let [body, footer] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
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
            let empty = if state["connected"] == true {
                "Choose a session from the sidebar"
            } else {
                "Waiting for Rex"
            };
            frame.render_widget(
                Paragraph::new(vec![
                    Line::raw(""),
                    Line::styled(empty, Style::new().fg(INK).bold()),
                    Line::styled("↑ ↓ Navigate  ·  Enter Open", Style::new().fg(MUTED)),
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
            if active {
                self.terminal_size = (inner.width, inner.height);
            }
        }
        let status = if !problem.is_empty() {
            problem
        } else if state["hosts"]["pending"] == true {
            "Opening workspace…"
        } else {
            text(state, "status")
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    if self.sidebar {
                        "  ▸ NAVIGATION  "
                    } else {
                        "  ▸ TERMINAL  "
                    },
                    Style::new().fg(MINT).bold(),
                ),
                Span::styled(
                    "F6 Focus   F3/F4 Tabs   F7 Pane   F10 Exit  ",
                    Style::new().fg(INK),
                ),
                Span::styled(
                    status,
                    Style::new().fg(if problem.is_empty() { MUTED } else { GOLD }),
                ),
            ]))
            .style(Style::new().bg(PANEL)),
            footer,
        );
    }

    fn draw_sidebar(&mut self, frame: &mut Frame<'_>, area: Rect, state: &Value) {
        let block = Block::default()
            .title(Line::styled(" ◈ Rex ", Style::new().fg(MINT).bold()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(if self.sidebar { MINT } else { BORDER }))
            .style(Style::new().bg(PANEL));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = rows(state);
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        let capacity = usize::from(inner.height.saturating_sub(2));
        let offset = self.selected.saturating_add(1).saturating_sub(capacity);
        let mut lines = vec![
            Line::styled("  HOSTS & SESSIONS", Style::new().fg(MUTED).bold()),
            Line::raw(""),
        ];
        for (index, row) in rows.iter().enumerate().skip(offset).take(capacity) {
            let cursor = self.sidebar && index == self.selected;
            let selected = row.host["active"] == true
                && row
                    .session
                    .is_none_or(|session| session["selected"] == true);
            let row_style = if cursor {
                Style::new().bg(MINT).fg(BASE).bold()
            } else if selected {
                Style::new().bg(SELECTED).fg(MINT)
            } else {
                Style::new().fg(INK)
            };
            let line = if let Some(tab) = row.tab {
                Line::styled(
                    format!(
                        "      {} {}",
                        if selected { "▰" } else { "▫" },
                        text(tab, "label")
                    ),
                    row_style,
                )
            } else if let Some(session) = row.session {
                Line::styled(
                    format!(
                        "    {} {}",
                        if selected { "›" } else { "┊" },
                        text(session, "label")
                    ),
                    row_style,
                )
            } else {
                Line::from(vec![
                    Span::styled(
                        if row.host["connected"] == true {
                            "  ● "
                        } else {
                            "  ○ "
                        },
                        if cursor {
                            row_style
                        } else {
                            row_style.fg(if row.host["connected"] == true {
                                MINT
                            } else {
                                MUTED
                            })
                        },
                    ),
                    Span::styled(text(row.host, "label"), row_style.bold()),
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
    let label = active
        .map(|host| text(host, "label"))
        .unwrap_or("Workspace");
    let block = Block::default()
        .title(Line::styled(format!(" {label} "), Style::new().fg(MUTED)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(BORDER))
        .style(Style::new().bg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let windows = items(&state["windows"]);
    let tabs = if windows.is_empty() {
        vec![Span::styled("  No tab selected", Style::new().fg(MUTED))]
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
                let name: String = text(tab, "label").chars().take(23).collect();
                let active = tab["id"] == state["window"];
                Span::styled(
                    format!(" {} {name} ", if active { "▰" } else { "▫" }),
                    if active {
                        Style::new().fg(BASE).bg(MINT).bold()
                    } else {
                        Style::new().fg(INK)
                    },
                )
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(Line::from(tabs)), inner);
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
    let background = rgb(&snapshot["bg"], BASE);
    let block = Block::default()
        .title(Line::styled(
            format!(
                " {} {} ",
                if focused { "▸" } else { "·" },
                text(pane, "label")
            ),
            Style::new().fg(if focused { MINT } else { MUTED }),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { MINT } else { BORDER }))
        .style(Style::new().bg(background));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if !snapshot.is_object() {
        frame.render_widget(
            Paragraph::new("  Waiting for terminal…").style(Style::new().fg(MUTED)),
            inner,
        );
        return inner;
    }
    let buffer = frame.buffer_mut();
    for (y, row) in items(&snapshot["cells"])
        .iter()
        .take(inner.height.into())
        .enumerate()
    {
        for (x, cell) in items(row).iter().take(inner.width.into()).enumerate() {
            if cell["spacer_tail"] == true {
                continue;
            }
            let mut style = Style::new()
                .fg(rgb(&cell["fg"], INK))
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
            let symbol = if cell["invisible"] == true || symbol.is_empty() {
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
