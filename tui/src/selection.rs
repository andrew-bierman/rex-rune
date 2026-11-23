use base64::{Engine as _, engine::general_purpose::STANDARD};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use serde_json::{Value, json};
use std::io::{self, Write};

use crate::{items, targeted};

const SCOPE_FIELDS: [&str; 4] = ["host", "epoch", "session", "window"];

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Grid {
    pub pane: String,
    pub generation: u64,
    pub area: Rect,
}

impl Grid {
    pub fn new(pane: &Value, mut area: Rect) -> Option<Self> {
        let frame = &pane["frame"];
        area.width = area.width.min(u16::try_from(frame["cols"].as_u64()?).ok()?);
        area.height = area
            .height
            .min(u16::try_from(frame["rows"].as_u64()?).ok()?);
        if area.is_empty() || pane["terminal"]["connected"] != true {
            return None;
        }
        Some(Self {
            pane: pane["pane"].as_str()?.to_owned(),
            generation: pane["terminal"]["generation"].as_u64()?,
            area,
        })
    }
}

struct Gesture {
    grid: Grid,
    target: Value,
    first_revision: u64,
    final_revision: Option<u64>,
    copy_requested: bool,
    anchor: Position,
    at: Position,
    reverse: bool,
}

impl Gesture {
    fn pane<'a>(&self, state: &'a Value) -> Option<&'a Value> {
        if state["connected"] != true
            || state["hosts"]["pending"] == true
            || SCOPE_FIELDS
                .iter()
                .any(|field| state[*field] != self.target[*field])
        {
            return None;
        }
        items(&state["panes"]).iter().find(|pane| {
            pane["pane"] == self.grid.pane
                && pane["terminal"]["generation"] == self.grid.generation
                && pane["terminal"]["connected"] == true
        })
    }

    fn clamp(&self, at: Position) -> Position {
        let area = self.grid.area;
        Position::new(
            at.x.clamp(area.left(), area.right() - 1),
            at.y.clamp(area.top(), area.bottom() - 1),
        )
    }

    fn request(&self, phase: &str, revision: u64) -> Value {
        let area = self.grid.area;
        let at = if phase == "down" {
            self.anchor
        } else {
            self.at
        };
        let x = at.x - area.left();
        let y = at.y - area.top();
        let mut request = self.target.clone();
        request["phase"] = json!(phase);
        request["clicks"] = json!(1);
        request["revision"] = json!(revision);
        request["generation"] = json!(self.grid.generation);
        request["x"] = json!(x);
        request["y"] = json!(y);
        // SGR reports cells. Span the whole endpoint cells so Ghostty's
        // pixel threshold includes both, using the same edge for a plain click.
        let right_edge = if phase == "down" || at == self.anchor {
            self.reverse
        } else {
            !self.reverse
        };
        request["px"] = json!(u32::from(x) * 8 + if right_edge { 7 } else { 0 });
        request["py"] = json!(u32::from(y) * 16 + 8);
        request["columns"] = json!(area.width);
        request["cell_width"] = json!(8);
        request["height"] = json!(u32::from(area.height) * 16);
        request
    }
}

#[derive(Default)]
pub(crate) struct Selection {
    gesture: Option<Gesture>,
    revision: u64,
}

impl Selection {
    pub fn reconcile(&mut self, state: &Value, grids: &[Grid], enabled: bool) -> Option<Value> {
        let gesture = self.gesture.as_ref()?;
        let visible = gesture.pane(state).is_some();
        if enabled && visible && grids.contains(&gesture.grid) {
            return None;
        }
        let gesture = self.gesture.take()?;
        if visible {
            self.revision += 1;
            return Some(gesture.request("clear", self.revision));
        }
        None
    }

    pub fn mouse(&mut self, state: &Value, grids: &[Grid], event: MouseEvent) -> Vec<Value> {
        if event.modifiers.contains(KeyModifiers::SHIFT) {
            return Vec::new();
        }
        let at = Position::new(event.column, event.row);
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            let Some(grid) = grids.iter().find(|grid| grid.area.contains(at)) else {
                return self.reconcile(state, grids, false).into_iter().collect();
            };
            let mut target = targeted(state, "selection");
            target["pane"] = json!(grid.pane);
            let gesture = Gesture {
                grid: grid.clone(),
                target,
                first_revision: self.revision + 1,
                final_revision: None,
                copy_requested: false,
                anchor: at,
                at,
                reverse: false,
            };
            if gesture.pane(state).is_none() {
                return Vec::new();
            }
            self.revision += 1;
            let request = gesture.request("down", self.revision);
            self.gesture = Some(gesture);
            return vec![request];
        }
        let release = match event.kind {
            MouseEventKind::Drag(MouseButton::Left) => false,
            MouseEventKind::Up(MouseButton::Left) => true,
            _ => return Vec::new(),
        };
        let Some(gesture) = self.gesture.as_mut() else {
            return Vec::new();
        };
        if gesture.final_revision.is_some()
            || !grids.contains(&gesture.grid)
            || gesture.pane(state).is_none()
        {
            return Vec::new();
        }

