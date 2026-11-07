//! Retained fall origin is accessed through public script property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::*;
use cyrus_assets::surface::Surface;
use cyrus_game::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::Thread;
use cyrus_soup::program::*;
use std::collections::BTreeMap;

fn cache() -> Scripts {
    let program = Program {
        version: 4,
        entry: 0,
        variables: vec![4],
        strings: vec!["logic".into()],
        steps: BTreeMap::new(),
    };
    let mut scripts = Scripts {
        version: 30,
        weapons: Default::default(),
        camera: Default::default(),
        combat_attacks: vec![],
        combat_defenses: vec![],
        in_combat_threshold: None,
        lineup_distance: None,
        view_distance: None,
        texture_frames: Default::default(),
        non_engaged_spacing: None,
        combat_clank_effects: [None; 2],
        node_timer: None,
        dialogue: cyrus_assets::script::DialogueSettings {
            menu_start: [35, 25],
            max_menu_items: 300,
            max_dialog_lines: 20,
            max_text_width: 500,
            menu_traverse_delay: 4,
            print_text: true,
            use_speech: true,
            max_distance: 1600,
            rtx_filename: "ENGLISH.RTX".into(),
        },
        player_distance_override: false,
        updates: cyrus_assets::script::UpdateSettings {
            rope: None,
            ropes_load: Some(true),
            fall_death_ticks: 144,
            fall_landing: None,
            dummy_model_manager: false,
            task_system: true,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: false,
            final_version: false,
        },
        clock: cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        },
        actor_pool_capacity: 32,
        content_id: String::new(),
        shells: Vec::new(),
        world: 7,
        definition: Definition {
            functions: vec![Function {
                name: cyrus_soup::Native::NullFunction,
                param_count: 0,
                kind: CallKind::Function,
            }],
            flags: vec![Flag {
                name: "Counter".into(),
                flag_type: "NUMBER".into(),
                initial_value: 12,
            }],
            references: vec![],
            attributes: vec![],
        },
        actors: BTreeMap::from([(
            "LOGIC".into(),
            Actor {
                header_index: 0,
                code_origin: 0,
                instances: 2,
                namespace: Some("logic".into()),
                instance_variables: vec![vec![4], vec![9]],
                location_offsets: vec![],
                program,
                attributes: Default::default(),
                combat_taunts: vec![],
                native_hook: None,
            },
        )]),
        instances: (0..2)
            .map(|id| Instance {
                object_id: id,
                actor: "LOGIC".into(),
                object_type: Default::default(),
                active: 0,
                model_name: String::new(),
                model_mode: 0,
                light_intensity: 0,
                light_radius: 0,
                light_rgb: [0; 3],
                texture: 0,
                map_position: [0; 3],
                angles: [0; 3],
            })
            .collect(),
        unsupported: BTreeMap::new(),
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

fn origin_scripts(object: Object, value: i32, write: bool) -> Scripts {
    let mut scripts = cache();
    scripts.definition.references = vec![cyrus_soup::Property::Origin; 295];
    let place = Place::Property {
        object,
        reference: 294,
    };
    let assign = |destination, first| Operation::Assign {
        destination,
        formula: Formula {
            first,
            rest: vec![],
        },
    };
    let mut operations = Vec::new();
    if write {
        operations.push(assign(place.clone(), Value::Integer { value }));
    }
    operations.push(assign(Place::Local { index: 0 }, Value::Read { place }));
    operations.push(assign(
        Place::Flag { index: 0 },
        Value::Integer { value: 73 },
    ));
    operations.push(Operation::End { target: 0 });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = operations
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            (
                index as u32,
                Step {
                    next: index as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

fn advance(world: &mut ScriptWorld, caller: usize) -> anyhow::Result<()> {
    world.advance(
        world.id(caller)?,
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        20,
    )?;
    Ok(())
}

#[test]
fn origin_property_preserves_signed_q8_bits_and_continues_the_caller() {
    for value in [i32::MIN, -65537, 0, 65537, i32::MAX] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = origin_scripts(object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [value]);
            assert_eq!(world.flags(), [73]);
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .fall_origin_y,
                0
            );
            let mut expected = before;
            expected["fall_origin_y"] = value.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                expected
            );
        }
    }
}

