//! Deep walking checks its edge before classification and support.
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::*;
use cyrus_assets::surface::Surface;
use cyrus_game::script::MotionDirection;
use cyrus_game::script::{ScriptHud, ScriptInputs, ScriptSound, ScriptStage, ScriptWorld};
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
                object_type: PlacementType::MODEL,
                active: 0,
                model_name: String::new(),
                model_mode: 0,
                light_intensity: 0,
                light_radius: 0,
                light_rgb: [0; 3],
                texture: 0,
                map_position: [258, 0, 1000],
                angles: [0; 3],
            })
            .collect(),
        unsupported: BTreeMap::new(),
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

fn fixture(group: u16, ai_type: i32) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = cache();
    scripts.definition.references = vec![cyrus_script::Property::UnderSurface; 295];
    scripts.definition.functions.extend([
        Function {
            name: cyrus_script::Native::PlayAnimation,
            param_count: 2,
            kind: CallKind::Function,
        },
        Function {
            name: cyrus_script::Native::Wait,
            param_count: 1,
            kind: CallKind::Task,
        },
        Function {
            name: cyrus_script::Native::SetAiType,
            param_count: 1,
            kind: CallKind::Function,
        },
    ]);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::ForwardWalk] = 6;
    actor.attributes[Attribute::BackwardWalk] = 10;
    actor.attributes[Attribute::StepType] = 0;
    actor.attributes[Attribute::Collide] = 0;
    let call = |function_id, name: &str, kind, arguments| Operation::Call {
        call: Call {
            target: Object::Me,
            function_id,
            function: cyrus_script::Native::from_name(name).expect("known fixture native"),
            kind,
            arguments,
        },
    };
    let operations = [
        Operation::Assign {
            destination: Place::Property {
                object: Object::Me,
                reference: 294,
            },
            formula: Formula {
                first: Value::Integer { value: 2 },
                rest: vec![],
            },
        },
        call(
            3,
            "SetAIType",
            CallKind::Function,
            vec![Value::Integer { value: ai_type }],
        ),
        call(
            1,
            "PlayAnimation",
            CallKind::Function,
            vec![
                Value::Integer {
                    value: group.into(),
                },
                Value::Integer { value: 0 },
            ],
        ),
        call(2, "Wait", CallKind::Task, vec![Value::Integer { value: 4 }]),
        Operation::Assign {
            destination: Place::Flag { index: 0 },
            formula: Formula {
                first: Value::Integer { value: 77 },
                rest: vec![],
            },
        },
        Operation::End { target: 0 },
    ];
    actor.program.steps = operations
        .into_iter()
        .enumerate()
        .map(|(i, operation)| {
            (
                i as u32,
                Step {
                    next: i as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
            "script_name":"LOGIC","model_load_path":"none",
            "animation_groups":([0,20,22].into_iter().map(|g|
                serde_json::json!({"group_index":g,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":g},{"opcode":2,"value":0}]})).collect::<Vec<_>>()) }]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world
        .bind_animation(
            world.id(0).unwrap(),
            &data,
            cyrus_script::AnimationGroup::PANIC,
        )
        .unwrap();
    advance(&mut world, ScriptStage::Body(Thread::Main)).unwrap();
    (world, data)
}

fn advance(world: &mut ScriptWorld, stage: ScriptStage) -> anyhow::Result<()> {
    let hud = ScriptHud::default();
    world.advance(
        world.id(0)?,
        stage,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            hud: Some(&hud),
            shell_execution: Some(false),
            ..Default::default()
        },
        Some(&mut |_, _| Ok(())),
        None,
        30,
    )
}

fn ordinary_collision_scene(
    world: u32,
    models: cyrus_collision::actor_models::ActorModelCollision,
) -> cyrus_game::player::data::PlayerScene {
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_game::player::data::{MovementSettings, PlayerScene};
    PlayerScene {
        source_identity: None,
        world,
        model_radii: Default::default(),
        control_codes: None,
        settings: MovementSettings {
            joystick: None,
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

fn models() -> cyrus_collision::actor_models::ActorModelCollision {
    cyrus_collision::actor_models::ActorModelCollision::new(
        &cyrus_assets::collision::ActorCollisionModels {
            object_count: 2,
            collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
            ..Default::default()
        },
    )
    .unwrap()
}

fn continue_walk(
    world: &mut ScriptWorld,
    geometry: Option<cyrus_game::script::LocomotionGeometry<'_>>,
) -> anyhow::Result<()> {
    let hud = ScriptHud::default();
    world.continue_ordinary_motion(
        world.id(0)?,
        ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(1),
            hud: Some(&hud),
            locomotion_geometry: geometry,
            shell_execution: Some(false),
            ..Default::default()
        },
        None,
        &mut |_, _: ScriptSound| panic!("Circle requested sound"),
    )
}

fn geometry<'a>(
    scene: &'a cyrus_game::player::data::PlayerScene,
    data: &'a cyrus_assets::animation::Animations,
    prepared: &'a cyrus_game::script::OrdinaryCollisionQuery,
    distance: u32,
) -> cyrus_game::script::LocomotionGeometry<'a> {
    cyrus_game::script::LocomotionGeometry {
        ground: cyrus_game::script::OrdinaryGroundQuery {
            scene,
            animations: data,
            prepared,
            distance,
            sidedness: false,
        },
        environment: cyrus_game::script::ObstacleEnvironment {
            selected_player: None,
            auto_grab: false,
            item_system_ready: true,
            shell_execution: false,
            ticks: 0,
        },
    }
}

fn terrain(steep: bool) -> cyrus_collision::ground::TerrainGround {
    let heights = (0..256 * 256)
        .map(|index| {
            if steep {
                ((index % 256) * 512).min(32767) as i16
            } else {
                0
            }
        })
        .collect();
    cyrus_collision::ground::TerrainGround::new(
        heights,
        vec![false; 256 * 256],
        Some(vec![31; 256 * 256]),
    )
    .unwrap()
}

fn assert_stationary(
    world: &ScriptWorld,
    direction: MotionDirection,
    classification: MotionDirection,
) {
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(control.motion_direction, direction);
    assert_eq!(control.motion_classification, classification);
    assert_eq!(control.move_counter, 0);
    assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.position,
        [66048, 0, 256000]
    );
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert_eq!(world.flags(), [12]);
}

#[test]
fn deep_walk_acceptance_stores_classification_after_edge_and_preserves_real_wait_poll() {
    for (group, class, called, counter, z) in [
        (20, MotionDirection::WALK_FORWARD, 4, 16, 256768),
        (22, MotionDirection::WALK_BACKWARD, 8, 64, 253440),
    ] {
        let (mut world, data) = fixture(group, 7);
        let mut scene = ordinary_collision_scene(world.world, models());
        scene.terrain = Some(terrain(false));
        scene.collision.texture_surfaces = Some(Default::default());
        let id = world.id(0).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [258, 1000], &[])
            .unwrap();
        continue_walk(&mut world, Some(geometry(&scene, &data, &prepared, 128))).unwrap();
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.motion_classification, class);
        assert_eq!(control.motion_direction, class);
        assert_eq!(control.move_counter, counter);
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], called);
        assert_eq!(world.spatial(id).unwrap().local.position, [66048, 0, z]);
        assert_eq!(world.actor_surface(id).unwrap(), Surface::DEEPWATER);
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::NONE
        );
        advance(&mut world, ScriptStage::Tasks).unwrap();
        continue_walk(&mut world, Some(geometry(&scene, &data, &prepared, 128))).unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, [66048, 0, z]);
        for _ in 0..4 {
            advance(&mut world, ScriptStage::Tasks).unwrap();
        }
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        advance(&mut world, ScriptStage::Body(Thread::Main)).unwrap();
        assert_eq!(world.flags(), [77]);
    }
}

