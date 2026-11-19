use super::*;
use rex_rune::connection::{
    HostConnection, HostModel,
    perform::{Outcome, Pending, Request},
};
use std::sync::{Arc, Mutex};

fn pending(id: &str) -> Pending {
    Pending {
        epoch: 3,
        received: Instant::now(),
        request: serde_json::from_value::<Request>(
            json!({"call_id":id,"action":"client.sidebar.toggle"}),
        )
        .unwrap(),
    }
}
#[test]
fn duplicate_expired_and_replacement_epoch_calls_cannot_toggle_the_view_or_reply_twice() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings {
        hosts: vec![Host {
            label: "Fixture".into(),
            endpoint: format!("unix://{}/unused.sock", dir.path().display()),
            remote_control: true,
            collapsed: false,
            shell: None,
        }],
        ..Default::default()
    };
    settings.hosts.push(Host {
        label: "Visible fixture".into(),
        endpoint: format!("unix://{}/visible.sock", dir.path().display()),
        remote_control: false,
        collapsed: false,
        shell: None,
    });
    let mut hosts = Hosts::new(&settings);
    let model = HostModel {
        connected: true,
        connection_epoch: 3,
        ..Default::default()
    };
    let (commands, received) = flume::unbounded();
    let (_, wake) = flume::bounded(1);
    hosts.slots[0].model = model.clone();
    hosts.slots[0].connection = Some(HostConnection {
        model: Arc::new(Mutex::new(model)),
        commands,
        wake,
    });
    let mut backend = Backend {
        maintenance: maintenance::Maintenance::default(),
        server: server::Server::default(),
        server_clock: None,
        confirmation: Value::Null,
        setup_result: None,
        setup_rx: None,
        program_notices: Vec::new(),
        next_notice: 0,
        settings,
        activity: activity::Activity::default(),
        visible: true,
        preferences: Value::Null,
        path: dir.path().join("workspace.json"),
        hosts,
        adapters: HashMap::from([("host-1".into(), Adapter::default())]),
        generation: 1,
        discovery: Discovery::default(),
        after_state: Vec::new(),
        cursor_targets: HashMap::new(),
        remote: remote::Requests::default(),
        remote_navigation: None,
        remote_checks: Vec::new(),
    };
    backend.remote.context.ready = true;
    backend.remote.context.host = "host-1".into();
    backend.remote.context.epoch = 3;
    let before = backend.settings.sidebar_open;
    let first = pending("one");
    backend.remote_perform("host-1", first.clone()).unwrap();
    assert_eq!(
        before, backend.settings.sidebar_open,
        "no effect before native authorization"
    );
    let mut authorization = backend
        .after_state
        .iter()
        .find(|e| e["type"] == "remote_check")
        .unwrap()
        .clone();
    authorization["accepted"] = json!(true);
    backend.authorize_remote(&authorization).unwrap();
    assert_ne!(before, backend.settings.sidebar_open);
    assert_eq!(
        backend
            .after_state
            .iter()
            .filter(|e| e["type"] == "notice")
            .count(),
        1
    );
    backend.remote_perform("host-1", first).unwrap();
    assert_ne!(
        before, backend.settings.sidebar_open,
        "a duplicate must not toggle back"
    );
    let replies: Vec<_> = received
        .drain()
        .filter_map(|c| match c {
            HostCommand::PerformReply {
                call_id, outcome, ..
            } => Some((call_id, outcome)),
            _ => None,
        })
        .collect();
    assert_eq!(replies, vec![("one".into(), Outcome::Performed)]);
    let mut expired = pending("expired");
    expired.received = Instant::now() - Duration::from_secs(6);
    backend.remote_perform("host-1", expired).unwrap();
    assert_ne!(before, backend.settings.sidebar_open);
    assert!(matches!(
        received.recv().unwrap(),
        HostCommand::PerformReply {
            outcome: Outcome::Error {
                code: "internal",
                ..
            },
            ..
        }
    ));
    backend
        .remote_perform("host-1", pending("panel-race"))
        .unwrap();
    let mut authorization = backend.remote_checks.last().unwrap().2.clone();
    authorization["accepted"] = json!(true);
    // This UI update was unread when the request first arrived. Native authorization
    // follows it in the frontend pipe, so it cannot authorize cached panel state.
    backend
        .handle(
            &json!({"version":VERSION,"op":"remote_context","host":"host-1","epoch":3,
        "modal":true,"field":false,"sequence":false,"keypress":false}),
        )
        .unwrap();
    backend.authorize_remote(&authorization).unwrap();
    assert_ne!(before, backend.settings.sidebar_open);
    assert!(matches!(
        received.recv().unwrap(),
        HostCommand::PerformReply {
            outcome: Outcome::Error {
                code: "refused",
                ..
            },
            ..
        }
    ));
    backend.remote.context.modal = false;
    backend
        .remote_perform("host-1", pending("late-authorization"))
        .unwrap();
    let mut authorization = backend.remote_checks.last().unwrap().2.clone();
    authorization["accepted"] = json!(true);
    backend.remote_checks.last_mut().unwrap().1.received = Instant::now() - Duration::from_secs(6);
    backend.authorize_remote(&authorization).unwrap();
    assert_ne!(before, backend.settings.sidebar_open);
    assert!(matches!(
        received.recv().unwrap(),
        HostCommand::PerformReply {
            outcome: Outcome::Error {
                code: "internal",
                ..
            },
            ..
        }
    ));
    backend.hosts.active = "host-2".into();
    backend.remote.context.host = "host-2".into();
    backend.remote.context.epoch = 0;
    let mut select = pending("visible-epoch-race");
    select.request = serde_json::from_value(json!({"call_id":"visible-epoch-race", "action":"session.select", "args":{"session_id":"own"}})).unwrap();
    backend.remote_perform("host-1", select).unwrap();
    let mut authorization = backend.remote_checks.last().unwrap().2.clone();
    assert_eq!(authorization["host"], "host-1");
    assert_eq!(authorization["epoch"], 3);
    assert_eq!(authorization["visible_host"], "host-2");
    assert_eq!(authorization["visible_epoch"], 0);
    authorization["accepted"] = json!(true);
    backend.hosts.slots[1].model.connection_epoch = 1;
    backend.authorize_remote(&authorization).unwrap();
    assert_eq!(backend.hosts.active, "host-2");
    assert!(matches!(
        received.recv().unwrap(),
        HostCommand::PerformReply {
            outcome: Outcome::Error {
                code: "refused",
                ..
            },
            ..
        }
    ));
    backend.hosts.active = "host-1".into();
    backend.remote.context.host = "host-1".into();
    backend.remote.context.epoch = 3;
    backend.hosts.slots[0].model.connection_epoch = 4;
    backend
        .remote_perform("host-1", pending("replacement"))
        .unwrap();
    assert_ne!(before, backend.settings.sidebar_open);
    assert!(
        received.is_empty(),
        "the replacement socket must receive no stale reply"
    );
}

