//! The session camera lives with the world and carries the `showPlayer` task through
//! the public stages and snapshots.
use cyrus_assets::{animation::Animations, script::*};
use cyrus_game::script::{
    GameplayGlobals, ScriptHud, ScriptInputs, ScriptStage, ScriptWorld, WorldRestoreContext,
    WorldSnapshotSources,
};
use cyrus_script::Thread;
use cyrus_script::program::*;
use serde_json::json;
use std::collections::BTreeMap;

const BODY: ScriptStage = ScriptStage::Body(Thread::Main);
const TASKS: ScriptStage = ScriptStage::Tasks;

fn actor(operations: Vec<Operation>, origin: u32) -> Actor {
    Actor {
        header_index: 0,
        code_origin: origin,
        instances: 1,
        namespace: None,
        instance_variables: vec![vec![]],
        location_offsets: vec![],
        attributes: BTreeMap::new(),
        combat_taunts: vec![],
        native_hook: None,
        program: Program {
            version: 4,
            entry: 0,
            variables: vec![],
            strings: vec![],
            steps: operations
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
                .collect(),
        },
    }
}

/// One map actor that calls `showPlayer` and then sets the flag, beside the standalone roles.
fn scripts(standalone: &[&str]) -> Scripts {
    let show_player = Operation::Call {
        call: Call {
            function_id: 1,
            function: "showPlayer".into(),
            target: Object::Me,
            kind: CallKind::Task,
            arguments: vec![],
        },
    };
    let set_flag = Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Integer { value: 9 },
            rest: vec![],
        },
    };
    let end = || vec![Operation::End { target: 0 }];
    let mut actors = BTreeMap::from([(
        "ACTOR".into(),
        actor(vec![show_player, set_flag, Operation::End { target: 0 }], 0),
    )]);
    for (index, name) in standalone.iter().enumerate() {
        actors.insert((*name).into(), actor(end(), 100 + index as u32 * 100));
    }
    let mut result = Scripts {
        version: 22,
        weapons: Default::default(),
        camera: Default::default(),
        combat_attacks: vec![],
        combat_defenses: vec![],
        in_combat_threshold: None,
        non_engaged_spacing: None,
        combat_clank_effects: [None; 2],
        node_timer: None,
        dialogue: DialogueSettings {
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
        updates: UpdateSettings {
            dummy_model_manager: false,
            task_system: true,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: false,
        },
        clock: ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        },
        actor_pool_capacity: 10,
        content_id: String::new(),
        world: 17,
        definition: Definition {
            functions: [
                ("NullFunction", 0, CallKind::Function),
                ("showPlayer", 0, CallKind::Task),
            ]
            .into_iter()
            .map(|(name, param_count, kind)| Function {
                name: name.into(),
                param_count,
                kind,
            })
            .collect(),
            flags: vec![Flag {
                name: "Ready".into(),
                flag_type: "NUMBER".into(),
                initial_value: 0,
            }],
            references: vec![],
            attributes: (0..256).map(|index| format!("attr_{index}")).collect(),
        },
        actors,
        instances: vec![Instance {
            object_id: 0,
            actor: "ACTOR".into(),
            object_type: 0,
            active: 0,
            model_name: String::new(),
            model_mode: 0,
            light_intensity: 0,
            light_radius: 0,
            light_rgb: [0; 3],
            map_position: [0; 3],
            angles: [0; 3],
        }],
        unsupported: BTreeMap::new(),
    };
    result.content_id = result.content_identity().unwrap();
    result
}