        let mut requests = Vec::new();
        let at = gesture.clamp(at);
        if at != gesture.at {
            let reverse = (at.y, at.x) < (gesture.anchor.y, gesture.anchor.x);
            if at != gesture.anchor && reverse != gesture.reverse {
                // Ghostty remembers the press pixel. Update it when the drag
                // crosses the anchor so the new direction includes that cell.
                gesture.reverse = reverse;
                self.revision += 1;
                requests.push(gesture.request("down", self.revision));
            }
            gesture.at = at;
            self.revision += 1;
            requests.push(gesture.request("move", self.revision));
        }
        if release {
            self.revision += 1;
            gesture.final_revision = Some(self.revision);
            requests.push(gesture.request("up", self.revision));
        }
        requests
    }

    pub fn highlights(&self, state: &Value, pane: &Value) -> bool {
        self.gesture.as_ref().is_some_and(|gesture| {
            pane["pane"] == gesture.grid.pane
                && gesture.pane(state).is_some()
                && pane["terminal"]["generation"] == gesture.grid.generation
                && pane["frame"]["selection_revision"]
                    .as_u64()
                    .is_some_and(|revision| revision >= gesture.first_revision)
        })
    }

    pub fn can_copy(&self, pane: &str) -> bool {
        self.gesture
            .as_ref()
            .is_some_and(|gesture| gesture.grid.pane == pane && gesture.final_revision.is_some())
    }

    pub fn request_copy(&mut self) -> Result<(), &'static str> {
        let gesture = self
            .gesture
            .as_mut()
            .ok_or("Select text inside a pane first")?;
        if gesture.final_revision.is_none() {
            return Err("Finish dragging before copying");
        }
        gesture.copy_requested = true;
        Ok(())
    }

    pub fn take_copy<'a>(&mut self, state: &'a Value) -> Option<Result<&'a str, &'static str>> {
        let gesture = self
            .gesture
            .as_mut()
            .filter(|gesture| gesture.copy_requested)?;
        let revision = gesture.final_revision?;
        let frame = &gesture.pane(state)?["frame"];
        let actual = frame["selection_revision"].as_u64()?;
        if actual < revision {
            return None;
        }
        gesture.copy_requested = false;
        if actual != revision || !frame["selection_error"].is_null() {
            return Some(Err("This pane selection could not be copied"));
        }
        Some(
            frame["selection_text"]
                .as_str()
                .filter(|text| !text.is_empty())
                .ok_or("Select text inside a pane first"),
        )
    }
}

pub(crate) fn write_clipboard(output: &mut impl Write, text: &str) -> io::Result<()> {
    write!(output, "\x1b]52;c;{}\x1b\\", STANDARD.encode(text))?;
    output.flush()
}

#[derive(Default)]
pub(crate) struct MouseCapture(bool);

impl MouseCapture {
    pub fn set(&mut self, enabled: bool) -> io::Result<()> {
        if self.0 != enabled {
            self.0 = enabled;
            let mut output = io::stdout().lock();
            // Button motion only, encoded using SGR coordinates.
            output.write_all(if enabled {
                b"\x1b[?1002h\x1b[?1006h"
            } else {
                b"\x1b[?1006l\x1b[?1002l"
            })?;
            output.flush()?;
        }
        Ok(())
    }
}

