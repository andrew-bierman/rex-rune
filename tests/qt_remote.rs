#[path = "../qt/backend/keymap.rs"]
#[allow(dead_code)]
mod keymap;
#[path = "../qt/backend/remote.rs"]
#[allow(dead_code)]
mod remote;
#[path = "../qt/backend/workspace.rs"]
#[allow(dead_code)]
mod workspace;
use rex_rune::connection::{
    HostModel,
    perform::{Pending, Request},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
fn model() -> HostModel {
    let mut m = HostModel {
        connected: true,
        connection_epoch: 3,
        selected: Some("session".into()),
        ..Default::default()
    };
    m.views.insert("session".into(),serde_json::from_value(json!({"revision":1,"active_window_id":"tab","windows":[{"window_id":"tab","focused_block_id":"pane","layers":[{"kind":"tiled","blocks":[{"block_id":"pane"}]}]}]})).unwrap());
    m
}
fn pending(action: &str, args: Value) -> Pending {
    Pending { epoch:3,received:Instant::now(),request:serde_json::from_value::<Request>(json!({"call_id":"once","action":action,"args":args,"session_id":"session","block_id":"pane"})).unwrap() }
}
fn requests() -> remote::Requests {
    let mut r = remote::Requests::default();
    r.context.ready = true;
    r
}
#[test]
fn only_the_existing_eight_view_actions_have_remote_authority() {
    let state = keymap::state(&model(), &Default::default(), 1);
    let mut allowed: Vec<_> = state["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["remote"] == "view")
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    allowed.sort();
    assert_eq!(
        allowed,
        vec![
            "client.sidebar.toggle",
            "client.tab.goto",
            "client.tab.next",
            "client.tab.previous",
            "pane.focus",
            "pane.scroll",
            "pane.zoom",
            "session.select"
        ]
    );
    for (id, args) in [
        ("client.sidebar.toggle", Value::Null),
        ("client.tab.goto", json!({"index":1})),
        ("client.tab.next", Value::Null),
        ("client.tab.previous", Value::Null),
        ("pane.focus", json!({"direction":"right"})),
        ("pane.scroll", json!({"lines":3})),
        ("pane.zoom", Value::Null),
        ("session.select", json!({"session_id":"session"})),
    ] {
        assert!(
            requests()
                .policy(&model(), &pending(id, args), true, true, true, false)
                .is_ok(),
            "{id}"
        );
    }
}
#[test]
fn request_identity_and_interaction_guards_fail_closed() {
    let m = model();
    let p = pending("client.tab.next", Value::Null);
    let mut r = requests();
    assert!(r.claim("host-a", &m, &p));
    assert!(!r.claim("host-a", &m, &p));
    let mut stale = p.clone();
    stale.epoch = 2;
    assert!(!r.claim("host-a", &m, &stale));
    assert!(r.policy(&m, &p, false, true, true, false).is_err());
    assert!(r.policy(&m, &p, true, false, true, false).is_err());
    assert!(r.policy(&m, &p, true, true, false, false).is_err());
    assert!(r.policy(&m, &p, true, true, true, true).is_err());
    r.context.modal = true;
    assert!(r.policy(&m, &p, true, true, true, false).is_err());
    r.context.modal = false;
    r.context.field = true;
    assert!(r.policy(&m, &p, true, true, true, false).is_err());
    r.context.field = false;
    r.context.sequence = true;
    assert!(r.policy(&m, &p, true, true, true, false).is_err());
    r.context.sequence = false;
    r.context.last_key = Some(Instant::now());
    assert!(r.policy(&m, &p, true, true, true, false).is_err());
    r.context.last_key = None;
    let mut expired = p.clone();
    expired.received = Instant::now() - Duration::from_secs(6);
    assert!(r.policy(&m, &expired, true, true, true, false).is_err());
    let mut other = p.clone();
    other.request.block_id = Some("replacement".into());
    assert!(r.policy(&m, &other, true, true, true, false).is_err());
    other = p;
    other.request.session_id = Some("other-host-session".into());
    assert!(r.policy(&m, &other, true, true, true, false).is_err());
}
#[test]
fn every_other_remote_action_is_denied_including_person_only_callbacks() {
    let r = requests();
    let m = model();
    for id in [
        "pane.copy",
        "pane.paste",
        "pane.send_key",
        "session.new",
        "client.tab.new",
        "pane.close",
        "client.settings",
        "pane.download",
        "pane.insert_path",
        "pane.go_to_directory",
        "client.pair_device",
        "client.server.stop",
        "client.server.setup",
        "client.host.switch",
        "made.up",
    ] {
        let error = r
            .policy(&m, &pending(id, Value::Null), true, true, true, false)
            .unwrap_err();
        assert!(
            matches!(
                error,
                rex_rune::connection::perform::Outcome::Error {
                    code: "not_supported",
                    ..
                }
            ),
            "{id}"
        );
    }
    for id in [
        "pane.insert_path",
        "pane.go_to_directory",
        "pane.download",
        "client.pair_device",
        "client.server.stop",
        "client.server.setup",
    ] {
        assert!(keymap::requires_person(id));
    }
}

#[test]
fn canonical_scroll_arguments_match_the_source_clients_whole_i32_contract() {
    for lines in [i32::MIN as i64, 0, i32::MAX as i64] {
        assert!(keymap::validate("pane.scroll", &json!({"lines":lines})).is_ok());
    }
    for lines in [i32::MIN as i64 - 1, i32::MAX as i64 + 1] {
        assert!(keymap::validate("pane.scroll", &json!({"lines":lines})).is_err());
    }
}
