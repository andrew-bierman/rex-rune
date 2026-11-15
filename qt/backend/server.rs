//! Cached facts and person-confirmed, single-use server operations.
use anyhow::{Context, Result, ensure};
use rex_rune::{connection::HostModel, qr, ssh};
use serde_json::{Value, json};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Stop { epoch: u64 },
    Setup { endpoint: String },
}
#[derive(Default)]
pub struct Server {
    pending: Option<Pending>,
    pair: Option<Value>,
    pair_report: Option<rex_rune::connection::server_info::ServerInfo>,
}
struct Pending {
    host: String,
    epoch: u64,
    endpoint: String,
    request: u64,
    setup: bool,
}

pub fn address(raw: &str) -> Option<String> {
    if raw.chars().any(char::is_control) {
        return None;
    }
    if let Some(path) = raw.strip_prefix("unix://") {
        return path.starts_with('/').then(|| raw.to_owned());
    }
    if let Ok(socket) = raw.parse::<std::net::SocketAddr>() {
        return Some(socket.to_string());
    }
    let mut url = url::Url::parse(raw).ok()?;
    match url.scheme() {
        "https" | "http" | "wss" | "ws" | "ssh" if url.host_str().is_some() => {
            url.set_password(None).ok()?;
            url.set_username("").ok()?;
        }
        _ => return None,
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}
pub fn copy_address(raw: &str) -> Option<String> {
    address(raw)
        .filter(|safe| safe == raw || safe.strip_suffix('/') == Some(raw))
        .map(|_| raw.to_owned())
}
fn pair_address(model: &HostModel) -> Option<String> {
    if !model.connected {
        return None;
    }
    let https = model
        .server_info
        .process
        .reported()?
        .tailscale
        .as_ref()?
        .https
        .as_ref()?;
    if https.state != "available" {
        return None;
    }
    address(https.url.as_deref()?)
        .filter(|s| s.starts_with("https://"))
        .map(|s| s.trim_end_matches('/').to_owned())
}
impl Server {
    pub fn observe(&mut self, host: &str, model: &HostModel, endpoint: &str) {
        if self.pending.as_ref().is_some_and(|p| {
            p.host != host || p.epoch != model.connection_epoch || p.endpoint != endpoint
        }) {
            self.pending = None;
        }
        if self.pair.as_ref().is_some_and(|p| {
            p["host"] != host
                || p["epoch"] != model.connection_epoch
                || self.pair_report.as_ref() != Some(&model.server_info)
                || pair_address(model).as_deref() != p["address"].as_str()
        }) {
            self.pair = None;
            self.pair_report = None;
        }
    }
    pub fn confirmation_valid(&self, request: u64) -> bool {
        self.pending.as_ref().is_some_and(|p| p.request == request)
    }
    pub fn cancel(&mut self) {
        self.pending = None;
    }
    pub fn close_pair(&mut self) {
        self.pair_report = None;
        self.pair = None;
    }
    pub fn prepare(
        &mut self,
        kind: &str,
        host: &str,
        model: &HostModel,
        endpoint: &str,
        request: u64,
    ) -> Result<Value> {
        self.pending = None;
        ensure!(request > 0, "confirmation needs a request id");
        let setup = match kind {
            "stop" => false,
            "setup" => true,
            _ => anyhow::bail!("unknown server operation"),
        };
        let command = if setup {
            ensure!(
                model.ssh_server_missing && !model.connected,
                "SSH setup is available only after a missing-server report"
            );
            let target = ssh::Target::parse(endpoint)?.context("SSH setup requires an SSH host")?;
            ssh::setup::command_preview(&target)?
        } else {
            ensure!(
                model.connected
                    && (endpoint.starts_with("unix://") || endpoint.starts_with("ssh://")),
                "stop requires a connected Unix or SSH host"
            );
            "server.stop {}".to_owned()
        };
        self.pending = Some(Pending {
            host: host.into(),
            epoch: model.connection_epoch,
            endpoint: endpoint.into(),
            request,
            setup,
        });
        Ok(
            json!({"host":host,"epoch":model.connection_epoch,"request_id":request,"kind":kind,"target":address(endpoint),"command":command}),
        )
    }
    pub fn confirm(
        &mut self,
        host: &str,
        model: &HostModel,
        endpoint: &str,
        request: u64,
    ) -> Result<Effect> {
        let pending = self
            .pending
            .take()
            .context("confirmation expired or already used")?;
        ensure!(
            pending.host == host
                && pending.epoch == model.connection_epoch
                && pending.endpoint == endpoint
                && pending.request == request,
            "confirmation target changed"
        );
        if pending.setup {
            ensure!(
                model.ssh_server_missing && !model.connected,
                "setup target changed"
            );
            Ok(Effect::Setup {
                endpoint: pending.endpoint,
            })
        } else {
            ensure!(
                model.connected
                    && (endpoint.starts_with("unix://") || endpoint.starts_with("ssh://")),
                "stop target is no longer available"
            );
            Ok(Effect::Stop {
                epoch: pending.epoch,
            })
        }
    }
    pub fn pair(&mut self, host: &str, model: &HostModel) -> Result<()> {
        self.pair = None;
        let address = pair_address(model).context("No available HTTPS pairing address")?;
        let symbol = qr::encode(address.as_bytes())
            .map_err(|_| anyhow::anyhow!("Address is too long for a QR code"))?;
        let matrix: Vec<Vec<bool>> = (0..symbol.size())
            .map(|y| (0..symbol.size()).map(|x| symbol.dark(x, y)).collect())
            .collect();
        self.pair_report = Some(model.server_info.clone());
        self.pair = Some(
            json!({"host":host,"epoch":model.connection_epoch,"address":address,"matrix":matrix,"report":null}),
        );
        Ok(())
    }
    pub fn snapshot(&self, host: &str, model: &HostModel, endpoint: &str) -> Value {
        let info = &model.server_info;
        let build = info.build.reported();
        let process = info.process.reported();
        let uptime = model
            .connected
            .then(|| process?.started_at.as_deref())
            .flatten()
            .and_then(|start| {
                rex_rune::connection::activity::time::reported_elapsed(start, SystemTime::now())
            })
            .map(|d| d.as_secs());
        let listeners: Vec<Value> = process.into_iter().flat_map(|p| &p.listeners).map(|l| json!({"transport":l.transport,"address":address(&l.endpoint),"primary":l.primary})).collect();
        let mut pair = self.pair.clone().unwrap_or(Value::Null);
        if let Some(pair) = pair.as_object_mut() {
            pair.remove("report");
        }
        json!({"host":host,"epoch":model.connection_epoch,"connected":model.connected,"status":if model.connected {"Reported on this connection"} else {"Disconnected · last reported"},"version":info.rex_version(),"platform":build.and_then(|b| b.platform.as_deref()),"commit":build.and_then(|b| b.commit.as_deref()),"built_at":build.and_then(|b| b.built_at.as_deref()),"dirty":build.and_then(|b| b.dirty),"api":info.api_revisions,"pid":process.and_then(|p| p.pid),"instance_id":process.and_then(|p| p.instance_id.as_deref()),"host_id":process.and_then(|p| p.host_id.as_deref()),"tailscale_state":process.and_then(|p| p.tailscale.as_ref()).map(|t| t.state.as_str()),"started_at":process.and_then(|p| p.started_at.as_deref()),"sessions":process.and_then(|p| p.session_count),"uptime_seconds":uptime,"listeners":listeners,"can_pair":pair_address(model).is_some(),"can_stop":model.connected && (endpoint.starts_with("unix://") || endpoint.starts_with("ssh://")),"can_setup":model.ssh_server_missing && !model.connected && ssh::Target::parse(endpoint).ok().flatten().is_some_and(|t| ssh::setup::start_args(&t).is_ok()),"pair":pair,"stop_result":model.server_stop_result})
    }
}
