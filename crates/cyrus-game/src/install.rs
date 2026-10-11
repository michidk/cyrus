//! Locating the player's own game files. The install is only ever read.

use bevy::prelude::*;
use std::path::{Path, PathBuf};

pub const ENV_VAR: &str = "REDGUARD_DIR";
pub const LINK_NAME: &str = "redguard";

#[derive(Resource, Clone, Debug)]
pub struct GameInstall {
    pub root: PathBuf,
}

impl GameInstall {
    /// `explicit` > `$REDGUARD_DIR` > `./redguard` link next to the working directory.
    pub fn locate(explicit: Option<&Path>) -> anyhow::Result<Self> {
        let candidate = explicit
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os(ENV_VAR).map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(LINK_NAME));
        Self::open(&candidate)
    }

    pub fn open(root: &Path) -> anyhow::Result<Self> {
        let install = cyrus_assets::install::GameInstall::open(root)?;
        install.cache_path(Path::new(cyrus_assets::CACHE_DIR))?;
        Ok(Self { root: install.root })
    }

    pub fn maps(&self) -> PathBuf {
        self.root.join("maps")
    }
    pub fn fxart(&self) -> PathBuf {
        self.root.join("fxart")
    }
}
