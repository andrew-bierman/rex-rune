use anyhow::{Context, Result, bail, ensure};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env,
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub const DEFAULT_FILE: &str = include_str!("../default-settings.toml");

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Settings,
    NewSession,
    PreviousTab,
    NextTab,
    NewTab,
    ToggleFocus,
    FilterSidebar,
    NextPane,
    SplitRight,
    SplitDown,
    CloseSession,
    CloseTab,
    ClosePane,
    RenameSession,
    RenameTab,
    RenamePane,
    ResizeLeft,
    ResizeRight,
    ResizeUp,
    ResizeDown,
    ComposeInput,
    CopySelection,
    Find,
    ScrollUp,
    ScrollDown,
    JumpToLiveOutput,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    MoveTabLeft,
    MoveTabRight,
    NextAttention,
    MoveSidebarUp,
    MoveSidebarDown,
    OpenLinks,
    Quit,
}

const DEFAULT_KEYS: [(Action, &str); 37] = [
    (Action::Settings, "F1"),
    (Action::NewSession, "F2"),
    (Action::PreviousTab, "F3"),
    (Action::NextTab, "F4"),
    (Action::NewTab, "F5"),
    (Action::ToggleFocus, "F6"),
    (Action::FilterSidebar, "/"),
    (Action::NextPane, "F7"),
    (Action::SplitRight, "F8"),
    (Action::SplitDown, "F9"),
    (Action::Quit, "F10"),
    (Action::CloseSession, "Shift+F2"),
    (Action::CloseTab, "Shift+F5"),
    (Action::ClosePane, "Shift+F9"),
    (Action::ResizeLeft, "Ctrl+Shift+Left"),
    (Action::ResizeRight, "Ctrl+Shift+Right"),
    (Action::ResizeUp, "Ctrl+Shift+Up"),
    (Action::ResizeDown, "Ctrl+Shift+Down"),
    (Action::ComposeInput, "F11"),
    (Action::CopySelection, "F12"),
    (Action::Find, "Alt+/"),
    (Action::ScrollUp, "Alt+PageUp"),
    (Action::ScrollDown, "Alt+PageDown"),
    (Action::JumpToLiveOutput, "Alt+End"),
    (Action::RenameSession, "Shift+F6"),
    (Action::RenameTab, "Shift+F7"),
    (Action::RenamePane, "Shift+F8"),
    (Action::FocusLeft, "Ctrl+Alt+Left"),
    (Action::FocusRight, "Ctrl+Alt+Right"),
    (Action::FocusUp, "Ctrl+Alt+Up"),
    (Action::FocusDown, "Ctrl+Alt+Down"),
    (Action::MoveTabLeft, "Alt+Shift+Left"),
    (Action::MoveTabRight, "Alt+Shift+Right"),
    (Action::NextAttention, "Alt+a"),
    (Action::MoveSidebarUp, "Alt+Shift+Up"),
    (Action::MoveSidebarDown, "Alt+Shift+Down"),
    (Action::OpenLinks, "Alt+o"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Chord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Chord {
    fn parse(value: &str) -> Result<Self> {
        let mut parts = value.split('+').map(str::trim).peekable();
        let mut modifiers = KeyModifiers::empty();
        let end = loop {
            let part = parts.next().context("Expected a key")?;
            if parts.peek().is_none() {
                break part.to_ascii_lowercase();
            }
            let modifier = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                "super" => KeyModifiers::SUPER,
                _ => bail!("Unknown modifier {part}"),
            };
            ensure!(!modifiers.contains(modifier), "Duplicated modifier {part}");
            modifiers.insert(modifier);
        };
        let code = match end.as_str() {
            "escape" | "esc" => KeyCode::Esc,
            "enter" => KeyCode::Enter,
            "tab" if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
            "tab" => KeyCode::Tab,
            "space" => KeyCode::Char(' '),
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "insert" => KeyCode::Insert,
            "delete" => KeyCode::Delete,
            "backspace" => KeyCode::Backspace,
            _ if end.len() == 1 => KeyCode::Char(end.as_bytes()[0] as char),
            _ => KeyCode::F(
                end.strip_prefix('f')
                    .and_then(|number| number.parse::<u8>().ok())
                    .filter(|number| (1..=24).contains(number))
                    .with_context(|| format!("Unknown key {end}"))?,
            ),
        };
        Ok(Self { code, modifiers })
    }

    fn from_event(event: KeyEvent) -> Self {
        let code = match event.code {
            KeyCode::Char(character) => KeyCode::Char(character.to_ascii_lowercase()),
            code => code,
        };
        Self {
            code,
            modifiers: event.modifiers,
        }
    }
}

#[derive(Debug)]
struct Binding {
    chord: Chord,
    label: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupFocus {
    #[default]
    Sidebar,
    Terminal,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarSide {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sidebar {
    pub side: SidebarSide,
    pub width: Option<u16>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Panes {
    pub borders: bool,
    pub headers: bool,
    pub default_colors: PaneDefaultColors,
    pub ansi_palette: PaneDefaultColors,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneDefaultColors {
    #[default]
    Terminal,
    Server,
}

impl Default for Panes {
    fn default() -> Self {
        Self {
            borders: true,
            headers: true,
            default_colors: PaneDefaultColors::default(),
            ansi_palette: PaneDefaultColors::Server,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ThemeFile {
    chrome: Chrome,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Chrome {
    #[serde(deserialize_with = "color")]
    pub text: Color,
    #[serde(deserialize_with = "color")]
    pub background: Color,
    #[serde(deserialize_with = "color")]
    pub accent: Color,
    #[serde(deserialize_with = "color")]
    pub border: Color,
    #[serde(deserialize_with = "color")]
    pub muted: Color,
    #[serde(deserialize_with = "color")]
    pub warning: Color,
    #[serde(deserialize_with = "optional_color")]
    pub selection_text: Option<Color>,
    #[serde(deserialize_with = "optional_color")]
    pub selection_background: Option<Color>,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            text: Color::Reset,
            background: Color::Reset,
            accent: Color::Cyan,
            border: Color::DarkGray,
            muted: Color::DarkGray,
            warning: Color::Yellow,
            selection_text: None,
            selection_background: None,
        }
    }
}

impl Chrome {
    pub fn style(self) -> Style {
        Style::new().fg(self.text).bg(self.background)
    }

    pub fn selection(self) -> Style {
        match (self.selection_text, self.selection_background) {
            (None, None) => self.style().add_modifier(Modifier::REVERSED).bold(),
            (text, background) => self
                .style()
                .fg(text.unwrap_or(self.text))
                .bg(background.unwrap_or(self.background))
                .bold(),
        }
    }
}

fn color<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Color, D::Error> {
    let raw = String::deserialize(input)?;
    if raw == "default" {
        return Ok(Color::Reset);
    }
    raw.parse().map_err(|_| {
        serde::de::Error::custom(format!(
            "invalid color {raw:?}: use default, an ANSI color name, or #RRGGBB"
        ))
    })
}

fn optional_color<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<Color>, D::Error> {
    color(input).map(Some)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Experimental {
    input_composer: bool,
    mouse_actions: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SelectionConfig {
    mouse: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NotificationConfig {
    attention: bool,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct File {
    theme: String,
    startup_focus: StartupFocus,
    sidebar: Sidebar,
    panes: Panes,
    experimental: Experimental,
    selection: SelectionConfig,
    notifications: NotificationConfig,
    keys: BTreeMap<Action, String>,
}

impl Default for File {
    fn default() -> Self {
        Self {
            theme: "terminal".into(),
            startup_focus: StartupFocus::Sidebar,
            sidebar: Sidebar::default(),
            panes: Panes::default(),
            experimental: Experimental::default(),
            selection: SelectionConfig::default(),
            notifications: NotificationConfig::default(),
            keys: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct Settings {
    pub theme: String,
    pub chrome: Chrome,
    pub startup_focus: StartupFocus,
    pub sidebar: Sidebar,
    pub panes: Panes,
    pub mouse_selection: bool,
    pub mouse_actions: bool,
    pub attention_notifications: bool,
    keys: BTreeMap<Action, Binding>,
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_file(File::default(), Path::new("")).expect("valid built-in settings")
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error).with_context(|| format!("Read {}", path.display())),
        };
        let file = toml::from_str(&source).with_context(|| format!("Parse {}", path.display()))?;
        Self::from_file(file, path.parent().unwrap_or_else(|| Path::new("")))
            .with_context(|| format!("Validate {}", path.display()))
    }

    fn from_file(mut file: File, directory: &Path) -> Result<Self> {
        if let Some(width) = file.sidebar.width {
            ensure!(
                (12..=100).contains(&width),
                "sidebar.width must be 12–100 columns"
            );
        }
        let chrome = if file.theme == "terminal" {
            Chrome::default()
        } else {
            ensure!(
                !file.theme.is_empty()
                    && file
                        .theme
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
                "Theme names may contain letters, digits, dashes, and underscores"
            );
            let path = directory
                .join("themes")
                .join(format!("{}.toml", file.theme));
            let contents = fs::read_to_string(&path)
                .with_context(|| format!("Read theme {}", path.display()))?;
            toml::from_str::<ThemeFile>(&contents)
                .with_context(|| format!("Parse theme {}", path.display()))?
                .chrome
        };
        let mouse_selection = file.selection.mouse || file.experimental.mouse_actions;
        let mut keys = BTreeMap::<Action, Binding>::new();
        let is_optional_default = |action| {
            matches!(
                action,
                Action::ScrollUp
                    | Action::ScrollDown
                    | Action::JumpToLiveOutput
                    | Action::RenameSession
                    | Action::RenameTab
                    | Action::RenamePane
                    | Action::ClosePane
                    | Action::FocusLeft
                    | Action::FocusRight
                    | Action::FocusUp
                    | Action::FocusDown
                    | Action::MoveTabLeft
                    | Action::MoveTabRight
                    | Action::NextAttention
                    | Action::Find
                    | Action::FilterSidebar
                    | Action::MoveSidebarUp
                    | Action::MoveSidebarDown
                    | Action::OpenLinks
            )
        };
        let mut defaults = DEFAULT_KEYS;
        // Resolve configured bindings before assigning newer defaults.
        defaults.sort_by_key(|(action, _)| {
            is_optional_default(*action) && !file.keys.contains_key(action)
        });
        for (action, fallback) in defaults {
            if action == Action::ComposeInput && !file.experimental.input_composer {
                continue;
            }
            if action == Action::CopySelection && !mouse_selection {
                continue;
            }
            let binding = file.keys.remove(&action);
            let optional_default = binding.is_none() && is_optional_default(action);
            let label = binding.unwrap_or_else(|| fallback.into());
            if label.eq_ignore_ascii_case("none") {
                continue;
            }
            let chord =
                Chord::parse(&label).with_context(|| format!("Invalid {action:?} binding"))?;
            if keys.values().any(|binding| binding.chord == chord) {
                // New defaults must not invalidate older custom bindings.
                if optional_default {
                    continue;
                }
                bail!("Duplicate shortcut {label}");
            }
            keys.insert(action, Binding { chord, label });
        }
        Ok(Self {
            theme: file.theme,
            chrome,
            startup_focus: file.startup_focus,
            sidebar: file.sidebar,
            panes: file.panes,
            mouse_selection,
            attention_notifications: file.notifications.attention,
            mouse_actions: file.experimental.mouse_actions,
            keys,
        })
    }

    pub fn action(&self, key: KeyEvent) -> Option<Action> {
        let chord = Chord::from_event(key);
        self.keys
            .iter()
            .find_map(|(action, binding)| (binding.chord == chord).then_some(*action))
    }

    pub fn shortcut(&self, action: Action) -> Option<&str> {
        self.keys.get(&action).map(|binding| binding.label.as_str())
    }
}

pub fn config_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    let platform_config = env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    #[cfg(not(windows))]
    let platform_config = None;
    let root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or(platform_config)
        .or_else(|| env::home_dir().map(|home| home.join(".config")))
        .context("Could not find a configuration directory")?;
    Ok(root.join("rex-tui"))
}

pub fn default_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("settings.toml"))
}

pub fn editor(path: &Path) -> Result<Command> {
    let value = env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .context("Set VISUAL or EDITOR to edit settings")?;
    let words = shlex::split(&value).context("Invalid VISUAL or EDITOR command")?;
    let mut command = Command::new(words.first().context("Editor command is empty")?);
    command.args(&words[1..]).arg(std::path::absolute(path)?);
    create_default_file(path)?;
    Ok(command)
}

fn create_default_file(path: &Path) -> Result<()> {
    if let Some(directory) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(directory)?;
    }
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(DEFAULT_FILE.as_bytes())?,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error).with_context(|| format!("Create {}", path.display())),
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub struct Fixture(pub PathBuf);

    impl Fixture {
        pub fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};

            static NEXT: AtomicUsize = AtomicUsize::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "rex-tui-settings-test-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed),
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("Could not create {}: {error}", path.display()),
                }
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tab_move_shortcuts_are_remappable_and_do_not_replace_existing_bindings() {
        let defaults = Settings::default();
        assert_eq!(
            defaults.shortcut(Action::MoveTabLeft),
            Some("Alt+Shift+Left")
        );
        let file =
            toml::from_str("[keys]\nnext_tab = \"Alt+Shift+Left\"\nmove_tab_right = \"Alt+r\"")
                .unwrap();
        let configured = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(configured.shortcut(Action::NextTab), Some("Alt+Shift+Left"));
        assert_eq!(configured.shortcut(Action::MoveTabLeft), None);
        assert_eq!(configured.shortcut(Action::MoveTabRight), Some("Alt+r"));
    }

    #[test]
    fn pane_close_default_respects_custom_shortcuts_and_can_be_disabled() {
        assert_eq!(
            Settings::default().shortcut(Action::ClosePane),
            Some("Shift+F9")
        );
        let file = toml::from_str("[keys]\nnext_tab = \"Shift+F9\"").unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.shortcut(Action::NextTab), Some("Shift+F9"));
        assert_eq!(settings.shortcut(Action::ClosePane), None);
        for (value, expected) in [("Alt+x", Some("Alt+x")), ("none", None)] {
            let file = toml::from_str(&format!("[keys]\nclose_pane = \"{value}\"")).unwrap();
            let settings = Settings::from_file(file, Path::new("")).unwrap();
            assert_eq!(settings.shortcut(Action::ClosePane), expected);
        }
    }

    #[test]
    fn rename_defaults_leave_existing_custom_shortcuts_in_place() {
        let file =
            toml::from_str("[keys]\ntoggle_focus = \"Shift+F6\"\nrename_tab = \"F14\"").unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.shortcut(Action::ToggleFocus), Some("Shift+F6"));
        assert_eq!(settings.shortcut(Action::RenameSession), None);
        assert_eq!(settings.shortcut(Action::RenameTab), Some("F14"));
        assert_eq!(settings.shortcut(Action::RenamePane), Some("Shift+F8"));
    }

    #[test]
    fn directional_focus_defaults_allow_existing_overrides_and_custom_chords() {
        let file = toml::from_str("[keys]\nprevious_tab = \"Ctrl+Alt+Left\"\nfocus_up = \"Alt+u\"")
            .unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.shortcut(Action::FocusLeft), None);
        assert_eq!(
            settings.shortcut(Action::PreviousTab),
            Some("Ctrl+Alt+Left")
        );
        assert_eq!(settings.shortcut(Action::FocusUp), Some("Alt+u"));
        assert_eq!(
            settings.shortcut(Action::FocusRight),
            Some("Ctrl+Alt+Right")
        );
        assert_eq!(settings.shortcut(Action::NextPane), Some("F7"));
    }

    #[test]
    fn a_configured_focus_binding_takes_priority_over_a_rename_default() {
        let file = toml::from_str("[keys]\nfocus_left = \"Shift+F6\"").unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.shortcut(Action::FocusLeft), Some("Shift+F6"));
        assert_eq!(settings.shortcut(Action::RenameSession), None);
    }

    #[test]
    fn a_new_scroll_default_does_not_steal_an_existing_custom_binding() {
        let file = toml::from_str("[keys]\nprevious_tab = \"Alt+PageUp\"").unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        let key = KeyEvent::new(KeyCode::PageUp, KeyModifiers::ALT);
        assert_eq!(settings.action(key), Some(Action::PreviousTab));
        assert_eq!(settings.shortcut(Action::ScrollUp), None);
    }

    #[test]
    fn live_output_shortcut_respects_existing_bindings_and_leaves_plain_end_available() {
        let key = |mods| KeyEvent::new(KeyCode::End, mods);
        let default = Settings::default();
        assert_eq!(
            default.action(key(KeyModifiers::ALT)),
            Some(Action::JumpToLiveOutput)
        );
        assert_eq!(default.action(key(KeyModifiers::NONE)), None);
        let file = toml::from_str("[keys]\nprevious_tab = \"Alt+End\"\n").unwrap();
        let config = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(
            config.action(key(KeyModifiers::ALT)),
            Some(Action::PreviousTab)
        );
        assert_eq!(config.shortcut(Action::JumpToLiveOutput), None);
        let file = toml::from_str("[keys]\njump_to_live_output = \"Ctrl+End\"\n").unwrap();
        let config = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(
            config.action(key(KeyModifiers::CONTROL)),
            Some(Action::JumpToLiveOutput)
        );
    }

    #[test]
    fn default_pane_scrollback_shortcuts_leave_plain_page_keys_available() {
        let settings = Settings::default();
        for (key, expected) in [
            (KeyCode::PageUp, Action::ScrollUp),
            (KeyCode::PageDown, Action::ScrollDown),
        ] {
            assert_eq!(
                settings.action(KeyEvent::new(key, KeyModifiers::NONE)),
                None
            );
            assert_eq!(
                settings.action(KeyEvent::new(key, KeyModifiers::ALT)),
                Some(expected)
            );
        }
    }

    #[test]
    fn copying_is_enabled_in_either_mouse_mode() {
        let configs = [
            ("", false, false),
            ("[selection]\nmouse = true", true, false),
            (
                "[selection]\nmouse = false\n[experimental]\nmouse_actions = true",
                true,
                true,
            ),
        ];
        for (source, selection, actions) in configs {
            let settings =
                Settings::from_file(toml::from_str(source).unwrap(), Path::new("")).unwrap();
            assert_eq!(settings.mouse_selection, selection);
            assert_eq!(settings.mouse_actions, actions);
            assert_eq!(
                settings.shortcut(Action::CopySelection),
                selection.then_some("F12")
            );
        }
    }

    #[test]
    fn composer_shortcut_is_reserved_only_when_enabled() {
        let disabled = Settings::default();
        let f11 = KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE);
        assert_eq!(disabled.action(f11), None);
        let source = "[experimental]\ninput_composer = true\n[keys]\ncompose_input = \"F12\"";
        let enabled = Settings::from_file(toml::from_str(source).unwrap(), Path::new("")).unwrap();
        assert_eq!(enabled.action(f11), None);
        assert_eq!(
            enabled.action(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE)),
            Some(Action::ComposeInput)
        );
        let duplicate = source.replace("F12", "F10");
        assert!(Settings::from_file(toml::from_str(&duplicate).unwrap(), Path::new("")).is_err());
        let disabled_duplicate = duplicate.replace("true", "false");
        assert!(
            Settings::from_file(toml::from_str(&disabled_duplicate).unwrap(), Path::new(""))
                .is_ok()
        );
    }

    #[test]
    fn editor_seeds_missing_settings_and_preserves_an_existing_file() {
        let fixture = Fixture::new();
        let path = fixture.0.join("nested/settings.toml");
        assert_eq!(Settings::load(&path).unwrap().theme, "terminal");
        create_default_file(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_FILE);
        fs::write(&path, "[sidebar]\nwidth = 29").unwrap();
        create_default_file(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().sidebar.width, Some(29));
    }

    #[test]
    fn named_theme_resolves_next_to_alternate_settings() {
        let fixture = Fixture::new();
        let settings = fixture.0.join("alternate.toml");
        let themes = fixture.0.join("themes");
        fs::create_dir(&themes).unwrap();
        fs::write(&settings, "theme = \"custom\"").unwrap();
        let error = Settings::load(&settings).unwrap_err();
        assert!(format!("{error:#}").contains("custom.toml"));
        fs::write(themes.join("custom.toml"), "[chrome]\naccent = \"#df812f\"").unwrap();
        assert_eq!(
            Settings::load(&settings).unwrap().chrome.accent,
            Color::Rgb(0xdf, 0x81, 0x2f)
        );
        fs::write(themes.join("custom.toml"), "[chrome]\nunknown = \"blue\"").unwrap();
        assert!(format!("{:#}", Settings::load(&settings).unwrap_err()).contains("unknown"));
    }

    #[test]
    fn sidebar_and_find_defaults_preserve_existing_bindings() {
        let file = toml::from_str(
            r#"[keys]
            new_tab = "/"
            settings = "Alt+/"
            split_down = "Alt+a"
        "#,
        )
        .unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.shortcut(Action::NewTab), Some("/"));
        assert_eq!(settings.shortcut(Action::Settings), Some("Alt+/"));
        assert_eq!(settings.shortcut(Action::SplitDown), Some("Alt+a"));
        assert_eq!(settings.shortcut(Action::FilterSidebar), None);
        assert_eq!(settings.shortcut(Action::Find), None);
        assert_eq!(settings.shortcut(Action::NextAttention), None);
    }

    #[test]
    fn generated_settings_match_the_unconfigured_shortcuts() {
        let file = toml::from_str(DEFAULT_FILE).unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.theme, "terminal");
        for (action, key) in DEFAULT_KEYS {
            let expected =
                (!matches!(action, Action::ComposeInput | Action::CopySelection)).then_some(key);
            assert_eq!(settings.shortcut(action), expected);
        }
    }

    #[test]
    fn remaps_and_disables_shortcuts_and_rejects_duplicates() {
        let file = toml::from_str(
            r#"[keys]
        next_tab = "Ctrl+n"
        split_down = "none"
        "#,
        )
        .unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.shortcut(Action::SplitDown), None);
        assert_eq!(
            settings.action(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
            Some(Action::NextTab)
        );
        assert_eq!(
            settings.action(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)),
            None
        );

        let file = toml::from_str(
            r#"[keys]
        next_tab = "f1"
        "#,
        )
        .unwrap();
        let error = Settings::from_file(file, Path::new("")).unwrap_err();
        assert!(error.to_string().contains("Duplicate shortcut"));
    }

    #[test]
    fn partial_theme_inherits_terminal_colors() {
        let file: ThemeFile = toml::from_str(
            r##"[chrome]
        accent = "#ab12cd"
        selection_text = "default"
        "##,
        )
        .unwrap();
        assert_eq!(file.chrome.accent, Color::Rgb(0xab, 0x12, 0xcd));
        assert_eq!(file.chrome.selection_text, Some(Color::Reset));
        assert_eq!(file.chrome.background, Color::Reset);
        assert_eq!(file.chrome.warning, Color::Yellow);
        assert!(toml::from_str::<ThemeFile>("[chrome]\nwarning = \"orange-ish\"").is_err());
    }

    #[test]
    fn pane_defaults_use_terminal_colors_unless_server_colors_are_requested() {
        let file = toml::from_str(DEFAULT_FILE).unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.panes.default_colors, PaneDefaultColors::Terminal);
        let file = toml::from_str("[panes]\ndefault_colors = \"server\"").unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.panes.default_colors, PaneDefaultColors::Server);
    }

    #[test]
    fn invalid_width_and_escaping_theme_names_are_rejected() {
        for source in ["[sidebar]\nwidth = 2", "theme = \"../other\""] {
            let file = toml::from_str(source).unwrap();
            assert!(Settings::from_file(file, Path::new("")).is_err());
        }
    }
}
