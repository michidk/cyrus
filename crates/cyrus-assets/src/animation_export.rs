//! Shared animation preparation through rgpre; no binary format parsing lives here.
use crate::animation::{AnimationModel, Animations, model_key};
use anyhow::Context;
use rgpre::import::{
    FileType,
    model3d::{self, FrameType, Model3DFile},
    registry::Registry,
    rgm::{self, PositionedModel, RgmFile, RgmSection},
    rob,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn model_data(model: &Model3DFile) -> anyhow::Result<AnimationModel> {
    let encoding = match model.frame_data.first().map(|frame| frame.frame_type) {
        Some(FrameType::Static3D) => "static_3d",
        Some(FrameType::Static3DC) => "static_3dc",
        Some(FrameType::AnimatedI16 | FrameType::AnimatedI16Unscaled) => "animated_i16_absolute",
        Some(FrameType::AnimatedI32) => "animated_i32",
        other => anyhow::bail!("unsupported frame encoding {other:?}"),
    };
    anyhow::ensure!(
        model.frame_vertex_data.len() == model.header.num_frames as usize,
        "incomplete vertex frame table"
    );
    anyhow::ensure!(
        model.frame_normal_data.len() == model.header.num_frames as usize,
        "incomplete normal frame table"
    );
    Ok(AnimationModel {
        radius: Some(model.header.radius as f32 / 256.0),
        base_positions: model
            .vertex_coords
            .iter()
            .map(|v| [v.x, v.y, v.z])
            .collect(),
        base_normals: model.face_normals.iter().map(|v| [v.x, v.y, v.z]).collect(),
        faces: model
            .face_data
            .iter()
            .map(|face| face.face_vertices.iter().map(|v| v.vertex_index).collect())
            .collect(),
        corner_normals: corner_normals(model),
        encoding: encoding.into(),
        positions: model
            .frame_vertex_data
            .iter()
            .map(|frame| frame.coords.iter().map(|v| [v.x, v.y, v.z]).collect())
            .collect(),
        normals: model
            .frame_normal_data
            .iter()
            .map(|frame| frame.face_normals.iter().map(|v| [v.x, v.y, v.z]).collect())
            .collect(),
    })
}

/// Mirrors rgpre's shading selection: the indirection table when present, otherwise
/// direct vertex lookup; zero, subnormal or non-finite normals fall back to the face.
fn corner_normals(model: &Model3DFile) -> Vec<Vec<Option<[f32; 3]>>> {
    let mut corner = 0;
    model
        .face_data
        .iter()
        .map(|face| {
            face.face_vertices
                .iter()
                .map(|vertex| {
                    let index = if model.normal_indices.is_empty() {
                        usize::try_from(vertex.vertex_index).ok()
                    } else {
                        model
                            .normal_indices
                            .get(corner)
                            .and_then(|&index| usize::try_from(index).ok())
                    };
                    corner += 1;
                    index
                        .and_then(|index| model.vertex_normals.get(index))
                        .filter(|n| (n.x * n.x + n.y * n.y + n.z * n.z).is_normal())
                        .map(|n| [n.x, n.y, n.z])
                })
                .collect()
        })
        .collect()
}

fn archive_model(
    name: &str,
    registry: &Registry,
    selected_archive: Option<&str>,
) -> anyhow::Result<Model3DFile> {
    let mut payload = None;
    for entry in registry
        .files
        .values()
        .filter(|entry| entry.file_type == FileType::Rob)
        .filter(|entry| {
            selected_archive.is_none_or(|path| {
                entry
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .eq_ignore_ascii_case(path)
            })
        })
    {
        let bytes = entry
            .data
            .as_ref()
            .with_context(|| format!("model bytes absent: {}", entry.path.display()))
            .with_context(|| entry.path.display().to_string())?;
        let archive =
            rob::parse_rob_file(bytes).with_context(|| entry.path.display().to_string())?;
        for segment in archive
            .segments
            .iter()
            .filter(|segment| segment.name().eq_ignore_ascii_case(name))
        {
            anyhow::ensure!(
                !segment.points_to_external_file(),
                "{}: model {name} references a missing standalone 3DC",
                entry.path.display()
            );
            anyhow::ensure!(
                segment.has_embedded_3d_data() || segment.has_special_embedded_data(),
                "{}: model {name} has unsupported archive segment type {}",
                entry.path.display(),
                segment.segment_type
            );
            if let Some((_, previous)) = &payload {
                anyhow::ensure!(
                    previous == &segment.data,
                    "model {name}: conflicting archive payloads; archive selection unsupported"
                );
            } else {
                payload = Some((&entry.path, segment.data.clone()));
            }
        }
    }
    let (path, bytes) = payload
        .with_context(|| format!("model {name}: no standalone, placed or embedded model"))?;
    model3d::parse_3d_file(&bytes).with_context(|| format!("{}: model {name}", path.display()))
}

/// Use the same installed geometry for frame tables and textured replacement meshes.
pub(crate) fn resolve_model<'a>(
    name: &str,
    placed: &'a [PositionedModel],
    registry: &Registry,
) -> anyhow::Result<std::borrow::Cow<'a, Model3DFile>> {
    if let Some(model) = placed
        .iter()
        .find(|model| model.model_name.eq_ignore_ascii_case(name))
    {
        return Ok(std::borrow::Cow::Borrowed(&model.model));
    }
    let entry = registry
        .files
        .values()
        .filter(|entry| {
            entry.name.eq_ignore_ascii_case(name)
                && matches!(entry.file_type, FileType::Model3d | FileType::Model3dc)
        })
        .min_by_key(|entry| registry.source_rank_key(&entry.path));
    let model = if let Some(entry) = entry {
        let bytes = entry
            .data
            .as_ref()
            .with_context(|| format!("model bytes absent: {}", entry.path.display()))?;
        model3d::parse_3d_file(bytes).with_context(|| entry.path.display().to_string())?
    } else {
        archive_model(name, registry, None)?
    };
    Ok(std::borrow::Cow::Owned(model))
}

