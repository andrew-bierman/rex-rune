#[path = "../qt/backend/workspace.rs"]
mod workspace;
use rex_rune::{
    connection::{HostCommand, HostModel},
    settings::Settings,
};
use rexrs::api::session;
use serde_json::{Value, json};
use workspace::Workspace;

fn fixture() -> HostModel {
    let mut model = HostModel {
        connected: true,
        connection_epoch: 7,
        selected: Some("s".into()),
        ..Default::default()
    };
    model.sessions = serde_json::from_value(
        json!([{"session_id":"s","label":"Work"},{"session_id":"other","label":"Other"}]),
    )
    .unwrap();
    model.views.insert("s".into(), serde_json::from_value(json!({"revision":1,"active_window_id":"w","windows":[
        {"window_id":"w","label":"Tab","focused_block_id":"a","layers":[{"kind":"tiled","bounds":[0,0,1,1],"layout":{"split":{"direction":"horizontal","ratio":0.7,"before":{"block_id":"a"},"after":{"block_id":"b"}}},"blocks":[{"block_id":"a","creator_name":"com.superlogical.terminal","label":"Shell A"},{"block_id":"b","creator_name":"com.superlogical.terminal","label":"Shell B"}]}]},
        {"window_id":"w2","label":"Other tab","focused_block_id":"c","layers":[{"kind":"tiled","bounds":[0,0,1,1],"layout":{"block_id":"c"},"blocks":[{"block_id":"c","creator_name":"com.superlogical.terminal"}]}]}
    ]})).unwrap());
    model
}
fn request(action: &str, args: Value) -> Value {
    json!({"action":action,"args":args,"epoch":7,"session":"s","window":"w","pane":"a","width":800,"height":400})
}
fn execute(ws: &mut Workspace, m: &HostModel, req: &Value) -> workspace::Outcome {
    ws.handle(m, &mut Settings::default(), "unix:///private", req)
        .unwrap()
}
fn requests(commands: Vec<HostCommand>) -> Vec<(String, Value)> {
    commands
        .into_iter()
        .map(|c| match c {
            HostCommand::Request { method, payload } => (method, payload),
            _ => panic!("expected scoped request"),
        })
        .collect()
}

#[test]
fn creation_is_explicit_and_uses_the_core_cwd_inheritance_command() {
    let m = fixture();
    let mut ws = Workspace::default();
    assert!(ws.state(&m).is_null());
    for (way, direction, side) in [
        ("right", "horizontal", "after"),
        ("left", "horizontal", "before"),
        ("down", "vertical", "after"),
        ("up", "vertical", "before"),
        ("best_fit", "horizontal", "after"),
    ] {
        let out = execute(
            &mut ws,
            &m,
            &request("pane.split", json!({"direction":way})),
        );
        match &out.commands[0] {
            HostCommand::NewTerminal {
                method,
                payload,
                cwd_from,
            } => {
                assert_eq!(method, "session.new_split");
                assert_eq!(cwd_from, "a");
                assert_eq!(payload["session_id"], "s");
                assert_eq!(payload["direction"], direction);
                assert_eq!(payload["side"], side);
                assert_eq!(payload["anchor_id"], "a");
            }
            _ => panic!("split must use core NewTerminal"),
        }
    }
    let out = execute(
        &mut ws,
        &m,
        &request("session.new", json!({"label":"Explicit"})),
    );
    assert!(
        matches!(&out.commands[0],HostCommand::CreateSession{label:Some(label),..} if label=="Explicit")
    );
    let out = execute(&mut ws, &m, &request("client.tab.new", json!({})));
    assert!(
        matches!(&out.commands[0],HostCommand::NewTerminal{method,cwd_from,..} if method=="session.new_window"&&cwd_from=="a")
    );
}

#[test]
fn close_is_cancelled_by_default_and_confirmation_never_retargets() {
    let m = fixture();
    let mut ws = Workspace::default();
    assert!(
        execute(&mut ws, &m, &request("pane.close", json!({})))
            .commands
            .is_empty()
    );
    let question = ws.state(&m);
    let id = question["id"].clone();
    assert_eq!(question["default"], "cancel");
    assert!(
        question["description"]
            .as_str()
            .unwrap()
            .contains("leaves programs running")
    );
    let cancel = request("confirm.cancel", json!({"id":id}));
    assert!(execute(&mut ws, &m, &cancel).commands.is_empty());
    assert!(ws.state(&m).is_null());
    assert!(
        ws.handle(
            &m,
            &mut Settings::default(),
            "unix:///private",
            &request("confirm.accept", json!({"id":id}))
        )
        .is_err()
    );
    execute(&mut ws, &m, &request("pane.close", json!({})));
    let id = ws.state(&m)["id"].clone();
    let mut changed = m.clone();
    changed.connection_epoch += 1;
    assert!(
        ws.handle(
            &changed,
            &mut Settings::default(),
            "unix:///private",
            &request("confirm.accept", json!({"id":id}))
        )
        .is_err()
    );
    execute(&mut ws, &m, &request("pane.close", json!({})));
    let id = ws.state(&m)["id"].clone();
    let sent =
        requests(execute(&mut ws, &m, &request("confirm.accept", json!({"id":id}))).commands);
    assert_eq!(
        sent,
        vec![(
            "block.close".into(),
            json!({"session_id":"s","block_id":"a"})
        )]
    );
    assert!(ws.state(&m).is_null());
}

