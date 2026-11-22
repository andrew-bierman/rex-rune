#[path = "../qt/backend/server.rs"]
#[allow(dead_code)]
mod server;
use rex_rune::{connection::HostModel, ssh};
use server::{Effect, Server};
#[test]
fn confirmation_is_captured_consumed_and_cancelled() {
    let mut state = Server::default();
    let model = HostModel {
        connected: true,
        connection_epoch: 5,
        ..Default::default()
    };
    state
        .prepare("stop", "one", &model, "unix:///private.sock", 1)
        .unwrap();
    assert!(
        state
            .confirm("two", &model, "unix:///private.sock", 1)
            .is_err()
    );
    assert!(
        state
            .confirm("one", &model, "unix:///private.sock", 1)
            .is_err()
    );
    state
        .prepare("stop", "one", &model, "unix:///private.sock", 2)
        .unwrap();
    state.cancel();
    assert!(
        state
            .confirm("one", &model, "unix:///private.sock", 2)
            .is_err()
    );
    state
        .prepare("stop", "one", &model, "unix:///private.sock", 3)
        .unwrap();
    assert_eq!(
        state
            .confirm("one", &model, "unix:///private.sock", 3)
            .unwrap(),
        Effect::Stop { epoch: 5 }
    );
    assert!(
        state
            .confirm("one", &model, "unix:///private.sock", 3)
            .is_err()
    );
    state
        .prepare("stop", "one", &model, "unix:///private.sock", 4)
        .unwrap();
    let mut newer = model.clone();
    newer.connection_epoch += 1;
    assert!(
        state
            .confirm("one", &newer, "unix:///private.sock", 4)
            .is_err()
    );
}
#[test]
fn stale_connection_and_ordinary_attachment_cannot_start() {
    let mut state = Server::default();
    let mut model = HostModel::default();
    assert!(
        state
            .prepare("setup", "one", &model, "ssh://fixture", 1)
            .is_err()
    );
    model.ssh_problem = Some(ssh::Problem::NoServer(ssh::Facts {
        user: "fixture".into(),
        os: "Linux".into(),
        uid: Some(1000),
        rex: true,
    }));
    let preview = state
        .prepare("setup", "one", &model, "ssh://fixture", 2)
        .unwrap();
    assert!(
        preview["command"]
            .as_str()
            .unwrap()
            .contains("--autostart=true")
    );
    model.connection_epoch += 1;
    assert!(state.confirm("one", &model, "ssh://fixture", 2).is_err());
}
#[test]
fn copy_strips_private_parts_and_rejects_unsanitized_input() {
    let raw = "https://person:secret@fixture.invalid/path?token=secret#private";
    assert_eq!(
        server::address(raw).unwrap(),
        "https://fixture.invalid/path"
    );
    assert!(server::copy_address(raw).is_none());
    assert!(server::address("https://fixture.invalid/\n").is_none());
    let model = HostModel::default();
    let snapshot = Server::default().snapshot("one", &model, raw).to_string();
    assert!(!snapshot.contains("secret"));
    assert!(snapshot.contains("last reported"));
}

#[test]
fn pairing_is_local_and_disappears_when_report_or_host_changes() {
    use rex_rune::connection::server_info::{Answer, Process};
    let mut model = HostModel {
        connected: true,
        connection_epoch: 7,
        ..Default::default()
    };
    let tailscale = serde_json::from_value(serde_json::json!({"state":"running","allow_tailnet_members":false,"https":{"state":"available","url":"https://qt-pair-fixture.invalid"}})).unwrap();
    model.server_info.process = Answer::Reported(Process {
        tailscale: Some(tailscale),
        ..Default::default()
    });
    let mut state = Server::default();
    state.pair("one", &model).unwrap();
    let snapshot = state.snapshot("one", &model, "unix:///private.sock");
    assert!(snapshot["pair"]["matrix"].as_array().unwrap().len() >= 21);
    assert_eq!(
        server::copy_address(snapshot["pair"]["address"].as_str().unwrap()).as_deref(),
        Some("https://qt-pair-fixture.invalid")
    );
    if let Some(path) = std::env::var_os("REX_QT_PAIR_FIXTURE") {
        std::fs::write(path, serde_json::to_vec(&snapshot["pair"]).unwrap()).unwrap();
    }
    if let Answer::Reported(process) = &mut model.server_info.process {
        process.session_count = Some(4);
    }
    state.observe("one", &model, "unix:///private.sock");
    assert!(state.snapshot("one", &model, "unix:///private.sock")["pair"].is_null());
    state.pair("one", &model).unwrap();
    state.observe("two", &model, "unix:///private.sock");
    assert!(state.snapshot("two", &model, "unix:///private.sock")["pair"].is_null());
}

#[test]
fn disconnected_facts_are_retained_but_duration_and_pair_are_unavailable() {
    use rex_rune::connection::server_info::{Answer, Build, Process};
    let mut model = HostModel::default();
    model.server_info.build = Answer::Reported(Build {
        rex_version: Some("fixture".into()),
        ..Default::default()
    });
    model.server_info.process = Answer::Reported(Process {
        started_at: Some("1900-01-01T00:00:00Z".into()),
        ..Default::default()
    });
    let state = Server::default();
    let facts = state.snapshot("one", &model, "unix:///private.sock");
    assert_eq!(facts["version"], "fixture");
    assert!(facts["uptime_seconds"].is_null());
    assert_eq!(facts["can_pair"], false);
    model.connected = true;
    assert!(state.snapshot("one", &model, "unix:///private.sock")["uptime_seconds"].is_null());
}
