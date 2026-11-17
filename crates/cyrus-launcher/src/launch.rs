//! What the launcher itself remembers, and how it starts the game.
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The game executable's file name, beside the launcher.
pub const GAME: &str = if cfg!(windows) { "cyrus.exe" } else { "cyrus" };

/// The launcher's own choices; everything the game keeps (fidelity, mods) is in the game's files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// A game-files folder the player picked here. Empty leaves the search to the game.
    pub install: Option<PathBuf>,
    /// Go straight to the book menu, without the splash screen.
    pub skip_splash: bool,
    /// Enter a new game without its introduction movie.
    pub skip_movies: bool,
}

pub fn options_file() -> PathBuf {
    cyrus_config::config_dir().join("launcher.json")
}

impl Options {
    /// A missing file is the defaults; a file that does not parse is an error naming it.
    pub fn load(file: &Path) -> anyhow::Result<Self> {
        match std::fs::read(file) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("{}: invalid launcher options", file.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| file.display().to_string()),
        }
    }

    pub fn save(&self, file: &Path) -> anyhow::Result<()> {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).with_context(|| parent.display().to_string())?;
        }
        std::fs::write(file, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("{}: saving launcher options", file.display()))
    }

    /// Selects a folder and resolves the cheat state the game will read from it.
    ///
    /// The config-directory cheat file takes precedence over the selected install, then the
    /// remembered install when automatic search is selected. A read or parse error leaves these
    /// options unchanged. The caller replaces its displayed cheat state with the returned value.
    /// This only reads cheat files; choosing an install never writes into either install.
    pub fn select_install(
        &mut self,
        install: Option<PathBuf>,
        remembered: Option<&Path>,
        local_cheats: &Path,
    ) -> anyhow::Result<crate::cheats::State> {
        let state = crate::cheats::load(local_cheats, install.as_deref().or(remembered))?;
        self.install = install;
        Ok(state)
    }

    /// The game's command-line arguments for these options.
    pub fn arguments(&self) -> Vec<OsString> {
        let mut args = Vec::new();
        if let Some(install) = &self.install {
            args.extend([OsString::from("--install"), install.into()]);
        }
        if self.skip_splash {
            args.push("--skip-startup".into());
        }
        if self.skip_movies {
            args.push("--skip-movies".into());
        }
        args
    }
}

/// The game-files folder the game itself remembered (`install.json`), if it has one.
pub fn remembered_install() -> Option<PathBuf> {
    let bytes = std::fs::read(cyrus_config::config_dir().join("install.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The game executable beside `launcher`, which must exist.
pub fn find_game(launcher: &Path) -> anyhow::Result<PathBuf> {
    let beside = launcher
        .parent()
        .map(|folder| folder.join(GAME))
        .context("the launcher's own folder is unknown")?;
    anyhow::ensure!(
        beside.is_file(),
        "{} is missing: the launcher starts the game from the folder it is in",
        beside.display()
    );
    Ok(beside)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_pass_no_arguments() {
        assert!(Options::default().arguments().is_empty());
    }

    #[test]
    fn options_become_the_games_arguments() {
        let options = Options {
            install: Some("D:/Games/Redguard".into()),
            skip_splash: true,
            skip_movies: true,
        };
        let args: Vec<_> = options
            .arguments()
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--install",
                "D:/Games/Redguard",
                "--skip-startup",
                "--skip-movies"
            ]
        );
    }

    #[test]
    fn options_round_trip_and_a_missing_file_is_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config/launcher.json");
        assert_eq!(Options::load(&file).unwrap(), Options::default());
        let options = Options {
            install: None,
            skip_splash: true,
            skip_movies: true,
        };
        options.save(&file).unwrap();
        assert_eq!(Options::load(&file).unwrap(), options);
    }

    #[test]
    fn selecting_an_install_refreshes_cheats_and_preserves_local_precedence() {
        use crate::cheats::{self, Cheat, State};
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first install");
        let second = dir.path().join("second install");
        let local = dir.path().join("config/REDGUARD.CHT");
        let mut first_state = State::default();
        first_state.set(Cheat::MagicCarpet, true);
        cheats::save(&first.join("REDGUARD.CHT"), &first_state).unwrap();
        let mut second_state = State::default();
        second_state.set(Cheat::DrEvil, true);
        cheats::save(&second.join("REDGUARD.CHT"), &second_state).unwrap();
        let mut options = Options::default();
        assert_eq!(
            options
                .select_install(Some(first.clone()), None, &local)
                .unwrap(),
            first_state
        );
        assert_eq!(
            options
                .select_install(Some(second.clone()), None, &local)
                .unwrap(),
            second_state
        );
        assert_eq!(options.install, Some(second.clone()));
        assert_eq!(
            options.select_install(None, Some(&first), &local).unwrap(),
            first_state
        );
        assert!(options.install.is_none());
        cheats::save(&local, &second_state).unwrap();
        assert_eq!(
            options
                .select_install(Some(first.clone()), None, &local)
                .unwrap(),
            second_state
        );
        assert_eq!(options.install, Some(first));
        assert_eq!(
            cheats::load(&second.join("REDGUARD.CHT"), None).unwrap(),
            second_state
        );
    }

    #[test]
    fn an_invalid_candidate_cheat_file_does_not_change_the_selection() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("candidate");
        std::fs::create_dir(&candidate).unwrap();
        std::fs::write(candidate.join("REDGUARD.CHT"), b"invalid").unwrap();
        let mut options = Options {
            install: Some("previous".into()),
            skip_splash: false,
            skip_movies: true,
        };
        let before = options.clone();
        let error = options
            .select_install(Some(candidate.clone()), None, &dir.path().join("local.CHT"))
            .unwrap_err();
        assert!(format!("{error:#}").contains("REDGUARD.CHT"));
        assert_eq!(options, before);
        assert_eq!(
            std::fs::read(candidate.join("REDGUARD.CHT")).unwrap(),
            b"invalid"
        );
    }

    #[test]
    fn the_game_must_sit_beside_the_launcher() {
        let dir = tempfile::tempdir().unwrap();
        let launcher = dir.path().join("cyrus-launcher");
        let error = format!("{:#}", find_game(&launcher).unwrap_err());
        assert!(error.contains(GAME), "{error}");
        std::fs::write(dir.path().join(GAME), b"").unwrap();
        assert_eq!(find_game(&launcher).unwrap(), dir.path().join(GAME));
    }
}
