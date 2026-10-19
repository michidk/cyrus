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
        version: 21,
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
    run(&mut world, 0, TASKS, Some(&hud)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(flags(&world, 0), 0x40);
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
            collision_inhibited: false,
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
