use super::*;

#[cfg(feature = "original-save-import")]
#[test]
fn original_subset_keeps_synthetic_logical_y_without_a_model_height_offset() -> anyhow::Result<()> {
    let (scripts, mut world, animations, _) = player_walking_fixture();
    let position = [1024, -40000, 3072];
    let player = world.selected_player().unwrap();
    world.set_model_support(
        player,
        cyrus_script::script::ModelSupportOwners {
            actor: Some(world.id(0)?),
            static_placement: None,
        },
    )?;
    let asset = inventory_asset(&[(0, 0)]);
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
    let subset = cyrus_script::script::original_save::OriginalSaveSubset::decode(
        &synthetic_original_pose_save(&scripts, position)?,
        &asset,
        &scripts.definition,
    )?;
    assert_eq!(subset.position_q8, position);
    subset.apply(&mut world, &hud, &animations)?;
    let player = world.selected_player().unwrap();
    assert_eq!(world.spatial(player)?.local.position, position);
    assert_eq!(world.spatial(player)?.local.angles[1], 123);
    assert_eq!(world.model_support(player)?.actor, None);
    Ok(())
}

#[cfg(feature = "original-save-import")]
pub(super) fn synthetic_original_player(position: [i32; 3]) -> rgpre::import::savegame::SaveObject {
    use rgpre::import::savegame::SaveObject;
    let mut body = [0; 783];
    for (axis, value) in position.iter().enumerate() {
        body[203 + axis * 4..207 + axis * 4].copy_from_slice(&value.to_le_bytes());
    }
    body[292..296].copy_from_slice(&123i32.to_le_bytes());
    body[571..575].copy_from_slice(&(-1i32).to_le_bytes());
    body[468..472].copy_from_slice(&(-1i32).to_le_bytes());
    SaveObject {
        object_id: -2,
        rope_id: 0,
        record_kind: 0,
        body,
        named_resource: None,
        attributes: None,
        opaque_41: None,
        opaque_20: None,
        animation_header: None,
        script_state: None,
        attachment: None,
        vectors: [0; 48],
        opaque_60: None,
        private_variables: vec![],
        extension: vec![],
    }
}

