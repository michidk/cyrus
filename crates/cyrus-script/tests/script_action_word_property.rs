//! Receiver-owned accepted directional word uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::*;
use cyrus_script::script::JumpMotion;
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
    scripts.definition.references = vec![cyrus_soup::Property::Action; 295];
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
fn action_routes_full_signed_words_and_preserves_other_motion_state() {
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
            before["actions"] = serde_json::json!({"pending": 0, "accepted": expected as u32});
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .actions
                    .accepted,
                0
            );
        }
    }
}

#[test]
fn action_reads_zero_and_rejects_absent_owners_before_continuation() {
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
                "action control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn ordinary_collision_scene(
    world: u32,
    models: cyrus_collision::actor_models::ActorModelCollision,
) -> cyrus_script::player::data::PlayerScene {
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::player::data::{MovementSettings, PlayerScene};
    PlayerScene {
        source_identity: None,
        world,
        model_radii: Default::default(),
        control_codes: None,
        settings: MovementSettings {
            standing_height: -2,
            post_collide_height: 1,
            slide_range: 1,
            slide_speed: None,
            slide_hurt: None,
            tap_time: None,
            sheath_sword_delay: None,
            defend_interval: None,
            swim_depth: None,
            max_remap_objects: None,
            turn_speed: 1,
            turn_max_speed: 1,
            poly_push_units: 1,
            joystick: None,
        },
        definition: None,
        terrain: None,
        collision: cyrus_assets::collision::CollisionScene {
            sphere_object_scale: Some(256),
            ..Default::default()
        },
        static_collision: StaticCollision::new(&Default::default()).unwrap(),
        actor_collision: Some(models),
        unavailable: None,
    }
}

fn automatic_fixture(
    seed: i32,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    let mut scripts = counter_scripts(Object::Me, seed, true);
    scripts.instances.truncate(1);
    scripts.instances[0].object_type = PlacementType::MODEL;
    scripts.instances[0].active = 1;
    scripts.instances[0].map_position = [600, 20, 600];
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    actor.attributes[Attribute::Collide] = 4;
    actor.attributes[Attribute::FallHold] = 1;
    let timer_write = Operation::Assign {
        destination: Place::Property {
            object: Object::Me,
            reference: 295,
        },
        formula: Formula {
            first: Value::Integer { value: 36 },
            rest: vec![],
        },
    };
    let mut old = std::mem::take(&mut actor.program.steps);
    let pending_write = Operation::Assign {
        destination: Place::Property {
            object: Object::Me,
            reference: 296,
        },
        formula: Formula {
            first: Value::Integer { value: 512 },
            rest: vec![],
        },
    };
    actor.program.steps = BTreeMap::from([
        (0, old.remove(&0).unwrap()),
        (
            1,
            Step {
                next: 2,
                operation: timer_write,
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: pending_write,
            },
        ),
        (
            3,
            Step {
                next: 4,
                operation: old.remove(&1).unwrap().operation,
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
                operation: Operation::End { target: 3 },
            },
        ),
    ]);
    scripts.definition.references.extend([
        cyrus_soup::Property::ActionTime,
        cyrus_soup::Property::ActionFlag,
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 7,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"mpob_objects":[{"model_id":-1}],"actors":[{
            "script_name":"LOGIC","model_load_path":"static","mpsz_bounds_primary":-1,
            "raex":{"unknown_12":0,"v_vertex":0,"v_size":0}}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let models = cyrus_collision::actor_models::ActorModelCollision::new(
        &cyrus_assets::collision::ActorCollisionModels {
            object_count: 1,
            models: std::collections::BTreeMap::from([(
                "STATIC".into(),
                cyrus_assets::collision::CollisionGeometry {
                    version: Some(*b"v5.0"),
                    radius: 42.0,
                    vertices: vec![[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [0.0, 0.0, 20.0]],
                    normals: vec![[0.0, -1.0, 0.0]],
                    faces: vec![vec![0, 1, 2]],
                    textures: Some(vec![None]),
                    subobjects: vec![cyrus_assets::collision::CollisionSubobject {
                        center: [0.0; 3],
                        radius: 42.0,
                        extents: Some([1.0; 3]),
                        faces: vec![0],
                    }],
                },
            )]),
            instances: std::collections::BTreeMap::from([(0, "STATIC".into())]),
            collision_flags: std::collections::BTreeMap::from([(0, 6)]),
            unsupported: Default::default(),
        },
    )
    .unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &models).unwrap();
    advance(&mut world, 0).unwrap();
    let scene = ordinary_collision_scene(7, models);
    (world, data, scene)
}

#[test]
fn accepted_bits_are_consumed_before_pending_promotion_and_high_bits_merge() {
    for (seed, cleared, merged) in [
        (257, 256, 768),
        (-255, -256, -256),
        (i32::MIN + 1, i32::MIN, i32::MIN + 512),
    ] {
        let (mut world, data, scene) = automatic_fixture(seed);
        assert_eq!(
            world.actors()[0].control_state().unwrap().actions,
            [512, seed as u32]
        );
        for (ticks, pending, timer, accepted) in [(0, 512, 36, cleared), (1, 0, 0, merged)] {
            world
                .update_ordinary(
                    &data,
                    [600, 600],
                    cyrus_script::script::OrdinaryInputs {
                        scripts: ScriptInputs {
                            nominal_ticks: Some(ticks),
                            frame_scale: Some(256),
                            fine_ticks: Some(0),
                            step_fps: Some(12),
                            ..Default::default()
                        },
                        scene: Some(&scene),
                    },
                    &mut cyrus_game::random::EngineRandom::default(),
                    &mut |_, _| Ok(()),
                    100,
                )
                .unwrap();
            assert!(
                world.actors()[0].failure().is_none(),
                "{:?}",
                world.actors()[0].failure()
            );
            let c = world.actors()[0].control_state().unwrap();
            assert_eq!(c.actions, [pending, accepted as u32]);
            assert_eq!(c.jump_window, timer);
            assert_eq!(c.queued_motion, JumpMotion::NONE);
            advance(&mut world, 0).unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [accepted]);
            assert_eq!(world.flags(), [73]);
        }
    }
}

#[test]
fn missing_scene_preserves_distinct_accepted_pending_words_and_blocks_continuation() {
    let (mut world, data, _) = automatic_fixture(257);
    world
        .update_ordinary(
            &data,
            [600, 600],
            cyrus_script::script::OrdinaryInputs {
                scripts: ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                scene: None,
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    assert!(
        world.actors()[0]
            .failure()
            .unwrap()
            .contains("ordinary collision scene unavailable")
    );
    let c = world.actors()[0].control_state().unwrap();
    assert_eq!(c.actions, [512, 257]);
    assert_eq!(c.jump_window, 36);
    assert!(advance(&mut world, 0).is_err());
    assert_eq!(world.flags(), [73]);
}
