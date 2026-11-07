//! Receiver-owned vertical-motion timer uses public property dispatch.
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
    scripts.definition.references = vec![cyrus_soup::Property::OriginTime; 295];
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
fn origin_time_routes_low_bytes_and_preserves_other_motion_state() {
    for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["fall_timer"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .fall_timer,
                0
            );
        }
    }
}

#[test]
fn origin_time_reads_zero_and_rejects_absent_owners_before_continuation() {
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
                "origin_time control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn falling_fixture(
    timer: i32,
    start_fall: bool,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = counter_scripts(Object::Me, timer, true);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::JumpHeight] = 16;
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::End { target: 1 };
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 7,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
            "script_name":"LOGIC","model_load_path":"none"}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    if start_fall {
        assert!(world.begin_static_ordinary_fall(id, &data).unwrap());
    }
    advance(&mut world, 0).unwrap();
    (world, data)
}

fn environment() -> cyrus_script::script::ObstacleEnvironment {
    cyrus_script::script::ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    }
}

#[test]
fn unsigned_property_controls_signed_falling_timer_and_real_tick_progress() {
    for (seed, expected_y, after) in [
        (30, 0, 22),
        (1, 3584, 249),
        (0, 2560, 0),
        (128, 2560, 128),
        (-1, 2560, 255),
    ] {
        let (mut world, data) = falling_fixture(seed, true);
        let id = world.id(0).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [seed & 255]
        );
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
        assert!(
            world
                .continue_ordinary_falling(id, &data, environment(), 8, 0, 50, &mut |_, _| panic!(
                    "static falling dispatched sound"
                ))
                .unwrap()
        );
        advance(&mut world, 0).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [after]);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [0, expected_y, 0]
        );
        assert_eq!(world.actors()[1].control_state().unwrap().fall_timer, 0);
        assert_eq!(world.actors()[0].control_state().unwrap().extra_time, 0);
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn a_timer_write_does_not_start_falling_and_existing_fall_refuses_reinitialization() {
    let (mut world, data) = falling_fixture(7, false);
    let id = world.id(0).unwrap();
    assert!(
        !world
            .continue_ordinary_falling(id, &data, environment(), 8, 0, 50, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
    assert!(world.begin_static_ordinary_fall(id, &data).unwrap());
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [30]);
    let (mut world, data) = falling_fixture(7, true);
    let id = world.id(0).unwrap();
    assert!(!world.begin_static_ordinary_fall(id, &data).unwrap());
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [7]);
}

#[test]
fn unsupported_factor_is_rejected_by_the_consumer_without_timer_or_position_writes() {
    let (mut world, data) = falling_fixture(70, true);
    let id = world.id(0).unwrap();
    let before = world.spatial(id).unwrap().local.position;
    let error = world
        .continue_ordinary_falling(id, &data, environment(), 8, 0, 50, &mut |_, _| Ok(()))
        .unwrap_err();
    assert!(format!("{error:#}").contains("factor index 70"));
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [70]);
    assert_eq!(world.flags(), [73]);
}
