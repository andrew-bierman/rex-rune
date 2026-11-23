use crate::{items, mouse::Target, selection::Grid, settings::Chrome};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::Line,
    widgets::{Block, Clear, Paragraph},
};
use serde_json::Value;

// The shared backend canonicalizes URLs. Keep the desktop dispatch boundary
// narrower than the backend's policy and accept only HTTP(S) arguments.
pub(crate) fn web_url(uri: &str) -> bool {
    uri.strip_prefix("https://")
        .or_else(|| uri.strip_prefix("http://"))
        .is_some_and(|rest| {
            !rest.is_empty()
                && !rest.starts_with(['/', '?', '#', '\\'])
                && uri.bytes().all(|byte| byte.is_ascii_graphic())
        })
}

#[derive(Clone, PartialEq, Eq)]
struct Link {
    uri: String,
    start: usize,
    end: usize,
    columns: usize,
}

fn visible_links<'a>(pane: &'a Value, grid: &'a Grid) -> impl Iterator<Item = Link> + 'a {
    let snapshot = &pane["frame"];
    let width = usize::from(grid.area.width);
    let height = usize::from(grid.area.height);
    items(&snapshot["links"]).iter().filter_map(move |item| {
        let uri = item["uri"].as_str().filter(|uri| web_url(uri))?;
        let start = usize::try_from(item["start"].as_u64()?).ok()?;
        let end = usize::try_from(item["end"].as_u64()?).ok()?;
        let columns = usize::from(u16::try_from(snapshot["cols"].as_u64()?).ok()?);
        let rows = usize::from(u16::try_from(snapshot["rows"].as_u64()?).ok()?);
        if width == 0 || height == 0 || start > end || end >= columns * rows || columns < width {
            return None;
        }
        let first = start / columns;
        let visible = (first..=(end / columns).min(height - 1))
            .any(|row| start.max(row * columns) <= end.min(row * columns + width - 1));
        visible.then(|| Link {
            uri: uri.into(),
            start,
            end,
            columns,
        })
    })
}

fn link_at(pane: &Value, grid: &Grid, at: Position) -> Option<Link> {
    if !grid.area.contains(at) {
        return None;
    }
    visible_links(pane, grid).find(|link| {
        let index =
            usize::from(at.y - grid.area.y) * link.columns + usize::from(at.x - grid.area.x);
        (link.start..=link.end).contains(&index)
    })
}

struct Click {
    target: Target,
    grid: Grid,
    link: Link,
}

#[derive(Default)]
pub(crate) struct LinkClick(Option<Click>);

impl LinkClick {
    pub(crate) fn cancel(&mut self) {
        self.0 = None;
    }

    pub(crate) fn event(
        &mut self,
        state: &Value,
        grids: &[Grid],
        event: MouseEvent,
    ) -> Option<Option<String>> {
        if !matches!(
            event.kind,
            MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Up(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left)
        ) {
            return None;
        }
        let pressed = self.0.take();
        if event.modifiers != KeyModifiers::CONTROL {
            return pressed.map(|_| None);
        }
        let at = Position::new(event.column, event.row);
        match event.kind {
            MouseEventKind::Down(_) => {
                self.0 = grids
                    .iter()
                    .find(|grid| grid.area.contains(at))
                    .and_then(|grid| {
                        let target = Target::for_grid(state, grid.clone())?;
                        let link = link_at(target.pane(state)?, grid, at)?;
                        Some(Click {
                            target,
                            grid: grid.clone(),
                            link,
                        })
                    });
                Some(None)
            }
            MouseEventKind::Up(_) => Some(pressed.and_then(|click| {
                if !click.target.visible(state, grids) {
                    return None;
                }
                let current = link_at(click.target.pane(state)?, &click.grid, at)?;
                (current == click.link).then_some(current.uri)
            })),
            MouseEventKind::Drag(_) => Some(None),
            _ => None,
        }
    }
}

pub(crate) struct Picker {
    target: Target,
    grid: Grid,
    links: Vec<Link>,
    cursor: usize,
}

pub(crate) enum Pick {
    Continue,
    Close,
    Changed,
    Open(String),
}

impl Picker {
    pub(crate) fn new(state: &Value, grids: &[Grid]) -> Option<Self> {
        let grid = grids
            .iter()
            .find(|grid| state["pane"] == grid.pane)?
            .clone();
        let target = Target::for_grid(state, grid.clone())?;
        let links: Vec<_> = visible_links(target.pane(state)?, &grid).collect();
        (!links.is_empty()).then_some(Self {
            target,
            grid,
            links,
            cursor: 0,
        })
    }

