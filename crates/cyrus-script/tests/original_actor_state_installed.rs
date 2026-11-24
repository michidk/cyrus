//! Typed native actor restoration against an explicitly supplied original save.
#![cfg(feature = "original-save-import")]
use anyhow::{Context, Result, ensure};
use cyrus_script::script::{ScriptActorSource, ScriptWorld};
use rgpre::import::savegame::{load_save_slot, parse_save_objects};

#[test]
fn retained_actor_population_restores_without_callbacks() -> Result<()> {
    let Some(path) = std::env::var_os("CYRUS_ORIGINAL_SAVE") else {
        eprintln!("skipped: no original save supplied with CYRUS_ORIGINAL_SAVE");
        return Ok(());
    };
    let slot = load_save_slot(path)?;
    let install = cyrus_assets::install::GameInstall::locate(None)?;
    let worlds = cyrus_assets::scene::read_worlds(&install)?;
    let entry = worlds
        .entries
        .iter()
        .find(|entry| entry.index == slot.game.metadata.world_id)
        .context("saved world absent from WORLD.INI")?;
    let filename = entry
        .map
        .rsplit(['\\', '/'])
        .next()
        .context("saved map name absent")?;
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(
        cyrus_assets::scene::find_file(&install.root.join("maps"), filename)
            .context("installed map absent")?,
    )?)?;
    let scripts = cyrus_assets::script_export::prepare(&map, &install.root, entry.index)?;
    let section = slot
        .game
        .sections
        .iter()
        .find(|section| section.name == *b"SMOB")
        .context("SMOB absent")?;
    let objects = parse_save_objects(&section.data)?;
    let mut world = ScriptWorld::prepare(&scripts, 101)?;
    world.prepare_spatial()?;
    let mut bindings = Vec::new();
    for object in &objects {
        let mut matching =
            world
                .actors()
                .iter()
                .enumerate()
                .filter(|(_, actor)| match &actor.source {
                    ScriptActorSource::Player => object.object_id == -2,
                    ScriptActorSource::AlternatePlayer => object.object_id == -3,
                    ScriptActorSource::Camera => object.object_id == 2,
                    ScriptActorSource::Map(placement) => {
                        !placement.object_type.has_separate_record()
                            && object.object_id >= 0
                            && actor
                                .runtime_identifier
                                .is_some_and(|id| id as u16 == object.runtime_identifier())
                    }
                    _ => false,
                });
        let (index, _) = matching
            .next()
            .with_context(|| format!("saved actor {} has no native receiver", object.object_id))?;
        ensure!(matching.next().is_none(), "ambiguous actor receiver");
        let id = world.id(index)?;
        if matches!(
            world.actors()[index].source,
            ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
        ) {
            let spatial = object.actor_state()?.spatial;
            world.initialize_player_spatial(
                id,
                spatial.initial_position_q8,
                spatial.initial_angles.map(|v| (v & 2047) as u16),
            )?;
        }
        if matches!(world.actors()[index].source, ScriptActorSource::Camera) {
            let spatial = object.actor_state()?.spatial;
            world.initialize_camera_pose(cyrus_script::script::camera::CameraPose {
                position: spatial.initial_position_q8,
                angles: spatial.initial_angles,
            })?;
        }
        bindings.push((id, object));
    }
    world.restore_original_actor_states(&bindings)?;
    for (id, object) in bindings {
        let saved = object.actor_state()?;
        let state = world.spatial(id)?;
        assert_eq!(state.local.position, saved.spatial.position_q8);
        assert_eq!(state.local.angles, saved.spatial.angles);
        assert_eq!(
            state.previous_local_position,
            saved.spatial.previous_position_q8
        );
        assert_eq!(state.angle_history, saved.spatial.previous_angles);
        assert_eq!(state.initial_angles, saved.spatial.initial_angles);
        assert_eq!(
            state.water_rollback_position,
            Some(saved.spatial.water_rollback_position_q8)
        );
        let control = world.actors()[id.index]
            .control_state()
            .context("controller absent")?;
        assert_eq!(control.move_counter, saved.controller.movement_counter);
        assert_eq!(
            control.stable_local_position,
            saved.spatial.stored_stable_position_q8
        );
        assert_eq!(
            control.torch_saved,
            saved.controller.extinguished_torch_timer
        );
        world.snapshot_spatial(id)?;
        assert_eq!(
            world.spatial(id)?.previous_local_position,
            saved.spatial.position_q8
        );
    }
    eprintln!(
        "restored {} original actor spatial/controller owners without executing callbacks",
        objects.len()
    );
    Ok(())
}
