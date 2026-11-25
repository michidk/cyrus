use super::*;

pub(super) fn wander_world(radius: i32, movement: i32, maps: serde_json::Value) -> ScriptWorld {
    use cyrus_assets::animation::Animations;
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: Native::MoveByAxis,
            param_count: 3,
            kind: CallKind::Task,
        },
        Function {
            name: Native::Wander,
            param_count: 2,
            kind: CallKind::Function,
        },
    ]);
    scripts.instances[0].map_position = [1000, 2, 2000];
    let mut operations = Vec::new();
    for (axis, delta) in [(0, movement), (1, 256)] {
        operations.push(wait(
            1,
            Native::MoveByAxis,
            CallKind::Multitask,
            vec![
                Value::Integer { value: axis },
                Value::Integer { value: delta },
                Value::Integer { value: 1 },
            ],
        ));
    }
    operations.push(Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 2,
                    function: Native::Wander,
                    kind: CallKind::Function,
                    target: Object::Me,
                    arguments: vec![
                        Value::Integer { value: radius },
                        Value::Integer { value: 32768 },
                    ],
                }),
            },
            rest: vec![],
        },
    });
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
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id, "walk_node_maps": maps}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    world.prepare_spatial().unwrap();
    world
}

#[test]
fn empty_map_wander_consumes_original_draws_and_restores_excess_displacement() {
    use cyrus_game::random::EngineRandom;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for (radius, movement, scale, state, restore) in [
        (4, 0, 256, 2524885223, false),
        (5, 0, 256, 3295386429, false),
        (1204, 0, 256, 2524885223, false),
        (1204, 8448, 256, 2524885223, true),
        (1204, 8192, 256, 2524885223, false),
        (1204, 256, 0, 2524885223, true),
    ] {
        let mut world = wander_world(radius * 256, movement, serde_json::json!([]));
        let id = world.id(0).unwrap();
        let mut random = EngineRandom::default();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(scale),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                Some(&mut random),
                20,
            )
            .unwrap();
        assert_eq!(world.flags(), [movement]);
        assert_eq!(random.state(), state, "radius {radius}");
        assert_eq!(world.actors()[0].attributes()[Attribute::NodeInUse], 1);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            if restore {
                [256000, 512, 512000]
            } else {
                [256000 + movement, 768, 512000]
            }
        );
    }
}

#[test]
fn wander_rejects_a_zero_radius_without_consuming_random() {
    use cyrus_game::random::EngineRandom;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut world = wander_world(255, 0, serde_json::json!([]));
    let id = world.id(0).unwrap();
    let mut random = EngineRandom::default();
    let error = world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                ..Default::default()
            },
            None,
            Some(&mut random),
            20,
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("positive map-unit radius"),
        "{error:#}"
    );
    assert_eq!(random.state(), 1);
    assert!(world.actors()[0].failure().is_some());
}

#[test]
fn wander_with_maps_picks_the_nearest_node_as_its_destination_and_ends_on_arrival() {
    use cyrus_game::random::EngineRandom;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    // The home is [1000, 2, 2000] map units; the first map has a node on it, the second none.
    for (maps, found) in [
        (
            serde_json::json!([{"node_count":1,"walk_nodes":[walk_node([1000, 2, 2000])]}]),
            true,
        ),
        (serde_json::json!([{"node_count":0,"walk_nodes":[]}]), false),
    ] {
        let mut world = wander_world(1204 * 256, 0, maps);
        let id = world.id(0).unwrap();
        let mut random = EngineRandom::default();
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
                Some(&mut random),
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].attributes()[Attribute::NodeInUse], 1);
        assert_eq!(random.state(), 2524885223, "the empty-map draw sequence");
        let navigation = world.actors()[0].control_state().unwrap().navigation;
        // The actor stands on the node, so a found destination is reached and cleared at once.
        assert_eq!(navigation.destination, -1);
        assert_eq!(navigation.map, found.then_some(0));
    }
}

