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
pub(crate) fn palette_path(source: &dyn ReadAssets, path: &str) -> Result<String> {
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
pub fn textures(
    source: &dyn ReadAssets,
    palette: Option<Palette>,
    requested: Option<&std::collections::HashSet<u16>>,
) -> Result<TextureCache> {
    let file_name = |path: &str| path.rsplit('/').next().unwrap_or("").to_ascii_uppercase();
    // One install holds one bank family; Glide wins if both appear.
    let software = !source
        .manifest()
        .files
        .iter()
        .any(|file| file_name(&file.path).starts_with("TEXBSI."));
    let prefix = if software { "TEXTURE." } else { "TEXBSI." };
    let mut data = HashMap::new();
    for file in &source.manifest().files {
        let name = file_name(&file.path);
        if let Some(id) = name
            .strip_prefix(prefix)
            .and_then(|id| id.parse::<u16>().ok())
            && let std::collections::hash_map::Entry::Vacant(entry) = data.entry(id)
        {
            entry.insert(&file.path);
        }
    }
    let selected = requested.map(|ids| {
        ids.iter()
            .map(|id| {
                TextureCache::resolve_texture_id(*id, |candidate| data.contains_key(&candidate))
            })
            .collect::<std::collections::HashSet<_>>()
    });
    let data = data
        .into_iter()
        .filter(|(id, _)| selected.as_ref().is_none_or(|ids| ids.contains(id)))
        .map(|(id, path)| Ok((id, source.read(path)?)))
        .collect::<Result<HashMap<_, _>>>()?;
    if software {
        Ok(TextureCache::from_software_data(data, palette)?)
    } else {
        Ok(TextureCache::from_data(data, palette))
    }
}
/// Terrain texture bank the original engine hard-wires for WLD terrain (per the
/// preservation crate's WLD exporter).
const TERRAIN_TEXBSI_ID: u16 = 302;

pub struct WorldGeometry {
    pub entry: world_ini::WorldEntry,
    pub rgm: rgm::RgmFile,
    pub models: Vec<rgm::PositionedModel>,
    pub registry: registry::Registry,
    pub terrain: Option<Box<wld::WldFile>>,
    pub palette: Palette,
    /// Texture bank cache already holding every texture the world geometry used.
    pub textures: TextureCache,
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
        None,
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
    let (rgm, models, lights, diagnostics) = rgm::parse_rgm_with_models_report(
        &source.read(&map)?,
        &registry,
        Path::new(&map).file_stem().and_then(|stem| stem.to_str()),
    )?;
    for diagnostic in diagnostics {
        if !diagnostic.model.is_empty() {
            source.diagnostic(crate::source::PreparationDiagnostic {
                category: "referenced_asset".into(),
                file: diagnostic.model.clone(),
                field: format!("referenced_by.{map}.{}", diagnostic.field),
                reason: diagnostic.reason.clone(),
                count: 1,
            });
        }
        source.diagnostic(crate::source::PreparationDiagnostic {
            category: "skipped_placement".into(),
            file: map.clone(),
            field: diagnostic.field,
            reason: diagnostic.reason,
            count: diagnostic.count,
        });
    }
    let terrain = entry
        .world
        .as_ref()
        .map(|path| -> Result<_> {
            Ok(Box::new(wld::parse_wld_file(
                &source.read(&map_path(source, path)?)?,
            )?))
        })
        .transpose()?;
    let (mut json, bytes) = if let Some(terrain) = &terrain {
        gltf::convert_wld_scene_to_gltf(
            terrain,
            TERRAIN_TEXBSI_ID,
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
    for material in &mut json.materials {
        let Some((bank, image)) = material.name.as_deref().and_then(glide_image) else {
            continue;
        };
        if let Some(key) = image_chroma_key(&rgm, &mut cache, bank, image) {
            let mut extras: serde_json::Map<String, serde_json::Value> = material
                .extras
                .as_deref()
                .map(|raw| serde_json::from_str(raw.get()))
                .transpose()?
                .unwrap_or_default();
            extras.insert("cyrus_texture_key".into(), serde_json::json!(key));
            material.extras = Some(serde_json::value::to_raw_value(&extras)?);
        }
    }
    report_textures(source, &cache);
    Ok(WorldGeometry {
        entry,
        rgm,
        models,
        registry,
        terrain,
        palette,
        textures: cache,
        glb: gltf::to_glb(&json, &bytes)?,
    })
}
/// Bank and image of a Glide texture material, from preservation's material name
/// `TEXBSI.<bank> / <image name> (image <id>)`.
// shortcut: the name is the only link from an exported material to its image; read the key from
// preservation's material metadata once it carries the FLAT image key.
fn glide_image(name: &str) -> Option<(u16, u8)> {
    let (bank, rest) = name.strip_prefix("TEXBSI.")?.split_once(" / ")?;
    let image = rest.rsplit_once("(image ")?.1.strip_suffix(')')?;
    Some((bank.parse().ok()?, image.parse().ok()?))
}

/// Chroma key of a texture image whose world `FLAT` record sets flag bit 1: the world palette
/// colour of the image's first stored texel, instead of palette entry zero.
/// See redguard-preservation `docs/formats/RGM.md#flat`.
fn image_chroma_key(
    rgm: &rgm::RgmFile,
    cache: &mut TextureCache,
    bank: u16,
    image: u8,
) -> Option<[u8; 3]> {
    let flags = rgm
        .flat_texture_attributes(bank << 7 | u16::from(image))?
        .flags;
    if flags & 2 == 0 {
        return None;
    }
    let (rgba, width, height) = cache.get_image_rgba(bank, image)?;
    // Exported rows are reversed, so the first stored row is the last exported one.
    let first = (usize::from(height).checked_sub(1)? * usize::from(width)) * 4;
    let texel = rgba.get(first..first + 4)?;
    // Entry zero exports transparent; its uploaded colour is black.
    Some(if texel[3] == 0 {
        [0; 3]
    } else {
        [texel[0], texel[1], texel[2]]
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

/// Octants and root half-extent follow preservation's PVO format contract.
pub fn visibility_cells(tree: &rgpre::import::pvo::PvoFile) -> Result<Vec<crate::SpatialCell>> {
    use rgpre::import::pvo::{CHILD_REF_EMPTY, LEAF_REF_NONE};
    let nodes: HashMap<_, _> = tree
        .octr_nodes
        .iter()
        .map(|n| (n.byte_offset as u32, n))
        .collect();
    let leaves: HashMap<_, _> = tree
        .plst_leaves
        .iter()
        .map(|leaf| (leaf.byte_offset as u32, leaf))
        .collect();
    let mut pending = vec![(
        0,
        [
            tree.header.center_x as f32,
            tree.header.center_y as f32,
            tree.header.center_z as f32,
        ],
        tree.header.cell_size as f32,
        0,
    )];
    let mut cells = Vec::new();
    while let Some((id, center, half_extent, depth)) = pending.pop() {
        anyhow::ensure!(
            cells.len() < 1_000_000,
            "PVO visualization exceeds one million spatial cells"
        );
        anyhow::ensure!(
            depth <= tree.header.depth,
            "PVO node {id}: exceeds declared depth"
        );
        let node = nodes
            .get(&id)
            .with_context(|| format!("PVO node {id}: missing child"))?;
        let visible_indices = if node.leaf_ref == LEAF_REF_NONE {
            0
        } else {
            leaves
                .get(&node.leaf_ref)
                .with_context(|| format!("PVO node {id}: missing leaf"))?
                .entries
                .iter()
                .map(|e| usize::from(e.count))
                .sum()
        };
        cells.push(crate::SpatialCell {
            center: rgm::world_position(center),
            half_extent: gltf::model_position([half_extent, 0., 0.])[0].abs(),
            depth,
            visible_indices,
            visibility_ids: tree.visibility_ids(node.leaf_ref)?,
        });
        for &(octant, child) in &node.child_refs {
            if child == CHILD_REF_EMPTY {
                continue;
            }
            let half = half_extent * 0.5;
            let child_center = [
                center[0] + if octant & 4 != 0 { half } else { -half },
                center[1] + if octant & 2 != 0 { half } else { -half },
                center[2] + if octant & 1 != 0 { half } else { -half },
            ];
            pending.push((child, child_center, half, depth + 1));
        }
    }
    Ok(cells)
}

/// Preserve the texture cache's existing missing-image fallback diagnostics.
pub(crate) fn report_textures(source: &dyn ReadAssets, cache: &TextureCache) {
    for (&(texture, image), reason) in cache.missing_requests() {
        source.diagnostic(crate::source::PreparationDiagnostic {
            category: "referenced_asset".into(),
            file: format!("TEXBSI.{texture:03}"),
            field: format!("images[{image}]"),
            reason: reason.clone(),
            count: 1,
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn glide_material_names_give_bank_and_image() {
        assert_eq!(
            super::glide_image("TEXBSI.357 / D57000 (image 0)"),
            Some((357, 0))
        );
        assert_eq!(
            super::glide_image("TEXBSI.302 / C02012 (image 12)"),
            Some((302, 12))
        );
        assert_eq!(super::glide_image("TEXTURE.357 / D57000 (image 0)"), None);
        assert_eq!(super::glide_image("TEXBSI.357 / D57000"), None);
    }
}
