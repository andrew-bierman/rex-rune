use anyhow::{Context, Result, bail, ensure};
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

pub const USAGE: &str = "Usage: rex-tui [--config PATH] [--settings PATH]
       rex-tui --check-settings [--settings PATH]
       rex-tui --print-default-settings
       rex-tui --version
";

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Launch,
    CheckSettings,
    PrintSettings,
    Help,
    Version,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Cli {
    pub mode: Mode,
    pub backend: Option<PathBuf>,
    pub profile: Option<PathBuf>,
    pub settings: Option<PathBuf>,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut args = args.into_iter().peekable();
        let mut cli = Self {
            mode: Mode::Launch,
            backend: None,
            profile: None,
            settings: None,
        };
        // Development tools pass an explicit backend and profile.
        if cli.mode == Mode::Launch
            && args
                .peek()
                .is_some_and(|arg| !arg.as_encoded_bytes().starts_with(b"-"))
        {
            cli.backend = args.next().map(PathBuf::from);
            cli.profile = Some(PathBuf::from(
                args.next().context("Expected a workspace profile")?,
            ));
        }
        while let Some(flag) = args.next() {
            let mode = match flag.to_str() {
                Some("--check-settings") => Some(Mode::CheckSettings),
                Some("--print-default-settings") => Some(Mode::PrintSettings),
                Some("--help" | "-h") => Some(Mode::Help),
                Some("--version" | "-V") => Some(Mode::Version),
                _ => None,
            };
            if let Some(mode) = mode {
                ensure!(cli.mode == Mode::Launch, "Choose one mode\n{USAGE}");
                cli.mode = mode;
                continue;
            }
            let path = match flag.to_str() {
                Some("--config") => &mut cli.profile,
                Some("--settings") => &mut cli.settings,
                _ => bail!("Unexpected argument {:?}\n{USAGE}", flag),
            };
            ensure!(path.is_none(), "Duplicate {}", flag.to_string_lossy());
            let value = args
                .next()
                .filter(|arg| !arg.is_empty() && !arg.as_encoded_bytes().starts_with(b"--"))
                .with_context(|| format!("Expected a path after {}", flag.to_string_lossy()))?;
            *path = Some(PathBuf::from(value));
        }
        ensure!(
            cli.mode == Mode::Launch || cli.profile.is_none(),
            "Workspace profiles are only used when launching rex-tui\n{USAGE}"
        );
        ensure!(
            matches!(cli.mode, Mode::Launch | Mode::CheckSettings) || cli.settings.is_none(),
            "This mode does not accept a settings path\n{USAGE}"
        );
        Ok(cli)
    }
}

fn adjacent_backend(frontend: &Path) -> PathBuf {
    frontend.with_file_name(format!("rex-qt-backend{}", env::consts::EXE_SUFFIX))
}

pub fn bundled_backend() -> Result<PathBuf> {
    let frontend = env::current_exe().context("Locate the rex-tui executable")?;
    let frontend = frontend
        .canonicalize()
        .with_context(|| format!("Resolve {}", frontend.display()))?;
    let backend = adjacent_backend(&frontend);
    ensure!(
        backend.is_file(),
        "Missing matching Rex backend at {}. Install both executables together.",
        backend.display()
    );
    Ok(backend)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli> {
        Cli::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn portable_launch_uses_default_or_explicit_paths() {
        let default = parse(&[]).unwrap();
        assert_eq!(default.mode, Mode::Launch);
        assert!(default.backend.is_none());
        assert!(default.profile.is_none());
        assert!(default.settings.is_none());
        let explicit = parse(&[
            "--settings",
            "my settings.toml",
            "--config",
            "my profile.json",
        ])
        .unwrap();
        assert_eq!(explicit.profile, Some("my profile.json".into()));
        assert_eq!(explicit.settings, Some("my settings.toml".into()));
        assert!(explicit.backend.is_none());
    }

    #[test]
    fn legacy_launcher_keeps_explicit_backend_and_profile() {
        let cli = parse(&["private backend", "a profile", "--settings", "path"]).unwrap();
        assert_eq!(cli.backend, Some("private backend".into()));
        assert_eq!(cli.profile, Some("a profile".into()));
        assert_eq!(cli.settings, Some("path".into()));
    }

    #[test]
    fn standalone_settings_and_help_modes_need_no_backend() {
        let check = parse(&["--check-settings", "--settings", "custom"]).unwrap();
        assert_eq!(
            check,
            parse(&["--settings", "custom", "--check-settings"]).unwrap()
        );
        assert_eq!(check.mode, Mode::CheckSettings);
        assert_eq!(check.settings, Some("custom".into()));
        assert_eq!(
            parse(&["--print-default-settings"]).unwrap().mode,
            Mode::PrintSettings
        );
        assert_eq!(parse(&["--help"]).unwrap().mode, Mode::Help);
        assert_eq!(parse(&["--version"]).unwrap().mode, Mode::Version);
        for args in [
            &["--config", "--settings"][..],
            &["--settings", "a", "--settings", "b"],
            &["--check-settings", "--config", "a"],
            &["--print-default-settings", "--settings", "a"],
            &["--bad"],
            &["backend"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn companion_uses_the_executable_directory_and_native_suffix() {
        let frontend =
            Path::new("release folder").join(format!("rex-tui{}", env::consts::EXE_SUFFIX));
        assert_eq!(
            adjacent_backend(&frontend),
            Path::new("release folder").join(format!("rex-qt-backend{}", env::consts::EXE_SUFFIX))
        );
    }
}
