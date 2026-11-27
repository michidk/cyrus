//! Public native admission keeps retained actor owners independent and rejects atomically.
use super::*;

#[test]
fn original_actor_state_preserves_histories_and_rejects_without_mutation() -> anyhow::Result<()> {
    let (scripts, mut world, data, mut scene) = player_walking_fixture();
    scene.source_identity = Some("synthetic-original-actor".into());
    let player = world.selected_player().unwrap();
    let mut object = synthetic_original_player([100, 200, 300]);
    object.body[20..22].copy_from_slice(
        &world.actors()[player.index]
            .runtime_identifier
            .unwrap()
            .to_le_bytes(),
    );
    for offset in [8, 12] {
        object.body[offset..offset + 4].copy_from_slice(&(-1i32).to_le_bytes());
    }
    for (offset, value) in [
        (0xfb, 91i32),
        (0xef, 71),
        (0x144, 83),
        (0x150, 57),
        (0x114, 61),
    ] {
        object.body[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for axis in 0..3 {
        object.body[0x51 + (axis * 3 + axis) * 4..0x55 + (axis * 3 + axis) * 4]
            .copy_from_slice(&1f32.to_bits().to_le_bytes());
    }
    object.body[0x39..0x3d].copy_from_slice(&12.5f32.to_bits().to_le_bytes());
    object.body[0x107] = 1;
    object.body[0x108..0x10c].copy_from_slice(&42i32.to_le_bytes());
    object.body[0x1cc] = 48;
    object.body[0x1fe] = 3;
    object.body[0x1ff] = 87;
    object.body[0x237] = 1;
    object.attributes = Some(*world.actors()[player.index].attributes().bytes());
    let sources = cyrus_script::script::WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: Some(&scene),
        scripts: &scripts,
        animations: &data,
        collision: &scene.collision,
    };
    let before = serde_json::to_value(world.snapshot(&sources)?)?;
    let mut foreign = object.clone();
    foreign.body[20..22].copy_from_slice(&999u16.to_le_bytes());
    let mut missing_support = object.clone();
    missing_support.body[0x1d4..0x1d8].copy_from_slice(&999i32.to_le_bytes());

    let mut broken = object.clone();
    broken.body[0x51..0x55].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
    for bindings in [
        vec![(player, &broken)],
        vec![(player, &foreign)],
        vec![(player, &missing_support)],
        vec![(player, &object), (player, &object)],
    ] {
        assert!(world.restore_original_actor_states(&bindings).is_err());
        assert_eq!(serde_json::to_value(world.snapshot(&sources)?)?, before);
    }
    object.body[0x21..0x25].copy_from_slice(&1u32.to_le_bytes());
    let mut link = [0u8; 60];
    for (index, value) in [-1i32, 11, 12, 13, 21, 22, 23, 31, 32, 33]
        .into_iter()
        .enumerate()
    {
        link[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    object.opaque_60 = Some(link);
    world.restore_original_actor_states(&[(player, &object)])?;
    let spatial = world.spatial(player)?;
    assert_eq!(spatial.local.position, [100, 200, 300]);
    assert_eq!(spatial.previous_local_position, [91, 0, 0]);
    assert_eq!(spatial.angle_history, [57, 0, 0]);
    assert_eq!(spatial.initial_angles, [83, 0, 0]);
    assert_eq!(spatial.water_rollback_position, Some([61, 0, 0]));
    let control = world.actors()[player.index].control_state().unwrap();
    assert!(control.object_update_excluded);
    assert_eq!(control.move_counter, 48);
    assert_eq!(control.stable_local_position, [42, 0, 0]);
    assert_eq!((control.torch_burning, control.torch_saved), (3, 87));
    let snapshot = serde_json::to_value(world.snapshot(&sources)?)?;
    assert_eq!(
        snapshot["spatial"]["actors"][player.index]["link_cache"],
        serde_json::json!([21, 22, 23])
    );
    assert_eq!(
        snapshot["spatial"]["actors"][player.index]["link_previous"],
        serde_json::json!([31, 32, 33])
    );
    assert!(world.spatial(player)?.linked_position().is_none());
    assert_eq!(
        snapshot["actors"][player.index]["model"]["position"][0],
        12.5f32.to_bits()
    );
    assert!(world.retain_ordinary_water_position(player)?);
    assert_eq!(
        world.spatial(player)?.water_rollback_position,
        Some([100, 200, 300])
    );
    object.body[0x237] = 0;
    world.restore_original_actor_states(&[(player, &object)])?;
    assert!(
        !world.actors()[player.index]
            .control_state()
            .unwrap()
            .object_update_excluded
    );
    world.snapshot_spatial(player)?;
    assert_eq!(
        world.spatial(player)?.previous_local_position,
        [100, 200, 300]
    );
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .torch_saved,
        87
    );
    Ok(())
}

#[test]
fn imported_start_position_survives_native_snapshot_restoration() -> anyhow::Result<()> {
    use cyrus_script::script::{ScriptActorSource, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity()?;
    let (mut world, data, _) = static_model_fixture(scripts.clone());
    let collision = Default::default();
    let actor = world.id(0)?;
    let ScriptActorSource::Map(_) = &world.actors()[actor.index].source else {
        panic!("map fixture required")
    };
    let mut object = synthetic_original_player([100, 200, 300]);
    object.object_id = i32::from(world.actors()[actor.index].runtime_identifier.unwrap() as u16);
    object.body[20..22].copy_from_slice(
        &world.actors()[actor.index]
            .runtime_identifier
            .unwrap()
            .to_le_bytes(),
    );
    object.attributes = Some(*world.actors()[actor.index].attributes().bytes());
    for offset in [8, 12] {
        object.body[offset..offset + 4].copy_from_slice(&(-1i32).to_le_bytes());
    }
    object.body[0xef..0xf3].copy_from_slice(&777i32.to_le_bytes());
    world.restore_original_actor_states(&[(actor, &object)])?;
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let snapshot = world.snapshot(&sources)?;
    let mut restored = ScriptWorld::from_snapshot(
        snapshot,
        &sources,
        WorldRestoreContext {
            generation: 101,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )?;
    let id = restored.id(actor.index)?;
    restored.snapshot_spatial(id)?;
    let snapshot = serde_json::to_value(restored.snapshot(&sources)?)?;
    assert_eq!(
        snapshot["spatial"]["actors"][actor.index]["locations"][0][0],
        777
    );
    Ok(())
}

#[test]
fn imported_local_matrix_refresh_is_independent_of_attached_angles() -> anyhow::Result<()> {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity()?;
    let (mut world, data, _) = static_model_fixture(scripts.clone());
    let id = world.id(0)?;
    let mut object = synthetic_original_player([100, 200, 300]);
    let identifier = world.actors()[id.index].runtime_identifier.unwrap();
    object.object_id = i32::from(identifier as u16);
    object.body[20..22].copy_from_slice(&identifier.to_le_bytes());
    object.attributes = Some(*world.actors()[id.index].attributes().bytes());
    let matrix = set_imported_quarter_turn(&mut object);
    world.restore_original_actor_states(&[(id, &object)])?;
    assert_eq!(world.spatial(id)?.effective.unwrap().angles, [0; 3]);
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    // Both direct import and a native save made before the first refresh must
    // retain the local matrix baseline independently of the attached cache.
    let mut resumed = ScriptWorld::from_snapshot(
        world.snapshot(&sources)?,
        &sources,
        WorldRestoreContext {
            generation: 101,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )?;
    for world in [&mut world, &mut resumed] {
        let id = world.id(0)?;
        world.snapshot_spatial(id)?;
        world.refresh_transform(id)?;
        assert_eq!(world.spatial(id)?.local.matrix, matrix);
        world.refresh_transform(id)?;
        assert_eq!(world.spatial(id)?.local.matrix, matrix);
        let mut turning = serde_json::to_value(world.snapshot(&sources)?)?;
        turning["spatial"]["actors"][0]["local"]["angles"][1] = 768.into();
        let mut turned = ScriptWorld::from_snapshot(
            serde_json::from_value(turning)?,
            &sources,
            WorldRestoreContext {
                generation: 102,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )?;
        let id = turned.id(0)?;
        turned.refresh_transform(id)?;
        assert_eq!(
            turned.spatial(id)?.local.matrix.transform([0, 0, 256])?,
            [-181, 0, -181]
        );
        let matrix = turned.spatial(id)?.local.matrix;
        turned.refresh_transform(id)?;
        assert_eq!(turned.spatial(id)?.local.matrix, matrix);
    }
    Ok(())
}

fn set_imported_quarter_turn(
    object: &mut rgpre::import::savegame::SaveObject,
) -> cyrus_world::transform::Matrix {
    for offset in [8, 12] {
        object.body[offset..offset + 4].copy_from_slice(&(-1i32).to_le_bytes());
    }
    // Local and previous yaw name the stored matrix. The independent attached
    // angle fields retain their allocation zeros.
    for offset in [0x124, 0x154] {
        object.body[offset..offset + 4].copy_from_slice(&512i32.to_le_bytes());
    }
    let matrix = cyrus_world::transform::Matrix::orientation([0, 512, 0]);
    for (index, component) in matrix.0.into_iter().flatten().enumerate() {
        object.body[0x51 + index * 4..0x55 + index * 4]
            .copy_from_slice(&component.to_bits().to_le_bytes());
    }
    matrix
}
