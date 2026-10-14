//! Initial live static geometry from actor loader paths and authored MPOB fallback.
use crate::animation::model_key;
use crate::collision::{ActorCollisionModels, CollisionGeometry};
use anyhow::{Context, Result, ensure};
use rgpre::import::{FileType, registry::Registry, rgm, rob};

pub(super) fn prepare(
    map: &rgm::RgmFile,
    registry: &Registry,
    archive: &rob::RobFile,
    world: u32,
) -> Result<ActorCollisionModels> {
    let metadata = rgm::export_rgm_metadata_json(map, None);
    let objects = metadata["mpob_objects"]
        .as_array()
        .context("missing MPOB metadata")?;
    let actors = metadata["actors"]
        .as_array()
        .context("missing RAHD metadata")?;
    let mut result = ActorCollisionModels {
        object_count: objects.len(),
        ..Default::default()
    };
    for (index, object) in objects.iter().enumerate() {
        if object["object_type"] != 1 {
            continue;
        }
        let decoded = (|| -> Result<Option<(String, CollisionGeometry)>> {
            let script = object["script_name"]
                .as_str()
                .context("missing script name")?;
            let actor = actors
                .iter()
                .find(|actor| {
                    actor["script_name"]
                        .as_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case(script))
                })
                .with_context(|| format!("actor {script}: missing model-path definition"))?;
            let path = match actor["model_load_path"].as_str() {
                Some("static") => actor["raan_entries"]
                    .as_array()
                    .and_then(|entries| entries.first())
                    .and_then(|entry| entry["file_path"].as_str())
                    .context("static actor has no RAAN model")?,
                Some("none") => {
                    let name = object["model_name"]
                        .as_str()
                        .context("missing MPOB model name")?;
                    if name.is_empty() || object["is_static"] != 1 {
                        return Ok(None);
                    }
                    name
                }
                Some("animation_table") => return Ok(None),
                other => anyhow::bail!("actor {script}: unsupported loaded model path {other:?}"),
            };
            let name = model_key(path);
            ensure!(!name.is_empty(), "empty live model name {path:?}");
            // Geometry is shared by model name only after source selection. Mutable
            // model instances and actor collision flags remain game-owned.
            let geometry = load(path, &name, registry, archive)?;
            Ok(Some((name, geometry)))
        })();
        match decoded {
            Ok(Some((name, geometry))) => {
                if let Some(previous) = result.models.get(&name) {
                    ensure!(
                        serde_json::to_vec(previous)? == serde_json::to_vec(&geometry)?,
                        "world {world} MPOB {index} model {name}: conflicting selected geometry"
                    );
                } else {
                    result.models.insert(name.clone(), geometry);
                }
                result.instances.insert(index, name);
            }
            Ok(None) => {}
            Err(error) => {
                result
                    .unsupported
                    .insert(index, format!("world {world} MPOB {index}: {error:#}"));
            }
        }
    }
    result.validate()?;
    Ok(result)
}

fn load(
    path: &str,
    name: &str,
    registry: &Registry,
    archive: &rob::RobFile,
) -> Result<CollisionGeometry> {
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let extension = filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_uppercase());
    let selected_type = match extension.as_deref() {
        Some("3D") => Some(FileType::Model3d),
        Some("3DC") => Some(FileType::Model3dc),
        _ => None,
    };
    let mut files: Vec<_> = registry
        .files
        .values()
        .filter(|entry| {
            entry.name.eq_ignore_ascii_case(name)
                && matches!(entry.file_type, FileType::Model3d | FileType::Model3dc)
                && selected_type.is_none_or(|kind| kind == entry.file_type)
        })
        .collect();
    files.sort_by_key(|entry| registry.source_rank_key(&entry.path));
    if let Some(first) = files.first() {
        let bytes = first
            .data
            .as_deref()
            .with_context(|| format!("{}: model bytes absent", first.path.display()))?;
        for entry in files.iter().skip(1) {
            ensure!(
                entry.data.as_deref() == Some(bytes),
                "model {path}: ambiguous standalone payloads; source selection unsupported"
            );
        }
        return super::decode_geometry(bytes).with_context(|| first.path.display().to_string());
    }
    let segment = archive
        .segments
        .iter()
        .find(|segment| segment.name().eq_ignore_ascii_case(name))
        .with_context(|| format!("model {path}: absent from standalone files and world archive"))?;
    if segment.points_to_external_file() {
        let mut external: Vec<_> = registry
            .files
            .values()
            .filter(|entry| {
                entry.name.eq_ignore_ascii_case(name) && entry.file_type == FileType::Model3dc
            })
            .collect();
        external.sort_by_key(|entry| registry.source_rank_key(&entry.path));
        let entry = external
            .first()
            .with_context(|| format!("model {path}: missing external 3DC"))?;
        let bytes = entry
            .data
            .as_deref()
            .with_context(|| format!("{}: model bytes absent", entry.path.display()))?;
        for other in external.iter().skip(1) {
            ensure!(
                other.data.as_deref() == Some(bytes),
                "model {path}: ambiguous external 3DC payloads"
            );
        }
        super::decode_geometry(bytes).with_context(|| entry.path.display().to_string())
    } else {
        ensure!(
            segment.has_embedded_3d_data() || segment.has_special_embedded_data(),
            "model {path}: unsupported archive segment type {}",
            segment.segment_type
        );
        super::decode_geometry(&segment.data).with_context(|| format!("world archive model {path}"))
    }
}
