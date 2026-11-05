//! Receiver-owned AI lifecycle selector reaches real public native producers and consumers.
use cyrus_assets::script::*;
use cyrus_game::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_script::program::*;
use cyrus_script::{Attribute, Thread};
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
                name: cyrus_script::Native::NullFunction,
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
                object_type: 0,
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
    scripts.definition.references = vec![cyrus_script::Property::AiType; 295];
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
fn ai_type_routes_unsigned_low_bytes_without_changing_adjacent_or_other_control() {
    for (value, expected) in [
        (0, 0),
        (6, 6),
        (8, 8),
        (128, 128),
        (207, 207),
        (-1, 255),
        (264, 8),
        (256, 0),
    ] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["ai_type"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .ai_type
                    .byte(),
                0
            );
        }
    }
}

#[test]
fn ai_type_allocation_zero_and_missing_owner_receiver_failures_are_distinct() {
    let scripts = counter_scripts(Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for missing_receiver in [false, true] {
            let mut scripts = counter_scripts(
                if missing_receiver {
                    Object::Player
                } else {
                    Object::Me
                },
                207,
                write,
            );
            if !missing_receiver {
                scripts.instances[0].object_type = 2;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            assert!(format!("{error:#}").contains(if missing_receiver {
                "selected player unavailable"
            } else {
                "ai_type control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn native_fixture(object: Object, value: i32, health: u8, native_producer: bool) -> Scripts {
    use cyrus_script::Native;
    let mut scripts = counter_scripts(object.clone(), value, true);
    for (name, param_count) in [
        (Native::SetAiType, 1),
        (Native::MyAiType, 0),
        (Native::IsDead, 0),
    ] {
        scripts.definition.functions.push(Function {
            name,
            param_count,
            kind: CallKind::Function,
        });
    }
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::Health] = health;
    actor.program.variables = vec![4, 5, 6];
    actor.instance_variables = vec![vec![4, 5, 6], vec![9, 10, 11]];
    let mut old = std::mem::take(&mut actor.program.steps);
    let first = if native_producer {
        Operation::ObjectCall {
            call: Call {
                function_id: 1,
                function: Native::SetAiType,
                target: object.clone(),
                kind: CallKind::Function,
                arguments: vec![Value::Integer { value }],
            },
        }
    } else {
        old.remove(&0).unwrap().operation
    };
    let assign_call = |index, function_id, function| Operation::Assign {
        destination: Place::Local { index },
        formula: Formula {
            first: Value::ObjectCall {
                call: Box::new(Call {
                    function_id,
                    function,
                    target: object.clone(),
                    kind: CallKind::Function,
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: first,
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: old.remove(&1).unwrap().operation,
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: assign_call(1, 2, Native::MyAiType),
            },
        ),
        (
            3,
            Step {
                next: 4,
                operation: assign_call(2, 3, Native::IsDead),
            },
        ),
        (
            4,
            Step {
                next: 5,
                operation: old.remove(&2).unwrap().operation,
            },
        ),
        (
            5,
            Step {
                next: 6,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn public_property_and_setter_share_the_real_ai_and_death_consumers() {
    for native_producer in [false, true] {
        for (value, selector, dead) in [
            (6, 6, 0),
            (8, 8, 1),
            (264, 8, 1),
            (300, 44, 0),
            (-1, 255, 0),
        ] {
            for health in [0, 123] {
                for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
                    let scripts = native_fixture(object, value, health, native_producer);
                    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
                    for _ in 0..2 {
                        advance(&mut world, 1).unwrap();
                        assert_eq!(
                            world.actors()[1].runtime().unwrap().variables(),
                            [selector, selector, dead]
                        );
                        assert_eq!(world.flags(), [73]);
                        let control = world.actors()[receiver].control_state().unwrap();
                        assert_eq!(control.ai_type.byte(), selector as u8);
                        assert_eq!(control.ai_mode, 0);
                        assert!(!control.killed);
                        assert_eq!(control.script_control, 0);
                        assert_eq!(
                            world.actors()[receiver].attributes()[Attribute::Health],
                            health
                        );
                        assert_eq!(
                            world.actors()[1 - receiver]
                                .control_state()
                                .unwrap()
                                .ai_type
                                .byte(),
                            0
                        );
                    }
                }
            }
        }
    }
}
