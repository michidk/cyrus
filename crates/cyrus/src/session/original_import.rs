//! Native installed sources for complete original-session admission, prepared off-live.
use super::*;
use cyrus_assets::{animation::Animations, environment::Environment, script::Scripts};
use cyrus_game::preferences::Preferences;
use cyrus_script::{
    player::data::PlayerScene,
    script::{
        LogicalSessionRestoreInputs, OriginalSessionImport, OriginalWorldAllocations,
        WorldSnapshotSources,
    },
};
use rgpre::import::savegame::{SaveModelAllocation, SaveModelAllocations, SaveSlot};
use std::collections::BTreeSet;

struct InstalledWorld {
    scripts: Scripts,
    animations: Animations,
    player: PlayerScene,
    environment: Environment,
    models: Vec<SaveModelAllocation>,
    walks: Vec<u32>,
    ropes: Vec<u32>,
}

pub(super) fn prepare(
    snapshot: RuntimeSessionSnapshot,
    slot: &SaveSlot,
    install: &GameInstall,
    preferences: &Preferences,
    generation: u64,
    bios_tick: u32,
) -> Result<PreparedLoad> {
    let saved_ids = std::iter::once(&slot.game)
        .chain(slot.maps.values())
        .map(|game| game.metadata.world_id)
        .collect::<BTreeSet<_>>();
    let prior_ids = snapshot.world_ids().collect::<BTreeSet<_>>();
    let worlds = prior_ids
        .union(&saved_ids)
        .map(|&id| Ok((id, InstalledWorld::load(id, install)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let prior_effects = snapshot.environmental_effect_worlds().collect();
    let prior = sources(&worlds, &prior_ids, &prior_effects);
    let original = sources(&worlds, &saved_ids, &saved_ids);
    let channels = (0u8..16).map(|slot| (slot, slot)).collect();
    let allocations = saved_ids
        .iter()
        .map(|&id| {
            let world = &worlds[&id];
            (
                id,
                OriginalWorldAllocations {
                    models: &world.models,
                    walk_maps: &world.walks,
                    ropes: &world.ropes,
                    ambient_channels: &channels,
                },
            )
        })
        .collect();
    let cache = cyrus_config::cache_dir();
    cyrus_assets::hud_export::export_hud(install, cache)?;
    let hud = serde_json::from_slice(&std::fs::read(cache.join("hud/overlay.hud"))?)?;
    let admission = snapshot.prepare_original(
        LogicalSessionRestoreInputs {
            sources: &prior,
            last_generation: generation,
            hud: &hud,
            preferences,
            bios_tick,
        },
        OriginalSessionImport {
            slot,
            sources: &original,
            allocations: &allocations,
            hud: &hud,
            preferences,
        },
    )?;
    // Resource requests must use the complete replacement census, including saved
    // visited worlds and stopped original-load services, rather than the prior session.
    let mut prepared = prepare_bytes(&admission.snapshot().encode_document()?, install)?;
    prepared.original = Some(admission);
    Ok(prepared)
}

fn sources<'a>(
    worlds: &'a BTreeMap<u32, InstalledWorld>,
    ids: &BTreeSet<u32>,
    effect_ids: &BTreeSet<u32>,
) -> BTreeMap<u32, WorldSnapshotSources<'a>> {
    ids.iter()
        .map(|&id| {
            let world = &worlds[&id];
            (
                id,
                WorldSnapshotSources {
                    scripts: &world.scripts,
                    animations: &world.animations,
                    collision: &world.player.collision,
                    player: Some(&world.player),
                    effects: effect_ids
                        .contains(&id)
                        .then_some(world.environment.effects.as_ref())
                        .flatten(),
                    identity_memo: None,
                },
            )
        })
        .collect()
}

impl InstalledWorld {
    fn load(id: u32, install: &GameInstall) -> Result<Self> {
        let cache = cyrus_config::cache_dir();
        cyrus_assets::scene::export_scene(
            install,
            &id.to_string(),
            &cache.join(format!("{id}.glb")),
        )?;
        let read = |extension| {
            std::fs::read(cache.join(format!("{id}.{extension}")))
                .with_context(|| format!("original import world {id} installed {extension}"))
        };
        let scripts = serde_json::from_slice(&read("scripts")?)?;
        let animations: Animations = serde_json::from_slice(&read("animations")?)?;
        let player = PlayerScene::decode(&read("player")?)?;
        let environment = serde_json::from_slice(&read("environment")?)?;
        let models = model_allocations(id, install, &animations)?;
        let walks = (0..animations.metadata["walk_node_maps"]
            .as_array()
            .context("original import installed walk maps absent")?
            .len())
            .map(|index| index as u32)
            .collect();
        let ropes = (1..=animations.metadata["ropes"].as_array().map_or(0, Vec::len))
            .map(|index| index as u32)
            .collect();
        Ok(Self {
            scripts,
            animations,
            player,
            environment,
            models,
            walks,
            ropes,
        })
    }
}

fn model_allocations(
    id: u32,
    install: &GameInstall,
    animations: &Animations,
) -> Result<Vec<SaveModelAllocation>> {
    let worlds = cyrus_assets::scene::read_worlds(install)?;
    let entry = worlds
        .entries
        .iter()
        .find(|entry| entry.index == id)
        .with_context(|| format!("original import world {id}: installed selection absent"))?;
    let filename = entry
        .map
        .rsplit(['/', '\\'])
        .next()
        .context("empty installed map name")?;
    let path = cyrus_assets::scene::find_file(&install.root.join("maps"), filename)
        .with_context(|| format!("original import world {id}: installed map {filename} absent"))?;
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path)?)?;
    let allocations = SaveModelAllocations::from_world(&map, &["CYRUS", "GREMLIN"], |source| {
        let file = source.rsplit(['/', '\\']).next().unwrap_or(source);
        let key = file
            .rsplit_once('.')
            .map_or(file, |(stem, _)| stem)
            .to_ascii_uppercase();
        let geometry = animations.models.get(&key).ok_or_else(|| {
            rgpre::error::Error::Parse(format!("installed geometry {key} missing for world {id}"))
        })?;
        Ok((key, geometry.positions.len()))
    })?;
    Ok(allocations.entries().to_vec())
}
