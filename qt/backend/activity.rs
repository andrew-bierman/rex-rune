use crate::{hosts, programs};
use rex_rune::connection::{
    HostModel,
    activity::{BlockedKind, ProgramState, TerminalProgress, time::Age},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

type Key = (String, String, String, u64);
struct Fold {
    epoch: u64,
    state: ProgramState,
    age: Option<Age>,
    done: bool,
    bell: bool,
}
#[derive(Default)]
pub struct Activity {
    folds: HashMap<Key, Fold>,
    bells: HashSet<Key>,
}
#[derive(Default)]
pub struct Presentation<'a> {
    pub visible: Option<(&'a str, &'a str)>,
    pub window_focused: bool,
    pub generated_titles: bool,
}
impl Activity {
    pub fn retain_hosts(&mut self, ids: &[String]) {
        self.folds.retain(|key, _| ids.contains(&key.0));
        self.bells.retain(|key| ids.contains(&key.0));
    }
    pub fn bell_for_block(&mut self, host: &str, model: &HostModel, session: &str, block: &str) {
        if let Some(window) = model
            .views
            .get(session)
            .and_then(|v| {
                v.windows.iter().find(|w| {
                    w.layers
                        .iter()
                        .flat_map(|l| &l.blocks)
                        .any(|b| b.block_id.as_deref() == Some(block))
                })
            })
            .and_then(|w| w.window_id.as_deref())
        {
            self.bell(host, session, window, model.connection_epoch);
        }
    }

