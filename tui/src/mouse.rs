use crate::{items, selection::Grid, settings::Chrome, targeted, text};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Clear, Paragraph},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) struct Target {
    grid: Grid,
    scope: Value,
}

impl Target {
    fn at(state: &Value, grids: &[Grid], at: Position) -> Option<Self> {
        let grid = grids.iter().find(|grid| grid.area.contains(at))?.clone();
        Self::for_grid(state, grid)
    }

    pub fn for_grid(state: &Value, grid: Grid) -> Option<Self> {
        let mut scope = targeted(state, "select_pane");
        scope["pane"] = json!(grid.pane);
        let target = Self { grid, scope };
        target.pane(state)?;
        Some(target)
    }

    pub fn pane<'a>(&self, state: &'a Value) -> Option<&'a Value> {
        if state["connected"] != true
            || state["hosts"]["pending"] == true
            || ["host", "epoch", "session", "window"]
                .iter()
                .any(|key| self.scope[*key] != state[*key])
        {
            return None;
        }
        items(&state["panes"]).iter().find(|pane| {
            pane["pane"] == self.grid.pane
                && pane["terminal"]["connected"] == true
                && pane["terminal"]["generation"] == self.grid.generation
        })
    }

    pub fn visible(&self, state: &Value, grids: &[Grid]) -> bool {
        self.pane(state).is_some() && grids.contains(&self.grid)
    }

    pub fn selected(&self, state: &Value) -> bool {
        state["pane"] == self.scope["pane"]
    }

    pub fn id(&self) -> &str {
        &self.grid.pane
    }
}

pub(crate) fn wheel(state: &Value, grids: &[Grid], event: MouseEvent) -> Option<Value> {
    if !event.modifiers.is_empty() {
        return None;
    }
    let direction = match event.kind {
        MouseEventKind::ScrollUp => -1,
        MouseEventKind::ScrollDown => 1,
        _ => return None,
    };
    let at = Position::new(event.column, event.row);
    let target = Target::at(state, grids, at)?;
    let pane = target.pane(state)?;
    let mut request = target.scope;
    if pane["frame"]["mouse_tracking"] == true && state["pane"] == request["pane"] {
        // The shared backend accepts remote wheel input only for its selected pane.
        request["op"] = json!("wheel");
        request["payload"] = json!({
            "x": at.x - target.grid.area.x,
            "y": at.y - target.grid.area.y,
            "delta_x": 0,
            "delta_y": direction,
        });
    } else {
        request["op"] = json!("scroll");
        request["lines"] = json!(3 * direction);
    }
    Some(request)
}

pub(crate) struct Focus {
    target: Target,
    request_id: u64,
    deadline: Instant,
}

pub(crate) enum FocusStatus {
    Pending,
    Selected,
    Rejected,
}

impl Focus {
    pub fn start(target: Target, request_id: u64) -> (Self, Value) {
        let mut request = target.scope.clone();
        request["request_id"] = json!(request_id);
        (
            Self {
                target,
                request_id,
                deadline: Instant::now() + Duration::from_secs(2),
            },
            request,
        )
    }

    pub fn status(&self, state: &Value, rejected: Option<u64>) -> FocusStatus {
        if rejected == Some(self.request_id) || self.target.pane(state).is_none() {
            FocusStatus::Rejected
        } else if self.target.selected(state) {
            FocusStatus::Selected
        } else if Instant::now() >= self.deadline {
            FocusStatus::Rejected
        } else {
            FocusStatus::Pending
        }
    }
}

struct Press {
    target: Target,
    event: MouseEvent,
    dragged: bool,
}

#[derive(Default)]
pub(crate) struct Pointer {
    press: Option<Press>,
}

pub(crate) enum PointerAction {
    None,
    Selection(MouseEvent),
    BeginDrag(MouseEvent, MouseEvent),
    Focus(Target),
    Menu(Menu),
}

impl Pointer {
    pub fn cancel(&mut self) {
        self.press = None;
    }

