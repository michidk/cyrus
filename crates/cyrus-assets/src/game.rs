//! Game asset preparation from the same read-only byte source on native and browser targets.
use crate::{preparation::Output, source::ReadAssets};
use anyhow::{Context, Result};
use std::{collections::BTreeMap, path::Path};

/// Original world IDs and map aliases, preserving installed palette variants.
pub fn worlds(source: &dyn ReadAssets) -> Result<Vec<String>> {
    let ini = crate::preparation::text(source, "WORLD.INI")?;
    let mut names = Vec::new();
    for entry in rgpre::import::world_ini::WorldIni::parse(&ini).entries {
        names.push(entry.index.to_string());
        let filename = entry
            .map
            .rsplit(['/', '\\'])
            .next()
            .context("empty map filename")?;
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .with_context(|| format!("world {}: invalid map {}", entry.index, entry.map))?;
        names.push(stem.to_ascii_uppercase());
    }
    names.sort();
    names.dedup();
    Ok(names)
}

pub(crate) fn scene(
    source: &dyn ReadAssets,
    name: &str,
    out: &Path,
    output: &mut dyn Output,
) -> Result<()> {
    source.progress(0, 6, "World geometry");
    let mut world = crate::runtime::geometry::world(source, name)?;
    let entry = &world.entry;
    source.progress(1, 6, "Animations");
    crate::animation_export::prepare(
        &world.rgm,
        &world.models,
        &world.registry,
        entry.index,
        source,
        out,
        output,
    )?;
    source.progress(2, 6, "Player models");
    let radii = crate::player_export::models::prepare(
        &world.rgm,
        &world.registry,
        &world.palette,
        &mut world.textures,
        out,
        output,
    )?;
    source.progress(3, 6, "Environment");
    crate::environment_export::prepare(source, entry, world.terrain.as_deref(), out, output)?;
    source.progress(4, 6, "Player inputs");
    crate::player_export::prepare(source, &world, &radii, out, output)?;
    source.progress(5, 6, "Scene file");
    output.publish(out, &world.glb)?;
    source.progress(6, 6, "World ready");
    Ok(())
}

/// Prepare the group owning a requested cache path. All returned paths stay cache-relative.
pub fn prepare(source: &dyn ReadAssets, path: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    crate::validate_path(path)?;
    let mut files = BTreeMap::new();
    let output: &mut dyn Output = &mut files;
    match path.split('/').next().unwrap_or_default() {
        "startup" => crate::startup::prepare_startup(source, output)?,
        "menu" => crate::startup::prepare_menu(source, output)?,
        "hud" => crate::hud_export::prepare(source, output)?,
        "sounds" => crate::sound_export::prepare(source, output)?,
        "music" => crate::music::prepare_track(source, path, output)?,
        _ => {
            let file = Path::new(path);
            let scene_path = if file.parent() == Some(Path::new("")) {
                file.with_extension("glb")
            } else {
                let parent = file.parent().context("invalid game cache path")?;
                anyhow::ensure!(
                    parent.extension().is_some_and(|ext| ext == "player-models")
                        && parent.parent() == Some(Path::new("")),
                    "unsupported game cache path {path}"
                );
                parent.with_extension("glb")
            };
            let name = scene_path
                .file_stem()
                .and_then(|s| s.to_str())
                .context("invalid scene cache path")?;
            scene(source, name, &scene_path, output)?;
        }
    }
    anyhow::ensure!(
        files.contains_key(path),
        "game preparation did not produce {path}"
    );
    Ok(files)
}