#[test]
fn ai_setters_master_slave_and_true_function_follow_their_bytes() {
    assert_eq!(
        run_native_calls(&[(Native::SetAiType, &[300]), (Native::MyAiType, &[])]).unwrap(),
        44
    );
    assert_eq!(
        run_native_calls(&[(Native::SetAiMode, &[7]), (Native::MyAiMode, &[])]).unwrap(),
        7
    );
    assert_eq!(run_native_calls(&[(Native::IsMaster, &[])]).unwrap(), 0);
    assert_eq!(
        run_native_calls(&[(Native::SetMyAttr, &[26, 3]), (Native::IsSlave, &[])]).unwrap(),
        1
    );
    assert_eq!(
        run_native_calls(&[(Native::SetMyAttr, &[25, 3]), (Native::IsMaster, &[])]).unwrap(),
        1
    );
    assert_eq!(
        run_native_calls(&[(Native::TrueFunction, &[0, 5])]).unwrap(),
        1
    );
}

fn ai_progress_fixture(
    mode: i32,
    slide: i32,
    response: i32,
    heading: u32,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    ai_progress_fixture_with_delay(mode, 17, slide, response, heading)
}

fn ai_progress_fixture_with_delay(
    mode: i32,
    delay: i32,
    slide: i32,
    response: i32,
    heading: u32,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let (mut world, mut data) =
        overlap_response_fixture_tweaked(0, 0, 83, 0, LivingState::ORDINARY, |scripts| {
            scripts.instances[0].angles[1] = heading;
            scripts.definition.references = vec![
                Property::AiMode,
                Property::AiDelay,
                Property::SlideFlag,
                Property::LastCollide,
            ];
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.attributes[Attribute::ForwardSpeed] = 4;
            actor.program.steps = [mode, delay, slide, response]
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    (
                        index as u32,
                        Step {
                            next: index as u32 + 1,
                            operation: Operation::Assign {
                                destination: Place::Property {
                                    object: Object::Me,
                                    reference: index as u16,
                                },
                                formula: Formula {
                                    first: Value::Integer { value },
                                    rest: vec![],
                                },
                            },
                        },
                    )
                })
                .chain([(
                    4,
                    Step {
                        next: 5,
                        operation: Operation::End { target: 0 },
                    },
                )])
                .collect();
        });
    data.metadata["actors"][0]["animation_groups"] = serde_json::Value::Array(
        [14, 15, 16, 17, 18, 20, 96, 97]
            .into_iter()
            .map(|group| {
                serde_json::json!({
                    "group_index":group,"anim_id":group+1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]
                })
            })
            .collect(),
    );
    world
        .bind_animation(world.id(0).unwrap(), &data, AnimationGroup::RUN)
        .unwrap();
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Animation,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            20,
        )
        .unwrap();
    world
        .advance(
            world.id(0).unwrap(),
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
    (world, data)
}

#[test]
fn ai_progress_preserves_or_resets_mode_after_retained_clearance() {
    use cyrus_script::script::{OrdinaryObstacleQuery, OrdinarySlopeInputs, ScriptInputs};
    for (mode, slide, response, distance, expected) in [
        (11, 0, 0, 0, Some((11, 17))),
        (11, 1, 0, 0, Some((0, 255))),
        (11, 0, 1, 0, Some((0, 255))),
        (11, 0, 0, 131073, Some((0, 255))),
        (0, 0, 0, 0, None),
    ] {
        let (mut world, data) = ai_progress_fixture(mode, slide, response, 0);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        let from = world.spatial(ids[0]).unwrap().local.position;
        let (_, _, models) = static_model_fixture(cache());
        let scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
            .unwrap();
        let before = world.inspect(0, 2);
        let result = world.progress_ai_direct_destination(
            ids[0],
            [from[0] + distance, from[1], from[2]],
            OrdinaryObstacleQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                vertical_offset: 0,
                respond: false,
            },
            OrdinarySlopeInputs {
                selected_player: Some(ids[1]),
                holding_weapon: false,
                combat_active: false,
                frame_scale: 256,
                ticks: 0,
            },
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                ..Default::default()
            },
            &mut |_, _| Ok(()),
        );
        if let Some((mode, delay)) = expected {
            assert_eq!(result.unwrap(), 1);
            let control = world.actors()[0].control_state().unwrap();
            assert_eq!((control.ai_mode.byte(), control.ai_delay), (mode, delay));
            assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 15);
            assert_eq!(world.spatial(ids[0]).unwrap().local.position, from);
        } else {
            assert!(format!("{:#}", result.unwrap_err()).contains("requires mode eleven"));
            assert_eq!(world.inspect(0, 2), before);
        }
    }
}

