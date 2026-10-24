//! Where cyrus keeps its own state (settings, the remembered install, exported caches and logs)
//! and the one way it reads and writes it. Nothing here is ever inside the read-only game install.
//!
//! [`read`] treats a missing file as no state; [`write`] replaces a document atomically, so a
//! failed write never damages the accepted one. Native stores use them instead of `std::fs`.
//!
//! The root is chosen once per process, in this order:
//!
//! 1. `CYRUS_CONFIG_DIR`, used as given. Tests, containers and scripts pin the location with it.
//! 2. A `game-config/` directory that already exists in the working directory. This is the
//!    checkout-local layout the development workflows, the web server and earlier releases use.
//! 3. The platform directories for an application named `cyrus` (XDG on Linux, Application
//!    Support on macOS, AppData on Windows), so a player's state does not depend on where the
//!    game was launched from.
//!
//! Cases 1 and 2 keep everything under one root (`cache/`, `logs/`); case 3 puts the cache and
//! the logs in the platform's own cache and state locations. The browser build has no
//! filesystem; it gets the checkout-local layout so shared code still compiles, and persists
//! through `localStorage` instead.
#[cfg(not(target_arch = "wasm32"))]
mod file;
#[cfg(not(target_arch = "wasm32"))]
pub use file::{read, write};

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Environment variable that pins the root, for tests, containers and scripts.
pub const ENV_VAR: &str = "CYRUS_CONFIG_DIR";
/// Checkout-local state directory, relative to the working directory.
pub const LOCAL_DIR: &str = "game-config";

/// The directories cyrus writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// Settings, the remembered install, preferences, saves, cheat and console files.
    pub config: PathBuf,
    /// Regenerable files exported from the install. Safe to delete.
    pub cache: PathBuf,
    /// Daily log files.
    pub logs: PathBuf,
}

impl Dirs {
    /// Everything under one root: `<root>/cache` and `<root>/logs`.
    fn rooted(root: PathBuf) -> Self {
        Self {
            cache: root.join("cache"),
            logs: root.join("logs"),
            config: root,
        }
    }

    /// Pure resolution of the order in the module documentation; the inputs are what the
    /// environment, the working directory and the platform report.
    fn choose(pinned: Option<OsString>, local_exists: bool, platform: Option<Self>) -> Self {
        match pinned.filter(|value| !value.is_empty()) {
            Some(root) => Self::rooted(root.into()),
            None if local_exists => Self::rooted(LOCAL_DIR.into()),
            None => platform.unwrap_or_else(|| Self::rooted(LOCAL_DIR.into())),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn platform() -> Option<Dirs> {
    let dirs = directories::ProjectDirs::from("", "", "cyrus")?;
    Some(Dirs {
        config: dirs.config_dir().to_path_buf(),
        cache: dirs.cache_dir().to_path_buf(),
        // Only Linux has a state directory; elsewhere logs live with the local data.
        logs: dirs
            .state_dir()
            .unwrap_or_else(|| dirs.data_local_dir())
            .join("logs"),
    })
}

#[cfg(target_arch = "wasm32")]
fn platform() -> Option<Dirs> {
    None
}

/// The directories for this process, fixed on first use so a directory created later cannot move
/// them mid-run.
pub fn dirs() -> &'static Dirs {
    static DIRS: OnceLock<Dirs> = OnceLock::new();
    DIRS.get_or_init(|| {
        Dirs::choose(
            std::env::var_os(ENV_VAR),
            Path::new(LOCAL_DIR).is_dir(),
            platform(),
        )
    })
}

/// Settings, the remembered install, preferences, saves, cheat and console files.
pub fn config_dir() -> &'static Path {
    &dirs().config
}

/// Regenerable files exported from the install.
pub fn cache_dir() -> &'static Path {
    &dirs().cache
}

/// Daily log files.
pub fn log_dir() -> &'static Path {
    &dirs().logs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform_dirs() -> Dirs {
        Dirs {
            config: "/home/p/.config/cyrus".into(),
            cache: "/home/p/.cache/cyrus".into(),
            logs: "/home/p/.local/state/cyrus/logs".into(),
        }
    }

    #[test]
    fn the_environment_pins_one_root() {
        let dirs = Dirs::choose(Some("/pinned".into()), true, Some(platform_dirs()));
        assert_eq!(dirs.config, Path::new("/pinned"));
        assert_eq!(dirs.cache, Path::new("/pinned/cache"));
        assert_eq!(dirs.logs, Path::new("/pinned/logs"));
    }

    #[test]
    fn an_empty_environment_value_is_unset() {
        let dirs = Dirs::choose(Some("".into()), false, Some(platform_dirs()));
        assert_eq!(dirs, platform_dirs());
    }

    #[test]
    fn an_existing_local_directory_beats_the_platform() {
        let dirs = Dirs::choose(None, true, Some(platform_dirs()));
        assert_eq!(dirs.config, Path::new("game-config"));
        assert_eq!(dirs.cache, Path::new("game-config/cache"));
        assert_eq!(dirs.logs, Path::new("game-config/logs"));
    }

    #[test]
    fn the_platform_is_the_default_for_a_fresh_launch() {
        assert_eq!(
            Dirs::choose(None, false, Some(platform_dirs())),
            platform_dirs()
        );
    }

    #[test]
    fn without_a_home_directory_the_local_layout_is_used() {
        let dirs = Dirs::choose(None, false, None);
        assert_eq!(dirs.config, Path::new("game-config"));
    }
}