    pub fn event(&mut self, state: &Value, grids: &[Grid], event: MouseEvent) -> PointerAction {
        if !event.modifiers.is_empty() {
            self.cancel();
            return PointerAction::None;
        }
        let at = Position::new(event.column, event.row);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.press = Target::at(state, grids, at).map(|target| Press {
                    target,
                    event,
                    dragged: false,
                });
                if self.press.is_none() {
                    return PointerAction::Selection(event);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(press) = &mut self.press
                    && press.target.visible(state, grids)
                {
                    if press.dragged {
                        return PointerAction::Selection(event);
                    }
                    if (press.event.column, press.event.row) != (event.column, event.row) {
                        press.dragged = true;
                        return PointerAction::BeginDrag(press.event, event);
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(press) = self.press.take()
                    && press.target.visible(state, grids)
                {
                    if press.dragged {
                        return PointerAction::Selection(event);
                    }
                    if (press.event.column, press.event.row) == (event.column, event.row) {
                        return PointerAction::Focus(press.target);
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) if self.press.is_none() => {
                if let Some(target) = Target::at(state, grids, at) {
                    return PointerAction::Menu(Menu {
                        target,
                        at,
                        cursor: 0,
                        pressed: None,
                    });
                }
            }
            _ => {}
        }
        PointerAction::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Focus,
    Copy,
    SplitRight,
    SplitDown,
}

impl Choice {
    const ITEMS: [(Self, &'static str); 4] = [
        (Self::Focus, "Focus pane"),
        (Self::Copy, "Copy selection"),
        (Self::SplitRight, "Split right"),
        (Self::SplitDown, "Split down"),
    ];

    fn enabled(self, selected: bool, can_copy: bool) -> bool {
        match self {
            Self::Focus => true,
            Self::Copy => can_copy,
            Self::SplitRight | Self::SplitDown => selected,
        }
    }
}

pub(crate) struct Menu {
    pub target: Target,
    at: Position,
    cursor: usize,
    pressed: Option<usize>,
}

pub(crate) enum MenuAction {
    Keep,
    Dismiss,
    Choose(Choice),
}

impl Menu {
    fn bounds(&self, area: Rect) -> Rect {
        let width = area.width.min(34);
        let height = area.height.min(7);
        Rect::new(
            self.at.x.clamp(area.left(), area.right() - width),
            self.at.y.clamp(area.top(), area.bottom() - height),
            width,
            height,
        )
    }

    fn choose(&self, selected: bool, can_copy: bool) -> MenuAction {
        let choice = Choice::ITEMS[self.cursor].0;
        if choice.enabled(selected, can_copy) {
            MenuAction::Choose(choice)
        } else {
            MenuAction::Keep
        }
    }

    pub fn key(&mut self, event: KeyEvent, selected: bool, can_copy: bool) -> MenuAction {
        if event.kind != KeyEventKind::Press || !event.modifiers.is_empty() {
            return MenuAction::Keep;
        }
        match event.code {
            KeyCode::Esc => return MenuAction::Dismiss,
            KeyCode::Up => {
                if let Some(index) = (0..self.cursor)
                    .rev()
                    .find(|index| Choice::ITEMS[*index].0.enabled(selected, can_copy))
                {
                    self.cursor = index;
                }
            }
            KeyCode::Down => {
                if let Some(index) = (self.cursor + 1..Choice::ITEMS.len())
                    .find(|index| Choice::ITEMS[*index].0.enabled(selected, can_copy))
                {
                    self.cursor = index;
                }
            }
            KeyCode::Enter => return self.choose(selected, can_copy),
            _ => {}
        }
        MenuAction::Keep
    }

    pub fn mouse(
        &mut self,
        event: MouseEvent,
        area: Rect,
        selected: bool,
        can_copy: bool,
    ) -> MenuAction {
        if !event.modifiers.is_empty() {
            return MenuAction::Keep;
        }
        let bounds = self.bounds(area);
        let at = Position::new(event.column, event.row);
        let index = (bounds.contains(at) && at.x > bounds.x && at.x + 1 < bounds.right())
            .then(|| at.y.saturating_sub(bounds.y + 1) as usize)
            .filter(|index| event.row > bounds.y && *index < Choice::ITEMS.len());
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) if !bounds.contains(at) => {
                return MenuAction::Dismiss;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.pressed =
                    index.filter(|index| Choice::ITEMS[*index].0.enabled(selected, can_copy));
                if let Some(index) = self.pressed {
                    self.cursor = index;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let pressed = self.pressed.take();
                if pressed.is_some() && index == pressed {
                    return self.choose(selected, can_copy);
                }
            }
            _ => {}
        }
        MenuAction::Keep
    }

    pub fn draw(&self, frame: &mut Frame<'_>, state: &Value, chrome: Chrome, can_copy: bool) {
        let Some(pane) = self.target.pane(state) else {
            return;
        };
        let area = frame.area();
        if area.width < 38 || area.height < 8 {
            return;
        }
        let bounds = self.bounds(area);
        let selected = self.target.selected(state);
        let label: String = text(pane, "label")
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        let title = if label.is_empty() {
            " Pane ".into()
        } else {
            format!(" {label} ")
        };
        let mut lines: Vec<_> = Choice::ITEMS
            .iter()
            .enumerate()
            .map(|(index, (choice, label))| {
                if !choice.enabled(selected, can_copy) {
                    return Line::styled(
                        format!("  {label} (unavailable)"),
                        Style::new().fg(chrome.muted),
                    );
                }
                if self.cursor == index {
                    Line::styled(format!("› {label}"), chrome.selection())
                } else {
                    Line::raw(format!("  {label}"))
                }
            })
            .collect();
        lines.push(Line::styled("Esc Dismiss", Style::new().fg(chrome.muted)));
        frame.render_widget(Clear, bounds);
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::bordered()
                    .title(title)
                    .style(chrome.style())
                    .border_style(Style::new().fg(chrome.accent)),
            ),
            bounds,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Value, Vec<Grid>) {
        let state = json!({
            "host":"h","epoch":1,"session":"s","window":"w","pane":"left","connected":true,
            "panes":[
                {"pane":"left","label":"Left shell","terminal":{"connected":true,"generation":2},"frame":{"cols":12,"rows":5}},
                {"pane":"right","label":"Right shell","terminal":{"connected":true,"generation":3},"frame":{"cols":15,"rows":5}}
            ]
        });
        let grids = vec![
            Grid::new(&state["panes"][0], Rect::new(20, 4, 15, 7)).unwrap(),
            Grid::new(&state["panes"][1], Rect::new(35, 4, 15, 7)).unwrap(),
        ];
        (state, grids)
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    #[test]
    fn wheel_is_pane_scoped_and_only_selected_tracking_apps_receive_remote_ticks() {
        let (mut state, grids) = fixture();
        let up = mouse(MouseEventKind::ScrollUp, 37, 6);
        let request = wheel(&state, &grids, up).unwrap();
        assert_eq!(request["op"], "scroll");
        assert_eq!(request["lines"], -3);
        assert_eq!(request["pane"], "right");
        state["panes"][1]["frame"]["mouse_tracking"] = json!(true);
        assert_eq!(wheel(&state, &grids, up).unwrap()["op"], "scroll");
        state["pane"] = json!("right");
        let down = mouse(MouseEventKind::ScrollDown, 37, 6);
        let request = wheel(&state, &grids, down).unwrap();
        assert_eq!(request["op"], "wheel");
        assert_eq!(
            request["payload"],
            json!({"x":2,"y":2,"delta_x":0,"delta_y":1})
        );
        let mut shift = up;
        shift.modifiers = crossterm::event::KeyModifiers::SHIFT;
        assert!(wheel(&state, &grids, shift).is_none());
        assert!(wheel(&state, &grids, mouse(MouseEventKind::ScrollUp, 2, 6)).is_none());
        state["hosts"]["pending"] = json!(true);
        assert!(wheel(&state, &grids, up).is_none());
    }

    #[test]
    fn click_waits_for_acknowledgment_and_targets_the_clicked_pane() {
        let (mut state, grids) = fixture();
        let mut pointer = Pointer::default();
        let down = mouse(MouseEventKind::Down(MouseButton::Left), 37, 7);
        assert!(matches!(
            pointer.event(&state, &grids, down),
            PointerAction::None
        ));
        let up = mouse(MouseEventKind::Up(MouseButton::Left), 37, 7);
        let PointerAction::Focus(target) = pointer.event(&state, &grids, up) else {
            panic!("Click should request focus");
        };
        let (focus, request) = Focus::start(target, 18);
        assert_eq!(request["pane"], "right");
        assert_eq!(request["request_id"], 18);
        assert!(matches!(focus.status(&state, None), FocusStatus::Pending));
        assert!(matches!(
            focus.status(&state, Some(18)),
            FocusStatus::Rejected
        ));
        state["pane"] = json!("right");
        assert!(matches!(focus.status(&state, None), FocusStatus::Selected));
        state["epoch"] = json!(2);
        assert!(matches!(focus.status(&state, None), FocusStatus::Rejected));
    }

    #[test]
    fn dragging_crosses_a_pane_without_retargeting_input() {
        let (state, grids) = fixture();
        let mut pointer = Pointer::default();
        let down = mouse(MouseEventKind::Down(MouseButton::Left), 37, 7);
        pointer.event(&state, &grids, down);
        let drag = mouse(MouseEventKind::Drag(MouseButton::Left), 21, 7);
        assert!(
            matches!(pointer.event(&state, &grids, drag), PointerAction::BeginDrag(origin, _) if origin.column == 37)
        );
        let up = mouse(MouseEventKind::Up(MouseButton::Left), 37, 7);
        assert!(matches!(
            pointer.event(&state, &grids, up),
            PointerAction::Selection(_)
        ));
    }

    #[test]
    fn stale_release_and_shift_never_focus_or_open_the_menu() {
        let (mut state, grids) = fixture();
        let mut pointer = Pointer::default();
        pointer.event(
            &state,
            &grids,
            mouse(MouseEventKind::Down(MouseButton::Left), 37, 7),
        );
        state["window"] = json!("other");
        assert!(matches!(
            pointer.event(
                &state,
                &grids,
                mouse(MouseEventKind::Up(MouseButton::Left), 37, 7)
            ),
            PointerAction::None
        ));
        let mut right = mouse(MouseEventKind::Down(MouseButton::Right), 37, 7);
        right.modifiers = crossterm::event::KeyModifiers::SHIFT;
        assert!(matches!(
            pointer.event(&state, &grids, right),
            PointerAction::None
        ));
    }

    #[test]
    fn menu_marks_unavailable_actions_and_cannot_execute_them() {
        let (state, grids) = fixture();
        let mut pointer = Pointer::default();
        let PointerAction::Menu(mut menu) = pointer.event(
            &state,
            &grids,
            mouse(MouseEventKind::Down(MouseButton::Right), 37, 7),
        ) else {
            panic!("Right-click should open menu");
        };
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        let frame = terminal
            .draw(|frame| menu.draw(frame, &state, Chrome::default(), false))
            .unwrap();
        let content: String = frame
            .buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("Right shell"));
        assert!(content.contains("Split right (unavailable)"));
        let area = Rect::new(0, 0, 80, 24);
        let down = mouse(MouseEventKind::Down(MouseButton::Left), 40, 10);
        let up = mouse(MouseEventKind::Up(MouseButton::Left), 40, 10);
        assert!(matches!(
            menu.mouse(down, area, false, false),
            MenuAction::Keep
        ));
        assert!(matches!(
            menu.mouse(up, area, false, false),
            MenuAction::Keep
        ));
        assert!(matches!(
            menu.mouse(down, area, true, false),
            MenuAction::Keep
        ));
        assert!(matches!(
            menu.mouse(up, area, true, false),
            MenuAction::Choose(Choice::SplitRight)
        ));
    }
}
