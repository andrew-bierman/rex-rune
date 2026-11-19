use serde_json::Value;

pub fn project(
    pending: &mut Vec<Value>,
    rows: &[Value],
    sessions: &[Value],
    later_epoch: impl Fn(&Value) -> bool,
) -> Vec<Value> {
    let mut ready = Vec::new();
    let mut deferred = Vec::new();
    for mut notice in pending.drain(..) {
        let matches = |row: &&Value| {
            row["host"] == notice["host"]
                && row["epoch"] == notice["epoch"]
                && row["activityEpoch"] == notice["activityEpoch"]
                && row["session"] == notice["session"]
        };
        let row = rows
            .iter()
            .filter(matches)
            .find(|row| {
                row["panes"]
                    .as_array()
                    .is_some_and(|panes| panes.iter().any(|pane| pane["pane"] == notice["block"]))
            })
            .or_else(|| sessions.iter().find(matches));
        if let Some(row) = row {
            notice["key"] = row["key"].clone();
            ready.push(notice);
        } else if later_epoch(&notice) {
            // The worker advanced after the catalog clone. Publish with its own
            // epoch's catalog, never the preceding connection's destination.
            deferred.push(notice);
        }
    }
    *pending = deferred;
    ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn notice() -> Value {
        json!({"event":1,"host":"host-a","epoch":7,"activityEpoch":4,"session":"session-a","block":"pane-a","title":"Owned notice","body":"Once"})
    }
    fn tab() -> Value {
        json!({"key":"exact-tab","host":"host-a","epoch":7,"activityEpoch":4,"session":"session-a","window":"tab-a","panes":[{"pane":"pane-a"}]})
    }
    #[test]
    fn a_new_connection_event_waits_for_its_catalog_then_is_consumed_once() {
        let mut pending = vec![notice()];
        let mut old = tab();
        old["epoch"] = json!(6);
        assert!(project(&mut pending, &[old], &[], |_| true).is_empty());
        assert_eq!(pending.len(), 1);
        let ready = project(&mut pending, &[tab()], &[], |_| false);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0]["key"], "exact-tab");
        assert!(pending.is_empty());
        assert!(project(&mut pending, &[tab()], &[], |_| false).is_empty());
    }
    #[test]
    fn stale_removed_or_other_host_destinations_are_not_adopted() {
        for field in ["host", "epoch", "activityEpoch", "session"] {
            let mut wrong = tab();
            wrong[field] = json!("different");
            let mut pending = vec![notice()];
            assert!(project(&mut pending, &[wrong.clone()], &[wrong], |_| false).is_empty());
            assert!(pending.is_empty());
        }
        let mut pending = vec![notice()];
        assert!(project(&mut pending, &[], &[], |_| false).is_empty());
        assert!(pending.is_empty());
    }
    #[test]
    fn an_uncached_program_tab_keeps_its_captured_session_destination() {
        let mut pending = vec![notice()];
        let mut session = tab();
        session["key"] = json!("captured-session");
        session["window"] = Value::Null;
        session["panes"] = Value::Null;
        let ready = project(&mut pending, &[], &[session], |_| false);
        assert_eq!(ready[0]["key"], "captured-session");
        assert_eq!(ready[0]["host"], "host-a");
        assert_eq!(ready[0]["epoch"], 7);
        assert!(pending.is_empty());
    }
}
