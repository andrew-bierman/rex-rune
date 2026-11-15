use anyhow::{Context, Result, bail, ensure};
use rex_rune::{
    connection::{HostCommand, HostModel, directory::Request},
    settings::{
        Settings,
        recent_paths::{PATH_MAX, refused, valid},
    },
    upload::File,
};
use rexrs::api::terminal::ListDir;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize, PartialEq, Eq)]
struct Target {
    epoch: u64,
    session: String,
    window: String,
    pane: String,
}
impl Target {
    fn check(&self, model: &HostModel) -> Result<()> {
        ensure!(
            model.connected && model.connection_epoch == self.epoch,
            "Connection changed; file action discarded."
        );
        ensure!(
            model.selected.as_ref() == Some(&self.session),
            "Session changed; file action discarded."
        );
        let window = model.window().context("Tab is no longer available.")?;
        ensure!(
            window.window_id.as_ref() == Some(&self.window)
                && window
                    .layers
                    .iter()
                    .filter(|l| l.state.as_deref() != Some("minimized"))
                    .flat_map(|l| &l.blocks)
                    .any(|b| b.block_id.as_ref() == Some(&self.pane)
                        && b.creator_name.as_deref() == Some("com.superlogical.terminal")
                        && window
                            .zoomed_block_id
                            .as_ref()
                            .is_none_or(|id| id == &self.pane)),
            "Pane changed; file action discarded."
        );
        Ok(())
    }
    fn value(&self) -> Value {
        json!({"epoch":self.epoch,"session":self.session,"window":self.window,"pane":self.pane})
    }
}
struct Browser {
    target: Target,
    endpoint: String,
    open: Arc<()>,
    text: String,
    go: bool,
    edited: bool,
    pending: Option<(u64, String)>,
    rows: Vec<Value>,
    truncated: bool,
    problem: String,
}
#[derive(Default)]
pub struct Files {
    browser: Option<Browser>,
    next: u64,
    download: Option<(Target, String, Option<u64>)>,
    cancelled: Vec<u64>,
    upload: Option<(Target, String, String)>,
}
#[derive(Default)]
pub struct Outcome {
    pub commands: Vec<HostCommand>,
    pub settings_changed: bool,
    pub paste: Option<String>,
}

