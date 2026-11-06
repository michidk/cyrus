//! Receiver-owned packed movement counter uses public property dispatch.
use cyrus_assets::script::*;
use cyrus_game::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_script::program::*;
use cyrus_script::{AnimationGroup, Attribute, Thread};
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
    scripts.definition.references = vec![cyrus_script::Property::MoveCounter; 295];
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
fn move_counter_routes_low_bytes_and_preserves_other_motion_state() {
    for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["move_counter"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .move_counter,
                0
            );
        }
    }
}

#[test]
fn move_counter_reads_zero_and_rejects_absent_owners_before_continuation() {
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
                "move_counter control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn forward_fixture(counter: i32, collide: u8, sound: bool) -> ScriptWorld {
    forward_fixture_with_living(counter, collide, sound, 0)
}

fn forward_fixture_with_living(counter: i32, collide: u8, sound: bool, living: u8) -> ScriptWorld {
    let mut scripts = counter_scripts(Object::Me, counter, true);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::ForwardSpeed] = 8;
    actor.attributes[Attribute::StepType] = 1;
    actor.attributes[Attribute::Collide] = collide;
    // Park at the property reader so later polls cannot reseed the counter.
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::End { target: 1 };
    if living != 0 {
        scripts
            .definition
            .references
            .push(cyrus_script::Property::ExtraFlag);
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
                        first: Value::Integer {
                            value: i32::from(living),
                        },
                        rest: vec![],
                    },
                },
            },
        );
    }
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
    let mut commands = vec![];
    if sound {
        commands.push(serde_json::json!({"opcode":4,"sound_param":-17,"volume_shift":0}));
    }
    commands.push(serde_json::json!({"opcode":0,"frame_reference":18}));
    commands.push(serde_json::json!({"opcode":2,"value":0}));
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 7,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
            "script_name":"LOGIC","animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
            {"group_index":18,"anim_id":1,"anim_type":0,"commands":commands}]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, AnimationGroup::PANIC)
        .unwrap();
    advance(&mut world, 0).unwrap();
    world
}

fn amount_inputs() -> cyrus_game::script::OrdinaryForwardAmountInputs {
    cyrus_game::script::OrdinaryForwardAmountInputs {
        selected_player: None,
        holding_weapon: false,
        frame_scale: 256,
        ticks: 1,
        duration: 4,
        elapsed: 0,
    }
}

#[test]
fn property_seed_controls_real_forward_startup_and_preserves_other_direction_bits() {
    for (seed, amount, after) in [
        (207, 512, 223),
        (223, 682, 239),
        (239, 1024, 255),
        (255, 2048, 255),
    ] {
        let mut world = forward_fixture(seed, 0, false);
        let id = world.id(0).unwrap();
        let position = world.spatial(id).unwrap().local.position;
        assert_eq!(
            world
                .ordinary_forward_amount(id, amount_inputs(), None, &mut |_, _| Ok(()))
                .unwrap(),
            amount
        );
        advance(&mut world, 0).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [after]);
        assert_eq!(world.actors()[1].control_state().unwrap().move_counter, 0);
        assert_eq!(world.spatial(id).unwrap().local.position, position);
    }
}

#[test]
fn saturated_counter_bypasses_support_but_support_and_sound_failures_do_not_advance_it() {
    let mut world = forward_fixture(48, 2, false);
    let id = world.id(0).unwrap();
    assert_eq!(
        world
            .ordinary_forward_amount(id, amount_inputs(), None, &mut |_, _| Ok(()))
            .unwrap(),
        2048
    );
    for (counter, sound, expected) in [
        (32, false, "step geometry unavailable"),
        (48, true, "counter sound failure"),
    ] {
        let mut world = forward_fixture(counter, 2, sound);
        let id = world.id(0).unwrap();
        let error = world
            .ordinary_forward_amount(id, amount_inputs(), None, &mut |_, _| {
                anyhow::bail!("counter sound failure")
            })
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        assert_eq!(
            world.actors()[0].control_state().unwrap().move_counter,
            counter as u8
        );
        if sound {
            assert!(world.actors()[0].animation_failure().is_some());
            assert!(advance(&mut world, 0).is_err());
        } else {
            advance(&mut world, 0).unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [counter]);
        }
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn living_refusal_retains_seed_without_support_or_sound_admission() {
    let mut world = forward_fixture_with_living(255, 2, true, 1);
    let id = world.id(0).unwrap();
    assert_eq!(
        world
            .ordinary_forward_amount(id, amount_inputs(), None, &mut |_, _| {
                panic!("refused movement dispatched sound")
            })
            .unwrap(),
        0
    );
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [255]);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        0
    );
    assert_eq!(world.flags(), [73]);
}
