//! Native scene export and installed world selection.
use crate::install::GameInstall;
use log::info;
use rgpre::gltf::{
    TextureCache, convert_positioned_models_to_gltf, convert_wld_scene_to_gltf, to_glb,
};
use rgpre::import::{
    palette::Palette,
    registry, rgm, wld,
    world_ini::{self, WorldEntry, WorldIni},
};
use std::path::{Path, PathBuf};

/// Terrain texture bank the original engine hard-wires for WLD terrain (per the
/// preservation crate's WLD exporter).
const TERRAIN_TEXBSI_ID: u16 = 302;

/// WLD scenes (outdoor) get terrain plus their RGM placements; indoor scenes are RGM only.
pub fn export_scene(install: &GameInstall, name: &str, out: &Path) -> anyhow::Result<()> {
    let worlds = read_worlds(install)?;
    let entry = resolve_world(&worlds, name)?;
    let registry = registry::scan_dir(&install.root)?;
    let palette_path =
        world_ini::find_palette_on_disk(&install.root, &entry.palette).ok_or_else(|| {
            anyhow::anyhow!("world {}: missing palette {}", entry.index, entry.palette)
        })?;
    let palette = Palette::parse(&std::fs::read(palette_path)?)?;
    let mut cache = TextureCache::new(
        install.root.clone(),
        Some(Palette {
            colors: palette.colors,
        }),
    );
    let rgm_path = map_path(install, &entry.map)?;
    let rgm_bytes = std::fs::read(&rgm_path)?;
    let stem = rgm_path.file_stem().and_then(|s| s.to_str());
    let (rgm, models, lights) = rgm::parse_rgm_with_models_for_stem(&rgm_bytes, &registry, stem)?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    crate::animation_export::export(&rgm, &models, &registry, entry.index, out)?;
    let (root, buffer) = match &entry.world {
        Some(path) => {
            let terrain = wld::parse_wld_file(&std::fs::read(map_path(install, path)?)?)?;
            crate::environment_export::export(install, entry, Some(&terrain), out)?;
            crate::player_export::export(&install.root, &rgm, Some(&terrain), entry.index, out)?;
            convert_wld_scene_to_gltf(
                &terrain,
                TERRAIN_TEXBSI_ID,
                &models,
                Some(&palette),
                Some(&mut cache),
                false,
            )?
        }
        None => {
            crate::environment_export::export(install, entry, None, out)?;
            crate::player_export::export(&install.root, &rgm, None, entry.index, out)?;
            convert_positioned_models_to_gltf(
                &models,
                &lights,
                Some(&palette),
                Some(&mut cache),
                false,
            )?
        }
    };
    let glb = to_glb(&root, &buffer)?;
    let temporary = out.with_extension("glb.tmp");
    std::fs::write(&temporary, glb)?;
    std::fs::rename(temporary, out)?;
    info!(
        "scene {name}: exported {} models to {}",
        models.len(),
        out.display()
    );
    Ok(())
}

/// Case-insensitive lookup, because the GOG tree mixes upper and lower case.
pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
}

/// `WORLD.INI` index of a scene name: a world ID or a map stem.
pub fn world_index(install: &GameInstall, name: &str) -> anyhow::Result<u32> {
    Ok(resolve_world(&read_worlds(install)?, name)?.index)
}

fn read_worlds(install: &GameInstall) -> anyhow::Result<WorldIni> {
    let path = world_ini::find_world_ini(&install.root)
        .ok_or_else(|| anyhow::anyhow!("missing WORLD.INI"))?;
    Ok(WorldIni::parse(&std::fs::read_to_string(path)?))
}

fn resolve_world<'a>(worlds: &'a WorldIni, name: &str) -> anyhow::Result<&'a WorldEntry> {
    anyhow::ensure!(
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "scene must be a WORLD.INI ID or map stem"
    );
    let entry = if let Ok(id) = name.parse::<u32>() {
        worlds.entries.iter().find(|entry| entry.index == id)
    } else {
        // Stem aliases select the first world, as in preservation's CLI.
        worlds.find_by_map_stem(name).first().copied()
    };
    entry.ok_or_else(|| anyhow::anyhow!("no world {name} in WORLD.INI"))
}

