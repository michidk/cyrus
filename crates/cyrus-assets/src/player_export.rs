//! Native extraction of player inputs through preservation's parsers.
use crate::player::{MovementSettings, SceneData};
use anyhow::Context;
use rgpre::import::{player_ini::PlayerConfig, rgm, soup_def, wld};
use std::path::Path;

mod collision;
pub(crate) mod models;

pub(crate) fn export(
    root: &Path,
    map: &rgm::RgmFile,
    terrain: Option<&wld::WldFile>,
    world: u32,
    model_radii: &std::collections::BTreeMap<String, f32>,
    out: &Path,
) -> anyhow::Result<()> {
    let path =
        crate::scene::find_file(root, "SYSTEM.INI").context("player export: missing SYSTEM.INI")?;
    let source = std::fs::read_to_string(&path)
        .with_context(|| format!("world {world}: read {}", path.display()))?;
    let config = PlayerConfig::parse(&source)
        .with_context(|| format!("world {world}: parse {}", path.display()))?;
    let settings = MovementSettings {
        standing_height: config.standing_height,
        post_collide_height: config.post_collide_height,
        slide_range: config.slide_range,
        turn_speed: config.turn_speed,
        turn_max_speed: config.turn_max_speed,
        poly_push_units: config.poly_push_units,
    };
    let names = soup_def::try_load_soup_def(root);
    let mut metadata = rgm::export_rgm_metadata_json(map, names.as_ref());
    let actors = metadata["actors"]
        .as_array()
        .context("player export: missing actor table")?;
    let actor = actors
        .iter()
        .find(|actor| actor["script_name"].as_str() == Some("CYRUS"));
    let actors: Vec<_> = actor.into_iter().cloned().collect();
    let definition = serde_json::json!({
        "actors": actors,
        "mpsz_entries": metadata["mpsz_entries"].take(),
    });
    let (heights, water, terrain_textures) = if let Some(terrain) = terrain {
        let heights = terrain
            .combined_map(0)?
            .into_iter()
            .map(wld::decode_height)
            .collect();
        let textures = terrain.combined_map(2)?;
        anyhow::ensure!(
            textures.len() == 256 * 256,
            "world {world}: invalid player terrain grid"
        );
        (
            Some(heights),
            Some(crate::environment::water_vertices(&textures, 256)),
            Some(textures),
        )
    } else {
        (None, None, None)
    };
    let keys_path =
        crate::scene::find_file(root, "KEYS.INI").context("player export: missing KEYS.INI")?;
    let keys_text = std::fs::read_to_string(&keys_path)
        .with_context(|| format!("world {world}: read {}", keys_path.display()))?;
    let keyboard_codes = rgpre::import::keys_ini::KeysIni::parse(&keys_text)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("world {world}: parse {}", keys_path.display()))?
        .keyboard_codes();
    let surface_path = crate::scene::find_file(root, "SURFACE.INI")
        .context("player export: missing SURFACE.INI")?;
    let surfaces = rgpre::import::surface_ini::SurfaceAssignments::parse(
        &std::fs::read_to_string(&surface_path)
            .with_context(|| format!("world {world}: read {}", surface_path.display()))?,
    )
    .map_err(anyhow::Error::msg)
    .with_context(|| format!("world {world}: parse {}", surface_path.display()))?;
    let mut collision = collision::export(root, map, world)
        .with_context(|| format!("world {world}: collision export"))?;
    collision.texture_surfaces = Some(
        (0..=u16::MAX)
            .filter_map(|key| {
                let surface = surfaces.surface(key);
                (surface != 0).then_some((key, surface))
            })
            .collect(),
    );
    let data = SceneData {
        version: 2,
        model_radii: model_radii.clone(),
        keyboard_codes: Some(keyboard_codes),
        world,
        settings,
        definition,
        heights,
        water,
        terrain_textures,
        collision: Some(collision),
    };
    let bytes = serde_json::to_vec(&data)
        .with_context(|| format!("world {world}: serialize player inputs"))?;
    data.validate()?;
    let path = out.with_extension("player");
    crate::native::publish(&path, &bytes).with_context(|| format!("publish {}", path.display()))?;
    Ok(())
}
