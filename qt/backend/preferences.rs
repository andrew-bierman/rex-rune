//! Editable Qt preferences are a projection of the public settings, never a second host store.
use anyhow::{Context, Result, ensure};
use rex_rune::{
    settings::Settings,
    terminal_theme::{BUILTINS, ThemeDirs},
};
use serde_json::{Value, json};
use std::path::Path;

const FIELDS: &[&str] = &[
    "font_family",
    "font_fallbacks",
    "font_size",
    "terminal",
    "terminal_theme",
    "chrome_theme",
    "ui_scale",
    "pane_density",
    "pane_header",
    "pane_padding",
    "interface",
    "vertical_tabs",
    "generated_titles",
];
fn editable(settings: &Settings) -> Value {
    let mut source = serde_json::to_value(settings).expect("settings serialize");
    source["terminal"]["show_recent_paths"] = json!(settings.terminal.show_recent_paths);
    Value::Object(
        FIELDS
            .iter()
            .map(|key| ((*key).into(), source[*key].clone()))
            .collect(),
    )
}
pub fn defaults() -> Value {
    editable(&Settings::default())
}
pub fn themes() -> Value {
    static THEMES: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    THEMES.get_or_init(discover_themes).clone()
}
fn discover_themes() -> Value {
    let mut rows = vec![
        json!({"value":"follow_server","title":"Follow server"}),
        json!({"value":"builtin_default","title":"Rex default colours"}),
        json!({"value":"follow_chrome","title":"Follow chrome"}),
    ];
    rows.extend(
        BUILTINS
            .iter()
            .map(|t| json!({"value":format!("named:rex/{}",t.id),"title":t.name})),
    );
    rows.extend(
        ThemeDirs::from_env()
            .list()
            .iter()
            .map(|t| json!({"value":format!("named:{}",t.name),"title":t.name})),
    );
    Value::Array(rows)
}
pub fn snapshot(settings: &Settings) -> Value {
    let mut value = editable(settings);
    value["_defaults"] = defaults();
    value["_themes"] = themes();
    if let Ok(Some(theme)) =
        ThemeDirs::from_env().resolve(&settings.terminal_theme, settings.chrome_theme)
    {
        value["_appearance"] = json!({"background":theme.background,"foreground":theme.foreground,"cursor":theme.cursor,"cursor_text":theme.cursor_text,"selection_background":theme.selection_background,"selection_foreground":theme.selection_foreground});
    }
    value
}
/// Only the latest authoritative settings are merged, so a dialog cannot roll back navigation.
pub fn apply(settings: &mut Settings, path: &Path, patch: &Value) -> Result<()> {
    let patch = patch.as_object().context("Preferences must be an object")?;
    ensure!(
        patch.keys().all(|key| FIELDS.contains(&key.as_str())),
        "Unknown preference"
    );
    let mut value = serde_json::to_value(&*settings)?;
    for (key, field) in patch {
        value[key] = field.clone();
    }
    let mut next: Settings = serde_json::from_value(value).context("Invalid preferences")?;
    ensure!(
        next.font_size.is_finite() && (8. ..=48.).contains(&next.font_size),
        "Font size must be 8–48 points"
    );
    ensure!(
        next.ui_scale.is_finite() && (0.7..=1.5).contains(&next.ui_scale),
        "Interface zoom must be 70–150%"
    );
    ensure!(
        next.pane_padding.is_finite() && (0. ..=32.).contains(&next.pane_padding),
        "Padding must be 0–32 points"
    );
    if let Some(padding) = next.interface.pane_padding_vertical {
        ensure!(
            padding.is_finite() && (0. ..=32.).contains(&padding),
            "Padding must be 0–32 points"
        );
    }
    ensure!(
        next.interface.tab_icon_count <= 5,
        "Choose at most five tab icons"
    );
    next.terminal.font.line_height = next.terminal.font.line_height();
    next.terminal.font.spacing = next.terminal.font.spacing();
    ensure!(!next.font_family.trim().is_empty(), "Choose a font family");
    if !next.terminal.show_recent_paths {
        next.recent_paths.clear();
    }
    next.font_fallbacks.retain(|name| !name.trim().is_empty());
    if patch.contains_key("terminal_theme") {
        next.terminal_theme_chosen = true;
    } else if next.chrome_theme != settings.chrome_theme
        && !next.terminal_theme_chosen
        && next.terminal_theme == rex_rune::settings::TerminalThemeChoice::FollowServer
    {
        next.terminal_theme = rex_rune::settings::TerminalThemeChoice::FollowChrome;
    }
    ThemeDirs::from_env().resolve(&next.terminal_theme, next.chrome_theme)?;
    next.save(path)?;
    // Settings::save is atomic; publish only after the file is safely written.
    *settings = next;
    Ok(())
}
pub fn declaration(settings: &Settings) -> Result<Option<rexrs::api::terminal::SetTheme>> {
    Ok(
        match ThemeDirs::from_env().resolve(&settings.terminal_theme, settings.chrome_theme)? {
            Some(theme) => Some(theme.args()),
            None => settings
                .terminal_theme_declared
                .then(rexrs::api::terminal::SetTheme::new),
        },
    )
}