#[test]
fn origin_property_reads_allocator_zero_without_spatial_preparation() {
    let scripts = origin_scripts(Object::Me, 0, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    assert_eq!(world.flags(), [73]);
}

#[test]
fn script_written_origin_is_consumed_by_the_existing_falling_writer() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::OrdinaryVerticalInputs;
    use cyrus_soup::Attribute;

    for (origin, expected_y) in [(8192, 4096), (-8192, -12288)] {
        let mut scripts = origin_scripts(Object::Me, origin, true);
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::JumpHeight] = 16;
        scripts.content_id = scripts.content_identity().unwrap();
        let data = Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({
                "script_content_id": scripts.content_id,
                "actors": [{"script_name": "LOGIC", "model_load_path": "none"}]
            }),
            models: Default::default(),
            sections: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        assert!(world.begin_static_ordinary_fall(id, &data).unwrap());
        assert_eq!(
            world.actors()[0].control_state().unwrap().fall_origin_y,
            4096
        );
        advance(&mut world, 0).unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
        world
            .apply_ordinary_falling_vertical(
                id,
                OrdinaryVerticalInputs::for_falling(30, 1, 0, 50, false).unwrap(),
            )
            .unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [0, expected_y, 0]
        );
        assert_eq!(
            world.actors()[0].control_state().unwrap().fall_origin_y,
            origin
        );
        assert_eq!(world.actors()[0].control_state().unwrap().fall_timer, 29);
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn script_written_origin_controls_the_public_landing_damage_threshold() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{
        ObstacleEnvironment, OrdinaryLandingDecision, OrdinaryLandingSettings,
    };
    use cyrus_soup::Attribute;

    for (origin, expected, health) in [
        (25600, OrdinaryLandingDecision::Ordinary, 100),
        (
            25856,
            OrdinaryLandingDecision::Hurt { health_change: -10 },
            90,
        ),
    ] {
        let mut scripts = origin_scripts(Object::Me, origin, true);
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Health] = 100;
        scripts.content_id = scripts.content_identity().unwrap();
        let data = Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({
                "script_content_id": scripts.content_id,
                "actors": [{"script_name": "LOGIC", "model_load_path": "none"}]
            }),
            models: Default::default(),
            sections: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        assert!(world.begin_static_ordinary_fall(id, &data).unwrap());
        world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
        advance(&mut world, 0).unwrap();
        let mut sounds = 0;
        let decision = world
            .resolve_ordinary_landing_damage(
                id,
                &data,
                ObstacleEnvironment {
                    selected_player: None,
                    auto_grab: false,
                    item_system_ready: true,
                    shell_execution: false,
                    ticks: 1,
                },
                OrdinaryLandingSettings {
                    death_height: 568,
                    hurt_height: 100,
                    hurt_zap: 10,
                },
                None,
                &mut |_, _| {
                    sounds += 1;
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(decision, expected);
        assert_eq!(world.actors()[0].attributes()[Attribute::Health], health);
        assert_eq!(
            world.actors()[0].control_state().unwrap().fall_origin_y,
            origin
        );
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
        assert_eq!(world.flags(), [73]);
        assert_eq!(sounds, 0);
    }
}

#[test]
fn origin_property_missing_owner_or_receiver_keeps_a_sticky_failure() {
    for write in [false, true] {
        for missing_receiver in [false, true] {
            let object = if missing_receiver {
                Object::Player
            } else {
                Object::Me
            };
            let mut scripts = origin_scripts(object, -12345, write);
            if !missing_receiver {
                scripts.instances[0].object_type = PlacementType::SCRIPT_ONLY;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            let expected = if missing_receiver {
                "Player property selected player unavailable"
            } else {
                "origin control owner unavailable"
            };
            assert!(format!("{error:#}").contains(expected), "{error:#}");
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            assert_eq!(world.flags(), [12]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
            assert_eq!(world.flags(), [12]);
        }
    }
}
