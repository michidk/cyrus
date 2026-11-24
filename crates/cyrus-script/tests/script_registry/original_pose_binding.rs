//! Public native pose import preserves both frames and rejects a whole binding set.
use super::*;
use rgpre::import::savegame::SaveModelAllocation;

#[test]
fn original_allocated_pose_is_atomic_and_survives_first_explicit_update() -> anyhow::Result<()> {
    let (scripts, mut world, data, mut scene) = player_walking_fixture_with_animation(
        |_| {},
        |data| {
            let actor = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap();
            actor["raan_entries"][0]["frame_count"] = 100.into();
            actor["animation_groups"][0]["commands"] = serde_json::json!([
                {"opcode":0,"frame_reference":88}, {"opcode":0,"frame_reference":89}, {"opcode":1}
            ]);
            let model = data.models.get_mut("PLAYER").unwrap();
            model.positions.resize(100, vec![]);
            model.normals.resize(100, vec![]);
        },
    );
    scene.source_identity = Some("synthetic-original-pose".into());
    let player = world.selected_player().unwrap();
    let mut object = synthetic_original_player([0; 3]);
    object.body[0x24f..0x251].copy_from_slice(&88i16.to_le_bytes());
    object.body[0x251..0x253].copy_from_slice(&7i16.to_le_bytes());
    object.body[0x253..0x255].copy_from_slice(&86i16.to_le_bytes());
    object.body[0x255..0x257].copy_from_slice(&7i16.to_le_bytes());
    object.body[0x257..0x259].copy_from_slice(&18i16.to_le_bytes());
    let mut header = [0; 37];
    header[2..4].copy_from_slice(&(-1i16).to_le_bytes());
    header[7] = 1;
    object.animation_header = Some(header);
    let mut attributes = [0; 256];
    attributes[Attribute::PanicGroup.index()] = 18;
    object.attributes = Some(attributes);
    let allocation = SaveModelAllocation {
        handle: 7,
        source: "PLAYER.3DC".into(),
        frame_count: 100,
        texture: 0,
    };
    let sources = cyrus_script::script::WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: Some(&scene),
        scripts: &scripts,
        animations: &data,
        collision: &scene.collision,
    };
    let before = serde_json::to_value(world.snapshot(&sources)?)?;
    let mut broken = object.clone();
    broken.body[0x255..0x257].copy_from_slice(&99i16.to_le_bytes());
    assert!(
        world
            .restore_original_bound_animations(
                &[(player, &object), (player, &broken)],
                std::slice::from_ref(&allocation),
                &data
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(world.snapshot(&sources)?)?, before);
    assert!(
        world
            .restore_original_bound_animations(
                &[(player, &broken)],
                std::slice::from_ref(&allocation),
                &data
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(world.snapshot(&sources)?)?, before);
    let mut foreign = allocation.clone();
    foreign.texture = 453;
    assert!(
        world
            .restore_original_bound_animations(&[(player, &object)], &[foreign], &data)
            .is_err()
    );
    assert_eq!(serde_json::to_value(world.snapshot(&sources)?)?, before);
    world.restore_original_bound_animations(&[(player, &object)], &[allocation], &data)?;
    assert_eq!(world.animation_frame(player)?, Some(88));
    let snapshot = serde_json::to_value(world.snapshot(&sources)?)?;
    assert_eq!(
        snapshot["actors"][player.index]["animation"]["playback"]["last_frame"],
        86
    );
    assert_eq!(
        snapshot["actors"][player.index]["animation"]["playback"]["default"],
        18
    );
    world.advance(
        player,
        ScriptStage::Animation,
        cyrus_script::script::ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        100,
    )?;
    assert_eq!(world.animation_frame(player)?, Some(89));
    Ok(())
}
