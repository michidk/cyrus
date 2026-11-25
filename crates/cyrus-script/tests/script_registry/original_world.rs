//! Original rope ownership replaces state without a frame update.
use super::*;
use cyrus_assets::animation::Animations;
use rgpre::import::savegame::parse_save_ropes;
use serde_json::json;

#[test]
fn original_rope_restores_stationary_owner_and_rejects_unknown_owner_atomically() {
    let mut scripts = wait_cache(vec![Operation::End { target: 0 }]);
    scripts.updates.ropes_load = Some(true);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        sections: BTreeMap::new(),
        models: BTreeMap::new(),
        unsupported: BTreeMap::new(),
        metadata: json!({"ropes":[{
            "position":[0,0,0],"angle_y":0,"rope_type":0,"swing":2,"speed":199,
            "length":36,"static_model":"","rope_model":"ROPELINK","unknown1":[62,62,63]
        }]}),
    };
    world.prepare_ropes(&data).unwrap();
    let mut bytes = [0u8; 142];
    bytes[..4].copy_from_slice(&1u32.to_le_bytes());
    bytes[4 + 12] = 1;
    for (offset, value) in [(30, 1024i32), (34, 2), (62, 36), (78, 1), (90, 199)] {
        bytes[4 + offset..8 + offset].copy_from_slice(&value.to_le_bytes());
    }
    let saved = parse_save_ropes(&bytes).unwrap();
    world.restore_original_ropes(&[1], &saved, &[]).unwrap();
    assert_eq!(world.ropes()[0].heading, 1024);
    assert_eq!(world.ropes()[0].state().amplitude, 0);
    assert!(world.ropes()[0].state().attachments.is_empty());
    let before = world.ropes()[0].state();
    assert!(world.restore_original_ropes(&[2], &saved, &[]).is_err());
    assert_eq!(world.ropes()[0].state(), before);
    let mut swinging = saved;
    swinging[0].body[0x56..0x5a].copy_from_slice(&1i32.to_le_bytes());
    assert!(world.restore_original_ropes(&[1], &swinging, &[]).is_err());
    assert_eq!(world.ropes()[0].state(), before);
}

#[test]
fn original_light_and_flat_keep_retained_phase_and_playback_without_script_calls() {
    let mut scripts = wait_cache(vec![Operation::End { target: 0 }]);
    scripts.texture_frames.insert(2, BTreeMap::from([(3, 5)]));
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let mut light = [0; 41];
    light[2] = 31;
    light[11..13].copy_from_slice(&234i16.to_le_bytes());
    light[19..21].copy_from_slice(&19u16.to_le_bytes());
    light[35..37].copy_from_slice(&(-42i16).to_le_bytes());
    let mut flat = [0; 20];
    flat[0] = 1;
    flat[8..10].copy_from_slice(&259u16.to_le_bytes());
    flat[10..12].copy_from_slice(&999u16.to_le_bytes());
    flat[12..14].copy_from_slice(&4u16.to_le_bytes());
    flat[14..16].copy_from_slice(&1u16.to_le_bytes());
    flat[16..18].copy_from_slice(&3u16.to_le_bytes());
    flat[18..].copy_from_slice(&[2, 0x81]);
    let runtime = world.actors()[0].runtime_identifier.unwrap() as u16;
    let mut body = [0; 783];
    body[0x14..0x16].copy_from_slice(&runtime.to_le_bytes());
    let object = rgpre::import::savegame::SaveObject {
        object_id: i32::from(runtime),
        rope_id: 0,
        record_kind: 0,
        body,
        named_resource: None,
        attributes: None,
        opaque_41: Some(light),
        opaque_20: Some(flat),
        animation_header: None,
        script_state: None,
        attachment: None,
        vectors: [0; 48],
        opaque_60: None,
        private_variables: vec![],
        extension: vec![],
    };
    world
        .restore_original_presentation(&[(id, &object)])
        .unwrap();
    let actor = &world.actors()[0];
    let light = actor.light_state().unwrap();
    assert_eq!(light["enabled"], false);
    assert_eq!(light["inactive_ticks"], 31);
    assert_eq!(light["intensity"], 234);
    assert_eq!(light["flicker"][1], 19);
    assert_eq!(light["starting_rgb"][0], -42);
    let flat = actor.flat_state().unwrap();
    assert_eq!(flat["bitmap"], json!([2, 3]));
    assert_eq!(flat["counter"], 1);
    assert_eq!(flat["frame"], 3);
    assert_eq!(flat["animate"], json!([2, 0x81]));
    assert!(
        world
            .restore_original_presentation(&[(id, &object), (id, &object)])
            .is_err()
    );
    assert_eq!(world.actors()[0].flat_state().unwrap(), flat);
    assert_original_presentation_checkpoint(&world, &scripts);
}

