#[path = "../qt/backend/activity.rs"]
mod activity;
#[path = "../qt/backend/hosts.rs"]
#[allow(dead_code)]
mod hosts;
#[path = "../qt/backend/programs.rs"]
mod programs;
use rex_rune::connection::{
    HostModel,
    activity::{ProgramState, StatusRecord, TerminalProgress, time::Age},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
fn fixture() -> HostModel {
    let mut m = HostModel {
        connected: true,
        connection_epoch: 7,
        activity_epoch: 2,
        ..Default::default()
    };
    m.sessions = serde_json::from_value(json!([{"session_id":"s","label":"Work"}])).unwrap();
    m.views.insert("s".into(),serde_json::from_value(json!({"revision":1,"windows":[{"window_id":"w","label":"Tab","focused_block_id":"a","layers":[{"kind":"tiled","blocks":[{"block_id":"a","creator_name":"com.superlogical.terminal"},{"block_id":"b","creator_name":"com.superlogical.terminal"}]}]}]})).unwrap());
    m.background_views.insert("s".into());
    m
}
fn report(m: &mut HostModel, pane: &str, state: ProgramState, now: Instant) {
    m.activity.entry(pane.into()).or_default().insert(
        "agent".into(),
        StatusRecord {
            state,
            live: true,
            age: Some(Age::live(now)),
            app: Some("claude".into()),
            ..Default::default()
        },
    );
}
fn snapshot(
    a: &mut activity::Activity,
    m: &HostModel,
    seen: bool,
    muted: bool,
    now: Instant,
) -> Value {
    a.snapshot(
        "h",
        "Host",
        m,
        |_, _| muted,
        activity::Presentation {
            visible: seen.then_some(("s", "w")),
            window_focused: true,
            generated_titles: false,
        },
        now,
    )
}
#[test]
fn unopened_inventory_has_captured_identity_without_opening_sessions() {
    let now = Instant::now();
    let mut m = fixture();
    report(&mut m, "b", ProgramState::Blocked, now);
    let before = m.background_views.clone();
    let v = snapshot(&mut activity::Activity::default(), &m, false, false, now);
    let r = &v["rows"][0];
    assert_eq!(r["session"], "s");
    assert_eq!(r["window"], "w");
    assert_eq!(r["epoch"], 7);
    assert_eq!(r["host"], "h");
    assert_eq!(r["word"], "Needs input");
    assert_eq!(r["eligible"], true);
    assert_eq!(m.background_views, before);
}
#[test]
fn seen_and_mute_are_local_and_scoped_but_do_not_hide_reports() {
    let now = Instant::now();
    let mut m = fixture();
    let mut a = activity::Activity::default();
    report(&mut m, "a", ProgramState::Working, now);
    snapshot(&mut a, &m, false, false, now);
    report(&mut m, "a", ProgramState::Done, now);
    assert_eq!(
        snapshot(&mut a, &m, false, false, now)["rows"][0]["unseen"],
        true
    );
    assert_eq!(
        snapshot(&mut a, &m, true, false, now)["rows"][0]["unseen"],
        false
    );
    report(&mut m, "a", ProgramState::Error, now);
    let v = snapshot(&mut a, &m, false, true, now);
    assert_eq!(v["rows"][0]["eligible"], false);
    assert_eq!(v["rows"][0]["word"], "Error");
    assert_eq!(m.activity["a"]["agent"].state, ProgramState::Error);
    a.bell("other", "s", "w", 7);
    assert_eq!(
        snapshot(&mut a, &m, false, false, now)["rows"][0]["unseen"],
        false
    );
}
#[test]
fn folded_clock_restarts_after_a_higher_priority_sibling_and_qualifies_reports() {
    let now = Instant::now();
    let mut m = fixture();
    let mut a = activity::Activity::default();
    report(&mut m, "a", ProgramState::Working, now);
    snapshot(&mut a, &m, false, false, now);
    report(
        &mut m,
        "b",
        ProgramState::Blocked,
        now + Duration::from_secs(120),
    );
    snapshot(&mut a, &m, false, false, now + Duration::from_secs(120));
    m.activity.remove("b");
    let v = snapshot(&mut a, &m, false, false, now + Duration::from_secs(240));
    assert_eq!(v["rows"][0]["age"]["seconds"], 0);
    m.activity
        .get_mut("a")
        .unwrap()
        .get_mut("agent")
        .unwrap()
        .age = Some(Age {
        observed: now,
        initial: Duration::from_secs(300),
        reported: true,
    });
    m.activity_epoch += 1;
    let v = snapshot(&mut a, &m, false, false, now);
    assert_eq!(v["rows"][0]["age"]["text"], "reported 5m");
}
#[test]
fn focused_first_dedup_mapping_progress_and_unknown_age() {
    let now = Instant::now();
    let mut m = fixture();
    m.programs.insert("a".into(), "vi".into());
    m.programs.insert("b".into(), "vim".into());
    m.progress
        .insert("a".into(), TerminalProgress::Set(Some(60)));
    m.progress
        .insert("b".into(), TerminalProgress::Error(Some(4)));
    let v = snapshot(&mut activity::Activity::default(), &m, false, false, now);
    assert_eq!(v["rows"][0]["icons"].as_array().unwrap().len(), 1);
    assert_eq!(v["rows"][0]["progress"]["percent"], 4);
    assert!(v["rows"][0]["age"].is_null());
    assert_eq!(
        programs::icon(Some("python3.12.exe"), None, None)["slug"],
        "python"
    );
    assert_eq!(
        programs::icon(
            Some("unknown"),
            Some("/pkg/copilot/npm-loader.js"),
            Some("node")
        )["slug"],
        "copilot"
    );
    assert_eq!(programs::icon(None, None, None)["glyph"], "\u{f489}");
}

#[test]
fn live_initial_done_and_removal_latch_but_new_work_clears_done() {
    let now = Instant::now();
    let mut m = fixture();
    let mut a = activity::Activity::default();
    report(&mut m, "a", ProgramState::Done, now);
    let v = snapshot(&mut a, &m, false, false, now);
    assert_eq!(v["rows"][0]["unseen"], true);
    assert_eq!(v["rows"][0]["news"], true);
    assert_eq!(
        snapshot(&mut a, &m, false, false, now)["rows"][0]["news"],
        false
    );
    report(&mut m, "a", ProgramState::Working, now);
    assert_eq!(
        snapshot(&mut a, &m, false, false, now)["rows"][0]["unseen"],
        false
    );
    m.activity.clear();
    let v = snapshot(&mut a, &m, false, false, now);
    assert_eq!(v["rows"][0]["word"], "Done");
    assert_eq!(v["rows"][0]["newsState"], "done");
    assert!(v["rows"][0]["age"].is_null());
    m.connected = false;
    assert_eq!(snapshot(&mut a, &m, false, false, now)["offline"], true);
    m.connected = true;
    assert_eq!(
        snapshot(&mut a, &m, false, false, now)["rows"][0]["unseen"],
        false
    );
}
#[test]
fn displayed_unfocused_tab_keeps_receipts_but_session_word_excludes_it() {
    let now = Instant::now();
    let mut m = fixture();
    let mut a = activity::Activity::default();
    report(&mut m, "a", ProgramState::Done, now);
    let v = a.snapshot(
        "h",
        "Host",
        &m,
        |_, _| false,
        activity::Presentation {
            visible: Some(("s", "w")),
            window_focused: false,
            generated_titles: false,
        },
        now,
    );
    assert_eq!(v["rows"][0]["unseen"], true);
    assert_eq!(v["rows"][0]["inFront"], true);
    assert!(v["sessions"][0]["word"].is_null());
    assert_eq!(
        snapshot(&mut a, &m, true, false, now)["rows"][0]["unseen"],
        false
    );
}
#[test]
fn youngest_return_and_unknown_sibling_invalidate_exact_looking_clock() {
    let now = Instant::now();
    let mut m = fixture();
    let mut a = activity::Activity::default();
    report(&mut m, "a", ProgramState::Working, now);
    report(
        &mut m,
        "b",
        ProgramState::Working,
        now + Duration::from_secs(60),
    );
    let v = snapshot(&mut a, &m, false, false, now + Duration::from_secs(120));
    assert_eq!(v["rows"][0]["age"]["seconds"], 120);
    m.activity
        .get_mut("b")
        .unwrap()
        .get_mut("agent")
        .unwrap()
        .age = None;
    assert!(
        snapshot(&mut a, &m, false, false, now + Duration::from_secs(120))["rows"][0]["age"]
            .is_null()
    );
}
#[test]
fn nonterminal_stale_records_are_not_presented_and_bells_map_public_blocks() {
    let now = Instant::now();
    let mut m = fixture();
    let mut a = activity::Activity::default();
    m.views.get_mut("s").unwrap().windows[0].layers[0].blocks[1].creator_name =
        Some("other".into());
    report(&mut m, "b", ProgramState::Blocked, now);
    let v = snapshot(&mut a, &m, false, false, now);
    assert_eq!(v["rows"][0]["state"], "idle");
    assert_eq!(v["rows"][0]["listed"], false);
    a.bell_for_block("h", &m, "s", "a");
    assert_eq!(
        snapshot(&mut a, &m, false, false, now)["rows"][0]["word"],
        "Bell"
    );
    a.retain_hosts(&[]);
    assert_eq!(
        snapshot(&mut a, &m, false, false, now)["rows"][0]["unseen"],
        false
    );
}

#[test]
fn reconnect_and_equal_labels_cannot_reuse_seen_or_bell_identity() {
    let now = Instant::now();
    let mut m = fixture();
    let mut a = activity::Activity::default();
    a.bell("h", "s", "w", 7);
    assert_eq!(
        snapshot(&mut a, &m, false, false, now)["rows"][0]["unseen"],
        true
    );
    let other = a.snapshot(
        "other",
        "Host",
        &m,
        |_, _| false,
        activity::Presentation::default(),
        now,
    );
    assert_eq!(other["rows"][0]["unseen"], false);
    m.connection_epoch = 8;
    m.activity_epoch += 1;
    let v = snapshot(&mut a, &m, false, false, now);
    assert_eq!(v["rows"][0]["unseen"], false);
    assert_eq!(v["rows"][0]["epoch"], 8);
    report(&mut m, "a", ProgramState::Done, now);
    m.activity
        .get_mut("a")
        .unwrap()
        .get_mut("agent")
        .unwrap()
        .live = false;
    assert_eq!(
        snapshot(&mut a, &m, false, false, now)["rows"][0]["unseen"],
        false
    );
}
