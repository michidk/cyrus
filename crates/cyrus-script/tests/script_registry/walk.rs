use super::*;

#[test]
fn object_axis_task_moves_in_receiver_orientation_and_does_not_snap_at_target() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for (axis, angles, delta, expected) in [
        (0, [0, 0, 0], 512, [256, 0, 0]),
        (1, [0, 0, 0], -512, [0, -256, 0]),
        (2, [0, 512, 0], 512, [-256, 0, 0]),
        (2, [512, 0, 0], 512, [0, 256, 0]),
        (0, [0, 0, 512], 512, [0, -256, 0]),
        (9, [0, 0, 0], 512, [0, 0, 0]),
    ] {
        let mut scripts = cache();
        scripts.definition.functions.push(Function {
            name: Native::MoveObjectAxis,
            param_count: 3,
            kind: CallKind::Task,
        });
        scripts.instances[0].angles = angles;
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = wait(
            1,
            Native::MoveObjectAxis,
            CallKind::Task,
            vec![
                Value::Integer { value: axis },
                Value::Integer { value: delta },
                Value::Integer { value: 2 },
            ],
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            expected,
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world
            .advance(
                id,
                ScriptStage::Tasks,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            expected,
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(
            world.spatial(world.id(1).unwrap()).unwrap().local.position,
            [0; 3]
        );
    }
}

#[test]
fn destination_support_restores_all_position_axes_on_success_miss_and_error() {
    use cyrus_script::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    for outcome in ["support", "miss", "error", "linked"] {
        let mut scripts = cache();
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .extend([(Attribute::Collide, 4), (Attribute::JumpTime, 8)]);
        let (mut world, data, models) = static_model_fixture_at(scripts, Some(20.0), [10, 20, 30]);
        let id = world.id(0).unwrap();
        let other = world.id(1).unwrap();
        world
            .refresh_model_presentations(&data, &[id, other])
            .unwrap();
        world.snapshot_spatial(id).unwrap();
        let saved = world.spatial(id).unwrap().local.position;
        let mut scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
            .unwrap();
        if outcome == "error" {
            scene.actor_collision = None;
        }
        if outcome == "linked" {
            // A linked actor's probe reads its link position, far from any support, whatever the
            // temporary local destination is.
            world.set_link_position_for_test(id, [2000 * 256, 21 * 256, 2000 * 256]);
        }
        let destination = if outcome == "miss" {
            [2000 * 256, 21 * 256, 2000 * 256]
        } else {
            [11 * 256, 21 * 256, 31 * 256]
        };
        let result = world.admit_ordinary_destination(
            id,
            destination,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 50,
                sidedness: false,
            },
            ObstacleEnvironment {
                selected_player: None,
                auto_grab: false,
                item_system_ready: true,
                shell_execution: false,
                ticks: 0,
            },
            &mut |_, _| Ok(()),
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            saved,
            "{outcome}"
        );
        match outcome {
            "support" => {
                assert!(result.unwrap());
                assert_eq!(world.model_support(id).unwrap().actor, Some(other));
            }
            "miss" | "linked" => assert!(!result.unwrap(), "{outcome}"),
            _ => {
                assert!(format!("{:#}", result.unwrap_err()).contains("live geometry unavailable"))
            }
        }
    }
}

#[test]
fn destination_support_rejects_steep_ground_after_prevalidation_bypass() {
    use cyrus_collision::ground::{SelectedSupport, TerrainGround};
    use cyrus_script::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Collide] = 2;
    let (mut world, data, models) = static_model_fixture_at(scripts, Some(20.0), [600, 2400, 600]);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    world.snapshot_spatial(id).unwrap();
    let saved = world.spatial(id).unwrap().local.position;
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 5000;
    let heights = (0_usize..256 * 256)
        .map(|index| ((index % 256).min(16) * 1024) as i16)
        .collect();
    scene.terrain = Some(TerrainGround::new(heights, vec![false; 256 * 256], None).unwrap());
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [600, 600], &[id])
        .unwrap();
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 5000,
        sidedness: false,
    };
    let selected = world.query_ordinary_ground(id, query()).unwrap().unwrap();
    assert!(
        matches!(selected, SelectedSupport::Primary { support, .. } if support.classification() == SurfaceClass::Steep)
    );
    world
        .query_ordinary_ground_prevalidation(id, query(), true)
        .unwrap();
    assert!(
        !world
            .admit_ordinary_destination(
                id,
                saved,
                query(),
                ObstacleEnvironment {
                    selected_player: None,
                    auto_grab: false,
                    item_system_ready: true,
                    shell_execution: false,
                    ticks: 0
                },
                &mut |_, _| Ok(()),
            )
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, saved);
}

