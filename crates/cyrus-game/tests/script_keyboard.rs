//! Explicit InKey inputs execute through the public native dispatcher.
use cyrus_assets::script::*;
use cyrus_game::script::{ScriptInputs, ScriptKeys, ScriptStage, ScriptWorld};
use cyrus_script::Thread;
use cyrus_script::program::*;
use std::collections::BTreeMap;

fn cache() -> Scripts {
    let program = Program {
        version: 4,
        entry: 0,
        variables: vec![4],
        strings: vec![],
        steps: BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Increment {
                        destination: Place::Local { index: 0 },
                        delta: 1,
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::Assign {
                        destination: Place::Flag { index: 0 },
                        formula: Formula {
                            first: Value::Read {
                                place: Place::Local { index: 0 },
                            },
                            rest: vec![],
                        },
                    },
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]),
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

#[test]
fn in_key_accepts_only_the_supplied_post_clear_event_without_consuming_it() -> anyhow::Result<()> {
    for (event, held, alt, code, expected) in [
        (Some(b'R'), 0, false, 114, 1),
        (Some(b'q'), 0, false, 114, 0),
        (None, 0, false, 114, 0),
        (None, 1 << (b'r' - b'a'), false, 114, 1),
        (Some(b'q'), 0, false, 0x6171, 0),
        (Some(b'q'), 0, true, 0x6171, 1),
    ] {
        let mut scripts = cache();
        scripts.definition.functions.push(Function {
            name: cyrus_script::Native::InKey,
            param_count: 1,
            kind: CallKind::Function,
        });
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Assign {
                        destination: Place::Local { index: 0 },
                        formula: Formula {
                            first: Value::Call {
                                call: Box::new(Call {
                                    function_id: 1,
                                    function: cyrus_script::Native::InKey,
                                    kind: CallKind::Function,
                                    target: Object::Me,
                                    arguments: vec![Value::Integer { value: code }],
                                }),
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
                    operation: Operation::Increment {
                        destination: Place::Flag { index: 0 },
                        delta: 1,
                    },
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]);
        scripts.content_id = scripts.content_identity()?;
        let mut world = ScriptWorld::prepare(&scripts, 1)?;
        let id = world.id(0).unwrap();
        let keys = ScriptKeys {
            latest_letter: event,
            letters: held,
            alt,
            ..Default::default()
        };
        for expected_flag in [13, 14] {
            world.advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    script_keys: Some(keys),
                    ..Default::default()
                },
                None,
                None,
                10,
            )?;
            assert_eq!(
                world.actors()[0].runtime().unwrap().variables()[0],
                expected
            );
            assert_eq!(world.flags()[0], expected_flag);
        }
        // The caller's next snapshot models the documented event clear.
        world.advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                script_keys: Some(ScriptKeys {
                    latest_letter: None,
                    ..keys
                }),
                ..Default::default()
            },
            None,
            None,
            10,
        )?;
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables()[0],
            i32::from(held != 0)
        );
        assert_eq!(world.flags()[0], 15);
    }
    Ok(())
}

fn space_world() -> ScriptWorld {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: cyrus_script::Native::SpacePressed,
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(Call {
                        function_id: 1,
                        function: cyrus_script::Native::SpacePressed,
                        kind: CallKind::Function,
                        target: Object::Me,
                        arguments: vec![],
                    }),
                },
                rest: vec![],
            },
        },
        Operation::Increment {
            destination: Place::Flag { index: 0 },
            delta: 1,
        },
        Operation::End { target: 0 },
    ]
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
    ScriptWorld::prepare(&scripts, 1).unwrap()
}

#[test]
fn space_pressed_reads_held_space_repeatedly_without_using_latest_events() {
    for (space, event) in [
        (false, None),
        (true, None),
        (false, Some(b'R')),
        (true, Some(b'R')),
    ] {
        let mut world = space_world();
        let id = world.id(0).unwrap();
        let before = world.actors()[0].attributes().clone();
        let keys = ScriptKeys {
            space,
            latest_letter: event,
            ..Default::default()
        };
        for counter in [13, 14] {
            world
                .advance(
                    id,
                    ScriptStage::Body(Thread::Main),
                    ScriptInputs {
                        script_keys: Some(keys),
                        ..Default::default()
                    },
                    None,
                    None,
                    10,
                )
                .unwrap();
            assert!(
                world.actors()[0].failure().is_none(),
                "{:?}",
                world.actors()[0].failure()
            );
            assert_eq!(
                world.actors()[0].runtime().unwrap().variables()[0],
                i32::from(space)
            );
            assert_eq!(world.flags(), [counter]);
            assert_eq!(world.actors()[0].attributes(), &before);
        }
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    script_keys: Some(ScriptKeys {
                        space: false,
                        ..keys
                    }),
                    ..Default::default()
                },
                None,
                None,
                10,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 0);
        assert_eq!(world.flags(), [15]);
    }
}

#[test]
fn space_pressed_reports_a_missing_keyboard_sample_before_continuing() {
    let mut world = space_world();
    let error = world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs::default(),
            None,
            None,
            10,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("SpacePressed keyboard sample unavailable"));
    assert_eq!(world.flags(), [12]);
    assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 4);
}
