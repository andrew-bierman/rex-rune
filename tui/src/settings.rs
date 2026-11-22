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
    NextPane,
    SplitRight,
    SplitDown,
    Quit,
}

const DEFAULT_KEYS: [(Action, &str); 10] = [
    (Action::Settings, "F1"),
    (Action::NewSession, "F2"),
    (Action::PreviousTab, "F3"),
    (Action::NextTab, "F4"),
    (Action::NewTab, "F5"),
    (Action::ToggleFocus, "F6"),
    (Action::NextPane, "F7"),
    (Action::SplitRight, "F8"),
    (Action::SplitDown, "F9"),
    (Action::Quit, "F10"),
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
}

impl Default for Panes {
    fn default() -> Self {
        Self {
            borders: true,
            headers: true,
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

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct File {
    theme: String,
    startup_focus: StartupFocus,
    sidebar: Sidebar,
    panes: Panes,
    keys: BTreeMap<Action, String>,
}

impl Default for File {
    fn default() -> Self {
        Self {
            theme: "terminal".into(),
            startup_focus: StartupFocus::Sidebar,
            sidebar: Sidebar::default(),
            panes: Panes::default(),
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
        let mut keys = BTreeMap::<Action, Binding>::new();
        for (action, fallback) in DEFAULT_KEYS {
            let label = file.keys.remove(&action).unwrap_or_else(|| fallback.into());
            if label.eq_ignore_ascii_case("none") {
                continue;
            }
            let chord =
                Chord::parse(&label).with_context(|| format!("Invalid {action:?} binding"))?;
            ensure!(
                !keys.values().any(|binding| binding.chord == chord),
                "Duplicate shortcut {label}"
            );
            keys.insert(action, Binding { chord, label });
        }
        Ok(Self {
            theme: file.theme,
            chrome,
            startup_focus: file.startup_focus,
            sidebar: file.sidebar,
            panes: file.panes,
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

pub fn default_path() -> Result<PathBuf> {
    let root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::home_dir().map(|home| home.join(".config")))
        .context("Could not find a configuration directory")?;
    Ok(root.join("rex-tui/settings.toml"))
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
    fn generated_settings_match_the_unconfigured_shortcuts() {
        let file = toml::from_str(DEFAULT_FILE).unwrap();
        let settings = Settings::from_file(file, Path::new("")).unwrap();
        assert_eq!(settings.theme, "terminal");
        for (action, key) in DEFAULT_KEYS {
            assert_eq!(settings.shortcut(action), Some(key));
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
    fn invalid_width_and_escaping_theme_names_are_rejected() {
        for source in ["[sidebar]\nwidth = 2", "theme = \"../other\""] {
            let file = toml::from_str(source).unwrap();
            assert!(Settings::from_file(file, Path::new("")).is_err());
        }
    }
}
