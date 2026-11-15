//! The installed mods and which of them are switched on.
//!
//! A mod is a folder under `mods/` in the config directory with a `mod.toml`. Only the fields a
//! player reads in a list are taken from it here; the game reads the rest. The set of active mods is
//! `mods.json`, which the game reads at startup.
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The installed mods folder in the config directory.
pub fn mods_dir() -> PathBuf {
    cyrus_config::config_dir().join("mods")
}

/// The file listing the active mods, as the game reads it.
pub fn active_file() -> PathBuf {
    cyrus_config::config_dir().join("mods.json")
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    description: String,
    version: Option<String>,
    author: Option<String>,
}

/// One installed mod as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mod {
    /// The folder name, which keys activation and the mod's settings.
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
}

/// Installed mods in id order, and the folders that could not be read, by folder name with the reason.
#[derive(Debug, Default)]
pub struct Installed {
    pub mods: Vec<Mod>,
    pub problems: BTreeMap<String, String>,
}

/// Reads every folder of `dir` that has a `mod.toml`. A missing `dir` is no mods; a manifest that does
/// not read is reported without hiding the others.
pub fn scan(dir: &Path) -> anyhow::Result<Installed> {
    let mut installed = Installed::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(installed),
        Err(error) => return Err(error).with_context(|| dir.display().to_string()),
    };
    for entry in entries {
        let folder = entry.with_context(|| dir.display().to_string())?.path();
        let manifest = folder.join("mod.toml");
        let Some(id) = folder.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !manifest.is_file() {
            continue;
        }
        let read = std::fs::read_to_string(&manifest)
            .map_err(anyhow::Error::from)
            .and_then(|text| toml::from_str::<Manifest>(&text).map_err(anyhow::Error::from));
        match read {
            Ok(read) => installed.mods.push(Mod {
                id: id.to_owned(),
                name: read.name,
                description: read.description,
                version: read.version,
                author: read.author,
            }),
            Err(error) => {
                installed
                    .problems
                    .insert(id.to_owned(), format!("{error:#}"));
            }
        }
    }
    installed.mods.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(installed)
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    active: Vec<String>,
}

/// The active mod ids in `file`; a missing file is none.
pub fn load_active(file: &Path) -> anyhow::Result<BTreeSet<String>> {
    match std::fs::read(file) {
        Ok(bytes) => {
            let saved: Saved = serde_json::from_slice(&bytes)
                .with_context(|| format!("{}: invalid active mod list", file.display()))?;
            Ok(saved.active.into_iter().collect())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(error) => Err(error).with_context(|| file.display().to_string()),
    }
}

/// Writes the active mod ids to `file`, in id order.
pub fn save_active(file: &Path, active: &BTreeSet<String>) -> anyhow::Result<()> {
    let saved = Saved {
        active: active.iter().cloned().collect(),
    };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).with_context(|| parent.display().to_string())?;
    }
    std::fs::write(file, serde_json::to_vec_pretty(&saved)?)
        .with_context(|| format!("{}: saving the active mod list", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, id: &str, text: &str) {
        let folder = dir.join(id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("mod.toml"), text).unwrap();
    }

    #[test]
    fn scan_lists_mods_in_id_order_and_keeps_going_past_a_broken_one() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "b",
            "name = \"B\"\nversion = \"1\"\nauthor = \"me\"\n",
        );
        write(
            dir.path(),
            "a",
            "name = \"A\"\ndescription = \"first\"\n[[setting]]\nkey = \"x\"\n",
        );
        write(dir.path(), "broken", "description = \"no name\"\n");
        std::fs::create_dir(dir.path().join("not-a-mod")).unwrap();
        let installed = scan(dir.path()).unwrap();
        let ids: Vec<_> = installed.mods.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert_eq!(installed.mods[0].description, "first");
        assert_eq!(installed.mods[1].version.as_deref(), Some("1"));
        assert_eq!(installed.problems.keys().collect::<Vec<_>>(), ["broken"]);
    }

    #[test]
    fn a_missing_mods_folder_is_no_mods() {
        let dir = tempfile::tempdir().unwrap();
        let installed = scan(&dir.path().join("none")).unwrap();
        assert!(installed.mods.is_empty() && installed.problems.is_empty());
    }

    #[test]
    fn active_list_round_trips_and_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nested/mods.json");
        assert!(load_active(&file).unwrap().is_empty());
        let active: BTreeSet<String> = ["grass".into(), "blood".into()].into();
        save_active(&file, &active).unwrap();
        assert_eq!(load_active(&file).unwrap(), active);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.find("blood").unwrap() < text.find("grass").unwrap());
    }

    #[test]
    fn a_broken_active_list_is_an_error_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mods.json");
        std::fs::write(&file, "{").unwrap();
        let error = load_active(&file).unwrap_err();
        assert!(format!("{error:#}").contains("mods.json"));
    }
}