fn run(
    world: &mut ScriptWorld,
    index: usize,
    stage: ScriptStage,
    hud: Option<&ScriptHud>,
) -> anyhow::Result<()> {
    let id = world.id(index).unwrap();
    world.advance(
        id,
        stage,
        ScriptInputs {
            hud,
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        30,
    )
}

fn flags(world: &ScriptWorld, slot: usize) -> u8 {
    world.camera().unwrap().records[slot].flags
}

const PLAYER: usize = 1;
const CAMERA: usize = 2;

#[test]
fn camera_session_exists_only_with_the_player_and_camera_roles() {
    let world = ScriptWorld::prepare(&scripts(&["CYRUS", "CAMERA"]), 7).unwrap();
    let camera = world.camera().unwrap();
    assert_eq!(camera.selected, CameraSlot::new(0).unwrap());
    assert_eq!(camera.records[0].actor, world.id(PLAYER).unwrap());
    assert_eq!(camera.camera_actor, world.id(CAMERA).unwrap());
    assert_eq!(camera.alternate_actor, camera.camera_actor);
    assert_eq!(flags(&world, 0), 0x40);
    assert_eq!(flags(&world, 5), 0xc0);
    for roles in [&["CYRUS"][..], &["CAMERA"][..], &[][..]] {
        assert!(
            ScriptWorld::prepare(&scripts(roles), 7)
                .unwrap()
                .camera()
                .is_none()
        );
    }
}

use cyrus_game::script::camera::CameraSlot;

#[test]
fn show_player_holds_the_snap_bit_for_one_poll_then_completes() {
    let mut world = ScriptWorld::prepare(&scripts(&["CYRUS", "CAMERA"]), 7).unwrap();
    let hud = ScriptHud::default();
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert_eq!(world.flags(), [0]);
    assert_eq!(flags(&world, 0), 0x41);
    assert!(
        !world.camera().unwrap().marks.hold,
        "the first call only cuts"
    );
    run(&mut world, 0, TASKS, Some(&hud)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(flags(&world, 0), 0x40);
    assert!(
        world.camera().unwrap().marks.hold,
        "the release holds the render camera"
    );
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    assert_eq!(world.flags(), [9]);
}

#[test]
fn show_player_requests_the_combat_slot_while_the_player_holds_a_weapon() {
    let mut data = scripts(&["CYRUS", "CAMERA"]);
    data.actors
        .get_mut("CYRUS")
        .unwrap()
        .attributes
        .insert("attr_110".into(), 1);
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 7).unwrap();
    let hud = ScriptHud::default();
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    let camera = world.camera().unwrap();
    assert_eq!(camera.selected, CameraSlot::new(3).unwrap());
    assert_eq!(flags(&world, 3), 0x41);
    assert_eq!(flags(&world, 0), 0x40);
    run(&mut world, 0, TASKS, Some(&hud)).unwrap();
    assert_eq!(flags(&world, 3), 0x40);
}

#[test]
fn show_player_fails_before_changing_the_camera_without_its_inputs() {
    let mut world = ScriptWorld::prepare(&scripts(&["CYRUS", "CAMERA"]), 7).unwrap();
    let error = run(&mut world, 0, BODY, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("inventory owner unavailable"),
        "{error:#}"
    );
    assert_eq!(flags(&world, 0), 0x40);

    let mut world = ScriptWorld::prepare(&scripts(&["CYRUS"]), 7).unwrap();
    let hud = ScriptHud::default();
    let error = run(&mut world, 0, BODY, Some(&hud)).unwrap_err();
    assert!(
        format!("{error:#}").contains("camera session unavailable"),
        "{error:#}"
    );
}

#[test]
fn world_snapshot_resumes_show_player_on_the_rebound_camera_session() {
    let data = scripts(&["CYRUS", "CAMERA"]);
    let animations = Animations {
        version: 2,
        world: data.world,
        sections: BTreeMap::new(),
        models: BTreeMap::new(),
        unsupported: BTreeMap::new(),
        metadata: json!({"script_content_id": data.content_id, "actors": []}),
    };
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        player: None,
        scripts: &data,
        animations: &animations,
        collision: &collision,
    };
    let hud = ScriptHud::default();
    let mut live = ScriptWorld::prepare(&data, 7).unwrap();
    run(&mut live, 0, BODY, Some(&hud)).unwrap();
    let bytes = serde_json::to_vec(&live.snapshot(&sources).unwrap()).unwrap();
    let mut restored = ScriptWorld::from_snapshot(
        serde_json::from_slice(&bytes).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 19,
            globals: GameplayGlobals::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let camera = restored.camera().unwrap();
    assert_eq!(camera.records[0].actor, restored.id(PLAYER).unwrap());
    assert_eq!(camera.camera_actor, restored.id(CAMERA).unwrap());
    assert_eq!(flags(&restored, 0), 0x41);
    run(&mut restored, 0, TASKS, Some(&hud)).unwrap();
    assert_eq!(flags(&restored, 0), 0x40);
    assert_eq!(flags(&live, 0), 0x41, "the live world is unchanged");
}

#[test]
fn show_player_pan_task_runs_to_completion_and_restores_the_selection() {
    let mut data = scripts(&["CYRUS", "CAMERA"]);
    data.definition.functions[1] = Function {
        name: "showPlayerPan".into(),
        param_count: 1,
        kind: CallKind::Task,
    };
    data.actors
        .get_mut("ACTOR")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = Operation::Call {
        call: Call {
            function_id: 1,
            function: "showPlayerPan".into(),
            target: Object::Me,
            kind: CallKind::Task,
            arguments: vec![Value::Integer { value: 3 }],
        },
    };
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 7).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(PLAYER).unwrap();
    world
        .initialize_player_spatial(player, [4000, 9000, -6000], [100, 1900, 20])
        .unwrap();
    let hud = ScriptHud::default();
    let before = world.camera().unwrap().records[0].clone();
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert_eq!(
        world.camera().unwrap().selected.index(),
        2,
        "pan edits record two"
    );
    assert_eq!(world.camera().unwrap().scripted_active, 1);
    let mut passes = 0;
    while world.actors()[0].runtime().unwrap().active_tasks() != 0 {
        passes += 1;
        assert!(passes <= 8, "the pan must finish within its duration");
        run(&mut world, 0, TASKS, Some(&hud)).unwrap();
    }
    let camera = world.camera().unwrap();
    assert_eq!(camera.selected.index(), 0, "showPlayer re-selects the slot");
    assert_eq!((camera.scripted_active, camera.scripted_nesting), (0, 0));
    assert_eq!(camera.records[0].angle_offset, before.angle_offset);
    assert_eq!(camera.records[0].flags, 0x40);
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    assert_eq!(world.flags(), [9], "the script continues after the pan");
}

