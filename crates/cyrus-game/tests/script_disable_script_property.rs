//! Receiver-owned runtime script-control byte uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::*;
use cyrus_game::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_script::Thread;
use cyrus_script::program::*;
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
    scripts.definition.references = vec![cyrus_script::Property::DisableScript; 295];
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
fn disable_script_routes_low_bytes_and_preserves_other_motion_state() {
    for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["script_control"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .script_control,
                0
            );
        }
    }
}

#[test]
fn disable_script_reads_zero_and_rejects_absent_owners_before_continuation() {
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
                "disable_script control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn gate_fixture(flags: i32) -> ScriptWorld {
    let mut scripts = counter_scripts(Object::Me, flags, true);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::End { target: 4 };
    actor.program.steps.insert(
        4,
        Step {
            next: 5,
            operation: Operation::Assign {
                destination: Place::Flag { index: 0 },
                formula: Formula {
                    first: Value::Integer { value: 99 },
                    rest: vec![],
                },
            },
        },
    );
    actor.program.steps.insert(
        5,
        Step {
            next: 6,
            operation: Operation::End { target: 4 },
        },
    );
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.flags(), [73]);
    world
}

#[test]
fn property_controls_real_ordinary_main_without_stopping_explicit_body() {
    for (flags, expected) in [
        (0, 99),
        (1, 73),
        (2, 99),
        (4, 99),
        (5, 73),
        (128, 99),
        (129, 73),
    ] {
        let mut world = gate_fixture(flags);
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                30,
            )
            .unwrap();
        assert_eq!(world.flags(), [expected]);
        assert_eq!(
            world.actors()[0].control_state().unwrap().script_control,
            flags as u8
        );
        assert!(world.actors()[0].failure().is_none());
    }
}

#[test]
fn native_enable_script_changes_only_its_mask_in_property_seeded_byte() {
    let mut scripts = counter_scripts(Object::Me, 133, true);
    scripts.definition.functions.push(Function {
        name: cyrus_script::Native::EnableScript,
        param_count: 0,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    let mut old = std::mem::take(&mut actor.program.steps);
    actor.program.steps = BTreeMap::from([
        (0, old.remove(&0).unwrap()),
        (
            1,
            Step {
                next: 2,
                operation: Operation::Call {
                    call: Call {
                        function_id: 1,
                        function: cyrus_script::Native::EnableScript,
                        target: Object::Me,
                        kind: CallKind::Function,
                        arguments: vec![],
                    },
                },
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: old.remove(&1).unwrap().operation,
            },
        ),
        (
            3,
            Step {
                next: 4,
                operation: old.remove(&2).unwrap().operation,
            },
        ),
        (
            4,
            Step {
                next: 5,
                operation: Operation::End { target: 2 },
            },
        ),
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [132]);
    assert_eq!(
        world.actors()[0].control_state().unwrap().script_control,
        132
    );
    assert_eq!(world.actors()[1].control_state().unwrap().script_control, 0);
    assert_eq!(world.flags(), [73]);
}

#[test]
fn missing_task_timing_fails_before_ordinary_continuation_then_valid_retry_runs() {
    let mut world = gate_fixture(0);
    let id = world.id(0).unwrap();
    let error = world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs::default(),
            None,
            None,
            30,
        )
        .unwrap_err();
    assert!(error.to_string().contains("requires frame_scale"));
    assert_eq!(world.flags(), [73]);
    assert_eq!(world.actors()[0].control_state().unwrap().script_control, 0);
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            30,
        )
        .unwrap();
    assert_eq!(world.flags(), [99]);
}
