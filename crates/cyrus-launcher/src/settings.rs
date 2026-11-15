//! Saves only the configuration group the player explicitly changed.
use crate::{fidelity, launch, mods};
use std::collections::BTreeSet;
use std::path::Path;

/// One changed group. Unrelated files are never read or rewritten, including files that failed to
/// load when the launcher opened.
pub enum Change<'a> {
    GameOptions(&'a launch::Options),
    Mods(&'a BTreeSet<String>),
    Presentation(&'a fidelity::Choices),
}

impl Change<'_> {
    /// Saves this group under the selected config directory. Presentation saves retain the game's
    /// other settings and fail without writing if the existing TOML cannot be read.
    pub fn save(self, config: &Path) -> anyhow::Result<()> {
        match self {
            Self::GameOptions(options) => options.save(&config.join("launcher.json")),
            Self::Mods(active) => mods::save_active(&config.join("mods.json"), active),
            Self::Presentation(choices) => fidelity::save(&config.join("settings.toml"), choices),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_change_preserves_unreadable_mods_and_presentation() {
        let dir = tempfile::tempdir().unwrap();
        let mods = b"{\"active\":[\"lost-mod\"], BROKEN}\n";
        let settings = b"[fidelity\nmalformed but recoverable\n";
        std::fs::write(dir.path().join("mods.json"), mods).unwrap();
        std::fs::write(dir.path().join("settings.toml"), settings).unwrap();
        let options = launch::Options {
            skip_movies: true,
            ..Default::default()
        };
        Change::GameOptions(&options).save(dir.path()).unwrap();
        assert_eq!(
            launch::Options::load(&dir.path().join("launcher.json")).unwrap(),
            options
        );
        assert_eq!(std::fs::read(dir.path().join("mods.json")).unwrap(), mods);
        assert_eq!(
            std::fs::read(dir.path().join("settings.toml")).unwrap(),
            settings
        );
    }

    #[test]
    fn mod_change_preserves_unreadable_options_and_presentation() {
        let dir = tempfile::tempdir().unwrap();
        let damaged = b"recoverable malformed configuration\n";
        for name in ["launcher.json", "settings.toml"] {
            std::fs::write(dir.path().join(name), damaged).unwrap();
        }
        let active = BTreeSet::from(["chosen-mod".into()]);
        Change::Mods(&active).save(dir.path()).unwrap();
        assert_eq!(
            mods::load_active(&dir.path().join("mods.json")).unwrap(),
            active
        );
        for name in ["launcher.json", "settings.toml"] {
            assert_eq!(std::fs::read(dir.path().join(name)).unwrap(), damaged);
        }
    }

    #[test]
    fn presentation_change_preserves_unreadable_options_and_mods() {
        let dir = tempfile::tempdir().unwrap();
        let damaged = b"recoverable malformed configuration\n";
        for name in ["launcher.json", "mods.json"] {
            std::fs::write(dir.path().join(name), damaged).unwrap();
        }
        std::fs::write(dir.path().join("settings.toml"), "[audio]\nmaster = 0.37\n").unwrap();
        let mut choices = fidelity::Choices::default();
        choices.set(fidelity::Area::Fog, fidelity::Mode::Modern);
        Change::Presentation(&choices).save(dir.path()).unwrap();
        let settings = std::fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        let table: toml::Table = toml::from_str(&settings).unwrap();
        assert_eq!(table["audio"]["master"].as_float(), Some(0.37));
        assert_eq!(
            fidelity::load(&dir.path().join("settings.toml"))
                .unwrap()
                .mode(fidelity::Area::Fog),
            fidelity::Mode::Modern
        );
        for name in ["launcher.json", "mods.json"] {
            assert_eq!(std::fs::read(dir.path().join(name)).unwrap(), damaged);
        }
    }
}
