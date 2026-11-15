use anyhow::{Result, bail, ensure};
use rex_rune::{
    connection::{HostModel, keymap::Declaration},
    keymap::{self, Chord, Key, Resolved},
    settings::Settings,
};
use rexrs::api::client::{InspectResponseAction, Keymap};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
enum Arguments {
    None,
    Mode,
    Lines,
    Find,
}
#[derive(Clone, Copy)]
enum Remote {
    Never,
}

struct Spec {
    id: &'static str,
    title: &'static str,
    category: &'static str,
    offline: bool,
    repeats: bool,
    arguments: Arguments,
    remote: Remote,
}

const ACTIONS: &[Spec] = &[
    Spec {
        id: "pane.copy_id",
        title: "Copy pane ID",
        category: "Terminal",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "pane.clear",
        title: "Clear terminal",
        category: "Terminal",
        offline: false,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "pane.reset",
        title: "Reset terminal",
        category: "Terminal",
        offline: false,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "session.refresh",
        title: "Refresh sessions",
        category: "Navigate",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "session.switch",
        title: "Switch session",
        category: "Navigate",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "client.palette.toggle",
        title: "Commands",
        category: "Application",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "client.settings",
        title: "Settings",
        category: "Application",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "pane.copy",
        title: "Copy",
        category: "Terminal",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "pane.paste",
        title: "Paste",
        category: "Terminal",
        offline: false,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "pane.select_all",
        title: "Select all terminal text",
        category: "Terminal",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "client.search",
        title: "Find in terminal",
        category: "Terminal",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "client.find.next",
        title: "Find next",
        category: "Terminal",
        offline: true,
        repeats: true,
        arguments: Arguments::Find,
        remote: Remote::Never,
    },
    Spec {
        id: "client.find.previous",
        title: "Find previous",
        category: "Terminal",
        offline: true,
        repeats: true,
        arguments: Arguments::Find,
        remote: Remote::Never,
    },
    Spec {
        id: "client.prompt.previous",
        title: "Previous prompt",
        category: "Terminal",
        offline: true,
        repeats: true,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "client.prompt.next",
        title: "Next prompt",
        category: "Terminal",
        offline: true,
        repeats: true,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "client.scroll",
        title: "Scroll terminal",
        category: "Terminal",
        offline: true,
        repeats: true,
        arguments: Arguments::Lines,
        remote: Remote::Never,
    },
    Spec {
        id: "pane.reconnect",
        title: "Retry terminal connection",
        category: "Terminal",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "client.mode.enter",
        title: "Enter key mode",
        category: "Shortcuts",
        offline: true,
        repeats: false,
        arguments: Arguments::Mode,
        remote: Remote::Never,
    },
    Spec {
        id: "client.mode.exit",
        title: "Leave key mode",
        category: "Shortcuts",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
    Spec {
        id: "client.mode.exit_all",
        title: "Leave all key modes",
        category: "Shortcuts",
        offline: true,
        repeats: false,
        arguments: Arguments::None,
        remote: Remote::Never,
    },
];

fn schema(arguments: Arguments) -> Option<Value> {
    match arguments {
        Arguments::None => None,
        Arguments::Find => Some(
            json!({"type":"object","properties":{"when":{"enum":["searching"]}},"additionalProperties":false}),
        ),
        Arguments::Mode => Some(
            json!({"type":"object","properties":{"name":{"type":"string"},"once":{"type":"boolean"}},"required":["name"],"additionalProperties":false}),
        ),
        Arguments::Lines => Some(
            json!({"type":"object","properties":{"lines":{"type":"integer"}},"required":["lines"],"additionalProperties":false}),
        ),
    }
}

pub fn validate(id: &str, args: &Value) -> Result<()> {
    if crate::workspace::known(id) {
        return crate::workspace::validate(id, args);
    }
    let spec = ACTIONS
        .iter()
        .find(|spec| spec.id == id)
        .ok_or_else(|| anyhow::anyhow!("unknown client action: {id}"))?;
    match spec.arguments {
        Arguments::None => ensure!(
            args.is_null() || args.as_object().is_some_and(|o| o.is_empty()),
            "action takes no arguments"
        ),
        Arguments::Find => ensure!(
            args.is_null()
                || args.as_object().is_some_and(|o| o.is_empty()
                    || o.len() == 1 && o.get("when").is_some_and(|w| w == "searching")),
            "invalid find condition"
        ),
        Arguments::Mode => {
            ensure!(
                args["name"].as_str().is_some_and(|name| !name.is_empty()),
                "mode requires a name"
            );
            ensure!(
                args["once"].is_null() || args["once"].is_boolean(),
                "once must be a boolean"
            );
            ensure!(
                args.as_object()
                    .is_some_and(|o| o.keys().all(|k| k == "name" || k == "once")),
                "unknown mode argument"
            );
        }
        Arguments::Lines => {
            ensure!(
                args["lines"].as_i64().is_some(),
                "scroll requires a whole line count"
            );
            ensure!(
                args.as_object().is_some_and(|o| o.len() == 1),
                "unknown scroll argument"
            );
        }
    }
    Ok(())
}

pub fn known(id: &str) -> bool {
    ACTIONS.iter().any(|spec| spec.id == id) || crate::workspace::known(id)
}

fn declarations() -> Vec<InspectResponseAction> {
    ACTIONS
        .iter()
        .map(|spec| {
            let mut action = InspectResponseAction::new(spec.id);
            action.title = Some(spec.title.into());
            action.category = Some(spec.category.into());
            action.kind = Some("client".into());
            action.args = schema(spec.arguments);
            action.repeats = Some(spec.repeats);
            action
        })
        .chain(
            crate::workspace::ACTIONS
                .iter()
                .map(|(id, title, category)| {
                    let mut action = InspectResponseAction::new(*id);
                    action.title = Some((*title).into());
                    action.category = Some((*category).into());
                    action.kind = Some("client".into());
                    action.args = crate::workspace::schema(id);
                    action.repeats = Some(matches!(
                        *id,
                        "pane.focus" | "pane.focus_next" | "pane.focus_previous" | "pane.resize"
                    ));
                    action
                }),
        )
        .collect()
}

pub fn declaration(settings: &Settings, generation: u64) -> Declaration {
    let mut defaults = keymap::defaults();
    defaults.bindings.retain(|binding| known(&binding.action));
    let mut request = Keymap::new();
    request.actions = declarations();
    request.defaults = vec![defaults];
    request.overrides = vec![
        settings.shortcut_file.overrides.layer.clone(),
        settings.recorded_shortcuts.layer(),
    ];
    Declaration {
        generation,
        request,
        overrides: settings.shortcut_file.overrides.clone(),
    }
}

fn local_map(settings: &Settings) -> Resolved {
    let declared = declaration(settings, 0);
    keymap::resolve_all(
        &declared.request.defaults,
        &[
            &settings.shortcut_file.overrides,
            &settings.recorded_shortcuts.overrides(),
        ],
        &declared.request.actions,
    )
}

fn chord_value(chord: &Chord) -> Value {
    let (kind, key) = match chord.key {
        Key::Character(c) => ("character", c.to_string()),
        Key::Named(name) => ("named", name.into()),
        Key::Physical(name) => ("physical", name.into()),
    };
    json!({"kind":kind,"key":key,"label":chord.keycaps().join(" "),"ctrl":chord.modifiers.ctrl,"alt":chord.modifiers.alt,
        "shift":chord.modifiers.shift,"super":chord.modifiers.logo})
}

pub fn state(model: &HostModel, settings: &Settings, generation: u64) -> Value {
    let fallback;
    let snapshot = model
        .keymap
        .as_ref()
        .filter(|s| model.connected && s.generation == generation);
    let map = match snapshot {
        Some(snapshot) => &snapshot.resolved,
        None => {
            fallback = local_map(settings);
            &fallback
        }
    };
    let terminal = model.focused().and_then(|id| model.terminals.get(id));
    let frame = terminal.and_then(|connection| {
        connection
            .model
            .lock()
            .expect("terminal model")
            .frame
            .clone()
    });
    let active_search = frame
        .as_ref()
        .and_then(|frame| frame.search.as_ref())
        .is_some_and(|search| search.total > 0);
    let hint = |id: &str| {
        map.bindings
            .iter()
            .find(|b| b.binding.action == id && b.binding.mode.is_none())
            .map(|b| keymap::display(&b.binding.keys))
            .unwrap_or_default()
    };
    let actions = ACTIONS.iter().map(|spec| {
        json!({"id":spec.id,"title":spec.title,"category":spec.category,"kind":"client",
            "hint":hint(spec.id),"global":matches!(spec.id,"client.palette.toggle"|"client.settings") || matches!(spec.arguments, Arguments::Find),
            "context":if matches!(spec.arguments, Arguments::Find) { "search" } else { "" },"args":schema(spec.arguments),"repeats":spec.repeats,
            "enabled":if matches!(spec.id, "client.find.next" | "client.find.previous") { active_search }
                else if spec.id == "pane.reconnect" { terminal.is_some() }
                else if spec.category == "Terminal" { frame.is_some() && (spec.offline || model.connected) }
                else { spec.offline || model.connected },
            "remote":match spec.remote { Remote::Never => "never" }})
    }).chain(crate::workspace::ACTIONS.iter().map(|(id,title,category)| {
        let available = match *id {
            "session.new" => true,
            "session.rename" | "session.close" | "client.tab.new" => model.selected.is_some(),
            "pane.move_to_new_tab" => false,
            id if id.starts_with("pane.") => terminal.is_some(),
            _ => model.window().is_some(),
        };
        json!({"id":id,"title":title,"category":category,"kind":"client","handler":"workspace","hint":hint(id),
            "args":crate::workspace::schema(id),"enabled":(model.connected || *id == "client.tab.mute") && available,"remote":"never",
            "repeats":matches!(*id,"pane.focus"|"pane.focus_next"|"pane.focus_previous"|"pane.resize")})
    })).chain(snapshot.into_iter().flat_map(|s| s.reply.actions.iter()).filter(|a| a.kind.as_deref() != Some("client"))
        .map(|a| json!({"id":a.name,"title":a.title.as_deref().unwrap_or(&a.name),"category":a.category.as_deref().unwrap_or("Server"),
            "kind":"server","hint":hint(&a.name),"args":a.args,"repeats":a.repeats.unwrap_or(false),"enabled":model.connected,"remote":"never"}))).collect::<Vec<_>>();
    let mut bindings = map
        .bindings
        .iter()
        .map(|b| {
            let mut binding = serde_json::to_value(&b.binding).expect("binding JSON");
            binding["hint"] = json!(keymap::display(&b.binding.keys));
            binding["chords"] = b.chords.as_ref().map_or(Value::Null, |chords| {
                Value::Array(chords.iter().map(chord_value).collect())
            });
            binding
        })
        .collect::<Vec<_>>();
    if let Some(snapshot) = snapshot {
        for mode in &snapshot.reply.modes {
            for keys in mode.blocked.iter().flatten() {
                if let Ok(chords) = keymap::parse_keys(keys) {
                    bindings.push(
                        json!({"action":"client.mode.block","mode":mode.name,"keys":keys,
                        "chords":chords.iter().map(chord_value).collect::<Vec<_>>() }),
                    );
                }
            }
            let escape = Chord::parse("escape").expect("named Escape chord");
            bindings.push(json!({"action":"client.mode.exit","mode":mode.name,"keys":["escape"],"chords":[chord_value(&escape)]}));
        }
    }
    json!({"generation":generation,"revision":model.keymap_revision,"actions":actions,"bindings":bindings,
        "modes":snapshot.map(|s| &s.reply.modes),"diagnostics":map.diagnostics,"problem":model.keymap_error,
        "path":settings.shortcut_file.path,"recorded":settings.recorded_shortcuts.changes().iter().map(|change| {
            let mut row = serde_json::to_value(change).expect("shortcut JSON");
            row["hint"] = json!(keymap::display(&change.keys)); row
        }).collect::<Vec<_>>()})
}

pub fn record(
    settings: &mut Settings,
    action: &str,
    keys: Vec<String>,
    args: Option<Value>,
    mode: Option<String>,
    operation: &str,
    server_actions: &[InspectResponseAction],
) -> Result<()> {
    if known(action) {
        validate(action, args.as_ref().unwrap_or(&Value::Null))?;
    } else {
        ensure!(
            server_actions
                .iter()
                .any(|a| a.name == action && a.kind.as_deref() != Some("client")),
            "unknown server action"
        );
        ensure!(
            args.as_ref().is_none_or(|a| a.is_null() || a.is_object()),
            "action arguments must be an object"
        );
    }
    let keys = keymap::parse_keys(&keys)?
        .iter()
        .map(Chord::spelling)
        .collect::<Vec<_>>();
    ensure!(!keys.is_empty(), "record at least one chord");
    if operation == "bind" && !known(action) {
        let mut candidate = rex_rune::settings::recorded::Recorded::default();
        candidate.bind(keys.clone(), action, args.clone(), mode.clone());
        let layer = candidate.overrides();
        let resolved = keymap::resolve_all(&[], &[&layer], server_actions);
        ensure!(
            resolved.bindings.iter().any(|b| b.binding.action == action),
            "server action arguments are not valid for this binding"
        );
    }
    match operation {
        "bind" => settings.recorded_shortcuts.bind(keys, action, args, mode),
        "remove" => settings.recorded_shortcuts.remove(keys, action, args, mode),
        "restore" => {
            settings
                .recorded_shortcuts
                .restore(&keys, action, mode.as_deref());
            settings
                .recorded_shortcuts
                .unbind(&keys, action, args.as_ref(), mode.as_deref());
        }
        _ => bail!("unknown shortcut operation"),
    }
    Ok(())
}
