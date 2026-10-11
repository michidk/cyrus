//! Installed static placements and authored collision geometry for the portable player cache.
use super::super::collision_data::{
    CollisionModel, CollisionScene, CollisionSubobject, StaticPlacement,
};
use anyhow::Context;
use rgpre::import::{FileType, model3d, registry, rgm, rob};
use std::{collections::BTreeMap, path::Path};

pub(super) fn export(root: &Path, map: &rgm::RgmFile) -> anyhow::Result<CollisionScene> {
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
    let mut sources: BTreeMap<String, anyhow::Result<(u8, Vec<u8>)>> = scene
        .placements
        .iter()
        .map(|placement| {
            (
                placement.model.clone(),
                Err(anyhow::anyhow!("missing named ROB segment")),
            )
        })
        .collect();
    for entry in registry
        .files
        .values()
        .filter(|entry| entry.file_type == FileType::Rob)
    {
        let bytes = std::fs::read(&entry.path).with_context(|| entry.path.display().to_string())?;
        let archive =
            rob::parse_rob_file(&bytes).with_context(|| entry.path.display().to_string())?;
        for segment in archive.segments {
            let name = segment.name().to_ascii_uppercase();
            let Some(source) = sources.get_mut(&name) else {
                continue;
            };
            if scene.unsupported.contains_key(&name) {
                continue;
            }
            let flags = segment._face_count_low[0];
            let payload = if segment.points_to_external_file() {
                let entry = registry
                    .files
                    .values()
                    .filter(|entry| {
                        entry.name.eq_ignore_ascii_case(&name)
                            && matches!(entry.file_type, FileType::Model3d | FileType::Model3dc)
                    })
                    .min_by_key(|entry| registry.source_rank_key(&entry.path));
                entry
                    .with_context(|| format!("model {name}: missing external model"))
                    .and_then(|entry| {
                        std::fs::read(&entry.path).with_context(|| entry.path.display().to_string())
                    })
            } else if segment.has_embedded_3d_data() || segment.has_special_embedded_data() {
                Ok(segment.data)
            } else {
                Err(anyhow::anyhow!(
                    "unsupported ROB segment type {}",
                    segment.segment_type
                ))
            };
            match payload {
                Ok(bytes) => {
                    if let Ok((old_flags, old_bytes)) = source
                        && (*old_flags != flags || *old_bytes != bytes)
                    {
                        scene.unsupported.insert(name, "conflicting ROB geometry or collision flags; archive selection unsupported".into());
                        continue;
                    }
                    *source = Ok((flags, bytes));
                }
                Err(error) => {
                    scene.unsupported.insert(name, format!("{error:#}"));
                }
            }
        }
    }
    for (name, source) in sources {
        if scene.unsupported.contains_key(&name) {
            continue;
        }
        match source.and_then(|(flags, bytes)| decode(flags, &bytes)) {
            Ok(model) => {
                scene.models.insert(name, model);
            }
            Err(error) => {
                scene.unsupported.insert(name, format!("{error:#}"));
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
    result.polygons()?;
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
        let scene = export(&root, &map).unwrap();
        assert_eq!(scene.placements.len(), 1691);
        for name in ["PLANT000", "PLANT001", "PLANT002"] {
            assert!(
                scene.models.contains_key(name),
                "{name}: {:?}",
                scene.unsupported.get(name)
            );
            assert_eq!(scene.models[name].flags, 2, "{name}");
        }
        assert!(scene.unsupported["BARREL"].contains("conflicting ROB"));
        assert!(!scene.models.contains_key("BARREL"));
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
    }
}
