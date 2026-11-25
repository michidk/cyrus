use super::*;

#[test]
fn finished_ledge_climb_releases_a_static_receiver_and_continues_ordinary_updates() {
    use cyrus_script::script::{LivingState, OrdinaryInputs, ScriptInputs};
    for animation_system in [false, true] {
        let mut scripts = cache();
        scripts.updates.animation_system = animation_system;
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Collide] = 0;
        for placement in &mut scripts.instances {
            placement.active = 1;
            placement.object_type = PlacementType::MODEL;
            placement.map_position = [10, 20, 30];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let (world, data, models) = static_model_fixture(scripts.clone());
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["control"]["living_state"] = LivingState::CLIMB_FINISHED.byte().into();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let ids = [id, world.id(1).unwrap()];
        world.refresh_model_presentations(&data, &ids).unwrap();
        let scene = ordinary_collision_scene(world.world, models);
        assert_eq!(world.animation_frame(id).unwrap(), None);
        for update in 1..=2 {
            world
                .update_ordinary(
                    &data,
                    [10, 30],
                    OrdinaryInputs {
                        scripts: ScriptInputs {
                            nominal_ticks: Some(1),
                            fine_ticks: Some(1),
                            frame_scale: Some(256),
                            ..Default::default()
                        },
                        scene: Some(&scene),
                    },
                    &mut cyrus_game::random::EngineRandom::default(),
                    &mut |_, _| panic!("absent ledge receiver must not emit animation audio"),
                    100,
                )
                .unwrap();
            let actor = &world.actors()[id.index];
            assert!(actor.failure().is_none(), "{:?}", actor.failure());
            assert_eq!(
                actor.control_state().unwrap().living_state,
                LivingState::ORDINARY,
                "finished climb must release living-state ownership without an animation receiver"
            );
            assert_eq!(actor.runtime().unwrap().variables(), [4 + update]);
            assert_eq!(world.animation_frame(id).unwrap(), None);
        }
    }
}

#[test]
fn grounded_jump_key_selects_low_and_high_ledges_before_queued_jump() {
    grounded_jump_selects_ledges(false, LedgeReceiver::Eligible);
}

#[test]
fn selected_native_jump_selects_low_and_high_ledges_before_queued_jump() {
    grounded_jump_selects_ledges(true, LedgeReceiver::Eligible);
}

#[test]
fn excluded_high_ledge_receiver_keeps_its_animation_owner() {
    grounded_jump_selects_ledges(false, LedgeReceiver::Excluded);
}

#[test]
fn selected_jump_high_ledge_without_animation_receiver_completes_transition() {
    grounded_jump_selects_ledges(false, LedgeReceiver::Absent);
}

#[test]
fn selected_jump_rescans_far_ledges_after_four_rejected_near_records() {
    grounded_jump_selects_ledges_with_geometry(
        false,
        LedgeReceiver::Eligible,
        LedgeGeometry::RejectedNear,
    );
}

