//! Explicit, read-only installation validation shared by applications.
use std::path::{Path, PathBuf};

#[derive(bevy::prelude::Resource, Debug, Clone)]
pub struct GameInstall {
    pub root: PathBuf,
}
pub const ENV_VAR: &str = "REDGUARD_DIR";
pub const LINK_NAME: &str = "redguard";

impl GameInstall {
    /// Explicit path, then REDGUARD_DIR, then the folder the program runs from and the folders around
    /// it, then conventional local/GOG install folders.
    /// Invalid explicit or environment paths are errors rather than silent fallbacks.
    pub fn locate(explicit: Option<&Path>) -> anyhow::Result<Self> {
        if let Some(path) = explicit {
            return Self::resolve(path);
        }
        if let Some(path) = std::env::var_os(ENV_VAR) {
            return Self::resolve(Path::new(&path));
        }
        let program = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let candidates = search_folders(&Machine::current(), program.as_deref());
        for path in &candidates {
            if let Ok(install) = Self::resolve(path) {
                return Ok(install);
            }
        }
        anyhow::bail!(
            "game files not found; provide --install PATH or REDGUARD_DIR. Searched: {}",
            candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// Resolves a native selection to the read-only game-data directory.
    /// Accepts install descendants, GOG library folders and outer package folders.
    /// Searches ancestors and known Redguard layouts, without recursively scanning libraries.
    pub fn resolve(selected: &Path) -> anyhow::Result<Self> {
        let selected = selected.canonicalize().map_err(|error| {
            anyhow::anyhow!(
                "{}: {error}; provide --install PATH pointing to your Redguard installation",
                selected.display()
            )
        })?;
        // Prefer the enclosing install before looking for a neighboring package.
        for path in selected.ancestors() {
            if let Ok(install) = Self::open(path) {
                return Ok(install);
            }
        }
        let mut searched = Vec::new();
        for parent in selected.ancestors() {
            searched.push(parent.display().to_string());
            for layout in [
                "Redguard",
                "redguard",
                "app/Redguard",
                "Redguard/Redguard",
                "Redguard/app/Redguard",
            ] {
                let path = parent.join(layout);
                if let Ok(install) = Self::open(&path) {
                    return Ok(install);
                }
                searched.push(path.display().to_string());
            }
        }
        anyhow::bail!(
            "{}: game files not found; expected fxart, maps and RGFX.EXE; provide --install PATH or REDGUARD_DIR. Searched: {}",
            selected.display(),
            searched.join(", ")
        )
    }

    pub fn open(root: &Path) -> anyhow::Result<Self> {
        let root = root
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("{}: {e}", root.display()))?;
        for required in ["fxart", "maps", "RGFX.EXE"] {
            anyhow::ensure!(
                root.join(required).exists(),
                "{}: missing {required}; select your original Redguard installation",
                root.display()
            );
        }
        Ok(Self { root })
    }
    /// Resolves an output directory without creating anything in the read-only install.
    pub fn cache_path(&self, requested: &Path) -> anyhow::Result<PathBuf> {
        let path = std::path::absolute(requested)?;
        anyhow::ensure!(
            !path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
            "cache path must not contain parent traversal"
        );
        let mut existing = path.as_path();
        let mut suffix = Vec::new();
        while !existing.exists() {
            suffix.push(
                existing
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("invalid cache path"))?,
            );
            existing = existing
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid cache parent"))?;
        }
        let mut resolved = existing.canonicalize()?;
        for part in suffix.into_iter().rev() {
            resolved.push(part);
        }
        anyhow::ensure!(
            !resolved.starts_with(&self.root),
            "cache must be outside the original install: {}",
            resolved.display()
        );
        Ok(resolved)
    }
    pub fn maps(&self) -> PathBuf {
        self.root.join("maps")
    }
    pub fn fxart(&self) -> PathBuf {
        self.root.join("fxart")
    }
}
/// What the environment says about the machine, as a value so the search order can be tested.
#[derive(Default)]
pub struct Machine {
    pub home: Option<PathBuf>,
    pub system_drive: Option<String>,
    pub program_files: Vec<PathBuf>,
}

