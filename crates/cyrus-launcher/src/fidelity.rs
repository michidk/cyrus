//! The faithful or modern choice for each presentation area, as the game's `settings.toml` holds it.
//!
//! The game owns the areas (`cyrus_game::fidelity`); the launcher repeats their names so that it does
//! not link the game, and a test in this module pins the two lists together. Only the `[fidelity]`
//! table of `settings.toml` is touched, so the game's other settings survive a save.
use anyhow::Context;
use std::path::{Path, PathBuf};

/// The file the game reads its settings from.
pub fn settings_file() -> PathBuf {
    cyrus_config::config_dir().join("settings.toml")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Faithful,
    Modern,
}

impl Mode {
    fn key(self) -> &'static str {
        match self {
            Mode::Faithful => "faithful",
            Mode::Modern => "modern",
        }
    }
}

impl std::str::FromStr for Mode {
    type Err = ();
    fn from_str(text: &str) -> Result<Self, ()> {
        match text {
            "faithful" => Ok(Mode::Faithful),
            "modern" => Ok(Mode::Modern),
            _ => Err(()),
        }
    }
}

/// The presentation areas the game can render either way, in the order the window lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Fog,
    Sky,
    Lighting,
    Textures,
    AnisotropicFiltering,
    AudioResampling,
}

impl Area {
    pub const ALL: [Area; 6] = [
        Area::Fog,
        Area::Sky,
        Area::Lighting,
        Area::Textures,
        Area::AnisotropicFiltering,
        Area::AudioResampling,
    ];

    /// The area's key in `settings.toml`.
    pub fn key(self) -> &'static str {
        match self {
            Area::Fog => "fog",
            Area::Sky => "sky",
            Area::Lighting => "lighting",
            Area::Textures => "textures",
            Area::AnisotropicFiltering => "anisotropic_filtering",
            Area::AudioResampling => "audio_resampling",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Area::Fog => "Fog",
            Area::Sky => "Sky",
            Area::Lighting => "Lighting and shadows",
            Area::Textures => "Texture sampling",
            Area::AnisotropicFiltering => "Anisotropic filtering",
            Area::AudioResampling => "Audio resampling",
        }
    }

    /// What Modern changes, for the line under the toggle.
    pub fn modern_effect(self) -> &'static str {
        match self {
            Area::Fog => "Replaces the original's fog presentation.",
            Area::Sky => "Replaces the original's sky presentation.",
            Area::Lighting => "Physically based shading, shadows and screen-space effects.",
            Area::Textures => "Point-samples world textures instead of bilinear filtering.",
            Area::AnisotropicFiltering => {
                "Averages up to sixteen samples along texture footprints."
            }
            Area::AudioResampling => "Windowed-sinc upsampling in place of the original's.",
        }
    }
}

/// The mode of every area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Choices {
    modes: [Mode; Area::ALL.len()],
}

impl Choices {
    fn index(area: Area) -> usize {
        Area::ALL
            .iter()
            .position(|a| *a == area)
            .expect("listed area")
    }
    pub fn mode(&self, area: Area) -> Mode {
        self.modes[Self::index(area)]
    }
    pub fn set(&mut self, area: Area, mode: Mode) {
        self.modes[Self::index(area)] = mode;
    }
    pub fn all(mode: Mode) -> Self {
        Self {
            modes: [mode; Area::ALL.len()],
        }
    }
    /// The mode every area shares, or `None` when they differ.
    pub fn uniform(&self) -> Option<Mode> {
        let first = self.modes[0];
        self.modes.iter().all(|m| *m == first).then_some(first)
    }
}

