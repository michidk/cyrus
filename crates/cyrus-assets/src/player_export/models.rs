//! Textured player frame-source models, prepared for the shared GLB loader.
use anyhow::Context;
use rgpre::{
    gltf::{TextureCache, convert_positioned_models_to_gltf, to_glb},
    import::{FileType, model3d, palette::Palette, registry::Registry, rgm},
};
use std::{collections::BTreeSet, path::Path};

pub(crate) fn export(
    map: &rgm::RgmFile,
    registry: &Registry,
    palette: &Palette,
    textures: &mut TextureCache,
    out: &Path,
) -> anyhow::Result<()> {
    let metadata = rgm::export_rgm_metadata_json(map, None);
    let actors = metadata["actors"]
        .as_array()
        .context("player models: missing actors")?;
    let Some(actor) = actors.iter().find(|actor| actor["script_name"] == "CYRUS") else {
        return Ok(());
    };
    let entries = actor["raan_entries"]
        .as_array()
        .context("CYRUS: missing frame sources")?;
    let mut names = BTreeSet::new();
    for entry in entries {
        let path = entry["file_path"]
            .as_str()
            .context("CYRUS: missing frame-source path")?;
        let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let name = filename.rsplit_once('.').map_or(filename, |(name, _)| name);
        anyhow::ensure!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "CYRUS: invalid frame-source name {name:?}"
        );
        names.insert(name.to_ascii_uppercase());
    }
    let directory = out.with_extension("player-models");
    std::fs::create_dir_all(&directory)?;
    for name in names {
        let entry = registry
            .files
            .values()
            .filter(|entry| {
                entry.name.eq_ignore_ascii_case(&name)
                    && matches!(entry.file_type, FileType::Model3d | FileType::Model3dc)
            })
            .min_by_key(|entry| registry.source_rank_key(&entry.path))
            .with_context(|| format!("CYRUS model {name}: standalone frame source unavailable"))?;
        let model = model3d::parse_3d_file(&std::fs::read(&entry.path)?)
            .with_context(|| format!("CYRUS model {name}: {}", entry.path.display()))?;
        let model = rgm::PositionedModel {
            model,
            transform: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            model_name: name.clone(),
            source_id: None,
        };
        let (root, buffer) =
            convert_positioned_models_to_gltf(&[model], &[], Some(palette), Some(textures), false)
                .with_context(|| format!("CYRUS model {name}: GLB export"))?;
        let path = directory.join(format!("{name}.glb"));
        let temporary = path.with_extension("glb.tmp");
        std::fs::write(&temporary, to_glb(&root, &buffer)?)?;
        std::fs::rename(&temporary, &path)?;
    }
    Ok(())
}
