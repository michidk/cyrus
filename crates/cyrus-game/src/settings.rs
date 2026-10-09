//! Local state lives in `cyrus-config/` next to the working directory, never in the install.

use crate::fidelity::Fidelity;
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
        Path::new(CONFIG_DIR).join("cache")
    }
}