    pub fn bell(&mut self, host: &str, session: &str, window: &str, epoch: u64) {
        self.bells
            .insert((host.into(), session.into(), window.into(), epoch));
    }
    pub fn snapshot(
        &mut self,
        host: &str,
        label: &str,
        model: &HostModel,
        muted: impl Fn(&str, &str) -> bool,
        presentation: Presentation<'_>,
        now: Instant,
    ) -> Value {
        let mut rows = Vec::new();
        let mut retained = HashSet::new();
        if !model.connected {
            self.folds.retain(|key, _| key.0 != host);
            self.bells.retain(|key| key.0 != host);
            return json!({"rows":[],"sessions":[],"reading":false,"incomplete":false,"offline":true});
        }
        let mut sessions = Vec::new();
        for session in &model.sessions {
            let Some(view) = model.views.get(&session.session_id) else {
                continue;
            };
            for window in &view.windows {
                let Some(id) = window.window_id.as_deref() else {
                    continue;
                };
                let blocks: Vec<_> = window
                    .layers
                    .iter()
                    .flat_map(|l| &l.blocks)
                    .filter(|b| {
                        b.creator_name.as_deref() == Some(rex_rune::connection::activity::TERMINAL)
                    })
                    .filter_map(|b| b.block_id.as_deref())
                    .collect();
                let focused = window
                    .focused_block_id
                    .as_deref()
                    .filter(|f| blocks.contains(f))
                    .or_else(|| blocks.first().copied());
                let records: Vec<_> = blocks
                    .iter()
                    .filter_map(|b| model.activity.get(*b))
                    .flat_map(|r| r.values())
                    .collect();
                let top = records
                    .iter()
                    .max_by_key(|r| (r.state, r.progress.is_some()))
                    .copied();
                let state = top.map(|r| r.state).unwrap_or_default();
                let candidate = records
                    .iter()
                    .filter(|r| r.state == state)
                    .map(|r| r.age)
                    .collect::<Option<Vec<_>>>()
                    .and_then(|ages| ages.into_iter().max_by_key(|a| a.elapsed(now)));
                let key = (
                    host.into(),
                    session.session_id.clone(),
                    id.into(),
                    model.connection_epoch,
                );
                retained.insert(key.clone());
                let previous = self
                    .folds
                    .get(&key)
                    .filter(|f| f.epoch == model.activity_epoch)
                    .map(|f| f.state);
                let completed = previous.is_some_and(|before| {
                    matches!(before, ProgramState::Working | ProgramState::Blocked)
                        && matches!(state, ProgramState::Done | ProgramState::Idle)
                });
                let news = previous != Some(state)
                    && (completed || top.is_some_and(|r| r.live && state != ProgramState::Idle));
                let fold = self.folds.entry(key.clone()).or_insert(Fold {
                    epoch: model.activity_epoch,
                    state,
                    age: candidate,
                    done: false,
                    bell: false,
                });
                if fold.epoch != model.activity_epoch {
                    *fold = Fold {
                        epoch: model.activity_epoch,
                        state,
                        age: candidate,
                        done: false,
                        bell: false,
                    };
                }
                if fold.state != state {
                    fold.age = candidate.map(|a| if a.reported { a } else { Age::live(now) });
                } else {
                    fold.age = match (fold.age, candidate) {
                        (Some(old), Some(new)) => Some(if new.elapsed(now) < old.elapsed(now) {
                            new
                        } else {
                            old
                        }),
                        (_, new) => new,
                    };
                }
                fold.state = state;
                fold.bell |= self.bells.remove(&key);
                let in_front = presentation.visible == Some((session.session_id.as_str(), id));
                let seen = in_front && presentation.window_focused;
                if news && (completed || state == ProgramState::Done) {
                    fold.done = true;
                }
                if seen || matches!(state, ProgramState::Working | ProgramState::Blocked) {
                    fold.done = false;
                }
                if seen {
                    fold.done = false;
                    fold.bell = false;
                }
                let progress = blocks
                    .iter()
                    .filter_map(|b| model.progress.get(*b).copied())
                    .max_by_key(|p| p.rank())
                    .or_else(|| {
                        top.filter(|r| r.state == ProgramState::Working)
                            .and_then(|r| r.progress)
                            .map(|n| TerminalProgress::Set(Some(n)))
                    });
                let word = match state {
                    ProgramState::Blocked => match top.and_then(|r| r.kind) {
                        Some(BlockedKind::Permission) => "Needs approval",
                        Some(BlockedKind::Question) => "Question",
                        Some(BlockedKind::Auth) => "Needs sign-in",
                        None => "Needs input",
                    }
                    .to_owned(),
                    ProgramState::Error => "Error".into(),
                    _ if fold.bell => "Bell".into(),
                    _ => match progress {
                        Some(TerminalProgress::Error(_)) => "Error".into(),
                        Some(TerminalProgress::Paused(Some(n))) => format!("Paused {n}%"),
                        Some(TerminalProgress::Paused(None)) => "Paused".into(),
                        _ if fold.done => "Done".into(),
                        _ => String::new(),
                    },
                };
                let panes: Vec<_> = blocks.iter().map(|b| {
                    let record = model.activity.get(*b).and_then(|r| r.values().max_by_key(|r| (r.state,r.progress.is_some())));
                    let program = record.and_then(|r| r.app.as_deref()).or_else(|| model.programs.get(*b).map(String::as_str));
                    json!({"pane":b,"program":program,"icon":programs::icon(program,model.scripts.get(*b).map(String::as_str),model.executables.get(*b).map(String::as_str))})
                }).collect();
                let mut icons = Vec::new();
                for pane in focused
                    .into_iter()
                    .chain(blocks.iter().copied().filter(|b| Some(*b) != focused))
                {
                    let icon = &panes.iter().find(|p| p["pane"] == pane).unwrap()["icon"];
                    if !icons.iter().any(|v: &Value| v["slug"] == icon["slug"]) {
                        icons.push(icon.clone());
                    }
                }
                if icons.is_empty() {
                    icons.push(programs::icon(None, None, None));
                }
                let progress_json = progress.map(|p| {
                    let (kind, n) = match p {
                        TerminalProgress::Set(n) => ("normal", n),
                        TerminalProgress::Indeterminate => ("indeterminate", None),
                        TerminalProgress::Paused(n) => ("paused", n),
                        TerminalProgress::Error(n) => ("error", n),
                    };
                    json!({"kind":kind,"percent":n})
                });
                let title = hosts::tab_title(model, window, presentation.generated_titles);
                let is_muted = muted(&session.session_id, id);
                let list_rank = match state {
                    ProgramState::Blocked => 0,
                    ProgramState::Error => 1,
                    ProgramState::Working => 3,
                    _ => 2,
                };
                let listed = !is_muted
                    && (matches!(
                        state,
                        ProgramState::Blocked | ProgramState::Error | ProgramState::Working
                    ) || fold.done);
                let word_rank = match state {
                    ProgramState::Blocked => 0,
                    _ if word == "Error" => 1,
                    _ if fold.bell || matches!(progress, Some(TerminalProgress::Paused(_))) => 2,
                    _ => 3,
                };
                rows.push(json!({"key":serde_json::to_string(&key).unwrap(),"host":host,"hostLabel":label,"epoch":model.connection_epoch,"activityEpoch":model.activity_epoch,
                    "session":session.session_id,"sessionLabel":session.label,"window":id,"pane":focused,"label":title,"state":state_name(state),"rank":list_rank,"word":word,"wordRank":word_rank,"listed":listed,
                    "listWord":match state {ProgramState::Working=>"Working",_ if fold.done&&!matches!(state,ProgramState::Blocked|ProgramState::Error)=>"Done",_=>word.as_str()},
                    "groupName":match list_rank {0=>"Needs you",1=>"Error",2=>"Done",_=>"Working"},
                    "message":top.and_then(|r|r.msg.as_ref().or(r.title.as_ref())),"progress":progress_json,"icons":icons,"panes":panes,"age":fold.age.map(|a|elapsed(a,now)),
                    "muted":is_muted,"inFront":in_front,"seen":seen,"unseen":fold.done||fold.bell,"eligible":!seen&&!is_muted&&(state==ProgramState::Blocked||state==ProgramState::Error||fold.done||fold.bell),
                    "news":news&&!seen&&!is_muted,"newsState":if completed {"done"} else {state_name(state)},"background":model.background_views.contains(&session.session_id)}));
            }
            let word = rows
                .iter()
                .filter(|r| {
                    r["session"] == session.session_id
                        && r["muted"] == false
                        && r["inFront"] == false
                        && r["word"] != ""
                })
                .min_by_key(|r| r["wordRank"].as_u64().unwrap_or(3));
            let icon_window = view
                .windows
                .iter()
                .find(|w| w.window_id.is_some() && w.window_id == view.active_window_id)
                .or_else(|| view.windows.first());
            let icon = icon_window
                .and_then(|w| {
                    rows.iter().find(|r| {
                        Some(r["window"].as_str().unwrap_or_default()) == w.window_id.as_deref()
                    })
                })
                .and_then(|r| r["icons"].as_array())
                .and_then(|i| i.first())
                .cloned()
                .unwrap_or_else(|| programs::icon(None, None, None));
            sessions.push(json!({"host":host,"session":session.session_id,"word":word.map(|r|r["word"].clone()),"wordRank":word.map(|r|r["wordRank"].clone()),"tabLabel":word.map(|r|r["label"].clone()),"icon":icon}));
        }
        self.folds
            .retain(|k, _| k.0 != host || retained.contains(k));
        self.bells.retain(|k| k.0 != host || retained.contains(k));
        json!({"rows":rows,"sessions":sessions,"reading":model.activity_reading,"incomplete":model.activity_incomplete,"offline":false})
    }
}
fn state_name(state: ProgramState) -> &'static str {
    match state {
        ProgramState::Idle => "idle",
        ProgramState::Done => "done",
        ProgramState::Working => "working",
        ProgramState::Error => "error",
        ProgramState::Blocked => "blocked",
    }
}
fn elapsed(age: Age, now: Instant) -> Value {
    let seconds = age.elapsed(now).as_secs();
    let text = if seconds < 60 {
        "now".into()
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86400)
    };
    json!({"text":if age.reported {format!("reported {text}")} else {text},"seconds":seconds,"reported":age.reported,"description":if age.reported {"Age of last report; state may have begun earlier"} else {"Observed duration of current state"}})
}
