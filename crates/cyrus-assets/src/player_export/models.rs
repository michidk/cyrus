//! Textured actor frame-source models, prepared for the shared GLB loader.
use anyhow::Context;
use rgpre::{
    gltf::{TextureCache, convert_positioned_models_to_gltf, to_glb},
    import::{palette::Palette, registry::Registry, rgm},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

// Independent export inputs, as in the other preparation entry points.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    map: &rgm::RgmFile,
    placed: &[rgm::PositionedModel],
    registry: &Registry,
    palette: &Palette,
    textures: &mut TextureCache,
    out: &Path,
    output: &mut dyn crate::preparation::Output,
    weapons: &crate::animation_export::WeaponModels,
) -> anyhow::Result<BTreeMap<String, f32>> {
    let metadata = rgm::export_rgm_metadata_json(map, None);
    let actors = metadata["actors"]
        .as_array()
        .context("actor models: missing actors")?;
    let mut names = BTreeSet::new();
    let mut player_names = BTreeSet::new();
    for actor in actors {
        let actor_name = actor["script_name"].as_str().unwrap_or("unnamed actor");
        let Some(entries) = actor["raan_entries"].as_array() else {
            anyhow::ensure!(actor_name != "CYRUS", "CYRUS: missing frame sources");
            continue;
        };
        let texture_set = crate::animation::actor_texture_set(actor)?;
        for entry in entries {
            let path = entry["file_path"]
                .as_str()
                .with_context(|| format!("{actor_name}: missing frame-source path"))?;
            let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
            let name = filename.rsplit_once('.').map_or(filename, |(name, _)| name);
            anyhow::ensure!(
                !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "{actor_name}: invalid frame-source name {name:?}"
            );
            let name = name.to_ascii_uppercase();
            if actor_name == "CYRUS" {
                player_names.insert(name.clone());
            }
            names.insert((name, texture_set));
        }
    }
    // Weapon items draw with their own textures, like an actor frame source without a skin.
    names.extend(weapons.names.iter().map(|name| (name.clone(), None)));
    let directory = out.with_extension("player-models");
    let mut radii = BTreeMap::new();
    for (name, texture_set) in names {
        let model = if weapons.names.contains(&name) {
            std::borrow::Cow::Owned(
                crate::animation_export::archive_model(&name, registry, weapons.archive.as_deref())
                    .with_context(|| format!("weapon model {name}"))?,
            )
        } else {
            crate::animation_export::resolve_model(&name, placed, registry)
                .with_context(|| format!("actor model {name}"))?
        };
        if player_names.contains(&name) {
            radii.insert(name.clone(), model.header.radius as f32 / 256.0);
        }
        let mut model = model.into_owned();
        if let Some(set) = texture_set {
            rgm::apply_texture_override(&mut model, set);
        }
        let model = rgm::PositionedModel {
            model,
            transform: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            model_name: name.clone(),
            source_id: None,
            render_attributes: None,
            flat: None,
        };
        let (root, buffer) =
            convert_positioned_models_to_gltf(&[model], &[], Some(palette), Some(textures), false)
                .with_context(|| {
                    format!("actor model {name} texture set {texture_set:?}: GLB export")
                })?;
        output.publish(
            &directory.join(crate::animation::frame_source_file(&name, texture_set)),
            &to_glb(&root, &buffer)?,
        )?;
    }
    Ok(radii)
}