#[test]
fn destination_body_checks_models_without_moving_the_receiver() {
    use cyrus_script::script::OrdinaryObstacleQuery;
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let saved = world.spatial(id).unwrap().local.position;
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    for (height, expected) in [(20, false), (100, true)] {
        assert_eq!(
            world
                .ordinary_destination_body_clear(
                    id,
                    [10 * 256, height * 256, 30 * 256],
                    OrdinaryObstacleQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        vertical_offset: 0,
                        respond: false
                    },
                )
                .unwrap(),
            expected
        );
        assert_eq!(world.spatial(id).unwrap().local.position, saved);
    }
}

#[test]
fn walk_forward_body_refusal_marks_step_without_running_later_ground_query() {
    use cyrus_script::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let saved = world.spatial(id).unwrap().local.position;
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    assert!(
        !world
            .walk_forward_step_clear(
                id,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: u32::MAX,
                    sidedness: false
                },
                ObstacleEnvironment {
                    selected_player: None,
                    auto_grab: false,
                    item_system_ready: true,
                    shell_execution: false,
                    ticks: 0
                },
                &mut |_, _| Ok(()),
            )
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, saved);
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .last_collide
            .bits(),
        0x80
    );
    world.finish_ordinary_script_response(id, Some(1)).unwrap();
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .last_collide
            .bits(),
        0
    );
}

#[test]
fn directional_body_refusal_retains_markers_and_skips_support_without_translation() {
    use cyrus_script::script::{ObstacleEnvironment, OrdinaryGroundQuery, StepDirection};
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let saved = world.spatial(id).unwrap().local.position;
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    for (direction, markers) in [
        (StepDirection::Left, 0x20),
        (StepDirection::Right, 0x60),
        (StepDirection::Backward, 0x160),
    ] {
        assert!(
            !world
                .ordinary_step_clear(
                    id,
                    direction,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: u32::MAX,
                        sidedness: false
                    },
                    ObstacleEnvironment {
                        selected_player: None,
                        auto_grab: false,
                        item_system_ready: true,
                        shell_execution: false,
                        ticks: 0
                    },
                    &mut |_, _| panic!("body refusal must not reach response sounds"),
                )
                .unwrap(),
            "{direction:?}"
        );
        assert_eq!(world.spatial(id).unwrap().local.position, saved);
        assert_eq!(
            world.actors()[id.index]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            markers
        );
    }
}

#[test]
fn directional_early_results_precede_geometry_and_preserve_status_bytes() {
    use cyrus_script::script::{
        ObstacleEnvironment, OrdinaryGroundQuery, StepDirection, WorldRestoreContext,
        WorldSnapshotSources,
    };
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
        instance.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (world, data, models) = static_model_fixture_at(scripts.clone(), Some(20.0), [10, 20, 30]);
    let scene = ordinary_collision_scene(world.world, models);
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &scene.collision,
    };
    let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for rope in [false, true] {
        for direction in [
            StepDirection::Left,
            StepDirection::Right,
            StepDirection::Backward,
        ] {
            let mut snapshot = saved.clone();
            snapshot["actors"][0]["control"]["rope_attached"] = rope.into();
            snapshot["actors"][0]["control"]["death_selector"] = 7.into();
            snapshot["actors"][0]["control"]["collision_response_auxiliary"] =
                serde_json::json!([0x95, 0xA2]);
            let mut world = ScriptWorld::from_snapshot(
                serde_json::from_value(snapshot).unwrap(),
                &sources,
                WorldRestoreContext {
                    generation: 2,
                    globals: Default::default(),
                    samples: &BTreeMap::new(),
                },
            )
            .unwrap();
            let id = world.id(0).unwrap();
            let other = world.id(1).unwrap();
            world
                .refresh_model_presentations(&data, &[id, other])
                .unwrap();
            let prepared = world
                .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
                .unwrap();
            // A mismatched scene must never be read by either early branch.
            let (_, _, unrelated_models) = static_model_fixture(cache());
            let unavailable = ordinary_collision_scene(999, unrelated_models);
            assert_eq!(
                world
                    .ordinary_step_clear(
                        id,
                        direction,
                        OrdinaryGroundQuery {
                            scene: &unavailable,
                            animations: &data,
                            prepared: &prepared,
                            distance: u32::MAX,
                            sidedness: false
                        },
                        ObstacleEnvironment {
                            selected_player: None,
                            auto_grab: false,
                            item_system_ready: false,
                            shell_execution: false,
                            ticks: 0
                        },
                        &mut |_, _| panic!("early result reached sounds")
                    )
                    .unwrap(),
                !rope
            );
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .last_collide
                    .bits(),
                0xA200_0095
            );
        }
    }
}

