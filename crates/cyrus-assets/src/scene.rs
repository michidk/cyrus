//! Native scene export and installed world selection.
use crate::install::GameInstall;
pub use crate::native::geometry::resolve_world;
use log::info;
use rgpre::import::world_ini::{self, WorldIni};
use std::path::{Path, PathBuf};

/// WLD scenes (outdoor) get terrain plus their RGM placements; indoor scenes are RGM only.
pub fn export_scene(install: &GameInstall, name: &str, out: &Path) -> anyhow::Result<()> {
    export_scene_reporting(install, name, out, &|_, _, _| {})
}

/// Preparation stages in order; `progress(done, total, stage)` precedes each one.
pub const SCENE_STAGES: [&str; 6] = [
    "World geometry",
    "Animations",
    "Player models",
    "Environment",
    "Player inputs",
    "Scene file",
];

pub fn export_scene_reporting(
    install: &GameInstall,
    name: &str,
    out: &Path,
    progress: &dyn Fn(usize, usize, &str),
) -> anyhow::Result<()> {
    struct Reporting<'a> {
        files: crate::native::Files,
        progress: &'a dyn Fn(usize, usize, &str),
    }
    impl crate::source::ReadAssets for Reporting<'_> {
        fn manifest(&self) -> &crate::source::Manifest {
            self.files.manifest()
        }
        fn read_range(&self, path: &str, start: u64, length: usize) -> anyhow::Result<Vec<u8>> {
            self.files.read_range(path, start, length)
        }
        fn progress(&self, done: usize, total: usize, stage: &str) {
            (self.progress)(done, total, stage);
        }
    }
    let source = Reporting {
        files: crate::native::Files::scan(install, false)?,
        progress,
    };
    let filename = Path::new(
        out.file_name()
            .ok_or_else(|| anyhow::anyhow!("scene output has no filename"))?,
    );
    crate::game::scene(
        &source,
        name,
        filename,
        &mut crate::preparation::DiskOutput(out.parent().unwrap_or(Path::new("."))),
    )?;
    info!("scene {name}: exported to {}", out.display());
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

/// WORLD.INI IDs and map-stem aliases accepted by native and browser scene selection.
pub fn world_names(install: &GameInstall) -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in read_worlds(install)?.entries {
        names.push(entry.index.to_string());
        let map = map_path(install, &entry.map)?;
        let stem = map
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| {
                anyhow::anyhow!("world {}: invalid map path {}", entry.index, entry.map)
            })?;
        names.push(stem.to_ascii_uppercase());
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn read_worlds(install: &GameInstall) -> anyhow::Result<WorldIni> {
    let path = world_ini::find_world_ini(&install.root)
        .ok_or_else(|| anyhow::anyhow!("missing WORLD.INI"))?;
    Ok(WorldIni::parse(&std::fs::read_to_string(path)?))
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
                        path.with_extension("scripts"),
                        out.join(format!("{stem}.scripts")),
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
            rgpre::import::palette::Palette::parse(&std::fs::read(path).unwrap()).unwrap();
        }
    }
}