pub(crate) fn prepare(
    rgm: &RgmFile,
    placed: &[PositionedModel],
    registry: &Registry,
    world: u32,
    source: &dyn crate::source::ReadAssets,
    out: &Path,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    let mut metadata = rgm::export_rgm_metadata_json(rgm, None);
    let scripts = crate::script_export::prepare_source(rgm, source, world, Some(registry))?;
    metadata["script_content_id"] = serde_json::json!(scripts.content_id);
    metadata["animation_scripts"] = serde_json::to_value(
        scripts
            .actors
            .iter()
            .map(|(name, actor)| (name, &actor.program))
            .collect::<BTreeMap<_, _>>(),
    )?;
    output.publish(
        &out.with_extension("scripts"),
        &serde_json::to_vec(&scripts)?,
    )?;
    let mut sections = BTreeMap::new();
    for section in &rgm.sections {
        let (tag, bytes) = match section {
            RgmSection::Rahd(_, bytes) => ("RAHD", bytes),
            RgmSection::Raan(_, bytes) => ("RAAN", bytes),
            RgmSection::Ragr(_, bytes) => ("RAGR", bytes),
            RgmSection::Ranm(_, bytes) => ("RANM", bytes),
            RgmSection::Rasc(_, bytes) => ("RASC", bytes),
            RgmSection::Rast(_, bytes) => ("RAST", bytes),
            RgmSection::Rasb(_, bytes) => ("RASB", bytes),
            RgmSection::Rava(_, bytes) => ("RAVA", bytes),
            _ => continue,
        };
        anyhow::ensure!(
            sections.insert(tag.into(), bytes.clone()).is_none(),
            "world {world}: duplicate animation section {tag}"
        );
    }
    let mut names = BTreeSet::new();
    for weapon in scripts.weapons.values() {
        names.insert(weapon.hand_model.clone());
        names.insert(weapon.hilt_model.clone());
    }
    for actor in metadata["actors"]
        .as_array()
        .context("rgpre metadata: missing actors")?
    {
        if let Some(entries) = actor["raan_entries"].as_array() {
            for entry in entries {
                let path = entry["file_path"]
                    .as_str()
                    .context("RAAN file_path missing")?;
                names.insert(model_key(path));
            }
        }
    }
    names.extend(
        rgm.sections
            .iter()
            .filter_map(|section| match section {
                RgmSection::MpobParsed(_, records) => Some(records),
                _ => None,
            })
            .flatten()
            .filter(|record| {
                record.object_type == 1
                    && record.is_static != 1
                    && metadata["actors"].as_array().is_some_and(|actors| {
                        actors.iter().any(|actor| {
                            actor["script_name"].as_str() == Some(record.script_name().as_str())
                                && actor["raan_entries"]
                                    .as_array()
                                    .is_none_or(|entries| entries.is_empty())
                        })
                    })
            })
            .map(|record| model_key(&record.model_name()))
            .filter(|name| !name.is_empty()),
    );
    names.extend(
        placed
            .iter()
            .filter(|p| p.model.header.num_frames > 1)
            .map(|p| p.model_name.to_ascii_uppercase()),
    );
    let mut result = Animations {
        version: 2,
        world,
        metadata,
        sections,
        models: BTreeMap::new(),
        unsupported: BTreeMap::new(),
    };
    let weapon_names: BTreeSet<_> = scripts
        .weapons
        .values()
        .flat_map(|weapon| [&weapon.hand_model, &weapon.hilt_model])
        .collect();
    let weapon_archive = if weapon_names.is_empty() {
        None
    } else {
        Some(crate::script::weapon::world_archive_path(source, world)?)
    };
    for name in names {
        let decoded = if weapon_names.contains(&name) {
            archive_model(&name, registry, weapon_archive.as_deref())
                .and_then(|model| model_data(&model))
        } else {
            resolve_model(&name, placed, registry).and_then(|model| model_data(&model))
        };
        match decoded {
            Ok(model) => {
                result.models.insert(name, model);
            }
            Err(error) => {
                result
                    .unsupported
                    .insert(name, format!("world {world}: {error:#}"));
            }
        }
    }
    result.validate()?;
    if !result.unsupported.is_empty() {
        log::warn!(
            "world {world}: {} unsupported animation models; details in {}",
            result.unsupported.len(),
            out.with_extension("animations").display()
        );
    }
    output.publish(
        &out.with_extension("animations"),
        &serde_json::to_vec(&result)?,
    )?;
    Ok(())
}

#[cfg(all(test, feature = "native", not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn installed_cyrus_compressed_frames_export_absolute_geometry() {
        let root = std::env::var_os(crate::install::ENV_VAR)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        if !root.join("RGFX.EXE").is_file() {
            eprintln!("skipped: no install for compressed Cyrus frame export");
            return;
        }
        let bytes = std::fs::read(root.join("fxart/CYRSA001.3DC")).unwrap();
        let parsed = rgpre::import::model3d::parse_3d_file(&bytes).unwrap();
        let model = super::model_data(&parsed).unwrap();
        assert_eq!(model.encoding, "animated_i16_absolute");
        for (frame, expected) in [
            (1, [11.996_094, -0.488_281_25, 3.226_562_5]),
            (2, [12.382_812_5, -0.265_625, 2.425_781_3]),
            (10, [12.214_844, -0.628_906_25, 3.492_187_5]),
        ] {
            assert_eq!(model.positions[frame][0], expected);
        }
    }
}