#[test]
fn ai_progress_uses_full_response_word_from_real_overlap() {
    use cyrus_script::script::{OrdinaryObstacleQuery, OrdinarySlopeInputs, ScriptInputs};
    let (mut world, data) = ai_progress_fixture(11, 0, 0, 0);
    let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
    world
        .resolve_ordinary_actor_overlap(ids[0], ids[1], &data)
        .unwrap();
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide
            .bits(),
        0x400
    );
    let point = world.spatial(ids[0]).unwrap().local.position;
    let (_, _, models) = static_model_fixture(cache());
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
        .unwrap();
    assert_eq!(
        world
            .progress_ai_direct_destination(
                ids[0],
                point,
                OrdinaryObstacleQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    vertical_offset: 0,
                    respond: false
                },
                OrdinarySlopeInputs {
                    selected_player: Some(ids[1]),
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: 256,
                    ticks: 0
                },
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                &mut |_, _| Ok(())
            )
            .unwrap(),
        1
    );
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            control.ai_mode.byte(),
            control.ai_delay,
            control.last_collide.bits()
        ),
        (0, 255, 0x400)
    );
}

#[test]
fn ai_progress_strict_heading_gate_calls_real_movement_and_registers_position() {
    use cyrus_script::script::{OrdinaryObstacleQuery, OrdinarySlopeInputs, ScriptInputs};
    let hud = cyrus_script::script::ScriptHud::default();
    for (heading, admitted) in [
        (255, true),
        (256, false),
        (1793, true),
        (1792, false),
        (0, true),
    ] {
        let (mut world, data) = ai_progress_fixture(11, 0, 0, heading);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        let from = world.spatial(ids[0]).unwrap().local.position;
        let destination = [from[0], from[1], from[2] + 25600];
        let (_, _, models) = static_model_fixture(cache());
        let scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
            .unwrap();
        let result = world.progress_ai_direct_destination(
            ids[0],
            destination,
            OrdinaryObstacleQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                vertical_offset: 0,
                respond: false,
            },
            OrdinarySlopeInputs {
                selected_player: Some(ids[1]),
                holding_weapon: false,
                combat_active: false,
                frame_scale: 256,
                ticks: 0,
            },
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                forward_step_clear: Some(true),
                hud: Some(&hud),
                shell_execution: Some(false),
                ..Default::default()
            },
            &mut |_, _| Ok(()),
        );
        assert_eq!(result.unwrap(), 1, "heading {heading}");
        assert_eq!(
            world.actors()[0].control_state().unwrap().motion_direction,
            if admitted {
                MotionDirection::FORWARD
            } else {
                MotionDirection::NONE
            },
            "heading {heading}"
        );
        assert_eq!(
            world.spatial(ids[0]).unwrap().local.position,
            if heading == 0 {
                [from[0], from[1], from[2] + 1024]
            } else {
                from
            },
            "heading {heading}"
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        if admitted {
            world.flush_map_updates([0; 3], false).unwrap();
        }
    }
}

#[test]
fn ai_progress_matches_original_public_caller_heading_and_refusal() {
    use cyrus_script::script::{OrdinaryObstacleQuery, OrdinarySlopeInputs, ScriptInputs};
    for slide in [0, 1] {
        let (mut world, data) = ai_progress_fixture(11, slide, 0, 1216);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        let from = world.spatial(ids[0]).unwrap().local.position;
        let (_, _, models) = static_model_fixture(cache());
        let scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
            .unwrap();
        // Verified original public mode-eleven/Guard observation, translated to the fixture origin.
        let destination = [from[0] + 873472, from[1] - 84992, from[2] + 435456];
        assert_eq!(
            world
                .progress_ai_direct_destination(
                    ids[0],
                    destination,
                    OrdinaryObstacleQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        vertical_offset: 0,
                        respond: false
                    },
                    OrdinarySlopeInputs {
                        selected_player: Some(ids[1]),
                        holding_weapon: false,
                        combat_active: false,
                        frame_scale: 256,
                        ticks: 0
                    },
                    ScriptInputs::default(),
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            1
        );
        assert_eq!(world.spatial(ids[0]).unwrap().local.angles[1], 1224);
        assert_eq!(world.spatial(ids[0]).unwrap().local.position, from);
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!((control.ai_mode.byte(), control.ai_delay), (0, 255));
    }
}

