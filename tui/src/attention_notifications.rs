use crate::{
    attention::{Status, reported_tabs},
    text,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{self, Write},
};

#[derive(PartialEq, Eq)]
struct Seen {
    epoch: u64,
    activity_epoch: u64,
    status: Option<Status>,
}

type Window = (String, String, String);

#[derive(Default)]
pub(crate) struct Notifications {
    seen: HashMap<Window, Seen>,
}

impl Notifications {
    pub(crate) fn clear(&mut self) {
        self.seen.clear();
    }

    pub(crate) fn observe(&mut self, state: &Value, output: &mut impl Write) -> io::Result<()> {
        let selected = (
            text(state, "host"),
            text(state, "session"),
            text(state, "window"),
        );
        let mut seen = HashMap::new();
        let mut changed = 0;
        for report in reported_tabs(state) {
            let Some(activity_epoch) = report.activity_epoch else {
                continue;
            };
            let (host, session, window) = report.scope;
            let key = (host.to_owned(), session.to_owned(), window.to_owned());
            let now = Seen {
                epoch: report.epoch,
                activity_epoch,
                status: report.status,
            };
            if report.observed
                && !report.muted
                && report.scope != selected
                && report.status.is_some()
                && self.seen.get(&key).is_some_and(|before| {
                    before.epoch == now.epoch
                        && before.activity_epoch == now.activity_epoch
                        && before.status != now.status
                })
            {
                changed += 1;
            }
            seen.insert(key, now);
        }
        self.seen = seen;
        if changed != 0 {
            let body = if changed == 1 {
                "A Rex tab needs attention."
            } else {
                "Rex tabs need attention."
            };
            write!(output, "\x1b]777;notify;rex-tui;{body}\x1b\\")?;
            output.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sends_only_new_background_attention_in_the_same_epoch() {
        let mut state = json!({
            "host":"host","session":"session","window":"active",
            "hosts":{"rows":[{"id":"host","epoch":1,"connected":true}]},
            "activity":{"rows":[
                {"host":"host","session":"session","window":"active","epoch":1,"activityEpoch":4,"state":"working","age":{"reported":false}},
                {"host":"host","session":"session","window":"background","epoch":1,"activityEpoch":4,"state":"idle",
                 "label":"Private title", "message":"Private message"},
                {"host":"host","session":"session","window":"muted","epoch":1,"activityEpoch":4,"state":"working","muted":true,"age":{"reported":false}}
            ]}
        });

        let mut notifications = Notifications::default();
        let mut output = Vec::new();
        notifications.observe(&state, &mut output).unwrap();
        assert!(output.is_empty(), "startup statuses are baseline");
        state["activity"]["rows"][1]["state"] = json!("blocked");
        state["activity"]["rows"][1]["age"] = json!({"reported":true});
        notifications.observe(&state, &mut output).unwrap();
        assert!(output.is_empty(), "the startup status sweep is historical");
        state["activity"]["rows"][0]["state"] = json!("blocked");
        state["activity"]["rows"][2]["state"] = json!("error");
        notifications.observe(&state, &mut output).unwrap();
        assert!(output.is_empty(), "current and muted tabs stay quiet");
        state["activity"]["rows"][1]["state"] = json!("error");
        state["activity"]["rows"][1]["age"] = json!({"reported":false});
        notifications.observe(&state, &mut output).unwrap();
        notifications.observe(&state, &mut output).unwrap();
        assert_eq!(
            output,
            b"\x1b]777;notify;rex-tui;A Rex tab needs attention.\x1b\\"
        );

        output.clear();
        state["activity"]["rows"][1]["state"] = json!("blocked");
        state["activity"]["rows"][1]["activityEpoch"] = json!(5);
        notifications.observe(&state, &mut output).unwrap();
        state["hosts"]["rows"][0]["connected"] = json!(false);
        notifications.observe(&state, &mut output).unwrap();
        state["hosts"]["rows"][0]["connected"] = json!(true);
        state["hosts"]["rows"][0]["epoch"] = json!(2);
        state["activity"]["rows"][1]["epoch"] = json!(2);
        notifications.observe(&state, &mut output).unwrap();
        assert!(
            output.is_empty(),
            "activity reset and reconnect are baselines"
        );
        notifications.clear();
        state["activity"]["rows"][1]["state"] = json!("error");
        notifications.observe(&state, &mut output).unwrap();
        assert!(
            output.is_empty(),
            "reenabling notifications does not replay old attention"
        );
    }
}