pub fn quote(path: &str) -> Result<String> {
    ensure!(
        path.len() <= PATH_MAX && !path.chars().any(refused),
        "Paths with control or invisible formatting characters can't be inserted."
    );
    let path = if path.starts_with('-') {
        format!("./{path}")
    } else {
        path.into()
    };
    Ok(
        if !path.is_empty()
            && path
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/_-.".contains(&c))
        {
            path
        } else {
            format!("'{}'", path.replace('\'', "'\\''"))
        },
    )
}
fn split(text: &str) -> (&str, &str) {
    if text == "~" {
        ("~", "")
    } else {
        text.rfind('/')
            .map_or(("", text), |n| (&text[..=n], &text[n + 1..]))
    }
}
fn join(path: &str, name: &str) -> String {
    if name.is_empty() {
        path.into()
    } else {
        format!("{}/{name}", path.trim_end_matches('/'))
    }
}
impl Files {
    pub fn clear(&mut self) {
        self.browser = None;
        self.upload = None;
        if let Some((_, _, Some(id))) = self.download.take() {
            self.cancelled.push(id);
        }
    }
    fn read(&mut self) -> Result<HostCommand> {
        let b = self.browser.as_mut().context("Path browser is closed.")?;
        ensure!(
            b.text.len() <= PATH_MAX && !b.text.chars().any(refused),
            "Invalid path."
        );
        self.next = self.next.wrapping_add(1);
        let (path, prefix) = split(&b.text);
        b.pending = Some((self.next, prefix.into()));
        b.rows.clear();
        b.problem.clear();
        Ok(HostCommand::ListDir(Box::new(Request {
            id: self.next,
            epoch: b.target.epoch,
            session: b.target.session.clone(),
            block: b.target.pane.clone(),
            args: ListDir::new().path(path).prefix(prefix).files(!b.go),
            deadline: Instant::now() + Duration::from_secs(8),
            open: Arc::downgrade(&b.open),
        })))
    }
    pub fn handle(
        &mut self,
        model: &HostModel,
        settings: &mut Settings,
        endpoint: &str,
        request: &Value,
    ) -> Result<Outcome> {
        let op = request["op"].as_str().unwrap_or_default();
        if op == "download.close" {
            if let Some((_, _, Some(id))) = self.download.take() {
                self.cancelled.push(id);
            }
            return Ok(Outcome {
                commands: self
                    .cancelled
                    .drain(..)
                    .map(HostCommand::DownloadCancel)
                    .collect(),
                ..Default::default()
            });
        }
        if op == "download.cancel" {
            let id = request["id"].as_u64().context("Missing transfer.")?;
            ensure!(
                model
                    .downloads
                    .iter()
                    .any(|t| t.id == id && !t.state.is_final()),
                "Transfer is no longer running."
            );
            return Ok(Outcome {
                commands: vec![HostCommand::DownloadCancel(id)],
                ..Default::default()
            });
        }
        if op == "directory.close" {
            self.browser = None;
            return Ok(Outcome::default());
        }
        let target: Target =
            serde_json::from_value(request.clone()).context("Missing file target.")?;
        target.check(model)?;
        let mut out = Outcome::default();
        match op {
            "download.open" => {
                if let Some((_, _, Some(id))) = self.download.take() {
                    out.commands.push(HostCommand::DownloadCancel(id));
                }
                self.download = Some((target, endpoint.into(), None));
            }
            "download.start" | "download.confirm" => {
                ensure!(
                    model.protocol.api_revision >= rex_rune::download::MIN_API_REVISION,
                    "Update Rex on this host to download files (API 13 required)."
                );
                let (captured, host, id) = self
                    .download
                    .as_mut()
                    .context("Download dialog is closed.")?;
                ensure!(
                    *host == endpoint && captured.value() == target.value(),
                    "Download target changed."
                );
                if op == "download.confirm" {
                    let id = id.context("No pending download.")?;
                    ensure!(
                        model.downloads.iter().any(|t| t.id == id
                            && matches!(
                                t.state,
                                rex_rune::download::State::AwaitingConfirmation(_)
                            )),
                        "No pending confirmation."
                    );
                    out.commands.push(HostCommand::DownloadConfirm(id));
                } else {
                    ensure!(id.is_none(), "Download already requested.");
                    let path = request["path"]
                        .as_str()
                        .context("Missing remote path.")?
                        .trim();
                    ensure!(
                        !path.is_empty() && path.len() <= PATH_MAX,
                        "Enter the path of a file on the host."
                    );
                    let directory = request["directory"]
                        .as_str()
                        .context("Missing destination folder.")?
                        .trim();
                    let folder = if directory.is_empty() {
                        rex_rune::download::store::default_directory()
                    } else {
                        rex_rune::download::store::expand_home(directory)
                    };
                    ensure!(
                        folder.is_absolute() && !directory.chars().any(refused),
                        "Choose an absolute destination folder."
                    );
                    let next = rex_rune::download::next_id();
                    out.commands.push(HostCommand::Download(Box::new(
                        rex_rune::connection::downloads::Request::new(
                            next,
                            &target.session,
                            &target.pane,
                            path,
                            folder,
                        ),
                    )));
                    *id = Some(next);
                    settings.downloads.directory =
                        (!directory.is_empty()).then(|| directory.into());
                    out.settings_changed = true;
                }
            }
            "directory.open" => {
                let go = request["go"].as_bool().unwrap_or(false);
                self.browser = Some(Browser {
                    target,
                    endpoint: endpoint.into(),
                    open: Arc::new(()),
                    text: String::new(),
                    go,
                    edited: false,
                    pending: None,
                    rows: Vec::new(),
                    truncated: false,
                    problem: String::new(),
                });
                out.commands.push(self.read()?);
                if settings.terminal.show_recent_paths {
                    let b = self.browser.as_mut().unwrap();
                    if let Some(paths) = settings.recent_paths.get(endpoint) {
                        b.rows=paths.iter().filter(|p| !go||p.directory).map(|p|json!({"label":p.path,"path":p.path,"directory":p.directory,"recent":true})).collect();
                    }
                }
            }
            "directory.edit" => {
                self.match_browser(&target, endpoint)?;
                self.browser.as_mut().unwrap().edited = true;
                self.browser.as_mut().unwrap().text =
                    request["text"].as_str().context("Missing path.")?.into();
                out.commands.push(self.read()?);
            }
            "directory.choose" => {
                self.state(model, settings, endpoint);
                self.match_browser(&target, endpoint)?;
                let b = self.browser.as_ref().unwrap();
                let path = request["path"].as_str().context("Missing chosen path.")?;
                let row = b
                    .rows
                    .iter()
                    .find(|r| r["path"].as_str() == Some(path))
                    .context("That path is no longer listed.")?;
                let directory = row["directory"].as_bool() == Some(true);
                ensure!(
                    valid(path) && (!b.go || directory),
                    "Choose a listed absolute path."
                );
                let quoted = quote(path)?;
                out.paste = Some(if b.go { format!("cd {quoted}") } else { quoted });
                let before = settings.recent_paths.clone();
                settings.remember_path(endpoint, path.into(), directory);
                out.settings_changed = before != settings.recent_paths;
                self.browser = None;
            }
            "files.upload" => {
                ensure!(
                    model.protocol.api_revision >= rex_rune::upload::MIN_API_REVISION,
                    "This Rex server can't receive files. Update Rex on the host (API 11 required)."
                );
                let paths = request["paths"]
                    .as_array()
                    .context("Missing local files.")?;
                ensure!(!paths.is_empty(), "No local files selected.");
                let files = paths
                    .iter()
                    .map(|p| {
                        let path = PathBuf::from(p.as_str().context("Invalid local file.")?);
                        ensure!(path.is_absolute(), "Local file path must be absolute.");
                        let file = File::path(path);
                        file.size()?;
                        Ok(file)
                    })
                    .collect::<Result<Vec<_>>>()?;
                out.commands.push(HostCommand::Upload {
                    session: target.session.clone(),
                    block: target.pane.clone(),
                    files,
                });
                self.upload = Some((
                    target,
                    endpoint.into(),
                    "Upload requested. Rex inserts the paths when the transfer finishes.".into(),
                ));
            }
            _ => bail!("Unknown file action."),
        }
        Ok(out)
    }
    fn match_browser(&self, target: &Target, endpoint: &str) -> Result<()> {
        let b = self.browser.as_ref().context("Path browser is closed.")?;
        ensure!(
            b.endpoint == endpoint && b.target == *target,
            "Path browser target changed."
        );
        Ok(())
    }
    pub fn reconcile(&mut self, model: &HostModel, endpoint: &str) -> Vec<HostCommand> {
        if self
            .download
            .as_ref()
            .is_some_and(|(t, e, _)| e != endpoint || t.check(model).is_err())
        {
            self.clear();
        }
        if let Some((_, _, Some(id))) = &self.download
            && model.downloads.iter().any(|t| {
                t.id == *id
                    && matches!(
                        t.state,
                        rex_rune::download::State::Receiving { .. }
                            | rex_rune::download::State::Saved { .. }
                            | rex_rune::download::State::Cancelled
                    )
            })
        {
            self.download = None;
        }
        self.cancelled
            .drain(..)
            .map(HostCommand::DownloadCancel)
            .collect()
    }
    pub fn state(&mut self, model: &HostModel, settings: &Settings, endpoint: &str) -> Value {
        if self
            .browser
            .as_ref()
            .is_some_and(|b| b.endpoint != endpoint || b.target.check(model).is_err())
        {
            self.browser = None;
        }
        if self
            .upload
            .as_ref()
            .is_some_and(|(t, e, _)| e != endpoint || t.check(model).is_err())
        {
            self.upload = None;
        }
        if let Some(b) = self.browser.as_mut() {
            if !settings.terminal.show_recent_paths {
                b.rows.retain(|r| r["recent"] != true);
            }
            if let (Some((id, prefix)), Some(reply)) = (&b.pending, &model.directory)
                && *id == reply.id
                && reply.epoch == b.target.epoch
            {
                match &reply.result {
                    Err(e) => b.problem = e.message().into(),
                    Ok(r) => {
                        b.truncated = r.truncated;
                        let mut entries = r
                            .entries
                            .iter()
                            .filter(|e| {
                                !e.name.is_empty()
                                    && !matches!(e.name.as_str(), "." | "..")
                                    && !e.name.contains('/')
                                    && !e.name.chars().any(refused)
                                    && (!e.name.starts_with('.') || prefix.starts_with('.'))
                                    && e.name.to_lowercase().starts_with(&prefix.to_lowercase())
                                    && (!b.go || e.file != Some(true))
                            })
                            .collect::<Vec<_>>();
                        entries.sort_by(|a, b| {
                            a.name
                                .to_lowercase()
                                .cmp(&b.name.to_lowercase())
                                .then(a.name.cmp(&b.name))
                        });
                        let mut rows = Vec::new();
                        let path = join(&r.path, prefix);
                        if valid(&path) && (!b.go || prefix.is_empty()) {
                            rows.push(
                                json!({"label":path,"path":path,"directory":prefix.is_empty()}),
                            );
                        }
                        rows.extend(entries.into_iter().map(|e| {
                            let p = join(&r.path, &e.name);
                            json!({"label":e.name,"path":p,"directory":e.file!=Some(true)})
                        }));
                        if !b.edited {
                            let mut recent = b
                                .rows
                                .iter()
                                .filter(|r| r["recent"] == true)
                                .cloned()
                                .collect::<Vec<_>>();
                            rows.retain(|r| !recent.iter().any(|p| p["path"] == r["path"]));
                            recent.extend(rows);
                            b.rows = recent;
                        } else {
                            b.rows = rows;
                        }
                        if r.status != "ok" {
                            b.problem = format!("Folder unavailable: {}", r.status);
                        }
                    }
                }
                b.pending = None;
            }
        }
        let download = self.download.as_ref().map(|(t, _, id)| json!({"target":t.value(),"id":id,"unsupported":model.protocol.api_revision < rex_rune::download::MIN_API_REVISION,"directory":settings.downloads.directory.clone().unwrap_or_else(|| rex_rune::download::store::default_directory().display().to_string())}));
        json!({"download":download,"directory":self.browser.as_ref().map(|b|json!({"target":b.target.value(),"text":b.text,"go":b.go,"pending":b.pending.is_some(),"rows":b.rows,"truncated":b.truncated,"problem":b.problem})),"upload":self.upload.as_ref().map(|(t,_,message)|json!({"target":t.value(),"message":message}))})
    }
}