#[test]
fn inactive_session_selection_keeps_visible_context_and_input_guards() {
    let mut requests = remote::Requests::default();
    requests.context.ready = true;
    let model = HostModel {
        connected: true,
        connection_epoch: 3,
        ..Default::default()
    };
    let mut select = pending("inactive");
    select.request = serde_json::from_value(
        json!({"call_id":"inactive", "action":"session.select", "args":{"session_id":"own"}}),
    )
    .unwrap();
    assert!(
        requests
            .policy(&model, &select, true, false, true, false)
            .is_ok()
    );
    for action in [
        "client.sidebar.toggle",
        "client.tab.next",
        "client.tab.previous",
        "client.tab.goto",
        "pane.focus",
        "pane.zoom",
        "pane.scroll",
    ] {
        let mut other = pending(action);
        other.request.action = action.into();
        other.request.args = match action {
            "client.tab.goto" => json!({"index":1}),
            "pane.focus" => json!({"direction":"left"}),
            "pane.scroll" => json!({"lines":1}),
            _ => json!({}),
        };
        assert!(
            requests
                .policy(&model, &other, true, false, true, false)
                .is_err(),
            "{action}"
        );
    }
    assert!(
        requests
            .policy(&model, &select, false, false, true, false)
            .is_err()
    );
    assert!(
        requests
            .policy(&model, &select, true, false, false, false)
            .is_err()
    );
    assert!(
        requests
            .policy(&model, &select, true, false, true, true)
            .is_err()
    );
    requests.context.modal = true;
    assert!(
        requests
            .policy(&model, &select, true, false, true, false)
            .is_err()
    );
    requests.context.modal = false;
    requests.context.field = true;
    assert!(
        requests
            .policy(&model, &select, true, false, true, false)
            .is_err()
    );
    requests.context.field = false;
    requests.context.sequence = true;
    assert!(
        requests
            .policy(&model, &select, true, false, true, false)
            .is_err()
    );
    requests.context.sequence = false;
    requests.context.last_key = Some(Instant::now());
    assert!(
        requests
            .policy(&model, &select, true, false, true, false)
            .is_err()
    );
}