#[test]
fn native_walk_forward_uses_live_body_admission_on_call_and_poll() {
    use cyrus_script::script::{
        LocomotionGeometry, ObstacleEnvironment, OrdinaryGroundQuery, ScriptInputs, ScriptStage,
    };
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::WalkForward,
        param_count: 1,
        kind: CallKind::Task,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(
                    1,
                    Native::WalkForward,
                    CallKind::Task,
                    vec![Value::Integer { value: 4 }],
                ),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    for query_owner in [0, 1] {
        let (mut world, data, models) =
            static_model_fixture_at(scripts.clone(), Some(20.0), [10, 20, 30]);
        let id = world.id(0).unwrap();
        let other = world.id(1).unwrap();
        world
            .refresh_model_presentations(&data, &[id, other])
            .unwrap();
        let saved = world.spatial(id).unwrap().local.position;
        let scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(
                if query_owner == 0 { id } else { other },
                &scene,
                &data,
                [10, 30],
                &[id, other],
            )
            .unwrap();
        let hud = cyrus_script::script::ScriptHud::default();
        for stage in [ScriptStage::Body(Thread::Main), ScriptStage::Tasks] {
            let result = world.advance(
                id,
                stage,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    forward_step_clear: Some(true),
                    locomotion_geometry: Some(LocomotionGeometry {
                        ground: OrdinaryGroundQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            distance: u32::MAX,
                            sidedness: false,
                        },
                        environment: ObstacleEnvironment {
                            selected_player: None,
                            auto_grab: false,
                            item_system_ready: false,
                            shell_execution: false,
                            ticks: 1,
                        },
                    }),
                    ..Default::default()
                },
                None,
                None,
                30,
            );
            if query_owner == 1 {
                let error = result.unwrap_err();
                assert!(
                    format!("{error:#}")
                        .contains("collision query belongs to another actor or world"),
                    "{error:#}"
                );
                assert_eq!(world.spatial(id).unwrap().local.position, saved);
                assert_eq!(
                    world.actors()[id.index]
                        .control_state()
                        .unwrap()
                        .last_collide
                        .bits(),
                    0
                );
                break;
            }
            result.unwrap();
            assert_eq!(world.spatial(id).unwrap().local.position, saved);
            assert_eq!(
                world.actors()[id.index]
                    .control_state()
                    .unwrap()
                    .last_collide
                    .bits(),
                0x80
            );
            assert_eq!(
                world.actors()[id.index]
                    .control_state()
                    .unwrap()
                    .motion_direction,
                MotionDirection::NONE
            );
            world.finish_ordinary_script_response(id, Some(1)).unwrap();
        }
        assert_eq!(
            world.actors()[id.index].runtime().unwrap().active_tasks(),
            0
        );
    }
}

fn forward_amount_world(
    initial_group: u16,
    suppressed: bool,
    animation_system: bool,
    edit_actor: impl FnOnce(&mut serde_json::Value),
) -> ScriptWorld {
    movement_fixture(
        initial_group,
        suppressed,
        animation_system,
        |_| {},
        |saved| {
            edit_actor(&mut saved["actors"][0]);
        },
    )
    .0
}

