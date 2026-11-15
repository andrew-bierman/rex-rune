#[path = "../qt/backend/hosts.rs"]
#[allow(dead_code)]
mod hosts;
use hosts::{Hosts, Visit};
use rex_rune::settings::{Host, Settings};

fn scratch() -> tempfile::TempDir {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".build-tools/qt-host-tests");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

fn settings() -> Settings {
    Settings {
        hosts: vec![
            Host {
                label: "Same name".into(),
                endpoint: "unix:///tmp/qt-host-one.sock".into(),
                remote_control: false,
                collapsed: false,
                shell: None,
            },
            Host {
                label: "Same name".into(),
                endpoint: "unix:///tmp/qt-host-two.sock".into(),
                remote_control: true,
                collapsed: false,
                shell: None,
            },
        ],
        ..Settings::default()
    }
}

#[test]
fn duplicate_labels_never_alias_host_identity_and_stale_targets_fail() {
    let settings = settings();
    let hosts = Hosts::new(&settings);
    let ids = hosts.ids();
    assert_ne!(ids[0], ids[1]);
    assert!(hosts.require_active(&ids[0]).is_ok());
    assert!(hosts.require_active(&ids[1]).is_err());
}

#[test]
fn endpoint_edits_replace_identity_and_clear_only_its_local_state() {
    let dir = scratch();
    let path = dir.path().join("workspace.json");
    let mut settings = settings();
    settings
        .sessions
        .insert(settings.hosts[0].endpoint.clone(), "old-session".into());
    settings
        .sessions
        .insert(settings.hosts[1].endpoint.clone(), "other-session".into());
    let mut hosts = Hosts::new(&settings);
    let old = hosts.ids()[0].clone();
    let other = hosts.ids()[1].clone();
    hosts
        .edit(
            &mut settings,
            &path,
            &old,
            "Changed",
            "unix:///tmp/qt-host-new.sock",
            None,
        )
        .unwrap();
    assert!(!hosts.ids().contains(&old));
    assert!(hosts.ids().contains(&other));
    assert!(
        !settings
            .sessions
            .contains_key("unix:///tmp/qt-host-one.sock")
    );
    assert_eq!(
        settings
            .sessions
            .get("unix:///tmp/qt-host-two.sock")
            .map(String::as_str),
        Some("other-session")
    );
    assert!(!settings.hosts[0].remote_control);
    assert_eq!(
        Settings::load(&path).unwrap().hosts[0].endpoint,
        "unix:///tmp/qt-host-new.sock"
    );
    assert!(hosts.require_active(&old).is_err());
}

#[test]
fn invalid_and_unsavable_edits_leave_the_catalog_intact() {
    let dir = scratch();
    let mut settings = settings();
    let mut hosts = Hosts::new(&settings);
    let id = hosts.ids()[0].clone();
    assert!(
        hosts
            .edit(
                &mut settings,
                &dir.path().join("workspace.json"),
                &id,
                "",
                "unix:///tmp/a",
                None
            )
            .is_err()
    );
    assert!(
        hosts
            .edit(
                &mut settings,
                dir.path(),
                &id,
                "Changed",
                "unix:///tmp/a",
                None
            )
            .is_err()
    );
    assert_eq!(settings.hosts[0].label, "Same name");
    assert_eq!(hosts.ids()[0], id);
}

#[test]
fn exact_history_retains_duplicate_named_targets_and_truncates_a_branch() {
    let mut history = hosts::History::default();
    let a = Visit {
        host: "host-1".into(),
        session: "session-a".into(),
        window: Some("window-a".into()),
    };
    let b = Visit {
        host: "host-2".into(),
        session: "session-b".into(),
        window: Some("window-b".into()),
    };
    let c = Visit {
        host: "host-1".into(),
        session: "session-c".into(),
        window: None,
    };
    history.observe(a.clone());
    history.observe(b.clone());
    assert_eq!(history.step(-1), Some(a));
    history.observe(c.clone());
    assert_eq!(history.step(1), None);
    assert_eq!(history.current(), Some(&c));
    history.remove_host("host-1");
    assert!(history.current().is_none());
}

#[test]
fn a_custom_window_prefix_label_is_not_replaced_by_a_program_title() {
    let mut model = rex_rune::connection::HostModel::default();
    model
        .titles
        .insert("b".into(), ("program-title".into(), None));
    let window = serde_json::from_value(serde_json::json!({"window_id":"window:abcdefgh","label":"Window custom","focused_block_id":"b","layers":[]})).unwrap();
    assert_eq!(hosts::tab_title(&model, &window, false), "Window custom");
}