#[test]
fn close_others_captures_ids_and_skips_already_closed_objects() {
    let mut m = fixture();
    let mut ws = Workspace::default();
    execute(&mut ws, &m, &request("client.tab.close_others", json!({})));
    let id = ws.state(&m)["id"].clone();
    m.views.get_mut("s").unwrap().windows.push(session::Window {
        window_id: Some("new-later".into()),
        ..Default::default()
    });
    let sent =
        requests(execute(&mut ws, &m, &request("confirm.accept", json!({"id":id}))).commands);
    assert_eq!(
        sent,
        vec![(
            "session.close_window".into(),
            json!({"session_id":"s","window_id":"w2"})
        )]
    );
    execute(&mut ws, &m, &request("pane.close_others", json!({})));
    let id = ws.state(&m)["id"].clone();
    let layer = &mut m.views.get_mut("s").unwrap().windows[0].layers[0];
    layer.blocks.retain(|b| b.block_id.as_deref() != Some("b"));
    layer.blocks.push(session::LayerBlock {
        block_id: Some("created-later".into()),
        ..Default::default()
    });
    assert!(
        execute(&mut ws, &m, &request("confirm.accept", json!({"id":id})))
            .commands
            .is_empty()
    );
}

#[test]
fn changed_view_or_removed_anchor_refuses_confirmation() {
    for change in 0..3 {
        let mut m = fixture();
        let mut ws = Workspace::default();
        execute(&mut ws, &m, &request("pane.close_others", json!({})));
        let id = ws.state(&m)["id"].clone();
        match change {
            0 => m.selected = Some("other".into()),
            1 => m.views.get_mut("s").unwrap().active_window_id = Some("w2".into()),
            _ => m.views.get_mut("s").unwrap().windows[0].layers[0]
                .blocks
                .retain(|b| b.block_id.as_deref() != Some("a")),
        }
        assert!(
            ws.handle(
                &m,
                &mut Settings::default(),
                "unix:///private",
                &request("confirm.accept", json!({"id":id}))
            )
            .is_err()
        );
    }
}

#[test]
fn geometry_helpers_produce_server_requests_without_mutating_the_scene() {
    let m = fixture();
    let before = serde_json::to_value(m.window()).unwrap();
    let mut ws = Workspace::default();
    let focus = requests(
        execute(
            &mut ws,
            &m,
            &request("pane.focus", json!({"direction":"right"})),
        )
        .commands,
    );
    assert_eq!(
        focus,
        vec![(
            "session.focus_block".into(),
            json!({"session_id":"s","block_id":"b"})
        )]
    );
    let swap = requests(
        execute(
            &mut ws,
            &m,
            &request("pane.move", json!({"direction":"right"})),
        )
        .commands,
    );
    assert_eq!(swap[0].0, "session.swap_blocks");
    assert_eq!(swap[0].1["other_block_id"], "b");
    let resize = requests(
        execute(
            &mut ws,
            &m,
            &request("pane.resize", json!({"direction":"left","amount":5})),
        )
        .commands,
    );
    assert_eq!(resize[0].1["ratio"], 0.65);
    let balance = requests(execute(&mut ws, &m, &request("pane.balance", json!({}))).commands);
    assert_eq!(balance[0].1["ratio"], 0.5);
    let moved = requests(
        execute(
            &mut ws,
            &m,
            &request("pane.move_to_tab", json!({"window_id":"w2"})),
        )
        .commands,
    );
    assert_eq!(moved[0].0, "session.move_block");
    assert_eq!(moved[0].1["anchor_block_id"], "c");
    assert_eq!(serde_json::to_value(m.window()).unwrap(), before);
    assert!(
        ws.handle(
            &m,
            &mut Settings::default(),
            "unix:///private",
            &request("pane.move_to_new_tab", json!({}))
        )
        .err()
        .unwrap()
        .to_string()
        .contains("not supported")
    );
}

#[test]
fn local_mute_is_endpoint_scoped_and_rename_defaults_stay_server_owned() {
    let m = fixture();
    let mut settings = Settings::default();
    let mut ws = Workspace::default();
    let out = ws
        .handle(
            &m,
            &mut settings,
            "unix:///one",
            &request("client.tab.mute", json!({})),
        )
        .unwrap();
    assert!(out.settings_changed && out.commands.is_empty());
    assert_eq!(settings.muted_tabs[0].endpoint, "unix:///one");
    ws.handle(
        &m,
        &mut settings,
        "unix:///two",
        &request("client.tab.mute", json!({})),
    )
    .unwrap();
    assert_eq!(settings.muted_tabs.len(), 2);
    let rename = requests(
        execute(
            &mut ws,
            &m,
            &request("client.tab.rename", json!({"label":""})),
        )
        .commands,
    );
    assert_eq!(
        rename[0].1,
        json!({"session_id":"s","window_id":"w","label":""})
    );
    let reorder = requests(
        execute(
            &mut ws,
            &m,
            &request("client.tab.move", json!({"direction":"right"})),
        )
        .commands,
    );
    assert_eq!(reorder[0].0, "session.move_window");
    assert!(reorder[0].1.get("before_window_id").is_none());
}