pub(super) fn movement_fixture(
    initial_group: u16,
    suppressed: bool,
    animation_system: bool,
    edit_scripts: impl FnOnce(&mut Scripts),
    edit_saved: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    use cyrus_script::script::{
        ScriptInputs, ScriptStage, WorldRestoreContext, WorldSnapshotSources,
    };
    let mut scripts = cache();
    scripts.updates.animation_system = animation_system;
    for (attribute, value) in [
        (Attribute::ForwardSpeed, 8),
        (Attribute::BackwardSpeed, 12),
        (Attribute::StepType, 1),
        (Attribute::ForwardWalk, 4),
        (Attribute::FightLungeSpeed, 6),
        (Attribute::FightForwardSpeed, 10),
        (Attribute::FightBackwardSpeed, 6),
        (Attribute::FightForwardWalk, 3),
        (Attribute::FightCircleSpeed, 4),
    ] {
        scripts.actors.get_mut("LOGIC").unwrap().attributes[attribute] = value;
    }
    edit_scripts(&mut scripts);
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","animation_groups":([0,14,15,16,17,18,19,20,21,22,23,24,25,26,27,30,31,32,33,34,35,49,50,85,92,93,94,95,96,97,106,111,114,116,130,131,142,143].into_iter().map(|group| {
                let mut commands = vec![];
                if suppressed {commands.push(serde_json::json!({"opcode":5}));}
                commands.push(serde_json::json!({"opcode":0,"frame_reference":group}));
                commands.push(serde_json::json!({"opcode":2,"value":0}));
                serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":commands})
            }).collect::<Vec<_>>())}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, AnimationGroup::from_index(initial_group))
        .unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    // This logical fixture retains playback but has no rendered placement frame.
    saved["actors"][0]["animation"]["frame"] = serde_json::Value::Null;
    saved["actors"][0]["animation"]["playback"]["default"] = 0.into();
    edit_saved(&mut saved);
    let world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    (world, scripts, data)
}

fn forward_amount_inputs() -> cyrus_script::script::OrdinaryForwardAmountInputs {
    cyrus_script::script::OrdinaryForwardAmountInputs {
        selected_player: None,
        holding_weapon: false,
        frame_scale: 256,
        ticks: 1,
        duration: 1,
        elapsed: 0,
    }
}

#[test]
fn forward_amount_advances_shared_counter_and_called_state_without_translation() {
    let mut world = forward_amount_world(18, false, true, |_| {});
    let id = world.id(0).unwrap();
    let before = world.spatial(id).unwrap().local;
    for (amount, counter) in [(512, 0x10), (682, 0x20), (1024, 0x30), (2048, 0x30)] {
        assert_eq!(
            world
                .ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| Ok(()))
                .unwrap(),
            amount
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.move_counter, counter);
        assert_eq!(control.motion_direction, MotionDirection::FORWARD);
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 4);
        assert_eq!(world.spatial(id).unwrap().local.position, before.position);
        assert_eq!(world.spatial(id).unwrap().local.angles, before.angles);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn forward_amount_uses_live_combat_action_surface_torch_and_suppression() {
    use cyrus_script::script::CombatControlState;
    for (weapon, combat, action, surface, torch, group, amount) in [
        (false, false, -1, 0, 0, 18, 2048),
        (false, false, -1, 0, 1, 30, 2048),
        (false, false, 4, 2, 1, 142, 1024),
        (true, false, -1, 0, 0, 92, 2560),
        (false, true, 4, 0, 0, 106, 1536),
        (true, true, 4, 4, 0, 106, 768),
        (false, true, -1, 2, 0, 92, 1280),
    ] {
        for (suppressed, scale, multiplier) in
            [(false, 128, 1), (true, 128, 2), (false, u32::MAX, -1)]
        {
            let mut world = forward_amount_world(group, suppressed, true, |actor| {
                let control = &mut actor["control"];
                control["movement_flags"] = 0xF3.into();
                control["torch_burning"] = torch.into();
                let state = CombatControlState {
                    flags: CombatFlags::from_bits(if combat { 4 } else { 0 }),
                    action,
                    ..Default::default()
                };
                control["combat"] = serde_json::to_value(state).unwrap();
            });
            let id = world.id(0).unwrap();
            world
                .set_actor_surface(id, Surface::from_byte(surface))
                .unwrap();
            let mut inputs = forward_amount_inputs();
            inputs.holding_weapon = weapon;
            inputs.frame_scale = scale;
            let expected = if multiplier == -1 {
                -amount / 256
            } else {
                amount / 2 * multiplier
            };
            assert_eq!(
                world
                    .ordinary_forward_amount(id, inputs, None, &mut |_, _| Ok(()))
                    .unwrap(),
                expected,
                "weapon {weapon} combat {combat} action {action} surface {surface} suppressed {suppressed} scale {scale}"
            );
            assert_eq!(
                world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
                group
            );
            assert_eq!(
                world.actors()[0].control_state().unwrap().move_counter,
                0xF3
            );
        }
    }
}