#[test]
fn deep_walk_blocked_edge_short_circuits_invalid_support_without_storing_classification() {
    for (group, class, z) in [
        (20, MotionDirection::WALK_FORWARD, 1064),
        (22, MotionDirection::WALK_BACKWARD, 936),
    ] {
        let (mut world, data) = fixture(group, 0);
        let mut scene = ordinary_collision_scene(world.world, models());
        scene.terrain = Some(terrain(true));
        scene.collision.texture_surfaces = Some(Default::default());
        let point = [258 * 256, 0, z * 256];
        assert!(
            scene
                .terrain
                .as_ref()
                .unwrap()
                .edge_blocked(point, scene.collision.texture_surfaces.as_ref().unwrap())
                .unwrap()
        );
        let id = world.id(0).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [258, 1000], &[])
            .unwrap();
        for _ in 0..2 {
            continue_walk(
                &mut world,
                Some(geometry(&scene, &data, &prepared, u32::MAX)),
            )
            .unwrap();
            assert_stationary(&world, class, MotionDirection::NONE);
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .last_collide
                    .bits() as u8,
                0
            );
            advance(&mut world, ScriptStage::Tasks).unwrap();
        }
    }
}

#[test]
fn deep_walk_missing_geometry_keeps_only_direction_prefix() {
    for (group, class) in [
        (20, MotionDirection::WALK_FORWARD),
        (22, MotionDirection::WALK_BACKWARD),
    ] {
        let (mut world, _) = fixture(group, 7);
        continue_walk(&mut world, None).unwrap();
        assert_stationary(&world, class, MotionDirection::NONE);
    }
}

#[test]
fn deep_walk_support_refusal_after_clear_edge_retains_classification() {
    for (group, class, body_marker, transition_marker) in [
        (20, MotionDirection::WALK_FORWARD, 0x80, 0),
        (22, MotionDirection::WALK_BACKWARD, 0, 1),
    ] {
        let (mut world, data) = fixture(group, 0);
        let scene = ordinary_collision_scene(world.world, models());
        let id = world.id(0).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [258, 1000], &[])
            .unwrap();
        continue_walk(&mut world, Some(geometry(&scene, &data, &prepared, 128))).unwrap();
        assert_stationary(&world, class, class);
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            control.last_collide.bits(),
            body_marker | (transition_marker << 8)
        );
        advance(&mut world, ScriptStage::Tasks).unwrap();
        continue_walk(&mut world, Some(geometry(&scene, &data, &prepared, 128))).unwrap();
        assert_stationary(&world, class, class);
    }
}

#[test]
fn deep_walk_edge_failure_precedes_store_but_support_failure_retains_store() {
    for (group, class) in [
        (20, MotionDirection::WALK_FORWARD),
        (22, MotionDirection::WALK_BACKWARD),
    ] {
        let (mut world, data) = fixture(group, 0);
        let mut scene = ordinary_collision_scene(world.world, models());
        scene.terrain = Some(terrain(false));
        let id = world.id(0).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [258, 1000], &[])
            .unwrap();
        let error =
            continue_walk(&mut world, Some(geometry(&scene, &data, &prepared, 128))).unwrap_err();
        assert!(format!("{error:#}").contains("edge probe SURFACE.INI assignments unavailable"));
        assert_stationary(&world, class, MotionDirection::NONE);
        scene.terrain = None;
        scene.world = 999;
        let error =
            continue_walk(&mut world, Some(geometry(&scene, &data, &prepared, 128))).unwrap_err();
        assert!(format!("{error:#}").contains("collision query belongs to another actor or world"));
        assert_stationary(&world, class, class);
    }
}
