#[path = "../qt/backend/files.rs"]
mod files;
use files::Files;
use rex_rune::{
    connection::{HostCommand, HostModel},
    settings::Settings,
};
use serde_json::{Value, json};
fn fixture() -> HostModel {
    let mut m = HostModel {
        connected: true,
        connection_epoch: 7,
        selected: Some("s".into()),
        ..Default::default()
    };
    m.views.insert("s".into(),serde_json::from_value(json!({"revision":1,"active_window_id":"w","windows":[{"window_id":"w","focused_block_id":"p","layers":[{"kind":"tiled","bounds":[0,0,1,1],"layout":{"block_id":"p"},"blocks":[{"block_id":"p","creator_name":"com.superlogical.terminal"}]}]}]})).unwrap());
    m
}
fn req(op: &str) -> Value {
    json!({"op":op,"epoch":7,"session":"s","window":"w","pane":"p"})
}
#[test]
fn directory_captures_epoch_and_cancels_queued_reads() {
    let mut f = Files::default();
    let mut m = fixture();
    let mut settings = Settings::default();
    let out = f
        .handle(&m, &mut settings, "private", &req("directory.open"))
        .unwrap();
    let HostCommand::ListDir(read) = out.commands.into_iter().next().unwrap() else {
        panic!("public listing")
    };
    assert!(read.open.upgrade().is_some());
    m.connection_epoch += 1;
    assert!(f.state(&m, &settings, "private")["directory"].is_null());
    assert!(read.open.upgrade().is_none());
    assert!(
        f.handle(&m, &mut settings, "private", &req("directory.open"))
            .is_err()
    );
}
#[test]
fn listed_path_is_quoted_and_only_pasted_with_no_enter() {
    let mut f = Files::default();
    let m = fixture();
    let mut s = Settings::default();
    let endpoint = s.hosts[0].endpoint.clone();
    let path = "/tmp/Alice's $(touch should-never-exist)";
    s.remember_path(&endpoint, path.into(), true);
    let mut open = req("directory.open");
    open["go"] = json!(true);
    f.handle(&m, &mut s, &endpoint, &open).unwrap();
    let mut choose = req("directory.choose");
    choose["path"] = json!(path);
    let out = f.handle(&m, &mut s, &endpoint, &choose).unwrap();
    assert_eq!(
        out.paste.as_deref(),
        Some("cd '/tmp/Alice'\\''s $(touch should-never-exist)'")
    );
    assert!(out.commands.is_empty());
    assert!(f.state(&m, &s, &endpoint)["directory"].is_null());
    assert!(f.handle(&m, &mut s, &endpoint, &choose).is_err());
}
#[test]
fn unsupported_upload_is_visible_error_before_local_file_read() {
    let mut f = Files::default();
    let mut m = fixture();
    m.protocol.api_revision = 10;
    let mut r = req("files.upload");
    r["paths"] = json!(["/does-not-exist"]);
    let error = f
        .handle(&m, &mut Settings::default(), "private", &r)
        .err()
        .unwrap();
    assert!(error.to_string().contains("API 11"));
}
#[test]
fn path_controls_and_option_names_follow_paste_safety() {
    assert_eq!(files::quote("-f").unwrap(), "./-f");
    assert!(files::quote("/a\nb").is_err());
    assert!(files::quote("/a\u{202e}b").is_err());
}