impl Drop for MouseCapture {
    fn drop(&mut self) {
        let _ = self.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Value, Vec<Grid>) {
        let state = json!({
            "host":"h","epoch":1,"session":"s","window":"w","pane":"right","connected":true,
            "panes":[
                {"pane":"left","terminal":{"connected":true,"generation":2},"frame":{"cols":12,"rows":5}},
                {"pane":"right","terminal":{"connected":true,"generation":3},"frame":{"cols":15,"rows":5}}
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
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn drag_stays_inside_its_original_visible_cells() {
        let (state, grids) = fixture();
        let mut selection = Selection::default();
        let mut down = mouse(MouseEventKind::Down(MouseButton::Left), 25, 5);
        down.modifiers = KeyModifiers::SHIFT;
        assert!(selection.mouse(&state, &grids, down).is_empty());
        down.modifiers = KeyModifiers::NONE;
        let request = selection.mouse(&state, &grids, down).pop().unwrap();
        assert_eq!(request["pane"], "left");
        assert_eq!(request["generation"], 2);
        let drag = mouse(MouseEventKind::Drag(MouseButton::Left), 2, 1);
        let request = selection.mouse(&state, &grids, drag).pop().unwrap();
        assert_eq!(
            (request["x"].as_u64(), request["y"].as_u64()),
            (Some(0), Some(0))
        );
        let up = mouse(MouseEventKind::Up(MouseButton::Left), 46, 12);
        let request = selection.mouse(&state, &grids, up).pop().unwrap();
        assert_eq!(request["pane"], "left");
        assert_eq!(
            (request["x"].as_u64(), request["y"].as_u64()),
            (Some(11), Some(4))
        );
    }

    #[test]
    fn drag_includes_both_endpoint_cells_when_crossing_the_anchor() {
        let (state, grids) = fixture();
        let mut selection = Selection::default();
        let mut send = |kind, column, row| {
            selection
                .mouse(&state, &grids, mouse(kind, column, row))
                .into_iter()
                .map(|packet| {
                    (
                        packet["phase"].as_str().unwrap().to_owned(),
                        packet["px"].as_u64().unwrap(),
                        packet["y"].as_u64().unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let left = MouseButton::Left;
        assert_eq!(
            send(MouseEventKind::Down(left), 24, 6),
            [("down".into(), 32, 2)]
        );
        assert_eq!(
            send(MouseEventKind::Drag(left), 27, 6),
            [("move".into(), 63, 2)]
        );
        // The higher column is earlier on a previous row, including a wrapped row.
        assert_eq!(
            send(MouseEventKind::Drag(left), 29, 5),
            [("down".into(), 39, 2), ("move".into(), 72, 1)]
        );
        // Coming back to the anchor crosses no part of that cell.
        assert_eq!(
            send(MouseEventKind::Drag(left), 24, 6),
            [("move".into(), 39, 2)]
        );
        // Release can supply the last endpoint without a preceding motion packet.
        assert_eq!(
            send(MouseEventKind::Up(left), 50, 12),
            [
                ("down".into(), 32, 2),
                ("move".into(), 95, 4),
                ("up".into(), 95, 4),
            ]
        );
    }

    #[test]
    fn copy_requires_release_ack_matches_pane_and_rejects_stale_context() {
        let (mut state, grids) = fixture();
        let mut selection = Selection::default();
        assert!(
            selection
                .mouse(
                    &state,
                    &grids,
                    mouse(MouseEventKind::Down(MouseButton::Left), 2, 5)
                )
                .is_empty()
        );
        selection.mouse(
            &state,
            &grids,
            mouse(MouseEventKind::Down(MouseButton::Left), 21, 5),
        );
        assert!(selection.request_copy().is_err());
        let request = selection
            .mouse(
                &state,
                &grids,
                mouse(MouseEventKind::Up(MouseButton::Left), 25, 5),
            )
            .pop()
            .unwrap();
        state["panes"][1]["frame"]["selection_revision"] = request["revision"].clone();
        state["panes"][1]["frame"]["selection_text"] = json!("right pane");
        selection.request_copy().unwrap();
        assert!(selection.take_copy(&state).is_none());
        state["panes"][0]["frame"]["selection_revision"] = request["revision"].clone();
        state["panes"][0]["frame"]["selection_text"] = json!("left pane");
        assert_eq!(selection.take_copy(&state).unwrap().unwrap(), "left pane");
        assert!(selection.take_copy(&state).is_none());
        state["panes"][0]["frame"]["selection_error"] = json!("copy too large");
        selection.request_copy().unwrap();
        assert!(selection.take_copy(&state).unwrap().is_err());
        state["epoch"] = json!(2);
        assert!(selection.reconcile(&state, &grids, true).is_none());
        assert!(selection.request_copy().is_err());
    }

    #[test]
    fn changed_layout_or_modal_clears_gesture() {
        let (state, mut grids) = fixture();
        let mut selection = Selection::default();
        let down = mouse(MouseEventKind::Down(MouseButton::Left), 21, 5);
        selection.mouse(&state, &grids, down);
        grids[0].area.width = 6;
        assert_eq!(
            selection.reconcile(&state, &grids, true).unwrap()["phase"],
            "clear"
        );
        selection.mouse(&state, &grids, down);
        assert_eq!(
            selection.reconcile(&state, &grids, false).unwrap()["phase"],
            "clear"
        );
        assert!(selection.request_copy().is_err());
    }

    #[test]
    fn osc52_wraps_the_exact_text_without_raw_control_bytes() {
        let text = "café\n\x1b]52;c;not-a-command\x07";
        let mut encoded = Vec::new();
        write_clipboard(&mut encoded, text).unwrap();
        let payload = encoded
            .strip_prefix(b"\x1b]52;c;")
            .unwrap()
            .strip_suffix(b"\x1b\\")
            .unwrap();
        assert_eq!(STANDARD.decode(payload).unwrap(), text.as_bytes());
    }
}
