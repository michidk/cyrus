//! Local state lives in `cyrus-config/` next to the working directory, never in the install.

use cyrus_game::fidelity::Fidelity;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CONFIG_DIR: &str = "cyrus-config";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub scene: String,
    pub fidelity: Fidelity,
    pub control_addr: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scene: "ISLAND".into(),
            fidelity: Fidelity::default(),
            control_addr: cyrus_control::DEFAULT_ADDR.into(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        Path::new(CONFIG_DIR).join("settings.toml")
    }

    /// Missing file means defaults; a broken file is an error, not a silent reset.
    pub fn load() -> anyhow::Result<Self> {
        match std::fs::read_to_string(Self::path()) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn cache_dir() -> PathBuf {
        PathBuf::from(cyrus_assets::CACHE_DIR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_game::fidelity::Area;
    use cyrus_game::fidelity::Mode;

    #[test]
    fn default_settings_are_faithful_everywhere() {
        let settings = Settings::default();
        for area in Area::ALL {
            assert_eq!(settings.fidelity.mode(area), Mode::Faithful, "{area:?}");
        }
    }
}