fn assert_original_presentation_checkpoint(world: &ScriptWorld, scripts: &Scripts) {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let (_, _, animations) = revisit_test_app(scripts);
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts,
        animations: &animations,
        collision: &collision,
    };
    let samples = Default::default();
    let mut snapshot = world.snapshot(&sources).unwrap();
    for generation in [2, 3] {
        let restored = ScriptWorld::from_snapshot(
            snapshot,
            &sources,
            WorldRestoreContext {
                generation,
                globals: Default::default(),
                samples: &samples,
            },
        )
        .unwrap();
        assert_eq!(
            restored.actors()[0].light_state(),
            world.actors()[0].light_state()
        );
        assert_eq!(
            restored.actors()[0].flat_state(),
            world.actors()[0].flat_state()
        );
        snapshot = restored.snapshot(&sources).unwrap();
    }
}

#[test]
fn original_combat_replaces_constructor_defaults_and_rebinds_target_without_updates() {
    let mut scripts = wait_cache(vec![Operation::End { target: 0 }]);
    for definition in scripts.actors.values_mut() {
        definition.attributes[Attribute::Combat] = 1;
        for attribute in [
            Attribute::AttackSingle,
            Attribute::AttackDouble,
            Attribute::AttackTriple,
            Attribute::AttackThrust,
            Attribute::AttackLunge,
            Attribute::AttackLow,
            Attribute::AttackJumpUp,
            Attribute::AttackJumpForward,
            Attribute::AttackDisarm,
            Attribute::DefendLow,
            Attribute::DefendRight,
            Attribute::DefendLeft,
            Attribute::DefendHigh,
        ] {
            definition.attributes[attribute] = 0;
        }
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let ids = (0..world.actors().len())
        .filter(|&index| world.actors()[index].control_state().is_some())
        .map(|index| world.id(index).unwrap())
        .collect::<Vec<_>>();
    let data = Animations {
        version: 2,
        world: scripts.world,
        sections: BTreeMap::new(),
        models: BTreeMap::new(),
        unsupported: BTreeMap::new(),
        metadata: json!({"actors":scripts.actors.keys().map(|name|json!({"script_name":name})).collect::<Vec<_>>() }),
    };
    world
        .prepare_original_combat_allocations(&data, &ids)
        .unwrap();
    let mut bytes = [0; 422];
    for (offset, value) in [
        (0, -1i32),
        (4, 41),
        (8, 41),
        (12, 7),
        (0x38, -1),
        (0x58, 88),
        (0x139, 17),
        (0x141, 19),
        (0x145, 1),
        (0x14d, 2),
        (0x159, 43),
        (0x17d, -1),
        (0x18e, 8),
        (0x192, 2),
        (0x196, 11),
        (0x19a, 20),
        (0x19e, 1),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0x18d] = 0xff;
    let mut object = rgpre::import::savegame::SaveObject {
        object_id: 41,
        rope_id: 0,
        record_kind: 0,
        body: [0; 783],
        named_resource: None,
        attributes: None,
        opaque_41: None,
        opaque_20: None,
        animation_header: None,
        script_state: Some(bytes),
        attachment: None,
        vectors: [0; 48],
        opaque_60: None,
        private_variables: vec![],
        extension: vec![],
    };
    let id = ids[0];
    let receiver_index = world
        .actors()
        .iter()
        .position(|actor| actor.control_state().is_some())
        .unwrap();
    world.restore_original_combat(&[(id, &object)]).unwrap();
    let state = world.actors()[receiver_index]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(state.target, Some(id));
    assert_eq!(state.strength, 88);
    assert_eq!(state.flags.bits(), 0xbf);
    assert_eq!(state.cooldown, 43);
    assert_eq!(state.close_distance_passes, 19);
    assert_eq!(state.lost_sight_ticks, 20);
    assert_eq!(state.avoid_hold, 11);
    assert_eq!(state.taunt_cache, None);
    object.script_state.as_mut().unwrap()[8..12].copy_from_slice(&999i32.to_le_bytes());
    assert!(world.restore_original_combat(&[(id, &object)]).is_err());
    assert_eq!(
        world.actors()[receiver_index]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .strength,
        88
    );
}