#[test]
fn player_type_one_selects_the_reduced_camera_defaults_for_record_zero_only() {
    use cyrus_game::script::camera::CameraSettings;
    let settings = CameraSettings {
        offset_pos: [-3, 2000, 3003],
        target_offset: [41, 50, 63],
        distance: 251,
        min_distance: 121,
        max_vel: 900,
        ..Default::default()
    };
    for (player_type, offset, target, distance, minimum) in [
        (0, [-3, 2000, 3003], [41, 50, 63], 251, 121),
        (1, [-2, 6400, 1501], [20, 6400, 31], 125, 60),
    ] {
        let mut data = scripts(&["CYRUS", "CAMERA"]);
        data.camera = settings.clone();
        data.actors
            .get_mut("CYRUS")
            .unwrap()
            .attributes
            .insert("attr_105".into(), player_type);
        data.content_id = data.content_identity().unwrap();
        let world = ScriptWorld::prepare(&data, 7).unwrap();
        let records = &world.camera().unwrap().records;
        let zero = &records[0];
        assert_eq!(
            (
                zero.position_offset,
                zero.target_offset,
                zero.distance,
                zero.minimum_distance
            ),
            (offset, target, distance, minimum),
            "player type {player_type}"
        );
        // Other records keep the installed values the setup copied before the reload.
        assert_eq!(records[1].position_offset, [-3, 2000, 3003]);
        assert_eq!(records[1].distance, 251);
        assert_eq!(records[3].target_offset, [41, 50, 63]);
        assert_eq!(zero.flags, 0x40);
    }
}

/// The standalone roles plus a map actor whose only camera call is `name(arguments)`.
fn scripts_calling(name: &str, arguments: &[i32]) -> Scripts {
    let mut data = scripts(&["CYRUS", "CAMERA"]);
    data.definition.functions[1] = Function {
        name: name.into(),
        param_count: arguments.len() as u8,
        kind: CallKind::Task,
    };
    data.actors
        .get_mut("ACTOR")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = Operation::Call {
        call: Call {
            function_id: 1,
            function: name.into(),
            target: Object::Me,
            kind: CallKind::Task,
            arguments: arguments
                .iter()
                .map(|&value| Value::Integer { value })
                .collect(),
        },
    };
    data
}

fn prepared(data: &mut Scripts) -> ScriptWorld {
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(data, 7).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(PLAYER).unwrap();
    world
        .initialize_player_spatial(player, [4000, 9000, -6000], [100, 1900, 20])
        .unwrap();
    world
}

