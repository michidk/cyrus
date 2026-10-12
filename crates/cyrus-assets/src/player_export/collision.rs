//! Installed static placements and authored collision geometry for the portable player cache.
use crate::collision::{CollisionModel, CollisionScene, CollisionSubobject, StaticPlacement};
use anyhow::Context;
use rgpre::import::{FileType, model3d, registry, rgm, rob, world_ini};
use std::{collections::BTreeMap, path::Path};

pub(super) fn export(
    root: &Path,
    map: &rgm::RgmFile,
    world: u32,
) -> anyhow::Result<CollisionScene> {
    let mut scene = CollisionScene::default();
    for section in &map.sections {
        if let rgm::RgmSection::Mps(_, records) = section {
            for record in records {
                scene.placements.push(StaticPlacement {
                    id: record.id,
                    model: record.model_name().to_ascii_uppercase(),
                    position: [record.pos_x, record.pos_y, i32::try_from(record.pos_z)?],
                    rotation: record.rotation_matrix,
                });
            }
        }
    }
    let registry = registry::scan_dir(root).context("collision registry")?;
    let world_path =
        world_ini::find_world_ini(root).context("collision export: missing WORLD.INI")?;
    let worlds = world_ini::WorldIni::parse(&std::fs::read_to_string(&world_path)?);
    let entry = worlds
        .entries
        .iter()
        .find(|entry| entry.index == world)
        .with_context(|| format!("world {world}: missing WORLD.INI map"))?;
    let filename = entry.map.rsplit(['/', '\\']).next().unwrap_or(&entry.map);
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("world {world}: invalid map filename {}", entry.map))?;
    let archive_entry = registry
        .files
        .values()
        .filter(|entry| entry.file_type == FileType::Rob && entry.name.eq_ignore_ascii_case(stem))
        .min_by_key(|entry| registry.source_rank_key(&entry.path))
        .with_context(|| format!("world {world}: missing world archive {stem}.ROB"))?;
    let bytes = std::fs::read(&archive_entry.path)
        .with_context(|| archive_entry.path.display().to_string())?;
    let archive =
        rob::parse_rob_file(&bytes).with_context(|| archive_entry.path.display().to_string())?;
    let mut segments = BTreeMap::new();
    for segment in &archive.segments {
        segments
            .entry(segment.name().to_ascii_uppercase())
            .or_insert(segment);
    }
    for placement in &scene.placements {
        let name = &placement.model;
        if scene.models.contains_key(name) || scene.unsupported.contains_key(name) {
            continue;
        }
        let decoded = (|| {
            let segment = segments.get(name).with_context(|| {
                format!(
                    "{}: missing named model {name}",
                    archive_entry.path.display()
                )
            })?;
            let flags = segment._face_count_low[0];
            if segment.points_to_external_file() {
                let entry = registry
                    .files
                    .values()
                    .filter(|entry| {
                        entry.name.eq_ignore_ascii_case(name)
                            && entry.file_type == FileType::Model3dc
                    })
                    .min_by_key(|entry| registry.source_rank_key(&entry.path))
                    .with_context(|| format!("model {name}: missing external 3DC"))?;
                let bytes =
                    std::fs::read(&entry.path).with_context(|| entry.path.display().to_string())?;
                decode(flags, &bytes)
            } else {
                anyhow::ensure!(
                    segment.has_embedded_3d_data() || segment.has_special_embedded_data(),
                    "model {name}: unsupported ROB segment type {}",
                    segment.segment_type
                );
                decode(flags, &segment.data)
            }
        })();
        match decoded {
            Ok(model) => {
                scene.models.insert(name.clone(), model);
            }
            Err(error) => {
                scene
                    .unsupported
                    .insert(name.clone(), format!("world {world}: {error:#}"));
            }
        }
    }
    scene.validate()?;
    Ok(scene)
}

fn decode(flags: u8, bytes: &[u8]) -> anyhow::Result<CollisionModel> {
    let model = model3d::parse_3d_file(bytes)?;
    let subobjects = model3d::subobjects::parse_subobjects(bytes)?;
    let result = CollisionModel {
        version: Some(model.header.version),
        radius: model.header.radius as f32 / 256.0,
        flags,
        vertices: model
            .vertex_coords
            .iter()
            .map(|v| [v.x, v.y, v.z])
            .collect(),
        normals: model.face_normals.iter().map(|v| [v.x, v.y, v.z]).collect(),
        faces: model
            .face_data
            .iter()
            .map(|face| {
                face.face_vertices
                    .iter()
                    .map(|v| v.vertex_index as usize)
                    .collect()
            })
            .collect(),
        textures: Some(
            model
                .face_data
                .iter()
                .map(|face| match face.texture_data {
                    model3d::TextureData::SolidColor(_) => None,
                    model3d::TextureData::Texture {
                        texture_id,
                        image_id,
                    } => Some([texture_id, u16::from(image_id)]),
                })
                .collect(),
        ),
        subobjects: subobjects
            .into_iter()
            .map(|node| CollisionSubobject {
                center: node.center.map(|v| v as f32 / 256.0),
                radius: node.radius as f32 / 256.0,
                extents: node.extents,
                faces: node.faces.into_iter().map(usize::from).collect(),
            })
            .collect(),
    };
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_static_collision_retains_authored_inputs() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
        if !root.join("RGFX.EXE").is_file() {
            eprintln!("skipped: no install at {}", root.display());
            return;
        }
        let bytes = std::fs::read(root.join("maps/ISLAND.RGM")).unwrap();
        let map = rgm::parse_rgm_file(&bytes).unwrap();
        let scene = export(&root, &map, 1).unwrap();
        assert_eq!(scene.placements.len(), 1691);
        for name in ["PLANT000", "PLANT001", "PLANT002"] {
            assert!(
                scene.models.contains_key(name),
                "{name}: {:?}",
                scene.unsupported.get(name)
            );
            assert_eq!(scene.models[name].flags, 2, "{name}");
        }
        assert_eq!(scene.models["BARREL"].flags, 1);
        assert!(scene.unsupported.is_empty(), "{:?}", scene.unsupported);
        for (name, model) in &scene.models {
            let textures = model.textures.as_ref().expect("exported face textures");
            assert_eq!(textures.len(), model.faces.len(), "{name}");
        }
        assert!(
            scene
                .models
                .values()
                .any(|model| { model.textures.as_ref().unwrap().iter().any(Option::is_some) })
        );
        assert!(
            scene
                .models
                .values()
                .any(|model| !model.subobjects.is_empty())
        );
        let bytes = serde_json::to_vec(&scene).unwrap();
        let restored: CollisionScene = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.placements.len(), scene.placements.len());
        for (name, model) in &scene.models {
            assert_eq!(restored.models[name].textures, model.textures, "{name}");
        }
    }
}
