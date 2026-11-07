//! Receiver-owned accumulated displacement uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::*;
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::program::*;
use cyrus_soup::{Attribute, Thread};
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

fn counter_scripts(object: Object, value: i32, write: bool) -> Scripts {
    let mut scripts = cache();
    scripts.definition.references = vec![cyrus_soup::Property::OriginAccum; 295];
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
fn origin_accum_routes_full_signed_words_and_preserves_other_motion_state() {
    for (value, expected) in [
        (0, 0),
        (-1, -1),
        (256, 256),
        (i32::MIN, i32::MIN),
        (i32::MAX, i32::MAX),
    ] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["sliding_distance"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .sliding_distance,
                0
            );
        }
    }
}

#[test]
fn origin_accum_reads_zero_and_rejects_absent_owners_before_continuation() {
    let scripts = counter_scripts(Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for absent_receiver in [false, true] {
            let mut scripts = counter_scripts(
                if absent_receiver {
                    Object::Player
                } else {
                    Object::Me
                },
                48,
                write,
            );
            if !absent_receiver {
                scripts.instances[0].object_type = PlacementType::SCRIPT_ONLY;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            assert!(format!("{error:#}").contains(if absent_receiver {
                "Player property selected player unavailable"
            } else {
                "origin_accum control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn displacement_fixture(
    seed: i32,
    divisor: u8,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = counter_scripts(Object::Me, seed, true);
    scripts
        .definition
        .references
        .push(cyrus_soup::Property::JumpFlag);
    scripts.instances.truncate(1);
    scripts.instances[0].object_type = PlacementType::MODEL;
    scripts.instances[0].active = 1;
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    actor.attributes[Attribute::JumpTime] = divisor;
    actor.program.steps.get_mut(&0).unwrap().next = 4;
    actor.program.steps.insert(
        4,
        Step {
            next: 1,
            operation: Operation::Assign {
                destination: Place::Property {
                    object: Object::Me,
                    reference: 295,
                },
                formula: Formula {
                    first: Value::Integer { value: 8 },
                    rest: vec![],
                },
            },
        },
    );
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::End { target: 1 };
    if actor.program.steps.contains_key(&4) {
        let mut steps = std::mem::take(&mut actor.program.steps);
        actor.program.steps = [0, 4, 1, 2, 3]
            .into_iter()
            .enumerate()
            .map(|(offset, old)| {
                let mut step = steps.remove(&old).unwrap();
                step.next = offset as u32 + 1;
                if let Operation::End { target } = &mut step.operation {
                    *target = 2;
                }
                (offset as u32, step)
            })
            .collect();
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 7,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{"script_name":"LOGIC","model_load_path":"none"}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    advance(&mut world, 0).unwrap();
    (world, data)
}

#[test]
fn real_automatic_displacement_consumes_seed_before_the_later_stage_refusal() {
    for (seed, scale, after, z) in [
        (0, 256, 1024, 1024),
        (98300, 256, 98304, 4),
        (98304, 256, 98304, 0),
        (-1024, 256, 0, 1024),
        (i32::MAX, 256, i32::MAX, 0),
        (i32::MIN, u32::MAX, 98304, 0),
    ] {
        let (mut world, data) = displacement_fixture(seed, 64);
        let id = world.id(0).unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
        world
            .update_ordinary(
                &data,
                [0, 0],
                cyrus_script::script::OrdinaryInputs {
                    scene: None,
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(scale),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].control_state().unwrap().sliding_distance,
            after
        );
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, z]);
        assert!(
            world.actors()[0]
                .failure()
                .unwrap()
                .contains("ordinary queued motion stage unsupported")
        );

        assert_eq!(world.actors()[0].control_state().unwrap().fall_timer, 0);
        assert!(advance(&mut world, 0).is_err());
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn missing_jump_divisor_fails_before_accumulator_or_position_changes() {
    let (mut world, data) = displacement_fixture(17, 0);
    let id = world.id(0).unwrap();
    world
        .update_ordinary(
            &data,
            [0, 0],
            cyrus_script::script::OrdinaryInputs {
                scene: None,
                scripts: ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            20,
        )
        .unwrap();
    assert!(
        world.actors()[0]
            .failure()
            .unwrap()
            .contains("jump displacement needs attr_jump_time")
    );
    assert_eq!(
        world.actors()[0].control_state().unwrap().sliding_distance,
        17
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
    assert_eq!(world.flags(), [73]);
}
