//! Finding the player's own Redguard folder, so the launcher can ask for it before anything else.
//!
//! The launcher does not link the game (and so not `cyrus-assets`, which carries Bevy), so this
//! mirrors the game's rules for what counts as an install and where to look; the tests hold the
//! two to the same answers.
use anyhow::Context;
use std::path::{Path, PathBuf};

/// Where the folder is looked for when nothing was chosen: `REDGUARD_DIR`, then these.
#[derive(Default)]
pub struct Machine {
    pub home: Option<PathBuf>,
    pub system_drive: Option<String>,
    pub program_files: Vec<PathBuf>,
    pub wine_prefix: Option<PathBuf>,
}

impl Machine {
    pub fn current() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let home = var("HOME").or_else(|| var("USERPROFILE"));
        Self {
            wine_prefix: var("WINEPREFIX").or_else(|| home.as_ref().map(|h| h.join(".wine"))),
            home,
            system_drive: std::env::var("SystemDrive").ok(),
            program_files: ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"]
                .iter()
                .filter_map(|name| var(name))
                .collect(),
        }
    }
}

/// The folders to try, in order: the program's own folder, then the usual GOG, Wine, Heroic and
/// Lutris locations.
pub fn search_folders(machine: &Machine, program: Option<&Path>) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = program.into_iter().map(Path::to_path_buf).collect();
    folders.extend([PathBuf::from("redguard"), PathBuf::from("Redguard")]);
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
    if let Some(prefix) = &machine.wine_prefix {
        let drive = prefix.join("drive_c");
        for path in [
            "GOG Games/Redguard",
            "GOG Games/Redguard/app/Redguard",
            "Program Files (x86)/GOG Galaxy/Games/Redguard",
            "Program Files (x86)/GOG Galaxy/Games/Redguard/app/Redguard",
            "Program Files/GOG Galaxy/Games/Redguard",
        ] {
            folders.push(drive.join(path));
        }
    }
    if let Some(home) = &machine.home {
        for path in [
            "Games/Heroic/Redguard",
            "Games/gog/redguard",
            "Games/redguard",
        ] {
            folders.push(home.join(path));
        }
    }
    folders
}

fn is_install(root: &Path) -> bool {
    root.join("maps").exists()
        && ((root.join("fxart").exists() && root.join("RGFX.EXE").exists())
            || (root.join("3dart").exists() && root.join("RG.EXE").exists()))
}

/// The install `selected` names: the folder itself, a folder inside it, or a GOG or package folder
/// around it. Read-only.
pub fn resolve(selected: &Path) -> anyhow::Result<PathBuf> {
    let selected = selected
        .canonicalize()
        .with_context(|| format!("{}: this folder cannot be opened", selected.display()))?;
    let open = |path: &Path| path.canonicalize().ok().filter(|root| is_install(root));
    if let Some(root) = selected.ancestors().find_map(open) {
        return Ok(root);
    }
    for parent in selected.ancestors() {
        for layout in [
            "Redguard",
            "redguard",
            "app/Redguard",
            "Redguard/Redguard",
            "Redguard/app/Redguard",
        ] {
            if let Some(root) = open(&parent.join(layout)) {
                return Ok(root);
            }
        }
    }
    anyhow::bail!(
        "{}: no Redguard game files here (expected maps, fxart and RGFX.EXE)",
        selected.display()
    )
}

/// The install found without asking: `REDGUARD_DIR`, then the search folders. `None` when the
/// player has to choose.
pub fn locate(machine: &Machine, program: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("REDGUARD_DIR") {
        return resolve(Path::new(&path)).ok();
    }
    search_folders(machine, program)
        .iter()
        .find_map(|path| resolve(path).ok())
}

/// Remembers `root` where the game and the asset viewer read it (`install.json`).
pub fn remember(root: &Path) -> anyhow::Result<()> {
    let file = cyrus_config::config_dir().join("install.json");
    cyrus_config::write(&file, &serde_json::to_vec_pretty(root)?)
        .with_context(|| format!("{}: saving game-files location", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glide(root: &Path) {
        std::fs::create_dir_all(root.join("fxart")).unwrap();
        std::fs::create_dir_all(root.join("maps")).unwrap();
        std::fs::write(root.join("RGFX.EXE"), []).unwrap();
    }

    #[test]
    fn the_game_and_the_launcher_agree_on_what_an_install_is() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        glide(&base.join("direct"));
        glide(&base.join("gog/app/Redguard"));
        std::fs::create_dir_all(base.join("gog/app/Redguard/fxart/inner")).unwrap();
        glide(&base.join("package/Redguard/Redguard"));
        for software in ["3dart", "RG.EXE"] {
            let path = base.join("dos").join(software);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            if software == "3dart" {
                std::fs::create_dir(&path).unwrap();
            } else {
                std::fs::write(&path, []).unwrap();
            }
        }
        std::fs::create_dir(base.join("dos/maps")).unwrap();
        std::fs::create_dir_all(base.join("empty/inside")).unwrap();
        std::fs::create_dir_all(base.join("half/maps")).unwrap();
        for case in [
            "direct",
            "gog",
            "gog/app",
            "gog/app/Redguard/fxart/inner",
            "package",
            "dos",
            "empty/inside",
            "half",
            "missing",
        ] {
            let path = base.join(case);
            assert_eq!(
                resolve(&path).ok(),
                cyrus_assets::install::GameInstall::resolve(&path)
                    .ok()
                    .map(|install| install.root),
                "{case}"
            );
        }
    }

    #[test]
    fn the_search_order_is_the_games() {
        let machine = Machine {
            home: Some("/home/p".into()),
            system_drive: Some("C:".into()),
            program_files: vec!["/pf".into()],
            wine_prefix: Some("/wine".into()),
        };
        let theirs = cyrus_assets::install::Machine {
            home: machine.home.clone(),
            system_drive: machine.system_drive.clone(),
            program_files: machine.program_files.clone(),
            wine_prefix: machine.wine_prefix.clone(),
        };
        let program = Path::new("/opt/cyrus");
        assert_eq!(
            search_folders(&machine, Some(program)),
            cyrus_assets::install::search_folders(&theirs, Some(program))
        );
    }

    #[test]
    fn the_search_finds_a_gog_folder_and_stops_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let machine = Machine {
            home: Some(home.clone()),
            ..Machine::default()
        };
        // Skip when the developer's own machine answers through REDGUARD_DIR.
        if std::env::var_os("REDGUARD_DIR").is_some() {
            return;
        }
        assert_eq!(locate(&machine, None), None);
        glide(&home.join("GOG Games/Redguard/app/Redguard"));
        assert_eq!(
            locate(&machine, None),
            Some(
                home.join("GOG Games/Redguard/app/Redguard")
                    .canonicalize()
                    .unwrap()
            )
        );
    }
}