#[test]
fn recent_paths_stay_endpoint_scoped_bounded_and_disappear_when_disabled() {
    let mut files = Files::default();
    let model = fixture();
    let mut settings = Settings::default();
    let endpoint = settings.hosts[0].endpoint.clone();
    for n in 0..15 {
        settings.remember_path(&endpoint, format!("/folder/{n}"), true);
    }
    files
        .handle(&model, &mut settings, &endpoint, &req("directory.open"))
        .unwrap();
    assert_eq!(
        files.state(&model, &settings, &endpoint)["directory"]["rows"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    settings.terminal.show_recent_paths = false;
    settings.normalize_recent_paths();
    assert!(
        files.state(&model, &settings, &endpoint)["directory"]["rows"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(settings.recent_paths.is_empty());
    assert!(files.state(&model, &settings, "other-endpoint")["directory"].is_null());
}

#[test]
fn superseded_directory_reply_never_becomes_a_choice() {
    use rex_rune::connection::directory::Reply;
    use std::sync::Arc;
    let mut files = Files::default();
    let mut model = fixture();
    let mut settings = Settings::default();
    let out = files
        .handle(&model, &mut settings, "private", &req("directory.open"))
        .unwrap();
    let HostCommand::ListDir(old) = out.commands.into_iter().next().unwrap() else {
        panic!()
    };
    let mut edit = req("directory.edit");
    edit["text"] = json!("/new/");
    let out = files
        .handle(&model, &mut settings, "private", &edit)
        .unwrap();
    let HostCommand::ListDir(new) = out.commands.into_iter().next().unwrap() else {
        panic!()
    };
    let reply = serde_json::from_value(
        json!({"path":"/old","home":"/","status":"ok","entries":[],"truncated":false}),
    )
    .unwrap();
    model.directory = Some(Arc::new(Reply {
        id: old.id,
        epoch: 7,
        result: Ok(reply),
    }));
    assert_eq!(
        files.state(&model, &settings, "private")["directory"]["pending"],
        true
    );
    let reply=serde_json::from_value(json!({"path":"/new","home":"/","status":"ok","entries":[{"name":"b","file":true},{"name":"A","file":true},{"name":"bad\nname","file":true}],"truncated":false})).unwrap();
    model.directory = Some(Arc::new(Reply {
        id: new.id,
        epoch: 7,
        result: Ok(reply),
    }));
    let state = files.state(&model, &settings, "private");
    assert_eq!(state["directory"]["rows"][1]["path"], "/new/A");
    assert_eq!(state["directory"]["rows"][2]["path"], "/new/b");
    assert_eq!(state["directory"]["rows"].as_array().unwrap().len(), 3);
}

#[test]
fn closing_browser_cancels_the_unsent_public_read() {
    let mut files = Files::default();
    let model = fixture();
    let mut settings = Settings::default();
    let out = files
        .handle(&model, &mut settings, "private", &req("directory.open"))
        .unwrap();
    let HostCommand::ListDir(read) = out.commands.into_iter().next().unwrap() else {
        panic!()
    };
    files.clear();
    assert!(read.open.upgrade().is_none());
    assert!(files.state(&model, &settings, "private")["directory"].is_null());
}

#[test]
fn session_change_closes_browser_and_supported_upload_status_is_only_requested() {
    let mut files = Files::default();
    let mut model = fixture();
    let mut settings = Settings::default();
    files
        .handle(&model, &mut settings, "private", &req("directory.open"))
        .unwrap();
    model.selected = Some("other".into());
    assert!(files.state(&model, &settings, "private")["directory"].is_null());
    model = fixture();
    model.protocol.api_revision = 11;
    let local = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(local.path(), [0, 255, 13, 10]).unwrap();
    let mut request = req("files.upload");
    request["paths"] = json!([local.path()]);
    let out = files
        .handle(&model, &mut settings, "private", &request)
        .unwrap();
    let HostCommand::Upload {
        session,
        block,
        files: batch,
    } = out.commands.into_iter().next().unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (session.as_str(), block.as_str(), batch.len()),
        ("s", "p", 1)
    );
    assert_eq!(batch[0].size().unwrap(), 4);
    assert!(out.paste.is_none(), "only Rex inserts the upload paths");
    assert!(
        files.state(&model, &settings, "private")["upload"]["message"]
            .as_str()
            .unwrap()
            .starts_with("Upload requested.")
    );
    model.connection_epoch += 1;
    assert!(files.state(&model, &settings, "private")["upload"].is_null());
    assert!(
        files
            .handle(&model, &mut settings, "private", &request)
            .is_err()
    );
}

#[test]
fn download_uses_public_request_and_cancels_captured_target_on_navigation() {
    let destination = tempfile::tempdir().unwrap();
    let mut files = Files::default();
    let mut model = fixture();
    model.protocol.api_revision = 13;
    let mut settings = Settings::default();
    files
        .handle(&model, &mut settings, "private", &req("download.open"))
        .unwrap();
    let mut request = req("download.start");
    request["path"] = json!("~/remote file");
    request["directory"] = json!(destination.path());
    let outcome = files
        .handle(&model, &mut settings, "private", &request)
        .unwrap();
    assert!(outcome.paste.is_none());
    let HostCommand::Download(download) = &outcome.commands[0] else {
        panic!("public download request")
    };
    assert_eq!(download.session, "s");
    assert_eq!(download.block, "p");
    assert_eq!(download.path, "~/remote file");
    let id = download.id;
    assert!(
        files
            .handle(&model, &mut settings, "private", &request)
            .is_err()
    );
    model.selected = Some("other".into());
    assert!(
        matches!(files.reconcile(&model, "private").as_slice(), [HostCommand::DownloadCancel(cancelled)] if *cancelled == id)
    );
    assert!(files.reconcile(&model, "private").is_empty());
    assert!(files.state(&model, &settings, "private")["download"].is_null());
    assert!(
        files
            .handle(&model, &mut settings, "private", &request)
            .is_err()
    );
}

#[test]
fn download_revision_refusal_and_confirmation_are_explicit() {
    let destination = tempfile::tempdir().unwrap();
    use rex_rune::download::{Confirmation, Reason, State, Transfer};
    let mut files = Files::default();
    let mut model = fixture();
    model.protocol.api_revision = 12;
    let mut settings = Settings::default();
    files
        .handle(&model, &mut settings, "private", &req("download.open"))
        .unwrap();
    let mut request = req("download.start");
    request["path"] = json!("/remote");
    request["directory"] = json!(destination.path());
    assert!(
        files
            .handle(&model, &mut settings, "private", &request)
            .err()
            .unwrap()
            .to_string()
            .contains("API 13")
    );
    model.protocol.api_revision = 13;
    let out = files
        .handle(&model, &mut settings, "private", &request)
        .unwrap();
    let HostCommand::Download(download) = &out.commands[0] else {
        panic!()
    };
    let id = download.id;
    let mut transfer = Transfer::new(
        id,
        "s".into(),
        "p".into(),
        "/remote".into(),
        destination.path().to_owned(),
        7,
    );
    transfer.state = State::AwaitingConfirmation(Confirmation {
        reason: Reason::LowSpace,
        hint: 100,
        free: Some(200),
    });
    model.downloads.push(transfer);
    let rows = files::download_rows(&model);
    assert_eq!(rows[0]["confirmation"], true);
    assert!(rows[0]["status"].as_str().unwrap().contains("Low space"));
    assert!(rows[0]["saved"].is_null());
    assert!(
        matches!(files.handle(&model, &mut settings, "private", &req("download.confirm")).unwrap().commands.as_slice(), [HostCommand::DownloadConfirm(confirmed)] if *confirmed == id)
    );
    assert!(
        matches!(files.handle(&model, &mut settings, "private", &req("download.close")).unwrap().commands.as_slice(), [HostCommand::DownloadCancel(cancelled)] if *cancelled == id)
    );
}