impl Machine {
    pub fn current() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Self {
            home: var("HOME").or_else(|| var("USERPROFILE")),
            system_drive: std::env::var("SystemDrive").ok(),
            program_files: ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"]
                .iter()
                .filter_map(|name| var(name))
                .collect(),
        }
    }
}

/// Where to look when nothing was chosen: the folder the program runs from (and, through
/// [`GameInstall::resolve`], the folders around it: a `Redguard` or GOG `app/Redguard` beside it or
/// above it), then the local and standard GOG install folders.
pub fn search_folders(machine: &Machine, program: Option<&Path>) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = program.into_iter().map(Path::to_path_buf).collect();
    folders.extend([PathBuf::from(LINK_NAME), PathBuf::from("Redguard")]);
    if let Some(home) = &machine.home {
        folders.push(home.join("GOG Games/Redguard"));
        folders.push(home.join("GOG Games/Redguard/app/Redguard"));
    }
    if let Some(drive) = &machine.system_drive {
        folders.push(PathBuf::from(format!("{drive}/GOG Games/Redguard")));
    }
    for files in &machine.program_files {
        folders.push(files.join("GOG Galaxy/Games/Redguard"));
        folders.push(files.join("GOG Galaxy/Games/Redguard/app/Redguard"));
    }
    folders
}

/// Installed `KEYS.INI` keyboard-slot codes of the fifteen actions, in `[input]` order.
pub fn control_codes(install: &GameInstall) -> Result<[u8; 15], String> {
    let path = install.root.join("KEYS.INI");
    std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|text| rgpre::import::keys_ini::KeysIni::parse(&text))
        .map(|ini| ini.control_codes())
}
/// Installed joystick bindings and dead zone; `None` when SYSTEM.INI has no `joy_tolerance`.
pub fn joystick(install: &GameInstall) -> Result<Option<crate::player::JoystickSettings>, String> {
    let keys = std::fs::read_to_string(install.root.join("KEYS.INI")).map_err(|e| e.to_string())?;
    let system = system_ini(install).map_err(|e| e.to_string())?;
    let tolerance = rgpre::import::system_ini::SystemIni::parse(&system)
        .map_err(|e| e.to_string())?
        .cyrus
        .joy_tolerance;
    tolerance
        .map(|tolerance| crate::player::JoystickSettings::parse(&keys, tolerance))
        .transpose()
}
/// Decodes the selected installed bitmap font without retaining parser types.
pub fn font(path: &std::path::Path) -> Result<cyrus_text::Font, String> {
    std::fs::read(path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| rgpre::import::fnt::parse_fnt(&bytes).map_err(|e| e.to_string()))
        .map(|font| cyrus_text::Font::from(&font))
}
/// Installed SYSTEM.INI text, read-only; presentation chooses its existing fallback policy.
pub fn system_ini(install: &GameInstall) -> std::io::Result<String> {
    std::fs::read_to_string(install.root.join("SYSTEM.INI"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_folder_is_searched_before_the_standard_folders() {
        let machine = Machine {
            home: Some("/home/p".into()),
            system_drive: Some("C:".into()),
            program_files: vec!["C:/Program Files (x86)".into()],
        };
        let folders = search_folders(&machine, Some(Path::new("/opt/cyrus")));
        assert_eq!(folders[0], Path::new("/opt/cyrus"));
        let position = |expected: &str| {
            folders
                .iter()
                .position(|f| f == Path::new(expected))
                .unwrap()
        };
        assert!(position("/opt/cyrus") < position("Redguard"));
        assert!(position("Redguard") < position("/home/p/GOG Games/Redguard"));
        assert!(
            position("C:/GOG Games/Redguard")
                < position("C:/Program Files (x86)/GOG Galaxy/Games/Redguard")
        );
    }

    #[test]
    fn a_game_beside_the_program_is_found_through_its_parent_folder() {
        let root = std::env::temp_dir().join(format!("cyrus-locate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("GOG Games/Redguard/app/Redguard");
        for dir in ["fxart", "maps"] {
            std::fs::create_dir_all(game.join(dir)).unwrap();
        }
        std::fs::write(game.join("RGFX.EXE"), b"x").unwrap();
        let program = root.join("GOG Games/cyrus");
        std::fs::create_dir_all(&program).unwrap();
        let found = GameInstall::resolve(&program).unwrap();
        assert_eq!(found.root, game.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }
}
