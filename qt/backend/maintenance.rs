use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

pub const AGENTS: &[(&str, &str)] = &[
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("opencode", "OpenCode"),
    ("pi", "Pi"),
    ("omp", "OMP (shares Pi skills)"),
];

#[derive(Clone)]
struct Change {
    agent: String,
    remove: bool,
    naming: bool,
    force: bool,
}
impl Change {
    fn args(&self, preview: bool) -> Vec<String> {
        let agent = if self.agent == "omp" {
            "pi"
        } else {
            &self.agent
        };
        let mut args = vec![
            "--autostart=false".into(),
            "skill".into(),
            if self.remove { "remove" } else { "setup" }.into(),
            format!("--agent={agent}"),
        ];
        if !self.remove {
            args.push(format!("--session-naming={}", self.naming));
            if self.force {
                args.push("--force".into());
            }
        }
        if preview {
            args.push("--dry-run".into());
        }
        args.push("--json".into());
        args
    }
}
// Skill operations only inspect or update local integration files. Thirty seconds
// allows slow local storage while ensuring a stalled helper releases busy state.
const REX_TIMEOUT: Duration = Duration::from_secs(30);

fn rex(args: &[String]) -> Result<Value> {
    let result = rex_with_timeout(std::ffi::OsStr::new("rex"), args, REX_TIMEOUT);
    match result {
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            match rex_rune::platform::tools::off_path("rex") {
                Some(path) => rex_with_timeout(path.as_os_str(), args, REX_TIMEOUT),
                None => Err(error),
            }
        }
        result => result,
    }
}

