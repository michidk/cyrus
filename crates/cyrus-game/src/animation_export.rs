//! Native animation extraction through rgpre; no binary format parsing lives here.
use crate::animation::{AnimationModel, Animations};
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
        Some(FrameType::AnimatedI16) => "animated_i16",
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

fn archive_model(name: &str, registry: &Registry) -> anyhow::Result<AnimationModel> {
    let mut payload = None;
    for entry in registry
        .files
        .values()
        .filter(|entry| entry.file_type == FileType::Rob)
    {
        let bytes = std::fs::read(&entry.path).with_context(|| entry.path.display().to_string())?;
        let archive =
            rob::parse_rob_file(&bytes).with_context(|| entry.path.display().to_string())?;
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
    model_data(
        &model3d::parse_3d_file(&bytes)
            .with_context(|| format!("{}: model {name}", path.display()))?,
    )
}

pub(crate) fn export(
    rgm: &RgmFile,
    placed: &[PositionedModel],
    registry: &Registry,
    world: u32,
    out: &Path,
) -> anyhow::Result<()> {
    let metadata = rgm::export_rgm_metadata_json(rgm, None);
    let mut sections = BTreeMap::new();
    for section in &rgm.sections {
        let (tag, bytes) = match section {
            RgmSection::Rahd(_, bytes) => ("RAHD", bytes),
            RgmSection::Raan(_, bytes) => ("RAAN", bytes),
            RgmSection::Ragr(_, bytes) => ("RAGR", bytes),
            RgmSection::Ranm(_, bytes) => ("RANM", bytes),
            _ => continue,
        };
        anyhow::ensure!(
            sections.insert(tag.into(), bytes.clone()).is_none(),
            "world {world}: duplicate animation section {tag}"
        );
    }
    let mut names = BTreeSet::new();
    for actor in metadata["actors"]
        .as_array()
        .context("rgpre metadata: missing actors")?
    {
        if let Some(entries) = actor["raan_entries"].as_array() {
            for entry in entries {
                let path = entry["file_path"]
                    .as_str()
                    .context("RAAN file_path missing")?;
                let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
                let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
                names.insert(stem.to_ascii_uppercase());
            }
        }
    }
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
    for name in names {
        let decoded = if let Some(model) = placed
            .iter()
            .find(|p| p.model_name.eq_ignore_ascii_case(&name))
        {
            model_data(&model.model)
        } else {
            let entry = registry
                .files
                .values()
                .filter(|entry| {
                    entry.name.eq_ignore_ascii_case(&name)
                        && matches!(entry.file_type, FileType::Model3d | FileType::Model3dc)
                })
                .min_by_key(|entry| registry.source_rank_key(&entry.path));
            if let Some(entry) = entry {
                (|| {
                    let bytes = std::fs::read(&entry.path)
                        .with_context(|| entry.path.display().to_string())?;
                    let model = model3d::parse_3d_file(&bytes)
                        .with_context(|| entry.path.display().to_string())?;
                    model_data(&model)
                })()
            } else {
                archive_model(&name, registry)
            }
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
    let temporary = out.with_extension("animations.tmp");
    std::fs::write(&temporary, serde_json::to_vec(&result)?)?;
    std::fs::rename(&temporary, out.with_extension("animations"))?;
    Ok(())
}
