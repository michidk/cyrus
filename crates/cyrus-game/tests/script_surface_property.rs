//! Stored support surface is shared by property dispatch and native water queries.
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

fn surface_scripts(object: Object, value: Option<i32>) -> Scripts {
    let mut scripts = cache();
    scripts.definition.references = vec![cyrus_script::Property::UnderSurface; 295];
    scripts.definition.functions.push(Function {
        name: cyrus_script::Native::IsInDeepWater,
        param_count: 0,
        kind: CallKind::Function,
    });
    let place = Place::Property {
        object: object.clone(),
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
    if let Some(value) = value {
        operations.push(assign(place.clone(), Value::Integer { value }));
    }
    operations.push(assign(Place::Local { index: 0 }, Value::Read { place }));
    operations.push(assign(
        Place::Flag { index: 0 },
        Value::Call {
            call: Box::new(Call {
                target: object,
                function_id: 1,
                function: cyrus_script::Native::IsInDeepWater,
                kind: CallKind::Function,
                arguments: vec![],
            }),
        },
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
        30,
    )?;
    Ok(())
}

#[test]
fn surface_property_keeps_low_byte_and_native_water_query_reads_the_same_receiver() {
    for (value, byte, deep) in [
        (0, 0, 0),
        (1, 1, 0),
        (2, 2, 1),
        (4, 4, 1),
        (5, 5, 0),
        (11, 11, 0),
        (-1, 255, 0),
        (258, 2, 1),
        (256, 0, 0),
    ] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = surface_scripts(object, Some(value));
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [byte]);
            assert_eq!(world.flags(), [deep]);
            assert_eq!(
                world.actor_surface(world.id(receiver).unwrap()).unwrap(),
                byte as u8
            );
            assert_eq!(
                world
                    .actor_surface(world.id(1 - receiver).unwrap())
                    .unwrap(),
                0
            );
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
        }
    }
}

#[test]
fn surface_property_reads_allocated_zero_without_geometry_or_animation() {
    let scripts = surface_scripts(Object::Me, None);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    assert_eq!(world.flags(), [0]);
}

#[test]
fn surface_property_missing_owner_or_receiver_fails_before_later_native_and_stays_failed() {
    for write in [None, Some(2)] {
        for missing_receiver in [false, true] {
            let object = if missing_receiver {
                Object::Player
            } else {
                Object::Me
            };
            let mut scripts = surface_scripts(object, write);
            if !missing_receiver {
                scripts.instances[0].object_type = 2;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            let expected = if missing_receiver {
                "Player property selected player unavailable"
            } else {
                "surface state unavailable"
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
