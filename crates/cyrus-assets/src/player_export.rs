//! Shared preparation of player inputs through preservation's parsers.
use crate::player::{MovementSettings, SceneData};
use anyhow::Context;
use rgpre::import::{player_ini::PlayerConfig, rgm, wld};
use std::path::Path;

mod collision;
pub(crate) mod models;

pub(crate) fn prepare(
    source: &dyn crate::source::ReadAssets,
    geometry: &crate::runtime::geometry::WorldGeometry,
    model_radii: &std::collections::BTreeMap<String, f32>,
    out: &Path,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    let registry = &geometry.registry;
    let map = &geometry.rgm;
    let terrain = geometry.terrain.as_deref();
    let world = geometry.entry.index;
    let system_text = crate::preparation::text(source, "SYSTEM.INI")?;
    let system = rgpre::import::system_ini::SystemIni::parse(&system_text)
        .with_context(|| format!("world {world}: parse SYSTEM.INI slope settings"))?;
    let config = PlayerConfig::parse(&system_text)
        .with_context(|| format!("world {world}: parse SYSTEM.INI"))?;
    let settings = MovementSettings {
        standing_height: config.standing_height,
        post_collide_height: config.post_collide_height,
        slide_range: config.slide_range,
        slide_speed: system.game.slide_speed,
        turn_speed: config.turn_speed,
        turn_max_speed: config.turn_max_speed,
        poly_push_units: config.poly_push_units,
    };
    let names = crate::preparation::soup(source).ok();
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
    let keyboard_codes =
        rgpre::import::keys_ini::KeysIni::parse(&crate::preparation::text(source, "KEYS.INI")?)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("world {world}: parse KEYS.INI"))?
            .keyboard_codes();
    let surfaces = rgpre::import::surface_ini::SurfaceAssignments::parse(
        &crate::preparation::text(source, "SURFACE.INI")?,
    )
    .map_err(anyhow::Error::msg)
    .with_context(|| format!("world {world}: parse SURFACE.INI"))?;
    let mut collision = collision::export(source, registry, map, world)
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
    output
        .publish(&path, &bytes)
        .with_context(|| format!("publish {}", path.display()))?;
    Ok(())
}