#[test]
fn ai_progress_retains_slide_and_collision_writes_before_service_failure() {
    use cyrus_script::script::{OrdinaryObstacleQuery, OrdinarySlopeInputs, ScriptInputs};
    let (mut world, data) = ai_progress_fixture(11, 1, 0, 0);
    let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
    let from = world.spatial(ids[0]).unwrap().local.position;
    let (_, _, models) = static_model_fixture(cache());
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
        .unwrap();
    let error = world
        .progress_ai_direct_destination(
            ids[0],
            [from[0], from[1], from[2] + 25600],
            OrdinaryObstacleQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                vertical_offset: 0,
                respond: false,
            },
            OrdinarySlopeInputs {
                selected_player: Some(ids[1]),
                holding_weapon: false,
                combat_active: false,
                frame_scale: 256,
                ticks: 0,
            },
            ScriptInputs::default(),
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("MovePos requires frame_scale"),
        "{error:#}"
    );
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (control.ai_mode.byte(), control.ai_delay, control.slide_flag),
        (0, 255, 1)
    );
    assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 15);
    assert_eq!(world.spatial(ids[0]).unwrap().local.position, from);
}

#[test]
fn ai_admission_obeys_mode_delay_clearance_and_releases_owned_nodes() {
    use cyrus_script::script::{OrdinaryObstacleQuery, WalkNodeSlot};
    for (mode, delay, distance, admitted) in [
        (0, 0, 0, true),
        (10, 0, 131072, true),
        (0, 0, 131073, false),
        (11, 0, 0, false),
        (0, 1, 0, false),
        (0, 255, 0, false),
    ] {
        let (mut world, mut data) = ai_progress_fixture_with_delay(mode, delay, 0, 0, 0);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        let from = world.spatial(ids[0]).unwrap().local.position;
        data.metadata["walk_node_maps"] = serde_json::json!([
            {"node_count":2,"walk_nodes":[walk_node([0,0,0]),walk_node([1,0,0])]}
        ]);
        world.prepare_navigation(&data).unwrap();
        assert!(
            world
                .assign_walk_node(ids[0], 1, WalkNodeSlot::Next)
                .unwrap()
        );
        let navigation = world.actors()[0].control_state().unwrap().navigation;
        let (_, _, models) = static_model_fixture(cache());
        let scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
            .unwrap();
        let collide = world.actors()[0].attributes()[Attribute::Collide];
        assert_eq!(
            world
                .admit_ai_direct_destination(
                    ids[0],
                    ids[1],
                    [from[0] + distance, from[1], from[2]],
                    OrdinaryObstacleQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        vertical_offset: 0,
                        respond: false,
                    },
                )
                .unwrap(),
            admitted,
            "mode {mode} delay {delay} distance {distance}"
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.ai_mode.byte(), control.ai_delay),
            (if admitted { 11 } else { mode as u8 }, delay as u8)
        );
        assert_eq!(
            (
                control.navigation.map,
                control.navigation.destination,
                control.navigation.previous_next
            ),
            (
                navigation.map,
                navigation.destination,
                navigation.previous_next
            )
        );
        assert_eq!(
            (control.navigation.current, control.navigation.next),
            if admitted {
                (-1, -1)
            } else {
                (navigation.current, navigation.next)
            }
        );
        assert_eq!(world.spatial(ids[0]).unwrap().local.position, from);
        assert_eq!(world.actors()[0].attributes()[Attribute::Collide], collide);
        assert_eq!(world.reserve_walk_node(ids[1], 0, 1).unwrap(), admitted);
    }
}
