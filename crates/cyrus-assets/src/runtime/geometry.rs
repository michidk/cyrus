//! Geometry built from original file bytes, without filesystem access.
use crate::source::ReadAssets;
use anyhow::{Context, Result};
use rgpre::{
    gltf::{self, TextureCache},
    import::{FileType, palette::Palette, registry, rgm, wld, world_ini},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub fn find_name(source: &dyn ReadAssets, name: &str) -> Option<String> {
    let name = name.rsplit(['/', '\\']).next()?;
    source
        .manifest()
        .files
        .iter()
        .find(|file| {
            file.path
                .rsplit('/')
                .next()
                .is_some_and(|part| part.eq_ignore_ascii_case(name))
        })
        .map(|file| file.path.clone())
}
fn map_path(source: &dyn ReadAssets, path: &str) -> Result<String> {
    let name = path.rsplit(['/', '\\']).next().context("empty map path")?;
    source
        .manifest()
        .find(&format!("maps/{name}"))
        .map(|file| file.path.clone())
        .with_context(|| format!("missing map {path}"))
}
fn palette_path(source: &dyn ReadAssets, path: &str) -> Result<String> {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .context("empty palette path")?;
    ["fxart", "3dart"]
        .iter()
        .find_map(|directory| source.manifest().find(&format!("{directory}/{name}")))
        .map(|file| file.path.clone())
        .with_context(|| format!("missing palette {path}"))
}
pub fn textures(source: &dyn ReadAssets, palette: Option<Palette>) -> Result<TextureCache> {
    let mut data = HashMap::new();
    for file in &source.manifest().files {
        let name = file
            .path
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        if let Some(id) = name
            .strip_prefix("TEXBSI.")
            .and_then(|id| id.parse::<u16>().ok())
            && let std::collections::hash_map::Entry::Vacant(entry) = data.entry(id)
        {
            entry.insert(source.read(&file.path)?);
        }
    }
    Ok(TextureCache::from_data(data, palette))
}
pub struct WorldGeometry {
    pub entry: world_ini::WorldEntry,
    pub rgm: rgm::RgmFile,
    pub models: Vec<rgm::PositionedModel>,
    pub registry: registry::Registry,
    pub terrain: Option<Box<wld::WldFile>>,
    pub glb: Vec<u8>,
}
pub fn world(source: &dyn ReadAssets, name: &str) -> Result<WorldGeometry> {
    let ini = find_name(source, "WORLD.INI").context("missing WORLD.INI")?;
    let worlds = world_ini::WorldIni::parse(&source.text(&ini)?);
    let entry = resolve_world(&worlds, name)?.clone();
    let palette = Palette::parse(&source.read(&palette_path(source, &entry.palette)?)?)?;
    let mut cache = textures(
        source,
        Some(Palette {
            colors: palette.colors,
        }),
    )?;
    let mut registry = registry::Registry::new(PathBuf::new());
    for file in &source.manifest().files {
        if let Some(kind) = FileType::from_path(Path::new(&file.path)) {
            registry.add_file(&file.path, kind);
        }
    }
    for file in registry.files.values_mut() {
        if matches!(
            file.file_type,
            FileType::Rob | FileType::Model3d | FileType::Model3dc
        ) {
            file.data = Some(source.read(&file.path.to_string_lossy())?);
        }
    }
    let map = map_path(source, &entry.map)?;
    let (rgm, models, lights) = rgm::parse_rgm_with_models_for_stem(
        &source.read(&map)?,
        &registry,
        Path::new(&map).file_stem().and_then(|stem| stem.to_str()),
    )?;
    let terrain = entry
        .world
        .as_ref()
        .map(|path| -> Result<_> {
            Ok(Box::new(wld::parse_wld_file(
                &source.read(&map_path(source, path)?)?,
            )?))
        })
        .transpose()?;
    let (json, bytes) = if let Some(terrain) = &terrain {
        gltf::convert_wld_scene_to_gltf(
            terrain,
            302,
            &models,
            Some(&palette),
            Some(&mut cache),
            false,
        )?
    } else {
        gltf::convert_positioned_models_to_gltf(
            &models,
            &lights,
            Some(&palette),
            Some(&mut cache),
            false,
        )?
    };
    Ok(WorldGeometry {
        entry,
        rgm,
        models,
        registry,
        terrain,
        glb: gltf::to_glb(&json, &bytes)?,
    })
}
/// Numeric identity preserves world variants; a map stem selects its first authored variant.
pub fn resolve_world<'a>(
    worlds: &'a world_ini::WorldIni,
    name: &str,
) -> Result<&'a world_ini::WorldEntry> {
    anyhow::ensure!(
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "scene must be a WORLD.INI ID or map stem"
    );
    let entry = if let Ok(id) = name.parse::<u32>() {
        worlds.entries.iter().find(|e| e.index == id)
    } else {
        worlds.find_by_map_stem(name).first().copied()
    };
    entry.with_context(|| format!("no world {name} in WORLD.INI"))
}