#[test]
fn object_pan_task_binds_the_receiver_and_finishes_with_its_request() {
    let mut data = scripts_calling("showObjPan", &[3, -2, 5, 3]);
    let mut world = prepared(&mut data);
    let hud = ScriptHud::default();
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    let camera = world.camera().unwrap();
    assert_eq!(camera.selected.index(), 2, "the pan edits record two");
    assert_eq!(camera.records[2].actor, world.id(0).unwrap());
    assert_eq!(camera.scripted_active, 1);
    let mut passes = 0;
    while world.actors()[0].runtime().unwrap().active_tasks() != 0 {
        passes += 1;
        assert!(passes <= 8, "the pan must finish within its duration");
        run(&mut world, 0, TASKS, Some(&hud)).unwrap();
    }
    let position = world.spatial(world.id(0).unwrap()).unwrap().local.position;
    let camera = world.camera().unwrap();
    assert_eq!((camera.scripted_active, camera.scripted_nesting), (0, 0));
    let record = &camera.records[2];
    // The default attr_camera_move is zero: an absolute request of the vector plus the bound position.
    assert_eq!(
        record.request,
        [768 + position[0], -512 + position[1], 1280 + position[2]]
    );
    assert_eq!(
        record.flags & 0x09,
        0x08,
        "absolute request, no retained flag"
    );
    assert_eq!(flags(&world, 0), 0x40, "showObjPan marked the old record");
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    assert_eq!(world.flags(), [9], "the script continues after the pan");
}

#[test]
fn world_snapshot_resumes_an_object_pan_mid_flight() {
    let mut data = scripts_calling("showObjPan", &[3, -2, 5, 4]);
    let mut live = prepared(&mut data);
    let animations = Animations {
        version: 2,
        world: data.world,
        sections: BTreeMap::new(),
        models: BTreeMap::new(),
        unsupported: BTreeMap::new(),
        metadata: json!({"script_content_id": data.content_id, "actors": []}),
    };
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        player: None,
        scripts: &data,
        animations: &animations,
        collision: &collision,
    };
    let hud = ScriptHud::default();
    run(&mut live, 0, BODY, Some(&hud)).unwrap();
    run(&mut live, 0, TASKS, Some(&hud)).unwrap();
    let bytes = serde_json::to_vec(&live.snapshot(&sources).unwrap()).unwrap();
    let mut restored = ScriptWorld::from_snapshot(
        serde_json::from_slice(&bytes).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 19,
            globals: GameplayGlobals::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(restored.actors()[0].runtime().unwrap().active_tasks(), 1);
    for world in [&mut live, &mut restored] {
        let mut passes = 0;
        while world.actors()[0].runtime().unwrap().active_tasks() != 0 {
            passes += 1;
            assert!(passes <= 8, "the pan must finish within its duration");
            run(world, 0, TASKS, Some(&hud)).unwrap();
        }
    }
    let (live, restored) = (live.camera().unwrap(), restored.camera().unwrap());
    assert_eq!(live.scripted_nesting, 0);
    assert_eq!(restored.scripted_nesting, 0);
    assert_eq!(restored.records[2].request, live.records[2].request);
    assert_eq!(restored.records[2].flags, live.records[2].flags);
}

#[test]
fn look_task_fixes_the_orientation_until_it_finishes() {
    let mut data = scripts_calling("lookObj", &[3]);
    let mut world = prepared(&mut data);
    let hud = ScriptHud::default();
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    let camera = world.camera().unwrap();
    assert_eq!(camera.records[2].flags & 0x02, 0x02, "orientation fixed");
    assert_eq!(camera.records[2].actor, world.id(0).unwrap());
    let mut passes = 0;
    while world.actors()[0].runtime().unwrap().active_tasks() != 0 {
        passes += 1;
        assert!(passes <= 8, "the look must finish within its duration");
        run(&mut world, 0, TASKS, Some(&hud)).unwrap();
    }
    let camera = world.camera().unwrap();
    assert_eq!(camera.records[2].flags & 0x02, 0, "orientation released");
    assert_eq!((camera.scripted_active, camera.scripted_nesting), (0, 0));
}

#[test]
fn show_cyrus_pan_targets_the_player_and_does_nothing_while_it_holds_a_weapon() {
    let mut data = scripts_calling("showCyrusPan", &[3, -2, 5, 3]);
    let mut world = prepared(&mut data);
    let hud = ScriptHud::default();
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    assert_eq!(
        world.camera().unwrap().records[2].actor,
        world.id(PLAYER).unwrap()
    );
    assert_eq!(world.camera().unwrap().scripted_active, 1);

    let mut data = scripts_calling("showCyrusPan", &[3, -2, 5, 3]);
    data.actors
        .get_mut("CYRUS")
        .unwrap()
        .attributes
        .insert("attr_110".into(), 1);
    let mut world = prepared(&mut data);
    let before = world.camera().unwrap().clone();
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    let camera = world.camera().unwrap();
    assert_eq!(
        camera.scripted_active, 0,
        "the guard returns before any camera work"
    );
    assert_eq!(camera.selected, before.selected);
    assert_eq!(camera.records[0].flags, before.records[0].flags);
}

