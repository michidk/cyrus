//! Sflags reaches retained collision metadata through public script and model callers.
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
    scripts.definition.references = vec![cyrus_script::Property::Sflags; 295];
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

fn prepare_models(
    world: &mut ScriptWorld,
    scripts: &Scripts,
) -> cyrus_assets::animation::Animations {
    use cyrus_assets::collision::{ActorCollisionModels, CollisionGeometry, CollisionSubobject};
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "mpob_objects": [{"model_id":-1},{"model_id":-1}],
            "actors":[{"script_name":"LOGIC","model_load_path":"static","mpsz_bounds_primary":-1,
                "raex":{"unknown_12":0,"v_vertex":0,"v_size":0}}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let models = cyrus_collision::actor_models::ActorModelCollision::new(&ActorCollisionModels {
        object_count: 2,
        models: BTreeMap::from([(
            "STATIC".into(),
            CollisionGeometry {
                version: Some(*b"v5.0"),
                radius: 42.0,
                vertices: vec![[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [0.0, 0.0, 20.0]],
                normals: vec![[0.0, -1.0, 0.0]],
                faces: vec![vec![0, 1, 2]],
                textures: Some(vec![None]),
                subobjects: vec![CollisionSubobject {
                    center: [0.0; 3],
                    radius: 42.0,
                    extents: Some([1.0; 3]),
                    faces: vec![0],
                }],
            },
        )]),
        instances: BTreeMap::from([(0, "STATIC".into()), (1, "STATIC".into())]),
        collision_flags: BTreeMap::from([(0, 6), (1, 7)]),
        unsupported: Default::default(),
    })
    .unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &models).unwrap();
    data
}

fn fixture(
    object: Object,
    value: i32,
    write: bool,
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let mut scripts = counter_scripts(object, value, write);
    for placement in &mut scripts.instances {
        placement.object_type = 1;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    let data = prepare_models(&mut world, &scripts);
    (world, scripts, data)
}

#[test]
fn sflags_reads_loaded_metadata_and_routes_full_low_byte_writes_to_live_candidates() {
    use cyrus_collision::candidates::{
        CandidateOwner, CandidateQuery, CollisionCandidates, ModelBounds, ModelInstance,
    };
    for (object, receiver, initial, other) in [
        (Object::Me, 1, 7, 6),
        (Object::Named { string: 0 }, 0, 6, 7),
    ] {
        let (mut read, _, _) = fixture(object.clone(), 0, false);
        advance(&mut read, 1).unwrap();
        assert_eq!(read.actors()[1].runtime().unwrap().variables(), [initial]);
        assert_eq!(read.flags(), [73]);
        for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
            let (mut world, _, data) = fixture(object.clone(), value, true);
            let before: Vec<_> = world
                .actors()
                .iter()
                .map(|a| serde_json::to_value(a.control_state().unwrap()).unwrap())
                .collect();
            let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
            world.refresh_model_presentations(&data, &ids).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            let live = world.live_collision_inputs(&ids).unwrap();
            assert_eq!(live[receiver].collision_flags, expected as u8);
            assert_eq!(live[1 - receiver].collision_flags, other);
            for (actor, control) in world.actors().iter().zip(&before) {
                assert_eq!(
                    serde_json::to_value(actor.control_state().unwrap()).unwrap(),
                    *control
                );
            }
            let query = CandidateQuery::new(
                [0; 3],
                Some(ModelBounds {
                    instance: ModelInstance::Runtime(1 - receiver),
                    position: [0.0; 3],
                    radius: 42.0,
                }),
            )
            .unwrap();
            let candidates = CollisionCandidates::build(&query, &[], &live, &[]).unwrap();
            assert_eq!(candidates.faces.len(), 1);
            assert_eq!(candidates.faces[0].owner, CandidateOwner::Actor(receiver));
            assert_eq!(candidates.faces[0].collision_flags, expected as u8);
            assert!(candidates.actors.is_empty());
        }
    }
}

#[test]
fn sflags_unprepared_metadata_and_absent_receivers_fail_before_continuation() {
    for write in [false, true] {
        for missing_receiver in [false, true] {
            let scripts = counter_scripts(
                if missing_receiver {
                    Object::Player
                } else {
                    Object::Me
                },
                207,
                write,
            );
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            assert!(format!("{error:#}").contains(if missing_receiver {
                "selected player unavailable"
            } else {
                "sflags: collision metadata unavailable"
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
fn sflags_failed_write_does_not_invent_metadata_before_real_resource_preparation() {
    let mut scripts = counter_scripts(Object::Me, 207, true);
    for placement in &mut scripts.instances {
        placement.object_type = 1;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    assert!(advance(&mut world, 0).is_err());
    let data = prepare_models(&mut world, &scripts);
    let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
    world.refresh_model_presentations(&data, &ids).unwrap();
    let live = world.live_collision_inputs(&ids).unwrap();
    assert_eq!(
        live.iter().map(|a| a.collision_flags).collect::<Vec<_>>(),
        [6, 7]
    );
    assert_eq!(world.flags(), [12]);
    assert!(advance(&mut world, 0).is_err());
}
