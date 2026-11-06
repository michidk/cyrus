//! Receiver-owned movement classification uses public property dispatch.
use cyrus_assets::script::*;
use cyrus_game::script::OrdinaryForwardAmountInputs;
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

fn classification_scripts(object: Object, value: i32, write: bool) -> Scripts {
    let mut scripts = cache();
    scripts.definition.references = vec![cyrus_script::Property::LastDirection; 295];
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
fn last_direction_routes_low_bytes_and_continues_without_changing_other_state() {
    for (value, expected) in [
        (0, 0),
        (2, 2),
        (3, 3),
        (6, 6),
        (7, 7),
        (25, 25),
        (-1, 255),
        (258, 2),
        (256, 0),
    ] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = classification_scripts(object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut control =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            control["motion_classification"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                control
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .motion_classification,
                0
            );
        }
    }
}

#[test]
fn last_direction_reads_allocation_zero_without_spatial_or_animation_preparation() {
    let scripts = classification_scripts(Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    assert_eq!(world.flags(), [73]);
}

#[test]
fn last_direction_missing_owner_or_receiver_keeps_a_sticky_failure() {
    for write in [false, true] {
        for missing_receiver in [false, true] {
            let object = if missing_receiver {
                Object::Player
            } else {
                Object::Me
            };
            let mut scripts = classification_scripts(object, -12345, write);
            if !missing_receiver {
                scripts.instances[0].object_type = 2;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            let expected = if missing_receiver {
                "Player property selected player unavailable"
            } else {
                "last_direction control owner unavailable"
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

fn motion_fixture(
    group_present: bool,
    sound: bool,
    collide: u8,
    living: u8,
    pending: bool,
) -> ScriptWorld {
    let mut scripts = cache();
    scripts.definition.references = vec![
        cyrus_script::Property::ExtraFlag,
        cyrus_script::Property::LastDirection,
    ];
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::ForwardSpeed] = 8;
    actor.attributes[Attribute::StepType] = 1;
    actor.attributes[Attribute::Collide] = collide;
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference: 0,
                    },
                    formula: Formula {
                        first: Value::Integer {
                            value: i32::from(living),
                        },
                        rest: vec![],
                    },
                },
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    if pending {
        scripts.definition.functions.push(Function {
            name: cyrus_script::Native::PlayAnimation,
            param_count: 2,
            kind: CallKind::Function,
        });
        actor.program.steps.insert(
            1,
            Step {
                next: 2,
                operation: Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(Call {
                                target: Object::Me,
                                function_id: 1,
                                function: cyrus_script::Native::PlayAnimation,
                                kind: CallKind::Function,
                                arguments: vec![
                                    Value::Integer { value: 30 },
                                    Value::Integer { value: 0 },
                                ],
                            }),
                        },
                        rest: vec![],
                    },
                },
            },
        );
        actor.program.steps.insert(
            2,
            Step {
                next: 3,
                operation: Operation::End { target: 0 },
            },
        );
    }
    let end = if pending { 2 } else { 1 };
    actor.program.steps.insert(
        end,
        Step {
            next: end + 1,
            operation: Operation::Assign {
                destination: Place::Local { index: 0 },
                formula: Formula {
                    first: Value::Read {
                        place: Place::Property {
                            object: Object::Named { string: 0 },
                            reference: 1,
                        },
                    },
                    rest: vec![],
                },
            },
        },
    );
    actor.program.steps.insert(
        end + 1,
        Step {
            next: end + 2,
            operation: Operation::End { target: 0 },
        },
    );
    scripts.content_id = scripts.content_identity().unwrap();
    let mut groups = vec![
        serde_json::json!({"group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}),
    ];
    if pending {
        groups.push(
            serde_json::json!({"group_index":30,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":30},{"opcode":2,"value":0}]}),
        );
    }
    if group_present {
        let mut commands = vec![];
        if sound {
            commands.push(serde_json::json!({"opcode":4,"sound_param":-17,"volume_shift":0}));
        }
        commands.push(serde_json::json!({"opcode":0,"frame_reference":18}));
        commands.push(serde_json::json!({"opcode":2,"value":0}));
        groups.push(
            serde_json::json!({"group_index":18,"anim_id":1,"anim_type":0,"commands":commands}),
        );
    }
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","animation_groups":groups}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, cyrus_script::AnimationGroup::PANIC)
        .unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
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
    world
}

fn inputs() -> OrdinaryForwardAmountInputs {
    OrdinaryForwardAmountInputs {
        selected_player: None,
        holding_weapon: false,
        frame_scale: 256,
        ticks: 1,
        duration: 4,
        elapsed: 0,
    }
}

#[test]
fn last_direction_observes_a_real_running_producer_and_pre_store_refusal() {
    for (living, amount, classification) in [(0, 512, 2), (1, 0, 0)] {
        let mut world = motion_fixture(true, false, 0, living, false);
        let id = world.id(0).unwrap();
        assert_eq!(
            world
                .ordinary_forward_amount(id, inputs(), None, &mut |_, _| Ok(()))
                .unwrap(),
            amount
        );
        advance(&mut world, 0).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [classification]
        );
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .motion_classification,
            0
        );
    }
}

#[test]
fn last_direction_retains_the_running_store_after_animation_failure() {
    let mut world = motion_fixture(true, true, 0, 0, false);
    let id = world.id(0).unwrap();
    let error = world
        .ordinary_forward_amount(id, inputs(), None, &mut |_, _| {
            anyhow::bail!("classification sound refused")
        })
        .unwrap_err();
    assert!(format!("{error:#}").contains("classification sound refused"));
    // A separate script receiver reads the failed actor without advancing its failed animation.
    advance(&mut world, 1).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [2]);
    assert_eq!(world.actors()[0].control_state().unwrap().move_counter, 0);
}