#[test]
fn forward_amount_guards_precede_animation_and_geometry_and_keep_shared_state() {
    for (field, value) in [
        ("rope_attached", serde_json::json!(true)),
        ("sliding_state", 1.into()),
        ("falling_state", 1.into()),
        ("queued_motion", 1.into()),
        ("living_state", 15.into()),
    ] {
        let mut world = forward_amount_world(18, false, true, |actor| {
            actor["animation"] = serde_json::Value::Null;
            let control = &mut actor["control"];
            control[field] = value;
            control["movement_flags"] = 0xC3.into();
            control["motion_direction"] = 99.into();
        });
        let id = world.id(0).unwrap();
        assert_eq!(
            world
                .ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| panic!(
                    "refused query reached sounds"
                ))
                .unwrap(),
            0
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.move_counter, 0xC3);
        assert_eq!(control.motion_direction, MotionDirection::from_byte(99));
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
    }
}

#[test]
fn forward_amount_waits_for_current_animation_without_advancing_counter() {
    for (enabled, duration, elapsed, expected, counter, called) in [
        (true, 4, 0, 1, 0xC3, 0),
        (true, 1, 0, 0, 0xC3, 0),
        (true, 4, 3, 0, 0xC3, 0),
        (false, 4, 0, 512, 0xD3, 4),
    ] {
        let mut world = forward_amount_world(30, false, enabled, |actor| {
            actor["control"]["movement_flags"] = 0xC3.into();
            actor["control"]["motion_direction"] = 99.into();
        });
        let id = world.id(0).unwrap();
        let mut inputs = forward_amount_inputs();
        inputs.duration = duration;
        inputs.elapsed = elapsed;
        assert_eq!(
            world
                .ordinary_forward_amount(id, inputs, None, &mut |_, _| Ok(()))
                .unwrap(),
            expected
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.motion_direction, MotionDirection::FORWARD);
        assert_eq!(control.move_counter, counter);
        let state = world.inspect(0, 1);
        assert_eq!(state["globals"]["locomotion_called"], called);
        assert_eq!(
            state["actors"][0]["animation"]["current_group"],
            AnimationGroup::RUN_TORCH.index()
        );
        assert_eq!(
            state["actors"][0]["animation"]["pending_group"],
            if enabled {
                serde_json::json!(AnimationGroup::RUN.index())
            } else {
                serde_json::Value::Null
            }
        );
    }
}

#[test]
fn forward_amount_requires_live_step_geometry_only_below_saturated_counter() {
    for counter in [0xC3, 0xD3, 0xE3, 0xF3] {
        let mut world = forward_amount_world(18, false, true, |actor| {
            actor["attributes"][1] = 2.into();
            actor["control"]["movement_flags"] = counter.into();
            actor["control"]["motion_direction"] = 99.into();
        });
        let id = world.id(0).unwrap();
        let result =
            world.ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| Ok(()));
        if counter == 0xF3 {
            assert_eq!(result.unwrap(), 2048);
        } else {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains("world 7 instance 0 forward amount"),
                "{error}"
            );
            assert!(error.contains("step geometry unavailable"), "{error}");
            assert_eq!(
                world.actors()[0].control_state().unwrap().motion_direction,
                MotionDirection::from_byte(99)
            );
            assert_eq!(
                world.actors()[0].control_state().unwrap().move_counter,
                counter
            );
            assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
        }
    }
}

#[test]
fn forward_amount_halves_deep_water_before_the_shared_startup_ramp() {
    let mut world = forward_amount_world(142, false, true, |_| {});
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, Surface::DEEPWATER).unwrap();
    for expected in [256, 341, 512, 1024] {
        assert_eq!(
            world
                .ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| Ok(()))
                .unwrap(),
            expected
        );
    }
}

