#[path = "../qt/backend/keymap.rs"]
mod qt_keymap;
#[path = "../qt/backend/workspace.rs"]
#[allow(dead_code)]
mod workspace;
use rex_rune::{connection::HostModel, settings::Settings};
use serde_json::{Value, json};

#[test]
fn offline_defaults_and_recorded_removals_share_the_displayed_effective_map() {
    let mut settings = Settings::default();
    let initial = qt_keymap::state(&HostModel::default(), &settings, 1);
    let copy = |v: &Value| {
        v["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["action"] == "pane.copy")
    };
    assert!(
        copy(&initial),
        "copy must have the upstream default in the offline map"
    );
    settings
        .recorded_shortcuts
        .remove(vec!["ctrl+shift+c".into()], "pane.copy", None, None);
    let changed = qt_keymap::state(&HostModel::default(), &settings, 2);
    assert!(
        !copy(&changed),
        "removed defaults must leave both hints and dispatch"
    );
}

#[test]
fn recorded_binding_wins_without_rewriting_the_user_layer() {
    let mut settings = Settings::default();
    settings.shortcut_file.overrides = rex_rune::keymap::Overrides::parse(&json!([
        {"keys":"ctrl+k","action":"pane.copy"}
    ]));
    let original = settings.shortcut_file.overrides.clone();
    settings
        .recorded_shortcuts
        .bind(vec!["ctrl+k".into()], "pane.paste", None, None);
    let state = qt_keymap::state(&HostModel::default(), &settings, 2);
    let binding = state["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["keys"] == json!(["ctrl+k"]))
        .unwrap();
    assert_eq!(binding["action"], "pane.paste");
    assert_eq!(settings.shortcut_file.overrides, original);
}

#[test]
fn declarations_only_advertise_real_client_actions_and_a_remote_decision() {
    assert!(qt_keymap::requires_person("pane.insert_path"));
    assert!(qt_keymap::requires_person("pane.download"));
    assert!(!qt_keymap::requires_person("pane.copy"));
    let request = qt_keymap::declaration(&Settings::default(), 4);
    assert_eq!(request.generation, 4);
    assert!(
        request
            .request
            .actions
            .iter()
            .any(|a| a.name == "pane.copy")
    );
    let state = qt_keymap::state(&HostModel::default(), &Settings::default(), 4);
    assert!(
        state["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == "pane.download" && a["remote"] == "never")
    );
    for action in state["actions"].as_array().unwrap() {
        assert!(matches!(action["remote"].as_str(), Some("never" | "view")));
    }
}

#[test]
fn recording_validates_actions_and_can_restore_a_removed_default() {
    let mut settings = Settings::default();
    assert!(
        qt_keymap::record(
            &mut settings,
            "unknown.action",
            vec!["ctrl+c".into()],
            None,
            None,
            "bind",
            &[],
        )
        .is_err()
    );
    qt_keymap::record(
        &mut settings,
        "pane.copy",
        vec!["ctrl+shift+c".into()],
        None,
        None,
        "remove",
        &[],
    )
    .unwrap();
    assert!(
        settings
            .recorded_shortcuts
            .changes()
            .iter()
            .any(|c| c.remove)
    );
    qt_keymap::record(
        &mut settings,
        "pane.copy",
        vec!["ctrl+shift+c".into()],
        None,
        None,
        "restore",
        &[],
    )
    .unwrap();
    assert!(settings.recorded_shortcuts.is_empty());
}

#[test]
fn hints_follow_the_highest_priority_recorded_binding() {
    let mut settings = Settings::default();
    settings
        .recorded_shortcuts
        .bind(vec!["ctrl+k".into()], "pane.copy", None, None);
    let state = qt_keymap::state(&HostModel::default(), &settings, 2);
    let action = state["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "pane.copy")
        .unwrap();
    assert_eq!(action["hint"], "Ctrl K");
}

#[test]
fn declared_server_actions_can_be_recorded_without_reimplementing_resolution() {
    let mut settings = Settings::default();
    let mut action = rexrs::api::client::InspectResponseAction::new("fixture_action");
    action.kind = Some("lua".into());
    qt_keymap::record(
        &mut settings,
        "fixture_action",
        vec!["ctrl+alt+g".into()],
        None,
        None,
        "bind",
        &[action],
    )
    .unwrap();
    assert!(
        settings
            .recorded_shortcuts
            .changes()
            .iter()
            .any(|change| change.action == "fixture_action")
    );
}

#[test]
fn navigation_arguments_match_the_source_client_contract() {
    assert!(qt_keymap::validate("client.tab.goto", &json!({"index":-1})).is_ok());
    assert!(qt_keymap::validate("client.tab.goto", &json!({"index":0})).is_err());
    assert!(
        qt_keymap::validate("session.select", &json!({"session_id":"s","window_id":"w"})).is_ok()
    );
    assert!(
        qt_keymap::validate(
            "session.select",
            &json!({"session_id":"s","window_id":null})
        )
        .is_err()
    );
    assert!(
        qt_keymap::validate("session.select", &json!({"session_id":"s","host":"other"})).is_err()
    );
}