#[test]
fn show_obj_rot_completes_without_touching_the_camera() {
    let mut data = scripts_calling("showObjRot", &[1, 2, 3, 4]);
    let mut world = prepared(&mut data);
    let hud = ScriptHud::default();
    let before = world.camera().unwrap().clone();
    run(&mut world, 0, BODY, Some(&hud)).unwrap();
    run(&mut world, 0, TASKS, Some(&hud)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.camera().unwrap().selected, before.selected);
    assert_eq!(world.camera().unwrap().scripted_active, 0);
}

mod live {
    use super::*;
    use cyrus_game::script::ScriptActorId;
    use cyrus_game::script::camera::{
        CameraGeometry, CameraHitSelection, CameraPose, CameraSettings, CameraUpdateSettings,
        MovementLimits,
    };

    struct Geometry {
        fail: bool,
    }
    impl CameraGeometry for Geometry {
        fn segment(
            &mut self,
            _: [i32; 3],
            _: [i32; 3],
            _: ScriptActorId,
        ) -> anyhow::Result<Vec<cyrus_collision::collision_mesh::IntegerSegmentContact>> {
            anyhow::ensure!(!self.fail, "segment service failed");
            Ok(vec![])
        }
        fn contacts(&mut self, _: [i32; 3], _: ScriptActorId) -> anyhow::Result<Vec<[i32; 3]>> {
            Ok(vec![])
        }
        fn terrain_height(&mut self, _: [i32; 2]) -> anyhow::Result<f32> {
            anyhow::bail!("terrain is disabled in this scene")
        }
    }

    fn hits() -> CameraHitSelection {
        CameraHitSelection {
            minimum_position: [0; 3],
            maximum_position: [0; 3],
            minimum_normal: [0; 3],
            maximum_normal: [0; 3],
        }
    }

    fn settings() -> CameraUpdateSettings {
        CameraUpdateSettings {
            movement: MovementLimits {
                budget_increment: 5000,
                vertical: -5000,
                frame_scale: 256,
                selected_player_special_state: false,
            },
            body_radius: 10.0,
            body_clearance: 5.0,
            terrain_enabled: false,
        }
    }

    /// Player at (1000, 2000, 3000); record zero places the camera 2560 above it.
    fn world() -> ScriptWorld {
        let mut data = scripts(&["CYRUS", "CAMERA"]);
        data.camera = CameraSettings {
            offset_pos: [0, 2560, 0],
            max_vel: 100_000,
            ..Default::default()
        };
        data.content_id = data.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&data, 7).unwrap();
        world.prepare_spatial().unwrap();
        let player = world.id(PLAYER).unwrap();
        world
            .initialize_player_spatial(player, [1000, 2000, 3000], [0; 3])
            .unwrap();
        world
    }

    fn start() -> CameraPose {
        CameraPose {
            position: [1000, 3000, 3000],
            angles: [0; 3],
        }
    }

    #[test]
    fn update_publishes_one_pose_to_the_state_and_the_camera_actor_together() {
        let mut world = world();
        world.initialize_camera_pose(start()).unwrap();
        let camera = world.id(CAMERA).unwrap();
        assert_eq!(
            world.spatial(camera).unwrap().local.position,
            start().position
        );
        let pose = world
            .update_camera(&mut hits(), &settings(), &mut Geometry { fail: false })
            .unwrap();
        assert_eq!(pose.position, [1000, 4560, 3000]);
        let state = world.camera().unwrap();
        assert_eq!(state.current, pose);
        assert_eq!(state.previous, start());
        assert_eq!(world.spatial(camera).unwrap().local.position, pose.position);
    }

    #[test]
    fn failed_update_changes_neither_the_session_nor_the_actor() {
        let mut world = world();
        world.initialize_camera_pose(start()).unwrap();
        let camera = world.id(CAMERA).unwrap();
        let error = world
            .update_camera(&mut hits(), &settings(), &mut Geometry { fail: true })
            .unwrap_err();
        assert!(format!("{error:#}").contains("segment service failed"));
        assert_eq!(world.camera().unwrap().current, start());
        assert_eq!(
            world.spatial(camera).unwrap().local.position,
            start().position
        );
    }

    #[test]
    fn update_requires_the_camera_actor_pose_first_and_initialization_happens_once() {
        let mut world = world();
        let error = world
            .update_camera(&mut hits(), &settings(), &mut Geometry { fail: false })
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("published camera actor pose"),
            "{error:#}"
        );
        world.initialize_camera_pose(start()).unwrap();
        assert!(world.initialize_camera_pose(start()).is_err());
    }
}
