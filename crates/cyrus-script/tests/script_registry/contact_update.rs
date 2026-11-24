use super::*;
use cyrus_script::script::{
    CombatControlState, LocomotionGeometry, OrdinaryGroundQuery, ScriptInputs,
};

#[test]
fn ordinary_combat_update_dispatches_contact_and_ordered_defence_response() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let path = root.join("COMBAT.INI");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no install (contact update: {})", path.display());
        return;
    };
    let ini = rgpre::import::combat_ini::CombatIni::parse(&text).unwrap();
    let attack = combat::prepare(&ini).unwrap()[72].clone();
    let motion = &attack.motions[0];
    let near = (motion.min_range + motion.max_range) / 2;
    let required = motion.defense.unwrap() - 1;
    let start = motion.first_collide_frame;
    let end = start + motion.collide_duration;
    assert!(start >= motion.first_defend_frame.unwrap());
    assert!(start < motion.first_defend_frame.unwrap() + motion.defend_duration.unwrap());

    // Synthetic model extents are +/-12 map units: height 6400 Q8 is
    // outside their elevation overlap but inside the installed 3D range.
    // Motion limits, selectors and command windows come from the install.
    for (distance, height, selector, intent, cursor, blocked) in [
        (near, 0, required, true, start, true),
        (motion.min_range, 0, required, true, start, false),
        (motion.max_range, 0, required, true, start, false),
        (near, 6400, required, true, start, false),
        (-near, 0, required, true, start, false),
        (near, 0, required + 1, true, start, false),
        (near, 0, required, false, start, false),
        (near, 0, required, true, start - 1, false),
        (
            near,
            0,
            required,
            true,
            motion.first_defend_frame.unwrap() + motion.defend_duration.unwrap(),
            false,
        ),
    ] {
        let (world, scripts, mut data) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                scripts.combat_clank_effects = [Some(31), Some(32)];
                for instance in &mut scripts.instances {
                    instance.object_type = PlacementType::MODEL;
                }
                scripts.combat_attacks = vec![attack.clone()];
            },
            |saved| {
                saved["actors"][0]["control"]["death_selector"] = 0.into();
                for (index, pending, flags) in [
                    (0, -1, 0x81),
                    (1, selector, if intent { 0x8b } else { 0x89 }),
                ] {
                    saved["actors"][index]["control"]["combat"] =
                        serde_json::to_value(CombatControlState {
                            action: 0,
                            flags: CombatFlags::from_bits(flags),
                            attack_indices: [Some(0); 9],
                            retained_action_indices: [pending, -1],
                            ..Default::default()
                        })
                        .unwrap();
                }
                saved["actors"][0]["control"]["combat"]["retained_action_indices"][1] = 0.into();
                saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]);
                saved["spatial"]["actors"][1]["local"]["position"] =
                    serde_json::json!([0, height, distance]);
            },
        );
        let group = data.metadata["actors"][0]["animation_groups"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|group| group["group_index"] == 18)
            .unwrap();
        group["commands"] = serde_json::Value::Array(
            (0..end + 3)
                .map(|_| serde_json::json!({"opcode":0,"frame_reference":18}))
                .collect(),
        );
        data.metadata["actors"][0]["ravc_count"] = 0.into();
        data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
        data.metadata["actors"][0]["mpsz_bounds_primary"] = 0.into();
        data.metadata["actors"][0]["raex"] = serde_json::json!({"unknown_12":256});
        data.metadata["mpsz_entries"] = serde_json::json!([{
            "total_extent":[24,24,24],"pos_extent":[12,12,12],
            "neg_extent":[12,12,12],"center_offset":[0,0,0],"flags":0
        }]);
        data.metadata["actors"][0]["raan_entries"] =
            serde_json::json!([{"file_path":"CONTACT.3D","frame_count":32}]);
        data.metadata["mpob_objects"] = serde_json::json!([{"script_name":"LOGIC","object_type":1},{"script_name":"LOGIC","object_type":1}]);
        data.models.insert(
            "CONTACT".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some((motion.max_range >> 8) as f32 + 1.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i16".into(),
                positions: vec![vec![]; 32],
                normals: vec![vec![]; 32],
            },
        );
        let mut saved = movement_snapshot(&world, &scripts, &data);
        let mut playback = ScriptWorld::prepare(&scripts, 1).unwrap();
        playback.prepare_spatial().unwrap();
        for index in [0, 1] {
            let actor = playback.id(index).unwrap();
            playback
                .bind_animation(actor, &data, AnimationGroup::RUN)
                .unwrap();
            playback
                .advance(
                    actor,
                    ScriptStage::Animation,
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
        }
        let animation_saved = movement_snapshot(&playback, &scripts, &data);
        for index in [0, 1] {
            saved["actors"][index]["animation"] =
                animation_saved["actors"][index]["animation"].clone();
        }
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let attacker = world.id(0).unwrap();
        let defender = world.id(1).unwrap();
        let (_, _, models) = static_model_fixture(cache());
        world.prepare_model_resources(&data, &models).unwrap();
        let scene = ordinary_collision_scene(world.world, models);
        world
            .refresh_model_presentations(&data, &[attacker, defender])
            .unwrap();
        for actor in [attacker, defender] {
            world
                .prepare_ordinary_collision_shape(actor, &data, &scene)
                .unwrap();
        }
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["animation"]["playback"]["current"][1]["command"] = cursor.into();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let attacker = world.id(0).unwrap();
        let defender = world.id(1).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(attacker, &scene, &data, [0, 0], &[defender])
            .unwrap();
        assert!(
            prepared
                .candidates()
                .actors
                .iter()
                .any(|candidate| candidate.owner
                    == cyrus_collision::candidates::CandidateOwner::Actor(defender.index))
        );
        let mut random = cyrus_game::random::EngineRandom::from_state(1);
        let mut sounds = Vec::new();
        world
            .maintain_map_combat(
                attacker,
                1,
                false,
                ScriptInputs {
                    combat_clock_sample: Some(0),
                    locomotion_geometry: Some(LocomotionGeometry {
                        ground: OrdinaryGroundQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            distance: 0,
                            sidedness: false,
                        },
                        environment: cyrus_script::script::ObstacleEnvironment {
                            selected_player: None,
                            auto_grab: false,
                            item_system_ready: true,
                            shell_execution: false,
                            ticks: 1,
                        },
                    }),
                    ..Default::default()
                },
                Some(&mut random),
                &mut |actor, sound| {
                    sounds.push((actor, sound));
                    Ok(())
                },
            )
            .unwrap();
        let saved = movement_snapshot(&world, &scripts, &data);
        assert_eq!(
            saved["actors"][0]["animation"]["playback"]["reverse"],
            blocked
        );
        let flags = |index: usize| {
            world.actors()[index]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
        };
        let admitted = cursor >= start;
        assert_eq!(flags(0).contains(CombatFlags::CONTACT_TESTED), admitted);
        let contact =
            admitted && distance > motion.min_range && distance < motion.max_range && height == 0;
        assert_eq!(flags(0).contains(CombatFlags::MOVE_INTENT), !contact);
        assert_eq!(
            flags(1).bits(),
            if blocked {
                0x80
            } else if admitted
                && selector == required
                && intent
                && cursor < motion.first_defend_frame.unwrap() + motion.defend_duration.unwrap()
            {
                0x83
            } else if intent {
                0x8b
            } else {
                0x89
            }
        );
        assert_eq!(sounds.len(), usize::from(blocked));
        let block = world.consume_combat_message(attacker).unwrap();
        assert_eq!(
            block.map(|message| message.code),
            blocked.then_some(CombatMessageCode::BLOCK)
        );
        let hit = world.consume_combat_message(defender).unwrap();
        assert_eq!(
            hit.map(|message| message.code),
            (contact && !blocked).then_some(CombatMessageCode::HIT)
        );
    }
}
