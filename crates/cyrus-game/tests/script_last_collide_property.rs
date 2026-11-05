//! Receiver-owned collision-response low byte uses public property dispatch.
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
    scripts.definition.references = vec![cyrus_script::Property::LastCollide; 295];
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
fn last_collide_routes_low_bytes_and_preserves_other_motion_state() {
    for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["last_collide"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .last_collide
                    .bits() as u8,
                0
            );
        }
    }
}

#[test]
fn last_collide_reads_zero_and_rejects_absent_owners_before_continuation() {
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
                scripts.instances[0].object_type = 2;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            assert!(format!("{error:#}").contains(if absent_receiver {
                "Player property selected player unavailable"
            } else {
                "last_collide control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn response_fixture(value: i32, water_mark: bool) -> ScriptWorld {
    let mut scripts = counter_scripts(Object::Me, value, true);
    scripts
        .definition
        .references
        .push(cyrus_script::Property::LastCollideCount);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    let mut old = std::mem::take(&mut actor.program.steps);
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference: 295,
                    },
                    formula: Formula {
                        first: Value::Integer { value: 37 },
                        rest: vec![],
                    },
                },
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: old.remove(&0).unwrap().operation,
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
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    if water_mark {
        world.set_actor_surface(id, 0).unwrap();
        assert!(world.retain_ordinary_water_position(id).unwrap());
        let rollback = world
            .capture_ordinary_rollback(id, &mut |_, _| Ok(()))
            .unwrap();
        world.set_actor_surface(id, 1).unwrap();
        assert!(
            world
                .rollback_ordinary_water(id, None, &rollback, &mut |_, _| Ok(()))
                .unwrap()
        );
    }
    advance(&mut world, 0).unwrap();
    let c = world.actors()[0].control_state().unwrap();
    assert_eq!(c.last_collide.bits() as u8, value as u8);
    assert_eq!(
        c.last_collide.bits().wrapping_shr(16) as u8,
        u8::from(water_mark)
    );
    assert_eq!(c.last_collide_count, 37);
    assert_eq!(c.last_collide.bits() & 0xff00_ff00, 0);
    world
}

#[test]
fn wrapper_distinguishes_low_byte_from_independent_transition_and_age() {
    for (value, marked, age) in [
        (0, false, 0),
        (128, false, 38),
        (255, false, 38),
        (0, true, 38),
        (128, true, 38),
    ] {
        let mut world = response_fixture(value, marked);
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
        let c = world.actors()[0].control_state().unwrap();
        assert_eq!(c.last_collide.bits() as u8, 0);
        assert_eq!(c.last_collide.bits().wrapping_shr(16) as u8, 0);
        assert_eq!(c.last_collide.bits() & 0xff00_ff00, 0);
        assert_eq!(c.last_collide_count, age);
        advance(&mut world, 0).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn missing_response_clock_retains_low_byte_and_independent_transition_until_retry() {
    let mut world = response_fixture(128, true);
    let id = world.id(0).unwrap();
    let error = world.finish_ordinary_script_response(id, None).unwrap_err();
    assert!(error.to_string().contains("requires nominal_ticks"));
    let c = world.actors()[0].control_state().unwrap();
    assert_eq!(c.last_collide.bits() as u8, 128);
    assert_eq!(c.last_collide.bits().wrapping_shr(16) as u8, 1);
    assert_eq!(c.last_collide_count, 37);
    world.finish_ordinary_script_response(id, Some(1)).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide_count,
        38
    );
}