pub fn download_rows(model: &HostModel) -> Vec<Value> {
    use rex_rune::download::{Reason, State, size_text};
    model.downloads.iter().rev().map(|t| {
        let mut row = json!({"id":t.id,"epoch":t.epoch,"name":t.display_name(),"running":!t.state.is_final()});
        let text = match &t.state {
            State::Requested => "Waiting for Rex…".into(),
            State::AwaitingConfirmation(c) => {
                row["confirmation"] = json!(true);
                format!("{} file ({}). {} Download anyway?", if c.reason == Reason::Large {"Large"} else {"Low space for this"}, size_text(c.hint), c.free.map(|f|format!("{} free.",size_text(f))).unwrap_or_default())
            }
            State::Receiving {received,hint} => { row["progress"]=json!(if *hint == 0 {0.0} else {*received as f64 / *hint as f64}); format!("{} received of {}",size_text(*received),size_text(*hint)) }
            State::Saved {path,bytes,short_of} => {row["saved"]=json!(path); format!("Saved {}{}",size_text(*bytes),short_of.map(|n|format!("; Rex reported {}",size_text(n))).unwrap_or_default())}
            State::Cancelled => "Cancelled".into(),
            State::Failed(e) => e.to_string(),
        };
        row["status"]=json!(text); row
    }).collect()
}