#[test]
fn selected_jump_ledge_fit_truncates_negative_fractional_positions() {
    grounded_jump_selects_ledges_with_geometry(
        false,
        LedgeReceiver::Eligible,
        LedgeGeometry::NegativeFit,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LedgeReceiver {
    Eligible,
    Excluded,
    Absent,
}

fn grounded_jump_selects_ledges(native_jump: bool, receiver: LedgeReceiver) {
    grounded_jump_selects_ledges_with_geometry(native_jump, receiver, LedgeGeometry::Standard);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LedgeGeometry {
    Standard,
    RejectedNear,
    NegativeFit,
}

fn grounded_jump_selects_ledges_with_geometry(
    native_jump: bool,
    receiver: LedgeReceiver,
    geometry: LedgeGeometry,
) {
    use cyrus_assets::collision::{CollisionGeometry, CollisionModel, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };

    // Preservation player.md: grounded tops below the quarter-height boundary select class 2;
    // those above it select class 1. Both enter through the jump key before queued jumping.
    for (top, expected_group, expected_living) in [
        (-10, 90, 13),
        (-12, 90, 13),
        (-13, 86, 11),
        (-44, 86, 11),
        (-45, 32, 0),
    ] {
        if geometry != LedgeGeometry::Standard && top != -13 {
            continue;
        }
        if receiver != LedgeReceiver::Eligible
            && expected_group != AnimationGroup::PULL_UP_LEDGE.index()
        {
            continue;
        }
        let (scripts, world, mut data, mut scene) = player_walking_fixture_with(|scripts| {
            if native_jump {
                let function_id = u16::try_from(scripts.definition.functions.len()).unwrap();
                scripts.definition.functions.push(Function {
                    name: Native::Jump,
                    param_count: 0,
                    kind: CallKind::Function,
                });
                scripts
                    .actors
                    .get_mut("CYRUS")
                    .unwrap()
                    .program
                    .steps
                    .insert(0, assign_call(1, function_id, Native::Jump, &[]));
            }
        });
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let definition = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "CYRUS")
            .unwrap();
        for group in [32, 86, 90] {
            definition["animation_groups"].as_array_mut().unwrap().push(serde_json::json!({
                "group_index":group,"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":0},{"opcode":0,"frame_reference":0},{"opcode":0,"frame_reference":0},{"opcode":0,"frame_reference":0},{"opcode":1}]
            }));
        }
        let idle = definition["animation_groups"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|group| group["group_index"] == AnimationGroup::PANIC.index())
            .unwrap();
        idle["commands"].as_array_mut().unwrap().insert(
            0,
            serde_json::json!({"opcode":4,"sound_param":1,"volume_shift":0}),
        );
        let mut world = replace_health_test_animation(world, &scripts, &mut data, index, 0, |_| {});
        let player = world.id(index).unwrap();
        world
            .advance(
                player,
                cyrus_script::script::ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
        if receiver != LedgeReceiver::Eligible {
            let mut saved = movement_snapshot(&world, &scripts, &data);
            match receiver {
                LedgeReceiver::Excluded => {
                    saved["actors"][index]["control"]["object_update_excluded"] = true.into();
                }
                LedgeReceiver::Absent => {
                    saved["actors"][index]["animation"] = serde_json::Value::Null;
                }
                LedgeReceiver::Eligible => unreachable!(),
            }
            let globals = std::mem::take(world.globals_mut());
            world = restore_navigation_owner(saved, &scripts, &data).unwrap();
            *world.globals_mut() = globals;
        }
        let player = world.id(index).unwrap();
        world
            .place_selected_player([522 * 256, -1536, 542 * 256], 0)
            .unwrap();
        world.refresh_model_presentations(&data, &[player]).unwrap();
        let map = world.id(0).unwrap();
        scene.collision.placements.push(StaticPlacement {
            id: 300,
            model: "LEDGE".into(),
            position: [512, 0, 512],
            rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
        });
        scene.collision.models.insert(
            "LEDGE".into(),
            CollisionModel {
                flags: 0,
                geometry: CollisionGeometry {
                    version: Some(*b"v5.0"),
                    radius: 300.,
                    vertices: vec![
                        [-100., top as f32, 60.],
                        [100., top as f32, 60.],
                        [100., 20., 60.],
                        [-100., 20., 60.],
                        [-100., top as f32, 160.],
                        [100., top as f32, 160.],
                        [100., 20., 160.],
                        [-100., 20., 160.],
                    ],
                    normals: vec![
                        [0., 0., -1.],
                        [0., -1., 0.],
                        [0., 0., 1.],
                        [0., 1., 0.],
                        [-1., 0., 0.],
                        [1., 0., 0.],
                    ],
                    faces: vec![
                        vec![0, 3, 2, 1],
                        vec![0, 1, 5, 4],
                        vec![4, 5, 6, 7],
                        vec![3, 7, 6, 2],
                        vec![0, 4, 7, 3],
                        vec![1, 2, 6, 5],
                    ],
                    textures: Some(vec![None; 6]),
                    subobjects: vec![cyrus_assets::collision::CollisionSubobject {
                        center: [0., 0., 100.],
                        radius: 300.,
                        extents: Some([200.; 3]),
                        faces: (0..6).collect(),
                    }],
                },
            },
        );
        if geometry == LedgeGeometry::RejectedNear {
            let far = scene.collision.models.get_mut("LEDGE").unwrap();
            for vertex in &mut far.geometry.vertices {
                if vertex[2] == 60. {
                    vertex[2] = 84.;
                }
            }
            let mut near: CollisionModel =
                serde_json::from_value(serde_json::to_value(&*far).unwrap()).unwrap();
            for vertex in &mut near.geometry.vertices {
                vertex[1] = if vertex[1] == top as f32 { 100. } else { 120. };
                vertex[2] = if vertex[2] == 84. { 60. } else { 80. };
            }
            scene.collision.models.insert("LOW_NEAR".into(), near);
            // Four rejected tops fill the first pass's table. The farther pass must
            // start a fresh table before considering the climbable platform.
            for id in 301..305 {
                scene.collision.placements.push(StaticPlacement {
                    id,
                    model: "LOW_NEAR".into(),
                    position: [512, 0, 512],
                    rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
                });
            }
        }
        if geometry == LedgeGeometry::NegativeFit {
            // A nearly forward-facing wall puts the fit origin fractionally below X=0.
            // Truncating that origin to -1 leaves 12.5 units to this neighboring wall;
            // rounding down to -2 leaves 11.5 and falsely blocks the radius-12 body.
            world
                .place_selected_player([0, -1536, 542 * 256], 4)
                .unwrap();
            world.refresh_model_presentations(&data, &[player]).unwrap();
            scene.collision.placements.last_mut().unwrap().position[0] = 0;
            let ledge = scene.collision.models.get_mut("LEDGE").unwrap();
            for vertex in &mut ledge.geometry.vertices {
                vertex[2] += vertex[0] * 0.01;
                vertex[0] -= 10.;
            }
            ledge.geometry.normals[0] = [0.0099995, 0., -0.99995];
            let blocker = CollisionModel {
                flags: 0,
                geometry: CollisionGeometry {
                    version: Some(*b"v5.0"),
                    radius: 300.,
                    vertices: vec![
                        [-13.5, -200., 0.],
                        [-13.5, 200., 0.],
                        [-13.5, 200., 150.],
                        [-13.5, -200., 150.],
                    ],
                    normals: vec![[1., 0., 0.]],
                    faces: vec![vec![0, 1, 2, 3]],
                    textures: Some(vec![None]),
                    subobjects: vec![cyrus_assets::collision::CollisionSubobject {
                        center: [-13.5, 0., 75.],
                        radius: 300.,
                        extents: Some([300.; 3]),
                        faces: vec![0],
                    }],
                },
            };
            scene.collision.models.insert("FIT_BLOCKER".into(), blocker);
            scene.collision.placements.push(StaticPlacement {
                id: 305,
                model: "FIT_BLOCKER".into(),
                position: [0, 0, 512],
                rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
            });
        }
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [522, 542], &[player, map])
            .unwrap();
        let query = OrdinaryGroundQuery {
            scene: &scene,
            animations: &data,
            prepared: &prepared,
            distance: 4,
            sidedness: false,
        };
        let held = cyrus_script::player::input::PlayerInput {
            jump: true,
            ..Default::default()
        };
        let mut emitted = Vec::new();
        if receiver == LedgeReceiver::Absent {
            assert!(
                !world
                    .begin_ordinary_jump(
                        player,
                        &data,
                        8,
                        false,
                        cyrus_script::script::ObstacleEnvironment {
                            selected_player: Some(player),
                            auto_grab: false,
                            item_system_ready: false,
                            shell_execution: false,
                            ticks: 0,
                        },
                        Some((
                            query,
                            cyrus_script::script::LedgeTiming {
                                fine_ticks: 0,
                                frame_scale: Some(256),
                            }
                        )),
                        &mut |_, sound| {
                            emitted.push(sound);
                            Ok(())
                        },
                    )
                    .unwrap()
            );
        } else if native_jump {
            let activation = cyrus_script::script::ActivationInputs {
                player,
                actors: BTreeMap::new(),
                labels: BTreeMap::new(),
                lockout: 0,
                dialogue_open: false,
                lineup_result: 0,
                use_held: false,
                item_system_ready: false,
                shell_execution: false,
                render_disabled: false,
            };
            world
                .advance(
                    player,
                    ScriptStage::OrdinaryMain,
                    ScriptInputs {
                        activation: Some(&activation),
                        player_collision: Some(cyrus_script::script::PlayerCollisionSource {
                            scene: &scene,
                            animations: &data,
                            view: [522, 542],
                            actors: &[player, map],
                        }),
                        shell_execution: Some(false),
                        fine_ticks: Some(0),
                        nominal_ticks: Some(0),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    Some(&mut |_, sound| {
                        emitted.push(sound);
                        Ok(())
                    }),
                    None,
                    100,
                )
                .unwrap();
        } else {
            world
                .advance_player_controller(
                    player,
                    query,
                    cyrus_script::clock::ClockStep {
                        frame_scale: 256,
                        nominal_ticks: 0,
                        fine_ticks: 0,
                    },
                    ScriptInputs {
                        player_controller: Some(PlayerControllerInput {
                            held: &held,
                            walk_mode: false,
                            auto_defend: false,
                            steer: None,
                            claim: None,
                            probes: None,
                        }),
                        shell_execution: Some(false),
                        fine_ticks: Some(0),
                        nominal_ticks: Some(0),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    &mut cyrus_game::random::EngineRandom::default(),
                    &mut |_, sound| {
                        emitted.push(sound);
                        Ok(())
                    },
                )
                .unwrap();
        }
        if receiver != LedgeReceiver::Eligible {
            assert!(
                emitted.is_empty(),
                "ineligible climb must not retire or start audio ownership"
            );
        }
        let inspected = world.inspect(index, 1);
        let actor = &inspected["actors"][0];
        assert_eq!(
            actor["animation"]["current_group"],
            match receiver {
                LedgeReceiver::Eligible => serde_json::Value::from(expected_group),
                LedgeReceiver::Excluded => serde_json::Value::from(AnimationGroup::PANIC.index()),
                LedgeReceiver::Absent => serde_json::Value::Null,
            },
            "top {top}"
        );
        assert_eq!(
            actor["control"]["living_state"],
            if receiver != LedgeReceiver::Eligible {
                cyrus_script::script::LivingState::CLIMB_FINISHED.byte()
            } else {
                expected_living
            },
            "top {top}"
        );
        assert_eq!(
            actor["control"]["queued_motion"],
            u8::from(expected_group == 32)
        );
        assert_eq!(actor["control"]["falling_state"], 0);
        assert_eq!(
            actor["control"]["motion_classification"],
            if expected_group == 32 { 2 } else { 0 },
            "top {top}: edge handling precedes the jump classification store"
        );
    }
}