#[test]
fn equal_connection_epochs_on_two_hosts_do_not_authorize_cross_host_input() {
    let mut hosts = Hosts::new(&settings());
    for slot in &mut hosts.slots {
        slot.model.connection_epoch = 7;
    }
    let ids = hosts.ids();
    assert!(hosts.require_target(&ids[0], 7).is_ok());
    assert!(hosts.require_target(&ids[1], 7).is_err());
    assert!(hosts.require_target(&ids[0], 6).is_err());
    hosts.active = ids[1].clone();
    assert!(hosts.require_target(&ids[0], 7).is_err());
    assert!(hosts.require_target(&ids[1], 7).is_ok());
}

#[test]
fn background_activity_views_stay_one_navigation_stop_until_tabs_are_listed() {
    let mut settings = settings();
    let mut hosts = Hosts::new(&settings);
    let slot = &mut hosts.slots[0];
    slot.model.connected = true;
    slot.model.sessions = serde_json::from_value(serde_json::json!([
        {"session_id":"opened","label":"Same"}, {"session_id":"background","label":"Same"}
    ]))
    .unwrap();
    for session in ["opened", "background"] {
        slot.model.views.insert(session.into(), serde_json::from_value(serde_json::json!({"revision":1,"active_window_id":"w1","windows":[
            {"window_id":"w1","label":"First","layers":[]}, {"window_id":"w2","label":"Second","layers":[]}
        ]})).unwrap());
    }
    slot.model.background_views.insert("background".into());
    assert_eq!(hosts.stops(&settings, true).len(), 3);
    let projection = hosts.state(&settings);
    assert_eq!(projection["rows"][0]["sessions"][1]["opened"], false);
    assert_eq!(
        projection["rows"][0]["sessions"][1]["tabs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    settings.sidebar_open = true;
    settings.vertical_tabs = true;
    assert_eq!(hosts.stops(&settings, true).len(), 4);
    settings.hosts[0].collapsed = true;
    assert_eq!(hosts.stops(&settings, true).len(), 3);
    assert_eq!(hosts.stops(&settings, false).len(), 2);
}

#[test]
fn observed_active_host_round_trips_the_last_host_preference() {
    let mut settings = settings();
    let mut hosts = Hosts::new(&settings);
    hosts.active = hosts.ids()[1].clone();
    hosts.poll(&mut settings);
    let dir = scratch();
    let path = dir.path().join("workspace.json");
    settings.save(&path).unwrap();
    let reloaded = Settings::load(&path).unwrap();
    assert_eq!(
        reloaded.last_host.as_deref(),
        Some(reloaded.hosts[1].endpoint.as_str())
    );
    assert_eq!(Hosts::new(&reloaded).active, hosts.active);
}

#[test]
fn upstream_connection_failures_and_missing_ssh_fact_remain_visible_without_starting_workers() {
    use rex_rune::connection::HostModel;
    let settings = settings();
    let mut hosts = Hosts::new(&settings);
    assert!(hosts.slots.iter().all(|slot| slot.connection.is_none()));
    // These are core model facts, not simulated SSH or protocol handshakes.
    let unsupported = format!(
        "Disconnected · Rex negotiated API revision {}; this client supports {}–{}",
        rex_rune::transport::MIN_API_REVISION - 1,
        rex_rune::transport::MIN_API_REVISION,
        rex_rune::transport::API_REVISION
    );
    for (status, ssh_server_missing) in [
        ("Disconnected · Connection refused (os error 111)", false),
        (
            "Disconnected · No Rex server found on fixture: attachment never starts a server",
            true,
        ),
        (unsupported.as_str(), false),
    ] {
        hosts.slots[0].model = HostModel {
            connected: false,
            status: status.into(),
            ssh_server_missing,
            ..Default::default()
        };
        let projection = hosts.state(&settings);
        assert_eq!(projection["rows"][0]["status"], status);
        assert_eq!(
            projection["rows"][0]["ssh_server_missing"],
            ssh_server_missing
        );
        assert_eq!(projection["rows"][0]["connected"], false);
        assert_eq!(projection["rows"][0]["enabled"], false);
        assert_eq!(projection["rows"][1]["ssh_server_missing"], false);
        assert!(hosts.slots.iter().all(|slot| slot.connection.is_none()));
    }
}

#[test]
fn saving_a_host_edit_revokes_its_consent_without_copying_another_hosts_choice() {
    let dir = scratch();
    let path = dir.path().join("workspace.json");
    let mut settings = settings();
    settings.hosts[0].remote_control = true;
    let endpoint = settings.hosts[0].endpoint.clone();
    let mut hosts = Hosts::new(&settings);
    let id = hosts.ids()[0].clone();
    hosts
        .edit(&mut settings, &path, &id, "Edited label", &endpoint, None)
        .unwrap();
    assert!(!settings.hosts[0].remote_control);
    assert!(settings.hosts[1].remote_control);
    let saved = Settings::load(&path).unwrap();
    assert!(!saved.hosts[0].remote_control);
    assert!(saved.hosts[1].remote_control);
}
