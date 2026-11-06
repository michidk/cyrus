//! Receiver-owned AI delay retains unsigned property values through public snapshots.
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
    scripts.definition.references = vec![cyrus_script::Property::AiDelay; 295];
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
fn ai_delay_routes_unsigned_low_bytes_without_changing_adjacent_or_other_control() {
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
            before["ai_delay"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .ai_delay,
                0
            );
        }
    }
}

#[test]
fn ai_delay_allocation_zero_and_missing_owner_receiver_failures_are_distinct() {
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
                "ai_delay control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

#[test]
fn property_delay_is_consumed_by_public_save_restore_without_aliasing_ai_mode() {
    use cyrus_game::script::{WorldRestoreContext, WorldSnapshotSources};
    for (value, expected) in [(64, 64), (128, 128), (255, 255), (300, 44), (-1, 255)] {
        let mut scripts = counter_scripts(Object::Named { string: 0 }, value, true);
        scripts.definition.references[293] = cyrus_script::Property::AiMode;
        scripts.definition.references[292] = cyrus_script::Property::ExtraTime;
        let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
        let old = std::mem::take(&mut program.steps);
        for (offset, mut step) in old {
            step.next += 2;
            if let Operation::End { target } = &mut step.operation {
                *target = 3;
            }
            program.steps.insert(offset + 2, step);
        }
        for (offset, reference, value) in [(0, 292, 23), (1, 293, 11)] {
            program.steps.insert(
                offset,
                Step {
                    next: offset + 1,
                    operation: Operation::Assign {
                        destination: Place::Property {
                            object: Object::Named { string: 0 },
                            reference,
                        },
                        formula: Formula {
                            first: Value::Integer { value },
                            rest: vec![],
                        },
                    },
                },
            );
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        advance(&mut world, 1).unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: 7,
            metadata: serde_json::json!({"script_content_id": scripts.content_id, "actors": []}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let collision = Default::default();
        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let saved = world.snapshot(&sources).unwrap();
        let serialized = serde_json::to_vec(&saved).unwrap();
        let mut restored = ScriptWorld::from_snapshot(
            serde_json::from_slice(&serialized).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 9,
                globals: std::mem::take(world.globals_mut()),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let control = restored.actors()[0].control_state().unwrap();
        assert_eq!(control.ai_delay, expected);
        assert_eq!(control.ai_mode, 11);
        assert_eq!(control.ai_type.byte(), 0);
        assert_eq!(control.extra_time, 23);
        assert_eq!(restored.actors()[1].control_state().unwrap().ai_delay, 0);
        advance(&mut restored, 1).unwrap();
        assert_eq!(
            restored.actors()[1].runtime().unwrap().variables(),
            [i32::from(expected)]
        );
        assert_eq!(restored.flags(), [73]);
    }
}