#[test]
fn synchronous_forward_resamples_the_counter_mutated_by_the_amount_query() {
    use cyrus_script::script::{OrdinaryMovement, ScriptHud, ScriptInputs};
    let mut world = forward_amount_world(18, false, true, |_| {});
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    assert_eq!(
        world
            .ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| Ok(()))
            .unwrap(),
        512
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
    for (expected_position, expected_counter) in [([0, 0, 682], 0x20), ([0, 0, 1706], 0x30)] {
        assert_eq!(
            world
                .move_ordinary(
                    id,
                    OrdinaryMovement::Forward,
                    1,
                    0,
                    ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        forward_step_clear: Some(true),
                        ..Default::default()
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            0
        );
        assert_eq!(world.spatial(id).unwrap().local.position, expected_position);
        assert_eq!(
            world.actors()[0].control_state().unwrap().move_counter,
            expected_counter
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 4);
    }
}

#[test]
fn synchronous_movement_retains_duration_and_animation_wait_results_without_tasks() {
    use cyrus_script::script::{OrdinaryMovement, ScriptHud, ScriptInputs};
    for (group, enabled, duration, elapsed, expected, position, counter) in [
        (18, true, 4, 0, 1, [0, 0, 512], 0x10),
        (18, true, 4, 3, 0, [0, 0, 512], 0x10),
        (30, true, 4, 0, 1, [0, 0, 0], 0),
        (30, true, 1, 0, 0, [0, 0, 0], 0),
        (30, false, 1, 0, 0, [0, 0, 512], 0x10),
    ] {
        let mut world = forward_amount_world(group, false, enabled, |_| {});
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        assert_eq!(
            world
                .move_ordinary(
                    id,
                    OrdinaryMovement::Forward,
                    duration,
                    elapsed,
                    ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        forward_step_clear: Some(true),
                        ..Default::default()
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            expected
        );
        assert_eq!(world.spatial(id).unwrap().local.position, position);
        assert_eq!(
            world.actors()[0].control_state().unwrap().move_counter,
            counter
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn synchronous_backward_and_combat_strafes_use_existing_real_movement() {
    use cyrus_script::script::{CombatControlState, OrdinaryMovement, ScriptHud, ScriptInputs};
    for (movement, group, position, direction, counter, called) in [
        (
            OrdinaryMovement::Backward,
            19,
            [0, 0, -768],
            MotionDirection::WALK_BACKWARD,
            0x40,
            8,
        ),
        (
            OrdinaryMovement::Left,
            18,
            [-256, 0, 0],
            MotionDirection::LEFT,
            1,
            1,
        ),
        (
            OrdinaryMovement::Right,
            18,
            [256, 0, 0],
            MotionDirection::RIGHT,
            4,
            2,
        ),
    ] {
        let mut world = forward_amount_world(group, false, true, |actor| {
            if matches!(movement, OrdinaryMovement::Left | OrdinaryMovement::Right) {
                actor["control"]["combat"] = serde_json::to_value(CombatControlState {
                    flags: CombatFlags::from_bits(4),
                    ..Default::default()
                })
                .unwrap();
            }
        });
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        assert_eq!(
            world
                .move_ordinary(
                    id,
                    movement,
                    1,
                    0,
                    ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        ..Default::default()
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            0
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            position,
            "{movement:?}"
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.motion_direction, direction);
        assert_eq!(control.move_counter, counter);
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], called);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn synchronous_walk_forward_uses_live_body_refusal_without_creating_a_task() {
    use cyrus_script::script::{
        LocomotionGeometry, ObstacleEnvironment, OrdinaryGroundQuery, OrdinaryMovement, ScriptHud,
        ScriptInputs,
    };
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    let hud = ScriptHud::default();
    let before = world.spatial(id).unwrap().local.position;
    assert_eq!(
        world
            .move_ordinary(
                id,
                OrdinaryMovement::WalkForward,
                1,
                0,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    forward_step_clear: Some(true),
                    locomotion_geometry: Some(LocomotionGeometry {
                        ground: OrdinaryGroundQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            distance: u32::MAX,
                            sidedness: false
                        },
                        environment: ObstacleEnvironment {
                            selected_player: None,
                            auto_grab: false,
                            item_system_ready: true,
                            shell_execution: false,
                            ticks: 0
                        }
                    }),
                    ..Default::default()
                },
                &mut |_, _| panic!("body refusal reached sounds")
            )
            .unwrap(),
        0
    );
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide
            .bits(),
        0x80
    );
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn synchronous_movement_rejects_unknown_timing_before_receiver_writes() {
    use cyrus_script::script::{OrdinaryMovement, ScriptInputs};
    for (scale, ticks) in [(None, Some(1)), (Some(256), None)] {
        let mut world = forward_amount_world(18, false, true, |_| {});
        let id = world.id(0).unwrap();
        let before = world.inspect(0, 1);
        let error = world
            .move_ordinary(
                id,
                OrdinaryMovement::Forward,
                1,
                0,
                ScriptInputs {
                    frame_scale: scale,
                    nominal_ticks: ticks,
                    ..Default::default()
                },
                &mut |_, _| panic!("invalid timing reached sounds"),
            )
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(
            error.contains("world 7 instance 0 Forward synchronous movement"),
            "{error}"
        );
        assert!(
            error.contains("requires frame_scale and nominal_ticks"),
            "{error}"
        );
        assert_eq!(world.inspect(0, 1), before);
    }
}

#[test]
fn forward_continuation_walks_on_once_per_update_without_requesting_a_group() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let (mut world, _scripts, _data) = spacing_fixture(|_| {});
    world.ordinary_actors([0, 0]).unwrap();
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    let inputs = || ScriptInputs {
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        hud: Some(&hud),
        shell_execution: Some(false),
        ..Default::default()
    };
    // Without frame scale the step is unsupported rather than guessed.
    let error = world
        .continue_ordinary_motion(
            id,
            ScriptInputs {
                hud: Some(&hud),
                ..Default::default()
            },
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("frame_scale"));
    let start = world.spatial(id).unwrap().local.position;
    world
        .continue_ordinary_motion(id, inputs(), None, &mut |_, _| Ok(()))
        .unwrap();
    let after = world.spatial(id).unwrap().local.position;
    assert_ne!(after, start);
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(control.motion_direction, MotionDirection::FORWARD);
    assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 4);
    // The forward-called flag ends the continuation for the rest of the update.
    world
        .continue_ordinary_motion(id, inputs(), None, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(world.spatial(id).unwrap().local.position, after);
}

#[test]
fn backward_continuation_sets_its_own_called_flag() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    {
        let (group, flag, direction) = (93, 8, MotionDirection::BACKWARD);
        let (mut world, _scripts, _data) = spacing_fixture_group(group, |_| {});
        world.ordinary_actors([0, 0]).unwrap();
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        let start = world.spatial(id).unwrap().local.position;
        world
            .continue_ordinary_motion(
                id,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_ne!(
            world.spatial(id).unwrap().local.position,
            start,
            "group {group}"
        );
        assert_eq!(
            world.actors()[0].control_state().unwrap().motion_direction,
            direction
        );
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], flag);
    }
}

#[test]
fn walk_continuation_steps_and_geometry_steps_wait_for_their_check() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    // (group, moves without collision geometry, motion direction, called flag when it moved);
    // every step here needs the collision geometry for its step check.
    for (group, moves, direction, flag) in [
        (20, false, MotionDirection::WALK_FORWARD, 0),
        (31, false, MotionDirection::WALK_FORWARD, 0),
        (22, false, MotionDirection::WALK_BACKWARD, 0),
        (24, false, MotionDirection::LEFT, 0),
        (27, false, MotionDirection::RIGHT, 0),
    ] {
        let (mut world, _scripts, _data) = spacing_fixture_group(group, |_| {});
        world.ordinary_actors([0, 0]).unwrap();
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        let start = world.spatial(id).unwrap().local.position;
        world
            .continue_ordinary_motion(
                id,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        let moved = world.spatial(id).unwrap().local.position != start;
        assert_eq!(moved, moves, "group {group}");
        assert_eq!(
            world.actors()[0].control_state().unwrap().motion_direction,
            direction,
            "group {group}"
        );
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], flag);
    }
}

#[test]
fn a_swimming_continuation_waits_for_its_edge_probe() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let (mut world, _scripts, _data) = spacing_fixture_group(92, |_| {});
    world.ordinary_actors([0, 0]).unwrap();
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, Surface::DEEPWATER).unwrap();
    let hud = ScriptHud::default();
    let start = world.spatial(id).unwrap().local.position;
    world
        .continue_ordinary_motion(
            id,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                hud: Some(&hud),
                shell_execution: Some(false),
                ..Default::default()
            },
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(world.spatial(id).unwrap().local.position, start);
    assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
}
