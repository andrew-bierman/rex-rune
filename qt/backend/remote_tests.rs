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
    let dir = tempfile::tempdir_in(std::env::var_os("TMPDIR").unwrap()).unwrap();
    let settings = Settings {
        hosts: vec![Host {
            label: "Fixture".into(),
            endpoint: format!("unix://{}/unused.sock", dir.path().display()),
            remote_control: true,
            collapsed: false,
            shell: None,
        }],
        ..Default::default()
    };
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
