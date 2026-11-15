//! Local consent and connection-scoped, one-shot directed UI requests.
use crate::keymap::{self, Remote};
use rex_rune::connection::{
    HostModel,
    perform::{MAX_WAIT, Outcome, Pending},
};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Context {
    pub ready: bool,
    pub host: String,
    pub epoch: u64,
    pub modal: bool,
    pub field: bool,
    pub sequence: bool,
    pub last_key: Option<Instant>,
}
#[derive(Default)]
pub struct Requests {
    // Core assigns a fresh deadline to every event, even a repeated opaque ID.
    // Keep completed IDs for the live epoch: TTL or pending-only storage permits
    // replay. This costs O(unique IDs per live epoch); prune dead scopes below.
    seen: HashSet<(String, u64, String)>,
    off_notices: HashSet<String>,
    retained_hosts: Vec<(String, Option<u64>)>,
    pub context: Context,
}
impl Requests {
    pub fn retain_hosts(&mut self, hosts: &[(&str, Option<u64>)]) {
        if !hosts.is_empty()
            && self.retained_hosts.len() == hosts.len()
            && self.retained_hosts.iter().zip(hosts).all(
                |((old_host, old_epoch), (host, epoch))| old_host == host && old_epoch == epoch,
            )
        {
            return;
        }
        self.retained_hosts = hosts
            .iter()
            .map(|(host, epoch)| ((*host).into(), *epoch))
            .collect();
        self.seen.retain(|(host, epoch, _)| {
            hosts
                .iter()
                .any(|(id, live)| *id == host && *live == Some(*epoch))
        });
        self.off_notices
            .retain(|host| hosts.iter().any(|(id, _)| *id == host));
    }
    pub fn claim(&mut self, host: &str, model: &HostModel, pending: &Pending) -> bool {
        pending.epoch == model.connection_epoch
            && self
                .seen
                .insert((host.into(), pending.epoch, pending.request.call_id.clone()))
    }
    pub fn off_notice(&mut self, host: &str, action: &str, consent: bool) -> bool {
        !consent
            && matches!(keymap::remote(action), Remote::View { .. })
            && self.off_notices.insert(host.into())
    }
    pub fn policy(
        &self,
        model: &HostModel,
        pending: &Pending,
        consent: bool,
        active: bool,
        visible: bool,
        navigating: bool,
    ) -> Result<(), Outcome> {
        let request = &pending.request;
        let Remote::View { navigation } = keymap::remote(&request.action) else {
            return Err(Outcome::error(
                "not_supported",
                "This action is not available remotely; nothing was done.",
            ));
        };
        if !consent {
            return Err(Outcome::error(
                "refused",
                "Remote control is off for this saved host; nothing was done.",
            ));
        }
        if keymap::validate(&request.action, &request.args).is_err() {
            return Err(Outcome::error(
                "bad_request",
                "Invalid action arguments; nothing was done.",
            ));
        }
        if !model.connected || pending.received.elapsed() >= MAX_WAIT {
            return Err(Outcome::error(
                "internal",
                "The request expired or the host disconnected; nothing was done.",
            ));
        }
        let selecting = request.action == "session.select";
        if !self.context.ready
            || !active
            || !visible
            || self.context.modal
            || self.context.field
            || self.context.sequence
            || navigating
        {
            return Err(Outcome::error(
                "refused",
                "This host is not available for remote navigation while another view, field or panel is active; nothing was done.",
            ));
        }
        if request.session_id.as_ref().is_some_and(|s| {
            if selecting {
                request.args["session_id"].as_str() != Some(s)
            } else {
                model.selected.as_ref() != Some(s)
            }
        }) || request
            .block_id
            .as_deref()
            .is_some_and(|b| model.focused() != Some(b))
        {
            return Err(Outcome::error(
                "refused",
                "The requested session or pane is not on screen; nothing was done.",
            ));
        }
        if navigation
            && self
                .context
                .last_key
                .is_some_and(|at| at.elapsed() < Duration::from_millis(1500))
        {
            return Err(Outcome::error(
                "refused",
                "The person is typing; try again after a pause. Nothing was done.",
            ));
        }
        if request.action != "client.sidebar.toggle" && !selecting && model.window().is_none() {
            return Err(Outcome::error(
                "not_found",
                "No tab is on screen; nothing was done.",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn lifecycle_prunes_dead_scopes_but_preserves_every_live_epoch_call_id() {
        let mut requests = Requests::default();
        let model = HostModel {
            connected: true,
            connection_epoch: 7,
            ..Default::default()
        };
        let pending = |id: &str| Pending {
            epoch: 7,
            received: Instant::now(),
            request: serde_json::from_value(json!({"call_id":id,"action":"client.sidebar.toggle"}))
                .unwrap(),
        };
        for (host, id) in [
            ("live", "one"),
            ("live", "two"),
            ("changed", "one"),
            ("disconnected", "one"),
            ("removed", "one"),
        ] {
            assert!(requests.claim(host, &model, &pending(id)));
            requests.off_notice(host, "client.sidebar.toggle", false);
        }
        assert_eq!(requests.seen.len(), 5);
        requests.retain_hosts(&[
            ("live", Some(7)),
            ("changed", Some(8)),
            ("disconnected", None),
        ]);
        assert_eq!(requests.seen.len(), 2);
        assert!(!requests.off_notices.contains("removed"));
        assert!(requests.off_notices.contains("disconnected"));
        assert!(!requests.claim("live", &model, &pending("one")));
        assert!(!requests.claim("live", &model, &pending("two")));
        assert!(requests.claim("live", &model, &pending("three")));
        assert_eq!(requests.seen.len(), 3);
        requests.retain_hosts(&[]);
        assert!(requests.seen.is_empty());
        assert!(requests.off_notices.is_empty());
    }
}
