#[path = "../qt/backend/preferences.rs"]
#[allow(dead_code)]
mod preferences;
use rex_rune::settings::{Settings, TerminalThemeChoice};
use serde_json::json;
fn scratch() -> tempfile::TempDir {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".build-tools/qt-preferences-tests");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}
#[test]
fn editable_save_preserves_latest_hosts_navigation_and_read_only_override() {
    let dir = scratch();
    let path = dir.path().join("workspace.json");
    let override_path = dir.path().join("workspace.shortcuts.json");
    std::fs::write(&override_path, "{\"bindings\":[]}").unwrap();
    let mut settings = Settings::default();
    settings
        .sessions
        .insert("fixture".into(), "latest-session".into());
    let hosts = serde_json::to_value(&settings.hosts).unwrap();
    preferences::apply(&mut settings, &path, &json!({"font_size":21,"pane_padding":12,"interface":{"pane_padding_vertical":4,"tab_icon_count":3},"terminal":{"font":{"line_height":20,"spacing":2},"cursor_style":"bar"}})).unwrap();
    let loaded = Settings::load(&path).unwrap();
    assert_eq!(loaded.font_size, 21.);
    assert_eq!(loaded.padding(), (12., 4.));
    assert_eq!(loaded.terminal.font.line_height(), 20);
    assert_eq!(loaded.sessions["fixture"], "latest-session");
    assert_eq!(serde_json::to_value(loaded.hosts).unwrap(), hosts);
    assert_eq!(
        std::fs::read_to_string(override_path).unwrap(),
        "{\"bindings\":[]}"
    );
}
#[test]
fn follow_server_remembers_prior_declarations_across_restart() {
    let dir = scratch();
    let path = dir.path().join("workspace.json");
    let mut settings = Settings::default();
    assert!(preferences::declaration(&settings).unwrap().is_none());
    preferences::apply(
        &mut settings,
        &path,
        &json!({"terminal_theme":"named:rex/rose-pine"}),
    )
    .unwrap();
    assert!(preferences::declaration(&settings).unwrap().is_some());
    settings.terminal_theme_declared = true;
    preferences::apply(
        &mut settings,
        &path,
        &json!({"terminal_theme":"follow_server"}),
    )
    .unwrap();
    let loaded = Settings::load(&path).unwrap();
    assert_eq!(loaded.terminal_theme, TerminalThemeChoice::FollowServer);
    assert_eq!(
        preferences::declaration(&loaded).unwrap(),
        Some(rexrs::api::terminal::SetTheme::new())
    );
}
#[test]
fn invalid_theme_or_non_preferences_leave_memory_and_disk_unchanged() {
    let dir = scratch();
    let path = dir.path().join("workspace.json");
    let mut settings = Settings::default();
    settings.save(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    for patch in [
        json!({"hosts":[]}),
        json!({"font_size":3}),
        json!({"terminal_theme":"named:../unsafe"}),
    ] {
        assert!(preferences::apply(&mut settings, &path, &patch).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(settings.font_size, 15.);
    }
    let defaults = preferences::defaults();
    assert_eq!(defaults["terminal"]["copy_on_selection"], false);
    assert_eq!(defaults["terminal"]["primary_selection"], false);
    assert_eq!(defaults["terminal_theme"], "follow_server");
}

#[test]
fn download_folder_is_editable_local_preference_with_absolute_path_rules() {
    let dir = scratch();
    let path = dir.path().join("workspace.json");
    let mut settings = Settings::default();
    assert!(
        preferences::apply(
            &mut settings,
            &path,
            &json!({"downloads":{"directory":"relative"}})
        )
        .is_err()
    );
    assert!(!path.exists());
    let destination = dir.path().join("saved");
    preferences::apply(
        &mut settings,
        &path,
        &json!({"downloads":{"directory":destination}}),
    )
    .unwrap();
    assert_eq!(
        Settings::load(&path).unwrap().downloads.directory,
        Some(destination.display().to_string())
    );
    preferences::apply(
        &mut settings,
        &path,
        &json!({"downloads":{"directory":null}}),
    )
    .unwrap();
    assert_eq!(Settings::load(&path).unwrap().downloads.directory, None);
}
