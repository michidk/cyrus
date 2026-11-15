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

    /// The game's command-line arguments for these options.
    pub fn arguments(&self) -> Vec<OsString> {
        let mut args = Vec::new();
        if let Some(install) = &self.install {
            args.extend([OsString::from("--install"), install.into()]);
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
            skip_movies: true,
        };
        let args: Vec<_> = options
            .arguments()
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["--install", "D:/Games/Redguard", "--skip-movies"]);
    }

    #[test]
    fn options_round_trip_and_a_missing_file_is_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config/launcher.json");
        assert_eq!(Options::load(&file).unwrap(), Options::default());
        let options = Options {
            install: None,
            skip_movies: true,
        };
        options.save(&file).unwrap();
        assert_eq!(Options::load(&file).unwrap(), options);
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
