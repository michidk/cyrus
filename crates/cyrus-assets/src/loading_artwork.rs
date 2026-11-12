//! Destination-specific loading pictures, prepared on demand from WORLD.INI.
use bevy::prelude::{Asset, TypePath};
use serde::{Deserialize, Serialize};

/// Portable frames for one installed destination, preserving the archive's palette.
#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct LoadingArtwork {
    /// Numeric destination identity from WORLD.INI.
    pub world: u32,
    /// Full-screen archive frames with their own decoded palette.
    pub pictures: Vec<crate::menu::MenuPicture>,
}

impl LoadingArtwork {
    /// Require complete full-screen frames before publishing presentation assets.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.pictures.is_empty()
                && self
                    .pictures
                    .iter()
                    .all(|p| p.width == 640 && p.height == 480 && p.rgba.len() == 640 * 480 * 4),
            "world {} loading artwork: expected nonempty 640x480 RGBA frames",
            self.world
        );
        Ok(())
    }
}

#[cfg(feature = "parsers")]
pub(crate) fn prepare(
    source: &dyn crate::source::ReadAssets,
    path: &str,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let name = path
        .strip_prefix("loading/")
        .and_then(|p| p.strip_suffix(".loading"))
        .context("loading artwork: expected loading/WORLD.loading")?;
    let ini = crate::preparation::text(source, "WORLD.INI")?;
    let worlds = rgpre::import::world_ini::WorldIni::parse(&ini);
    let world = crate::runtime::geometry::resolve_world(&worlds, name)?.index;
    let filename = rgpre::import::world_ini::transition_artwork(&ini, world)
        .with_context(|| format!("world {world}: WORLD.INI loading artwork is missing"))?;
    let bytes = crate::preparation::read(source, &filename)?;
    let archive = rgpre::import::gxa::parse_gxa_file(&bytes)
        .with_context(|| format!("world {world}: loading artwork {filename}"))?;
    let artwork = LoadingArtwork {
        world,
        pictures: archive
            .frames
            .into_iter()
            .map(|frame| crate::menu::MenuPicture {
                width: frame.width,
                height: frame.height,
                rgba: frame.rgba,
            })
            .collect(),
    };
    artwork.validate()?;
    output.publish(std::path::Path::new(path), &serde_json::to_vec(&artwork)?)
}

/// Prepare only the requested destination, writing outside the read-only install.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export(
    install: &crate::install::GameInstall,
    name: &str,
    cache: &std::path::Path,
) -> anyhow::Result<()> {
    let cache = install.cache_path(cache)?;
    let source = crate::native::Files::scan(install, false)?;
    prepare(
        &source,
        &format!("loading/{name}.loading"),
        &mut crate::preparation::DiskOutput(&cache),
    )
}