/// The areas in `file`'s `[fidelity]` table; an absent file, table or key is Faithful. A value that is
/// neither mode is an error, as in the game.
pub fn load(file: &Path) -> anyhow::Result<Choices> {
    let table = read_table(file)?;
    let mut choices = Choices::default();
    let Some(fidelity) = table.get("fidelity").and_then(toml::Value::as_table) else {
        return Ok(choices);
    };
    for area in Area::ALL {
        if let Some(value) = fidelity.get(area.key()) {
            let mode = value
                .as_str()
                .and_then(|text| text.parse().ok())
                .with_context(|| {
                    format!(
                        "{}: fidelity.{} must be \"faithful\" or \"modern\"",
                        file.display(),
                        area.key()
                    )
                })?;
            choices.set(area, mode);
        }
    }
    Ok(choices)
}

/// Writes `choices` as `file`'s `[fidelity]` table and keeps every other setting as it was.
pub fn save(file: &Path, choices: &Choices) -> anyhow::Result<()> {
    let mut table = read_table(file)?;
    let mut fidelity = match table.remove("fidelity") {
        Some(toml::Value::Table(table)) => table,
        _ => toml::Table::new(),
    };
    for area in Area::ALL {
        fidelity.insert(
            area.key().into(),
            toml::Value::String(choices.mode(area).key().into()),
        );
    }
    table.insert("fidelity".into(), toml::Value::Table(fidelity));
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).with_context(|| parent.display().to_string())?;
    }
    std::fs::write(file, toml::to_string_pretty(&table)?)
        .with_context(|| format!("{}: saving settings", file.display()))
}

fn read_table(file: &Path) -> anyhow::Result<toml::Table> {
    match std::fs::read_to_string(file) {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("{}: invalid settings", file.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(error) => Err(error).with_context(|| file.display().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_lists_exactly_the_games_areas_under_the_games_keys() {
        use cyrus_game::fidelity as game;
        let mut value = serde_json::to_value(game::Fidelity::default()).unwrap();
        let game_keys: std::collections::BTreeSet<_> =
            value.as_object().unwrap().keys().cloned().collect();
        let launcher_keys: std::collections::BTreeSet<_> =
            Area::ALL.iter().map(|a| a.key().to_owned()).collect();
        assert_eq!(launcher_keys, game_keys);
        // And the game reads what the launcher writes: every area modern.
        for area in Area::ALL {
            value[area.key()] = serde_json::json!(Mode::Modern.key());
        }
        let parsed: game::Fidelity = serde_json::from_value(value).unwrap();
        for area in game::Area::ALL {
            assert_eq!(parsed.mode(area), game::Mode::Modern, "{area:?}");
        }
    }

    #[test]
    fn missing_settings_are_faithful_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        let choices = load(&dir.path().join("settings.toml")).unwrap();
        assert_eq!(choices.uniform(), Some(Mode::Faithful));
    }

    #[test]
    fn saving_keeps_the_games_other_settings() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.toml");
        std::fs::write(
            &file,
            "scene = \"START\"\ncontrol_addr = \"127.0.0.1:1\"\n[fidelity]\nfog = \"modern\"\n",
        )
        .unwrap();
        let mut choices = load(&file).unwrap();
        assert_eq!(choices.mode(Area::Fog), Mode::Modern);
        choices.set(Area::Sky, Mode::Modern);
        save(&file, &choices).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("scene = \"START\"") && text.contains("control_addr"),
            "{text}"
        );
        assert_eq!(load(&file).unwrap(), choices);
    }

    #[test]
    fn a_value_that_is_not_a_mode_names_the_file_and_key() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.toml");
        std::fs::write(&file, "[fidelity]\nsky = \"retro\"\n").unwrap();
        let error = format!("{:#}", load(&file).unwrap_err());
        assert!(
            error.contains("settings.toml") && error.contains("fidelity.sky"),
            "{error}"
        );
    }

    #[test]
    fn uniform_is_none_when_areas_differ() {
        let mut choices = Choices::all(Mode::Modern);
        assert_eq!(choices.uniform(), Some(Mode::Modern));
        choices.set(Area::Sky, Mode::Faithful);
        assert_eq!(choices.uniform(), None);
    }
}
