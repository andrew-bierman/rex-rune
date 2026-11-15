//! Frontend requests mapped to the public terminal worker interface.
use anyhow::{Context, Result, bail, ensure};
use rex_rune::{
    connection::Command,
    terminal::selection::{Geometry, Input, Phase},
};
use serde_json::Value;

pub fn command(r: &Value) -> Result<Command> {
    let integer = |name: &str| r[name].as_u64().with_context(|| format!("invalid {name}"));
    Ok(match r["op"].as_str().context("missing op")? {
        "scroll" => Command::Scroll(
            r["lines"]
                .as_i64()
                .context("invalid lines")?
                .clamp(-65535, 65535) as isize,
        ),
        "search" => Command::Search(
            r["text"]
                .as_str()
                .context("missing search text")?
                .to_owned(),
        ),
        "search_step" => Command::SearchStep(r["older"].as_bool().context("invalid direction")?),
        "prompt_step" => Command::PromptStep(r["older"].as_bool().context("invalid direction")?),
        "mouse" | "wheel" => {
            ensure!(r["payload"].is_object(), "missing pointer payload");
            if r["op"] == "mouse" {
                Command::Mouse(r["payload"].clone())
            } else {
                Command::Wheel(r["payload"].clone())
            }
        }
        "selection" => {
            let phase = match r["phase"].as_str() {
                Some("down") => Phase::Down(integer("clicks")?.min(3) as u32),
                Some("move") => Phase::Move,
                Some("up") => Phase::Up,
                Some("all") => Phase::All,
                Some("clear") => Phase::Clear,
                _ => bail!("invalid selection phase"),
            };
            Command::Selection(Input {
                generation: integer("generation")?,
                revision: integer("revision")?,
                phase,
                at: (
                    integer("x")?.min(65535) as u16,
                    integer("y")?.min(65535) as u16,
                ),
                position: (
                    r["px"].as_f64().context("invalid px")?,
                    r["py"].as_f64().context("invalid py")?,
                ),
                geometry: Geometry {
                    columns: integer("columns")?.clamp(1, 65535) as u32,
                    cell_width: integer("cell_width")?.clamp(1, 65535) as u32,
                    padding_left: 0,
                    screen_height: integer("height")?.clamp(1, 65535) as u32,
                },
            })
        }
        _ => bail!("unknown terminal operation"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selection_preserves_generation_revision_and_geometry() {
        let value = json!({"op":"selection","phase":"down","clicks":2,
            "generation":9,"revision":27,"x":4,"y":3,"px":45.5,"py":71.0,
            "columns":80,"cell_width":10,"height":400});
        let Command::Selection(input) = command(&value).unwrap() else {
            panic!("selection")
        };
        assert_eq!(
            (input.generation, input.revision, input.at, input.position),
            (9, 27, (4, 3), (45.5, 71.0))
        );
        assert!(matches!(input.phase, Phase::Down(2)));
        assert_eq!(input.geometry.columns, 80);
        assert!(command(&json!({"op":"selection","phase":"invented"})).is_err());
    }

    #[test]
    fn search_directions_and_empty_cancel_reach_public_commands() {
        assert!(
            matches!(command(&json!({"op":"search","text":""})).unwrap(), Command::Search(s) if s.is_empty())
        );
        assert!(matches!(
            command(&json!({"op":"search_step","older":true})).unwrap(),
            Command::SearchStep(true)
        ));
        assert!(matches!(
            command(&json!({"op":"prompt_step","older":false})).unwrap(),
            Command::PromptStep(false)
        ));
    }
}
