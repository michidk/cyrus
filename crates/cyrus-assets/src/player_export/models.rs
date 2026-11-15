//! Textured actor frame-source models, prepared for the shared GLB loader.
use anyhow::Context;
use rgpre::{
    gltf::{TextureCache, convert_positioned_models_to_gltf, to_glb},
    import::{FileType, palette::Palette, registry::Registry, rgm, rob::RobIndex},
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
    let mut nonzero_archive_flags = BTreeSet::new();
    for entry in registry
        .files
        .values()
        .filter(|entry| entry.file_type == FileType::Rob)
    {
        let bytes = entry.data.as_deref().with_context(|| {
            format!(
                "actor draw flags: archive bytes absent: {}",
                entry.path.display()
            )
        })?;
        let archive = RobIndex::read(&mut std::io::Cursor::new(bytes))
            .with_context(|| format!("actor draw flags: {}", entry.path.display()))?;
        nonzero_archive_flags.extend(
            archive
                .segments
                .iter()
                .filter(|segment| segment.segment_attribs != 0)
                .map(|segment| segment.name().to_ascii_uppercase()),
        );
    }
    let mut names = BTreeSet::new();
    let mut initialized = BTreeMap::<(String, Option<u16>), bool>::new();
    let mut player_names = BTreeSet::new();
    for actor in actors {
        let actor_name = actor["script_name"].as_str().unwrap_or("unnamed actor");
        let Some(entries) = actor["raan_entries"].as_array() else {
            anyhow::ensure!(actor_name != "CYRUS", "CYRUS: missing frame sources");
            continue;
        };
        let texture_set = crate::animation::actor_texture_set(actor)?;
        let retains_initialized_flags = retains_initialized_flags(actor, &nonzero_archive_flags)?;
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
            initialized
                .entry((name.clone(), texture_set))
                .and_modify(|known| *known &= retains_initialized_flags)
                .or_insert(retains_initialized_flags);
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
        let (mut root, buffer) =
            convert_positioned_models_to_gltf(&[model], &[], Some(palette), Some(textures), false)
                .with_context(|| {
                    format!("actor model {name} texture set {texture_set:?}: GLB export")
                })?;
        if !weapons.names.contains(&name)
            && initialized.get(&(name.clone(), texture_set)) == Some(&true)
        {
            for node in &mut root.nodes {
                if node.name.as_deref() == Some(name.as_str()) {
                    let mut extras: serde_json::Value = serde_json::from_str(
                        node.extras
                            .as_ref()
                            .context("actor render node extras absent")?
                            .get(),
                    )?;
                    extras["cyrus_actor_render"] =
                        serde_json::json!({"version": 1, "draw_flags": 0});
                    node.extras = Some(serde_json::value::to_raw_value(&extras)?);
                }
            }
        }
        output.publish(
            &directory.join(crate::animation::frame_source_file(&name, texture_set)),
            &to_glb(&root, &buffer)?,
        )?;
    }
    Ok(radii)
}

/// Resolve zero flags only when initialization and every possible first-model ROB match agree.
/// A nonzero archive match needs authoritative archive selection, rather than a guessed default.
/// https://github.com/michidk/redguard-preservation/blob/main/docs/engine/model-lighting.md#draw-flag-ownership
fn retains_initialized_flags(
    actor: &serde_json::Value,
    nonzero_archive_flags: &BTreeSet<String>,
) -> anyhow::Result<bool> {
    if actor["model_load_path"] != "animation_table" {
        return Ok(false);
    }
    let Some(first) = actor["raan_entries"]
        .as_array()
        .and_then(|entries| entries.first())
    else {
        return Ok(false);
    };
    let path = first["file_path"].as_str().with_context(|| {
        format!(
            "actor {}: first frame-source path absent",
            actor["script_name"]
        )
    })?;
    Ok(!nonzero_archive_flags.contains(&crate::animation::model_key(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_actor_flags_require_all_possible_first_archive_matches_to_agree() {
        let mut actor = serde_json::json!({
            "model_load_path": "animation_table",
            "raan_entries": [{"file_path": "3dart/first.3d"}, {"file_path": "3dart/later.3d"}]
        });
        let mut archives = BTreeSet::from(["LATER".into()]);
        assert!(retains_initialized_flags(&actor, &archives).unwrap());
        archives.insert("FIRST".into());
        assert!(!retains_initialized_flags(&actor, &archives).unwrap());
        actor["model_load_path"] = serde_json::json!("static");
        assert!(!retains_initialized_flags(&actor, &BTreeSet::new()).unwrap());
    }
}
