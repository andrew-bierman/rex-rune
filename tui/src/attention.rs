use crate::{items, text};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Status {
    Blocked,
    Error,
}

impl Status {
    pub(crate) fn badge(self) -> &'static str {
        match self {
            Self::Blocked => "! ",
            Self::Error => "× ",
        }
    }
}

type Scope<'a> = (&'a str, &'a str, &'a str);

#[derive(Default)]
pub(crate) struct Attention<'a> {
    tabs: HashMap<Scope<'a>, (Status, u64)>,
    hosts: HashMap<&'a str, usize>,
    sessions: HashMap<(&'a str, &'a str), usize>,
}

impl<'a> Attention<'a> {
    pub(crate) fn from_state(state: &'a Value) -> Self {
        let connected: HashMap<_, _> = items(&state["hosts"]["rows"])
            .iter()
            .filter(|host| host["connected"] == true)
            .filter_map(|host| Some((text(host, "id"), host["epoch"].as_u64()?)))
            .collect();
        let mut attention = Self::default();
        for row in items(&state["activity"]["rows"]) {
            let host = text(row, "host");
            let session = text(row, "session");
            let window = text(row, "window");
            let Some(epoch) = row["epoch"].as_u64() else {
                continue;
            };
            if host.is_empty()
                || session.is_empty()
                || window.is_empty()
                || connected.get(host) != Some(&epoch)
                || row["muted"] == true
            {
                continue;
            }
            let status = match text(row, "state") {
                "blocked" => Status::Blocked,
                "error" => Status::Error,
                _ => continue,
            };
            if attention
                .tabs
                .insert((host, session, window), (status, epoch))
                .is_none()
            {
                *attention.hosts.entry(host).or_default() += 1;
                *attention.sessions.entry((host, session)).or_default() += 1;
            }
        }
        attention
    }

    pub(crate) fn tab(&self, host: &str, session: &str, window: &str) -> Option<Status> {
        self.tabs
            .get(&(host, session, window))
            .map(|(status, _)| *status)
    }

    pub(crate) fn count(&self, host: &str, session: &str) -> usize {
        if session.is_empty() {
            self.hosts.get(host)
        } else {
            self.sessions.get(&(host, session))
        }
        .copied()
        .unwrap_or_default()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub(crate) fn next(&self, state: &Value) -> Option<Value> {
        if state["hosts"]["pending"] == true {
            return None;
        }
        let current = (
            text(state, "host"),
            text(state, "session"),
            text(state, "window"),
        );
        let mut tabs: Vec<_> = self.tabs.iter().collect();
        tabs.sort_unstable_by_key(|(scope, (status, _))| (*status, **scope));
        let index = tabs.iter().position(|(scope, _)| **scope == current);
        let (&(host, session, window), &(_, epoch)) = tabs
            .get(index.map_or(0, |index| (index + 1) % tabs.len()))
            .copied()?;
        if (host, session, window) == current {
            return None;
        }
        Some(
            json!({"op":"host","action":"navigate","host":host,"epoch":epoch,
                    "args":{"session":session,"window":window}}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_attention_counts_and_jumps_across_hosts_ignore_muted_stale_and_unreported_tabs() {
        let mut state = json!({
            "host":"local","session":"work","window":"quiet",
            "hosts":{"rows":[{"id":"local","connected":true,"epoch":1},
                              {"id":"remote","connected":true,"epoch":2},
                              {"id":"offline","connected":false,"epoch":3}]},
            "activity":{"rows":[
                {"host":"local","session":"work","window":"broken","epoch":1,"state":"error"},
                {"host":"remote","session":"pending","window":"question","epoch":2,"state":"blocked","seen":true},
                {"host":"local","session":"work","window":"muted","epoch":1,"state":"blocked","muted":true},
                {"host":"remote","session":"pending","window":"old","epoch":1,"state":"blocked"},
                {"host":"offline","session":"work","window":"offline","epoch":3,"state":"blocked"},
                {"host":"local","session":"work","window":"quiet","epoch":1,"label":"Needs approval","state":"working"}
            ]}
        });
        let attention = Attention::from_state(&state);
        assert_eq!(attention.count("local", ""), 1);
        assert_eq!(attention.count("remote", "pending"), 1);
        assert_eq!(
            attention
                .tab("remote", "pending", "question")
                .map(Status::badge),
            Some("! ")
        );
        assert_eq!(
            attention.tab("local", "work", "quiet").map(Status::badge),
            None
        );
        let jump = attention.next(&state).unwrap();
        assert_eq!(
            jump,
            json!({"op":"host","action":"navigate","host":"remote","epoch":2,
                               "args":{"session":"pending","window":"question"}})
        );
        let mut active = state.clone();
        active["host"] = jump["host"].clone();
        active["session"] = jump["args"]["session"].clone();
        active["window"] = jump["args"]["window"].clone();
        assert_eq!(attention.next(&active).unwrap()["args"]["window"], "broken");
        state["hosts"]["pending"] = json!(true);
        assert!(Attention::from_state(&state).next(&state).is_none());
    }
}