    pub(crate) fn key(
        &mut self,
        state: &Value,
        grids: &[Grid],
        key: KeyEvent,
        visible: bool,
    ) -> Pick {
        if key.kind == KeyEventKind::Release || !key.modifiers.is_empty() {
            return Pick::Continue;
        }
        match key.code {
            KeyCode::Esc => return Pick::Close,
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down => self.cursor = (self.cursor + 1).min(self.links.len() - 1),
            KeyCode::Enter if key.kind == KeyEventKind::Press && visible => {
                let current = &self.links[self.cursor];
                if self.target.selected(state)
                    && self.target.visible(state, grids)
                    && self.target.pane(state).is_some_and(|pane| {
                        visible_links(pane, &self.grid).any(|link| &link == current)
                    })
                {
                    return Pick::Open(current.uri.clone());
                }
                return Pick::Changed;
            }
            _ => {}
        }
        Pick::Continue
    }

    pub(crate) fn draw(&self, frame: &mut Frame<'_>, chrome: Chrome) {
        let area = frame.area();
        if area.width < 38 || area.height < 8 {
            return;
        }
        let width = area.width.min(92);
        let count = self.links.len().min(usize::from(area.height - 4));
        let height = count as u16 + 3;
        let bounds = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let start = self.cursor.saturating_add(1).saturating_sub(count);
        let lines = self
            .links
            .iter()
            .enumerate()
            .skip(start)
            .take(count)
            .map(|(index, link)| {
                Line::styled(
                    link.uri.as_str(),
                    if index == self.cursor {
                        chrome.selection()
                    } else {
                        chrome.style()
                    },
                )
            })
            .chain([Line::raw("↑↓ Choose  Enter Open  Esc Close")])
            .collect::<Vec<_>>();
        frame.render_widget(Clear, bounds);
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::bordered()
                    .title(" Pane links ")
                    .style(chrome.style()),
            ),
            bounds,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (Value, [Grid; 1]) {
        let state = json!({
            "host":"h","epoch":2,"session":"s","window":"w","pane":"p","connected":true,
            "panes":[{"pane":"p","terminal":{"connected":true,"generation":1},
                "frame":{"cols":10,"rows":3,"links":[
                    {"uri":"https://example.test/label","start":1,"end":3},
                    {"uri":"http://example.test/wrapped","start":8,"end":11},
                    {"uri":"file:///tmp/no","start":12,"end":13},
                    {"uri":"https://example.test/clipped","start":19,"end":19}
                ]}}]
        });
        let grid = Grid::new(&state["panes"][0], Rect::new(3, 5, 5, 2)).unwrap();
        (state, [grid])
    }

    fn mouse(kind: MouseEventKind, modifiers: KeyModifiers) -> MouseEvent {
        MouseEvent { kind, column: 4, row: 5, modifiers }
    }

    #[test]
    fn control_click_requires_an_unchanged_visible_link_and_leaves_selection_gestures_alone() {
        let (state, grids) = fixture();
        let mut click = LinkClick::default();
        let left_down = MouseEventKind::Down(MouseButton::Left);
        let left_up = MouseEventKind::Up(MouseButton::Left);
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(click.event(&state, &grids, mouse(left_down, KeyModifiers::NONE)), None);
        assert_eq!(click.event(&state, &grids, mouse(left_down, KeyModifiers::SHIFT | ctrl)), None);

        assert_eq!(click.event(&state, &grids, mouse(left_down, ctrl)), Some(None));
        assert_eq!(click.event(&state, &grids, mouse(left_up, ctrl)), Some(Some("https://example.test/label".into())));
        click.event(&state, &grids, mouse(left_down, ctrl));
        let mut changed = state.clone();
        changed["panes"][0]["frame"]["links"][0]["uri"] = json!("https://example.test/replacement");
        assert_eq!(click.event(&changed, &grids, mouse(left_up, ctrl)), Some(None));
        click.event(&state, &grids, mouse(left_down, ctrl));
        click.event(&state, &grids, mouse(MouseEventKind::Drag(MouseButton::Left), ctrl));
        assert_eq!(click.event(&state, &grids, mouse(left_up, ctrl)), Some(None));
        click.event(&state, &grids, mouse(left_down, ctrl));
        changed["epoch"] = json!(3);
        assert_eq!(click.event(&changed, &grids, mouse(left_up, ctrl)), Some(None));
    }

    #[test]
    fn picker_only_offers_visible_web_links_and_revalidates_before_opening() {
        let (state, grids) = fixture();
        let mut picker = Picker::new(&state, &grids).unwrap();
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert!(matches!(picker.key(&state, &grids, key(KeyCode::Down), true), Pick::Continue));
        assert!(matches!(
            picker.key(&state, &grids, key(KeyCode::Enter), true),
            Pick::Open(uri) if uri == "http://example.test/wrapped"
        ));
        picker.key(&state, &grids, key(KeyCode::Down), true);
        assert!(matches!(picker.key(&state, &grids, key(KeyCode::Enter), true), Pick::Open(uri) if uri == "http://example.test/wrapped"));
        let mut changed = state.clone();
        changed["pane"] = json!("other");
        assert!(matches!(picker.key(&changed, &grids, key(KeyCode::Enter), true), Pick::Changed));
        for uri in ["file:///tmp/no", "javascript:alert(1)", "--help", "mailto:a@example.test", "https://", "https://x.test/\x1b", "https://x.test/a b"] {
            assert!(!web_url(uri), "{uri:?}");
        }
    }
}