#[cfg(feature = "original-save-import")]
fn synthetic_original_pose_save(
    scripts: &Scripts,
    position: [i32; 3],
) -> anyhow::Result<rgpre::import::savegame::SaveGame> {
    use rgpre::import::savegame::{SaveGame, SaveMetadata, SaveSection, encode_save_objects};
    let mut clock = vec![0; 1542];
    for (offset, value) in [
        (0x9c, 25u32),
        (0xa0, 122),
        (0xc0, 24),
        (0xc4, 122),
        (0xec, 60),
        (0xe8, 1),
        (0xf8, 3072),
    ] {
        clock[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut globals = vec![0; 1666];
    globals[..4].copy_from_slice(&50i32.to_le_bytes());
    let mut flag_data = (scripts.definition.flags.len() as u32)
        .to_le_bytes()
        .to_vec();
    flag_data.resize(4 + scripts.definition.flags.len() * 4, 0);
    flag_data.extend(scripts.definition.flags.iter().map(|flag| {
        match flag.flag_type.to_ascii_lowercase().as_str() {
            "bool" => 0,
            "flipflop" => 1,
            "counter" => 2,
            "number" => 3,
            other => panic!("unexpected synthetic flag kind {other}"),
        }
    }));
    Ok(SaveGame {
        metadata: SaveMetadata {
            version: *b"1.31",
            unknown_04: 0,
            unknown_08: 0,
            description: "Synthetic".into(),
            world_id: scripts.world,
            bitmap_filename: String::new(),
        },
        sections: vec![
            SaveSection {
                name: *b"SVGV",
                data: clock,
            },
            SaveSection {
                name: *b"SMOB",
                data: encode_save_objects(&[synthetic_original_player(position)])?,
            },
            SaveSection {
                name: *b"SVG2",
                data: globals,
            },
            SaveSection {
                name: *b"SVIT",
                data: vec![0; 4],
            },
            SaveSection {
                name: *b"SVGF",
                data: flag_data,
            },
        ],
    })
}

#[cfg(feature = "original-save-import")]
#[test]
fn original_subset_restores_static_support_pose_and_binding() -> anyhow::Result<()> {
    use rgpre::import::savegame::{encode_save_objects, parse_save_objects};
    let (scripts, mut world, animations, _) = player_walking_fixture();
    let support = world.id(0)?;
    assert!(
        world.actors()[0]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main)
            .is_some()
    );
    let identity = world.actors()[0].runtime_identifier.unwrap();
    let position = [1024, -40000, 3072];
    let mut save = synthetic_original_pose_save(&scripts, position)?;
    let section = save
        .sections
        .iter_mut()
        .find(|section| section.name == *b"SMOB")
        .unwrap();
    let mut objects = parse_save_objects(&section.data)?;
    objects[0].body[468..472].copy_from_slice(&i32::from(identity).to_le_bytes());
    objects.push(synthetic_original_static_support(identity));
    section.data = encode_save_objects(&objects)?;
    let asset = inventory_asset(&[(0, 0)]);
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
    cyrus_script::script::original_save::OriginalSaveSubset::decode(
        &save,
        &asset,
        &scripts.definition,
    )?
    .apply(&mut world, &hud, &animations)?;
    let player = world.selected_player().unwrap();
    assert_eq!(world.spatial(player)?.local.position, position);
    assert_eq!(world.spatial(support)?.local.position, [512, -1024, 768]);
    assert_eq!(world.spatial(support)?.local.angles, [7, 1024, 2046]);
    assert_eq!(world.model_support(player)?.actor, Some(support));
    assert_eq!(
        world.actors()[0].attributes()[Attribute::CameraTargetOffsetZ],
        128
    );
    let runtime = world.actors()[support.index].runtime().unwrap();
    assert_eq!(runtime.program_counter(Thread::Main), None);
    assert_eq!(runtime.program_counter(Thread::Interrupt), None);
    assert_eq!(runtime.active_tasks(), 0);
    let presentation = world.model_presentation(support)?.unwrap();
    assert_eq!(presentation.position, [2.0, -4.0, 3.0]);
    assert_eq!(presentation.pose.angles, [7, 1024, 2046]);
    let flags = world.flags().to_vec();
    world.advance(
        support,
        ScriptStage::OrdinaryMain,
        cyrus_script::script::ScriptInputs {
            frame_scale: Some(1),
            nominal_ticks: Some(1),
            ..Default::default()
        },
        None,
        None,
        32,
    )?;
    assert_eq!(world.flags(), flags);
    assert_eq!(world.spatial(support)?.local.position, [512, -1024, 768]);
    Ok(())
}

#[cfg(feature = "original-save-import")]
fn synthetic_original_static_support(identity: i16) -> rgpre::import::savegame::SaveObject {
    let mut owner = synthetic_original_player([512, -1024, 768]);
    owner.object_id = i32::from(identity);
    owner.body[20..22].copy_from_slice(&(identity as u16).to_le_bytes());
    owner.body[193] = 1;
    owner.named_resource = Some(vec![1, b'S', b'T', b'A', b'T', b'I', b'C', 0, 0, 0]);
    owner.body[626..630].copy_from_slice(&1u32.to_le_bytes());
    let mut attributes = [0; 256];
    attributes[Attribute::CameraTargetOffsetZ.index()] = 128;
    owner.attributes = Some(attributes);
    let angles = [7i32, 1024, 2046];
    for (axis, angle) in angles.into_iter().enumerate() {
        owner.body[288 + axis * 4..292 + axis * 4].copy_from_slice(&angle.to_le_bytes());
    }
    owner
}

#[cfg(feature = "original-save-import")]
#[test]
fn original_subset_restores_selected_playback_and_clock_from_synthetic_records()
-> anyhow::Result<()> {
    use cyrus_soup::AnimationGroup;
    use rgpre::import::savegame::{encode_save_objects, parse_save_objects};
    let (scripts, mut world, animations, _) = player_walking_fixture();
    let mut save = synthetic_original_pose_save(&scripts, [1024, -40000, 3072])?;
    let section = save
        .sections
        .iter_mut()
        .find(|s| s.name == *b"SMOB")
        .unwrap();
    let mut objects = parse_save_objects(&section.data)?;
    objects[0].body[0x247..0x24b].copy_from_slice(&1u32.to_le_bytes());
    let mut header = [0u8; 37];
    header[..2].copy_from_slice(&(AnimationGroup::WALK.index() as i16).to_le_bytes());
    header[2..4].copy_from_slice(&(-1i16).to_le_bytes());
    header[5..7].copy_from_slice(&1i16.to_le_bytes());
    header[7] = 3;
    header[8] = 2;
    objects[0].animation_header = Some(header);
    section.data = encode_save_objects(&objects)?;
    let asset = inventory_asset(&[(0, 0)]);
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
    let subset = cyrus_script::script::original_save::OriginalSaveSubset::decode(
        &save,
        &asset,
        &scripts.definition,
    )?;
    let clock = subset.restore_clock(900)?;
    assert_eq!(clock.bios_sample(), 900);
    assert_eq!(clock.step().frame_scale, 122);
    assert_eq!(clock.fps(), 25);
    subset.apply(&mut world, &hud, &animations)?;
    let player = world.selected_player().unwrap();
    let state = world.inspect(player.index, 1);
    assert_eq!(
        state["actors"][0]["animation"]["current_group"],
        AnimationGroup::WALK.index()
    );
    assert_eq!(world.animation_frame(player)?, Some(0));
    Ok(())
}

#[cfg(feature = "original-save-import")]
#[test]
fn original_npc_playback_survives_first_default_selection_and_animation_update()
-> anyhow::Result<()> {
    use rgpre::import::savegame::{encode_save_objects, parse_save_objects};
    let (scripts, mut world, mut animations, scene) = player_walking_fixture();
    let actor = world.id(0)?;
    let identity = world.actors()[actor.index].runtime_identifier.unwrap();
    let metadata = animations.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["script_name"] == "LOGIC")
        .unwrap();
    metadata["model_load_path"] = "animation_table".into();
    metadata["raan_entries"] = serde_json::json!([{"file_path":"PLAYER.3D","frame_count":1}]);
    metadata["animation_groups"] = serde_json::json!([
        {"group_index":AnimationGroup::PANIC.index(),"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
        {"group_index":AnimationGroup::STILL.index(),"anim_id":1,"anim_type":1,"commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]}
    ]);
    animations.metadata["mpob_objects"][0]["script_name"] = "LOGIC".into();
    world.bind_animation(actor, &animations, AnimationGroup::PANIC)?;
    let mut save = synthetic_original_pose_save(&scripts, [1024, -40000, 3072])?;
    let section = save
        .sections
        .iter_mut()
        .find(|s| s.name == *b"SMOB")
        .unwrap();
    let mut objects = parse_save_objects(&section.data)?;
    let mut npc = synthetic_original_player([2560, 5120, 7680]);
    npc.object_id = i32::from(identity);
    npc.body[20..22].copy_from_slice(&identity.to_le_bytes());
    npc.body[0x247..0x24b].copy_from_slice(&1u32.to_le_bytes());
    npc.body[0x257..0x259].copy_from_slice(&(AnimationGroup::STILL.index() as i16).to_le_bytes());
    let mut attributes = [0; 256];
    attributes[Attribute::PanicGroup.index()] = AnimationGroup::STILL.index() as u8;
    npc.body[626..630].copy_from_slice(&1u32.to_le_bytes());
    npc.attributes = Some(attributes);
    let mut header = [0; 37];
    header[..2].copy_from_slice(&(AnimationGroup::STILL.index() as i16).to_le_bytes());
    header[2..4].copy_from_slice(&(-1i16).to_le_bytes());
    header[4] = 1;
    header[7] = 1;
    npc.animation_header = Some(header);
    objects.push(npc);
    section.data = encode_save_objects(&objects)?;
    let asset = inventory_asset(&[(0, 0)]);
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
    cyrus_script::script::original_save::OriginalSaveSubset::decode(
        &save,
        &asset,
        &scripts.definition,
    )?
    .apply(&mut world, &hud, &animations)?;
    let group = |world: &ScriptWorld| {
        world.inspect(actor.index, 1)["actors"][0]["animation"]["current_group"].clone()
    };
    assert_eq!(group(&world), AnimationGroup::STILL.index());
    world.update_ordinary(
        &animations,
        [10, 30],
        cyrus_script::script::OrdinaryInputs {
            scene: Some(&scene),
            scripts: cyrus_script::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                fine_ticks: Some(1),
                shell_execution: Some(false),
                ..Default::default()
            },
        },
        &mut cyrus_game::random::EngineRandom::default(),
        &mut |_, _| Ok(()),
        100,
    )?;
    assert_eq!(group(&world), AnimationGroup::STILL.index());
    let playback = world.inspect(actor.index, 1)["actors"][0]["animation"].clone();
    assert_eq!(playback["default_group"], AnimationGroup::STILL.index());
    assert!(playback["command_frame"].is_number());
    assert!(playback["speed_count"].is_number());
    assert!(playback["real_frame"].is_number());
    assert_eq!(world.animation_frame(actor)?, Some(0));
    Ok(())
}