#[test]
fn empty_sessions_can_be_renamed_and_closed_without_inventing_a_tab() {
    let mut m = fixture();
    m.views.get_mut("s").unwrap().windows.clear();
    let mut ws = Workspace::default();
    let rename = requests(
        execute(
            &mut ws,
            &m,
            &request("session.rename", json!({"label":"Empty"})),
        )
        .commands,
    );
    assert_eq!(rename[0].1, json!({"session_id":"s","label":"Empty"}));
    execute(&mut ws, &m, &request("session.close", json!({})));
    let id = ws.state(&m)["id"].clone();
    let closed =
        requests(execute(&mut ws, &m, &request("confirm.accept", json!({"id":id}))).commands);
    assert_eq!(
        closed[0],
        ("session.destroy".into(), json!({"session_id":"s"}))
    );
}

#[test]
fn new_tab_in_empty_session_has_no_cwd_source() {
    let mut m = fixture();
    m.views.get_mut("s").unwrap().windows.clear();
    let mut ws = Workspace::default();
    let mut req = request("client.tab.new", json!({}));
    req["window"] = Value::Null;
    req["pane"] = Value::Null;
    let sent = requests(execute(&mut ws, &m, &req).commands);
    assert_eq!(sent[0].0, "session.new_window");
    assert_eq!(sent[0].1["session_id"], "s");
    assert_eq!(
        sent[0].1["layout"],
        json!({"block":{"flavor":"com.superlogical.terminal.shell"}})
    );
}

#[test]
fn registry_schemas_and_validation_are_narrow_and_unknown_requests_fail() {
    for (id, _, _) in workspace::ACTIONS {
        assert!(workspace::known(id));
        let _ = workspace::schema(id);
    }
    assert!(!workspace::known("server.stop"));
    assert!(workspace::validate("pane.resize", &json!({"direction":"up","amount":-1})).is_err());
    assert!(
        workspace::validate(
            "pane.move",
            &json!({"direction":"up","method":"server.stop"})
        )
        .is_err()
    );
    assert!(workspace::validate("pane.split", &json!({"direction":"sideways"})).is_err());
}

#[test]
fn mute_remains_local_offline_and_questions_cannot_cross_hosts_or_return_after_view_change() {
    let mut m = fixture();
    let mut ws = Workspace::default();
    let mut settings = Settings::default();
    m.connected = false;
    let out = ws
        .handle(
            &m,
            &mut settings,
            "unix:///private",
            &request("client.tab.mute", json!({})),
        )
        .unwrap();
    assert!(out.settings_changed && out.commands.is_empty());
    m.connected = true;
    execute(&mut ws, &m, &request("pane.close", json!({})));
    let id = ws.state(&m)["id"].clone();
    assert!(
        ws.handle(
            &m,
            &mut settings,
            "unix:///different",
            &request("confirm.accept", json!({"id":id}))
        )
        .is_err()
    );
    execute(&mut ws, &m, &request("pane.close", json!({})));
    m.selected = Some("other".into());
    assert!(ws.state(&m).is_null());
    m.selected = Some("s".into());
    assert!(ws.state(&m).is_null());
}

#[test]
fn default_close_shortcut_asks_for_a_pane_in_splits_and_a_tab_with_one_pane() {
    let mut m = fixture();
    let mut ws = Workspace::default();
    execute(&mut ws, &m, &request("client.close", json!({})));
    let id = ws.state(&m)["id"].clone();
    assert_eq!(
        requests(execute(&mut ws, &m, &request("confirm.accept", json!({"id":id}))).commands)[0].0,
        "block.close"
    );
    m.views.get_mut("s").unwrap().windows[0].layers[0]
        .blocks
        .truncate(1);
    execute(&mut ws, &m, &request("client.close", json!({})));
    let id = ws.state(&m)["id"].clone();
    assert_eq!(
        requests(execute(&mut ws, &m, &request("confirm.accept", json!({"id":id}))).commands)[0].0,
        "session.close_window"
    );
}

#[test]
fn leaving_a_host_discards_its_question_without_reusing_the_confirmation_id() {
    let model = fixture();
    let mut workspace = Workspace::default();
    execute(&mut workspace, &model, &request("pane.close", json!({})));
    let first = workspace.state(&model)["id"].clone();
    workspace.clear();
    assert!(workspace.state(&model).is_null());
    execute(&mut workspace, &model, &request("pane.close", json!({})));
    assert_ne!(workspace.state(&model)["id"], first);
    assert!(
        workspace
            .handle(
                &model,
                &mut Settings::default(),
                "unix:///private",
                &request("confirm.accept", json!({"id":first}))
            )
            .is_err()
    );
}