fn rex_with_timeout(
    program: &std::ffi::OsStr,
    args: &[String],
    timeout: Duration,
) -> Result<Value> {
    // Files avoid waiting on pipe readers if a helper leaves inherited handles open.
    let mut stdout = tempfile::tempfile().context("Cannot capture local rex output")?;
    let mut stderr = tempfile::tempfile().context("Cannot capture local rex errors")?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()
        .context("Cannot run local rex command")?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => {
                let killed = child.kill();
                let reaped = child.wait();
                killed.context("Cannot stop local rex command")?;
                reaped.context("Cannot reap local rex command")?;
                if let Err(error) = result {
                    return Err(error).context("Cannot wait for local rex command");
                }
                bail!(
                    "Local rex command timed out. Changes may be partially applied; refresh installed state before trying again."
                );
            }
        }
    };
    let mut output = Vec::new();
    let mut errors = Vec::new();
    stdout.seek(SeekFrom::Start(0))?;
    stdout.read_to_end(&mut output)?;
    stderr.seek(SeekFrom::Start(0))?;
    stderr.read_to_end(&mut errors)?;
    ensure!(
        status.success(),
        "rex skill failed: {}",
        String::from_utf8_lossy(&errors)
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("command failed")
    );
    serde_json::from_slice(&output).context("Invalid rex skill JSON response")
}
struct Reply {
    kind: &'static str,
    result: Result<Value>,
    change: Option<Change>,
    generation: u64,
}
#[derive(Default)]
pub struct Maintenance {
    state: Value,
    rx: Option<Receiver<Reply>>,
    preview: Option<Change>,
    generation: u64,
}
impl Maintenance {
    pub fn snapshot(&self) -> Value {
        let mut state = if self.state.is_object() {
            self.state.clone()
        } else {
            json!({})
        };
        state["busy"] = json!(self.rx.is_some());
        state["agents"] = json!(
            AGENTS
                .iter()
                .map(|(id, name)| json!({"id":id,"name":name}))
                .collect::<Vec<_>>()
        );
        state["report"] = json!(report());
        state
    }
    pub fn request(&mut self, request: &Value, config: &Path) -> Result<()> {
        let action = request["action"]
            .as_str()
            .context("Missing maintenance action")?;
        if action == "cancel" {
            self.generation += 1;
            self.preview = None;
            self.state["preview"] = Value::Null;
            return Ok(());
        }
        ensure!(self.rx.is_none(), "Maintenance operation already running");
        self.state["error"] = Value::Null;
        self.state["message"] = Value::Null;
        let (kind, change) = match action {
            "status" => ("status", None),
            "preview" => {
                let agent = request["args"]["agent"]
                    .as_str()
                    .context("Select an agent")?;
                ensure!(AGENTS.iter().any(|(id, _)| *id == agent), "Unknown agent");
                (
                    "preview",
                    Some(Change {
                        agent: agent.into(),
                        remove: request["args"]["remove"] == true,
                        naming: request["args"]["naming"] == true,
                        force: request["args"]["force"] == true,
                    }),
                )
            }
            "confirm" => (
                "apply",
                Some(
                    self.preview
                        .take()
                        .context("Preview a change before confirming")?,
                ),
            ),
            "check" => ("update", None),
            _ => bail!("Unknown maintenance action"),
        };
        self.preview = None;
        self.state["preview"] = Value::Null;
        let state_path: PathBuf = config.with_file_name("verified-updates.json");
        self.generation += 1;
        let generation = self.generation;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let result = if kind == "update" {
                check(&state_path)
            } else {
                rex(&change.as_ref().map_or_else(
                    || {
                        vec![
                            "--autostart=false".into(),
                            "skill".into(),
                            "status".into(),
                            "--json".into(),
                        ]
                    },
                    |c| c.args(kind == "preview"),
                ))
            };
            let _ = tx.send(Reply {
                kind,
                result,
                change,
                generation,
            });
        });
        Ok(())
    }
    pub fn poll(&mut self) -> bool {
        let Some(reply) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.rx = None;
        if reply.generation != self.generation {
            return true;
        }
        match reply.result {
            Ok(value) => {
                self.state[reply.kind] = value;
                if reply.kind == "preview" {
                    self.preview = reply.change;
                }
                if reply.kind == "apply" {
                    self.state["message"] =
                        json!("Rex completed the change. Refresh to read installed state.");
                }
            }
            Err(e) => self.state["error"] = json!(format!("{e:#}")),
        }
        true
    }
}
fn check(path: &Path) -> Result<Value> {
    let config = rex_rune::update::CheckConfig {
        state_path: path.into(),
        ..Default::default()
    };
    let (outcome, _) = rex_rune::update::check_for_updates(&config, true)?;
    use rex_rune::update::CheckOutcome;
    let (version, message) = match outcome {
        CheckOutcome::UpToDate { version } => {
            (version, "Reused Rune core matches the signed release.")
        }
        CheckOutcome::UpdateAvailable { version, .. } => {
            (version, "A newer signed Rune core release is available.")
        }
        CheckOutcome::UnsupportedFloor { latest, .. } => (
            latest,
            "The reused Rune core is below the supported update floor.",
        ),
    };
    Ok(
        json!({"version":version,"message":message,"guidance":"This is a separate Qt source build. Update its checkout and rebuild the Qt frontend and private backend together. Rune release packages and installers do not update this application."}),
    )
}
pub fn report() -> String {
    format!(
        "Rex Qt Quick — separate source build\nReused Rex Rune core {} (MIT)\nPlatform: {} / {}\nQt Quick frontend; Qt runtime version shown separately.\nNo installer: rebuild Qt and backend together.\nProtocol/terminal: rexrs 0.1.0 (MIT OR Apache-2.0), ghostty-vt 0.8.0 and Ghostty VT (Apache-2.0); fonts: IBM Plex Mono (OFL-1.1), Symbols Nerd Font 3.5.1 (MIT and bundled upstream notices).\nReport intentionally contains no host, endpoint, session, credential, path or environment data.",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

#[cfg(all(test, target_os = "linux"))]
#[path = "maintenance_tests.rs"]
mod tests;