fn map_path(install: &GameInstall, path: &str) -> anyhow::Result<PathBuf> {
    let filename = path
        .rsplit(['\\', '/'])
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty map path"))?;
    find_file(&install.maps(), filename).ok_or_else(|| anyhow::anyhow!("missing map {path}"))
}

/// Rebuild every world variant plus the traditional map-stem aliases.
/// Numeric filenames preserve worlds that share geometry but use different palettes.
pub fn export_all_worlds(install: &GameInstall, out: &Path) -> anyhow::Result<()> {
    crate::sound_export::export(install, out)?;
    let worlds = read_worlds(install)?;
    anyhow::ensure!(!worlds.entries.is_empty(), "WORLD.INI contains no worlds");
    let mut aliases = std::collections::HashSet::new();
    let mut failures = Vec::new();
    for entry in &worlds.entries {
        let name = entry.index.to_string();
        let path = out.join(format!("{name}.glb"));
        match export_scene(install, &name, &path) {
            Ok(()) => {
                let map = map_path(install, &entry.map)?;
                let stem = map
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| anyhow::anyhow!("invalid map stem"))?
                    .to_ascii_uppercase();
                if aliases.insert(stem.clone()) {
                    std::fs::copy(&path, out.join(format!("{stem}.glb")))?;
                    std::fs::copy(
                        path.with_extension("animations"),
                        out.join(format!("{stem}.animations")),
                    )?;
                    std::fs::copy(
                        path.with_extension("player"),
                        out.join(format!("{stem}.player")),
                    )?;
                    std::fs::copy(
                        path.with_extension("environment"),
                        out.join(format!("{stem}.environment")),
                    )?;
                }
                println!("world {name}: {} -> {}", entry.map, path.display());
            }
            Err(error) => failures.push(format!("world {name}: {error:#}")),
        }
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_selection_preserves_terrain_and_palette_variants() {
        let worlds = WorldIni::parse(
            "world_map[0]=MAPS\\intro.rgm\nworld_world[0]=MAPS\\shore.WLD\nworld_palette[0]=3DART\\dusk.COL\n\
             world_map[1]=MAPS\\town.rgm\nworld_palette[1]=3DART\\day.COL\n\
             world_map[27]=MAPS\\town.rgm\nworld_palette[27]=3DART\\night.COL\n",
        );
        let intro = resolve_world(&worlds, "INTRO").unwrap();
        assert_eq!(intro.world.as_deref(), Some("MAPS\\shore.WLD"));
        assert_eq!(intro.palette, "3DART\\dusk.COL");
        assert_eq!(resolve_world(&worlds, "town").unwrap().index, 1);
        assert_eq!(
            resolve_world(&worlds, "27").unwrap().palette,
            "3DART\\night.COL"
        );
        assert!(resolve_world(&worlds, "town").unwrap().world.is_none());
        for invalid in ["", "../town", "unknown", "999", "/tmp/town"] {
            assert!(resolve_world(&worlds, invalid).is_err());
        }
    }

    #[test]
    fn installed_worlds_resolve_all_required_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
        let Ok(install) = GameInstall::locate(Some(&root)) else {
            eprintln!("skipped: no install (installed world check)");
            return;
        };
        let worlds = read_worlds(&install).unwrap();
        assert!(!worlds.entries.is_empty());
        for entry in worlds.entries {
            map_path(&install, &entry.map).unwrap();
            if let Some(path) = entry.world {
                map_path(&install, &path).unwrap();
            }
            let path = world_ini::find_palette_on_disk(&install.root, &entry.palette).unwrap();
            Palette::parse(&std::fs::read(path).unwrap()).unwrap();
        }
    }
}
