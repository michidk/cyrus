use super::*;

fn wait_line_up_scripts(object: Object) -> Scripts {
    let mut scripts = actor_reference_scripts(object);
    let function_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: Native::WaitLineUp,
        kind: CallKind::Task,
        param_count: 1,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.steps.get_mut(&1).unwrap().operation = Operation::Call {
        call: Call {
            function_id,
            function: Native::WaitLineUp,
            kind: CallKind::Task,
            target: Object::Me,
            arguments: vec![Value::Read {
                place: Place::Local { index: 0 },
            }],
        },
    };
    actor.program.steps.get_mut(&3).unwrap().operation = set_flag(5);
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn wait_line_up_stays_active_while_the_argument_actors_alignment_flag_is_set() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let scripts = wait_line_up_scripts(Object::Named { string: 0 });
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,"actors":[]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let inputs = ScriptInputs {
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    let world = ScriptWorld::prepare(&scripts, 17).unwrap();
    // Mask 2 is the flag; mask 1 and every other bit leave the task finished at once.
    for (flag_byte, waits) in [
        (0u8, false),
        (1, false),
        (2, true),
        (3, true),
        (0xfd, false),
    ] {
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["control"]["script_control"] = flag_byte.into();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let step = |world: &mut ScriptWorld, stage| {
            let id = world.id(1).unwrap();
            world.advance(id, stage, inputs, None, None, 20).unwrap()
        };
        // A script task statement always allocates its slot; the handler's answer applies on the
        // task pass.
        step(&mut world, ScriptStage::OrdinaryMain);
        assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
        step(&mut world, ScriptStage::Tasks);
        assert_eq!(
            world.actors()[1].runtime().unwrap().active_tasks(),
            usize::from(waits),
            "flag byte {flag_byte:#x}"
        );
        // The script's own yield follows the task.
        step(&mut world, ScriptStage::OrdinaryMain);
        step(&mut world, ScriptStage::OrdinaryMain);
        assert_eq!(world.flags()[0] == 5, !waits, "flag byte {flag_byte:#x}");
        if !waits {
            continue;
        }
        // The task keeps waiting while the flag stays set, across a restore, and completes the
        // update after the flag clears.
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["control"]["script_control"] = (flag_byte & !2).into();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        step(&mut world, ScriptStage::Tasks);
        assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 0);
        step(&mut world, ScriptStage::OrdinaryMain);
        step(&mut world, ScriptStage::OrdinaryMain);
        assert_eq!(world.flags()[0], 5);
    }
}

/// The map actor `LOGIC` (instance 0) is the receiver of `ObjectLineUp`; the `CALLER` actor
/// (instance 1) starts the task on it, waits for its alignment flag with `WaitLineUp` and then
/// sets flag 5. With `cancel`, the receiver's own script clears its tasks when it runs.
fn object_line_up_scripts(cancel: bool) -> Scripts {
    let mut scripts = actor_reference_scripts(Object::Named { string: 0 });
    let [line_up, wait_line_up, kill_tasks] = [
        (Native::ObjectLineUp, CallKind::Task, 3),
        (Native::WaitLineUp, CallKind::Task, 1),
        (Native::KillMyTasks, CallKind::Function, 0),
    ]
    .map(|(name, kind, param_count)| {
        let id = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name,
            kind,
            param_count,
        });
        id
    });
    let steps = |operations: Vec<Operation>| {
        operations
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
            .collect::<BTreeMap<_, _>>()
    };
    let mut caller = scripts.actors["LOGIC"].clone();
    caller.program.steps = steps(vec![
        Operation::ObjectCall {
            call: Call {
                function_id: line_up,
                function: Native::ObjectLineUp,
                kind: CallKind::Task,
                target: Object::Named { string: 0 },
                arguments: [0, 0, 1024].map(|value| Value::Integer { value }).to_vec(),
            },
        },
        Operation::Call {
            call: Call {
                function_id: wait_line_up,
                function: Native::WaitLineUp,
                kind: CallKind::Task,
                target: Object::Me,
                arguments: vec![Value::Read {
                    place: Place::Property {
                        object: Object::Named { string: 0 },
                        reference: 294,
                    },
                }],
            },
        },
        set_flag(5),
        Operation::End { target: 0 },
    ]);
    caller.code_origin = 400;
    scripts.actors.insert("CALLER".into(), caller);
    // The alignment arrival threshold reads the speed attribute of the current group, which a
    // receiver outside the table's walking groups answers with zero.
    for attribute in [
        Attribute::ForwardSpeed,
        Attribute::BackwardSpeed,
        Attribute::SideSpeed,
        Attribute::ForwardWalk,
        Attribute::BackwardWalk,
    ] {
        scripts.actors.get_mut("LOGIC").unwrap().attributes[attribute] = 3;
    }
    scripts.instances[1].actor = "CALLER".into();
    if cancel {
        // The interrupt thread (steps 0 and 1) runs the cancellation; the main thread idles.
        let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
        program.entry = 2;
        program.steps = steps(vec![
            wait(kill_tasks, Native::KillMyTasks, CallKind::Function, vec![]),
            Operation::Endint,
            Operation::Yield,
            Operation::End { target: 2 },
        ]);
    }
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

/// An ordinary (not selected) receiver with an animation table, collision shape and the caller.
struct OrdinaryLineUp {
    world: ScriptWorld,
    scripts: Scripts,
    data: cyrus_assets::animation::Animations,
    scene: cyrus_script::player::data::PlayerScene,
}

fn ordinary_line_up(cancel: bool) -> OrdinaryLineUp {
    let mut scripts = object_line_up_scripts(cancel);
    // The fixture places every instance as a static model at one point; keep the identity it
    // derives for the script set it builds the world from.
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, mut data, models) =
        static_model_fixture_at(scripts.clone(), Some(20.0), [10, 20, 30]);
    let (caller, receiver) = (world.id(1).unwrap(), world.id(0).unwrap());
    data.metadata["mpob_objects"] = serde_json::json!([
        {"script_name":"LOGIC","object_type":1,"model_id":-1},
        {"script_name":"CALLER","object_type":1,"model_id":-1}
    ]);
    let logic = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    logic["model_load_path"] = "animation_table".into();
    logic["raan_entries"] = serde_json::json!([{"file_path":"LOGIC.3D","frame_count":200}]);
    logic["animation_groups"] = serde_json::json!(
        [0, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 30, 31, 142, 143].map(|group| {
            serde_json::json!({
                "group_index":group,"anim_id":group+1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":group},{"opcode":2,"value":0}]
            })
        })
    );
    data.models.insert(
        "LOGIC".into(),
        cyrus_assets::animation::AnimationModel {
            radius: Some(31.0),
            base_positions: vec![],
            base_normals: vec![],
            faces: vec![],
            corner_normals: vec![],
            encoding: "animated_i16".into(),
            positions: vec![vec![]; 200],
            normals: vec![vec![]; 200],
        },
    );
    world
        .initialize_map_animation(receiver, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.refresh_model_presentations(&data, &[caller]).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 50;
    world
        .prepare_ordinary_collision_shape(receiver, &data, &scene)
        .unwrap();
    OrdinaryLineUp {
        world,
        scripts,
        data,
        scene,
    }
}

impl OrdinaryLineUp {
    /// One scheduler round: the caller's ordinary update, then the receiver's task update, or its
    /// interrupt script when `receiver_script` is set.
    fn round(&mut self, receiver_script: bool) -> anyhow::Result<()> {
        use cyrus_script::script::{PlayerCollisionSource, ScriptInputs, ScriptStage};
        let (caller, receiver) = (self.world.id(1)?, self.world.id(0)?);
        let actors = [receiver, caller];
        let hud = cyrus_script::script::ScriptHud::default();
        let inputs = ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            hud: Some(&hud),
            shell_execution: Some(false),
            player_collision: Some(PlayerCollisionSource {
                scene: &self.scene,
                animations: &self.data,
                view: [10, 30],
                actors: &actors,
            }),
            ..Default::default()
        };
        let mut random = cyrus_game::random::EngineRandom::default();
        let mut run = |world: &mut ScriptWorld, id, stage| {
            world.advance(
                id,
                stage,
                inputs,
                Some(&mut |_, _| Ok(())),
                Some(&mut random),
                20,
            )
        };
        run(&mut self.world, caller, ScriptStage::OrdinaryMain)?;
        let stage = if receiver_script {
            ScriptStage::Body(Thread::Interrupt)
        } else {
            ScriptStage::Tasks
        };
        run(&mut self.world, receiver, stage)
    }

    /// Replace the world through its snapshot, keeping the session globals and optionally
    /// putting the receiver's animation in `group`.
    fn replace(&mut self, group: Option<u16>) {
        use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
        let collision = Default::default();
        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &self.scripts,
            animations: &self.data,
            collision: &collision,
        };
        let mut saved = serde_json::to_value(self.world.snapshot(&sources).unwrap()).unwrap();
        if let Some(group) = group {
            saved["actors"][0]["animation"]["playback"]["current"][0] = group.into();
        }
        let generation = self.world.generation() + 1;
        let globals = std::mem::take(self.world.globals_mut());
        self.world = ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            WorldRestoreContext {
                generation,
                globals,
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
    }

    fn flag(&self) -> u8 {
        self.world.actors()[0]
            .control_state()
            .unwrap()
            .script_control
            & 2
    }

    fn position(&self) -> [i32; 3] {
        self.world
            .spatial(self.world.id(0).unwrap())
            .unwrap()
            .local
            .position
    }
}

#[test]
fn object_line_up_on_an_ordinary_actor_holds_the_flag_wait_line_up_reads_until_arrival() {
    // The receiver is not the selected player. Its task publishes the alignment flag in its own
    // script-control byte, the caller's WaitLineUp stays active while it is set and arrival clears it and releases the caller.
    let mut fixture = ordinary_line_up(false);
    let start = fixture.position();
    fixture.round(false).unwrap();
    assert_eq!(fixture.flag(), 2, "the call publishes the flag");
    assert_eq!(
        fixture.world.actors()[0].runtime().unwrap().active_tasks(),
        1
    );
    assert!(fixture.world.flags().is_empty() || fixture.world.flags()[0] != 5);
    let mut cleared = false;
    for round in 0..600 {
        if round == 3 {
            // The walking group selects the speed attribute the arrival test reads.
            fixture.replace(Some(18));
            assert_eq!(fixture.flag(), 2, "the flag survives the replacement");
        }
        fixture
            .round(false)
            .unwrap_or_else(|error| panic!("round {round}: {error:#}"));
        if fixture.flag() == 0 {
            cleared = true;
            break;
        }
        assert_ne!(
            fixture.world.flags().first(),
            Some(&5),
            "round {round}: the flag holds the caller"
        );
    }
    assert!(cleared, "arrival clears the flag");
    // The caller sees the cleared flag on its next update.
    fixture.round(false).unwrap();
    fixture.round(false).unwrap();
    let released = fixture.world.flags().first() == Some(&5);
    assert!(released, "the caller is released after arrival");
    assert_eq!(fixture.flag(), 0, "arrival clears the flag");
    assert_eq!(
        fixture.world.actors()[0].runtime().unwrap().active_tasks(),
        0
    );
    assert_ne!(fixture.position(), start, "the receiver walked");
}

#[test]
fn cancelling_an_object_line_up_leaves_the_flag_that_wait_line_up_keeps_reading() {
    // KillMyTasks clears the receiver's slots without running a handler, so nothing clears the
    // alignment flag: the caller keeps waiting.
    let mut fixture = ordinary_line_up(true);
    fixture.round(false).unwrap();
    assert_eq!(
        (
            fixture.flag(),
            fixture.world.actors()[0].runtime().unwrap().active_tasks()
        ),
        (2, 1)
    );
    fixture.round(true).unwrap();
    assert_eq!(
        fixture.world.actors()[0].runtime().unwrap().active_tasks(),
        0
    );
    for _ in 0..10 {
        fixture.round(false).unwrap();
    }
    assert_eq!(fixture.flag(), 2);
    assert_ne!(fixture.world.flags().first(), Some(&5));
}

#[test]
fn a_receivers_lineup_natives_walk_the_selected_player_onto_the_target() {
    use cyrus_script::script::{
        LocomotionGeometry, ObstacleEnvironment, OrdinaryGroundQuery, PlayerCollisionSource,
        ScriptActorSource, ScriptInputs, ScriptStage,
    };
    #[derive(Clone, Copy, Debug)]
    enum Continuation {
        Normal,
        MoveReference,
        MissingCollision,
        KillReceiver,
        KillCreatedReceiver,
    }
    use Continuation::*;
    // The receiver stands where the player does; the target is 8 map units ahead (+Z) and the
    // final heading (receiver heading 0 + angle 1024 + 1024) is the player's own, so no turn runs.
    // The pickup and parallel natives add their offsets unrotated; PlayerLineUp rotates them by the
    // reference heading plus the angle (half a turn here) and picks its mode from the distance.
    'cases: for (function, lineup_distance, z_offset, continuation) in [
        (Native::PlayerParallelLineUp, None, 2048, Normal),
        (Native::PlayerPickupLineUp, None, 2048, Normal),
        (Native::PlayerLineUp, Some(0), -2048, Normal),
        (Native::PlayerLineUp, Some(6655), -2048, Normal),
        (Native::PlayerLineUp, Some(6656), -2048, Normal),
        (Native::PlayerLineUp, Some(i32::MAX), -2048, Normal),
        (Native::PlayerLineUp, Some(i32::MAX), 2048, Normal),
        (Native::PlayerLineUp, Some(i32::MAX), -2048, MoveReference),
        (
            Native::PlayerLineUp,
            Some(i32::MAX),
            -8192,
            MissingCollision,
        ),
        (Native::PlayerLineUp, Some(i32::MAX), -8192, KillReceiver),
        (
            Native::PlayerLineUp,
            Some(i32::MAX),
            -8192,
            KillCreatedReceiver,
        ),
    ] {
        let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
            scripts.lineup_distance = lineup_distance;
            let id = u16::try_from(scripts.definition.functions.len()).unwrap();
            scripts.definition.functions.push(Function {
                name: function,
                param_count: 3,
                kind: CallKind::Task,
            });
            scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::ForwardWalk] = 3;
            let kill_id = u16::try_from(scripts.definition.functions.len()).unwrap();
            scripts.definition.functions.push(Function {
                name: Native::KillMe,
                param_count: 0,
                kind: CallKind::Function,
            });
            let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
            program.steps = [
                wait(
                    id,
                    function,
                    CallKind::Task,
                    vec![
                        Value::Integer { value: 1024 },
                        Value::Integer { value: 0 },
                        Value::Integer { value: z_offset },
                    ],
                ),
                set_flag(7),
                Operation::End { target: 0 },
            ]
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
            if matches!(continuation, KillReceiver | KillCreatedReceiver) {
                // The interrupt at zero kills the receiver while its main thread waits.
                program.entry = 3;
                program.steps = std::mem::take(&mut program.steps)
                    .into_iter()
                    .map(|(offset, mut step)| {
                        step.next += 3;
                        if let Operation::End { target } = &mut step.operation {
                            *target += 3;
                        }
                        (offset + 3, step)
                    })
                    .collect();
                program.steps.insert(
                    0,
                    Step {
                        next: 1,
                        operation: wait(kill_id, Native::KillMe, CallKind::Function, vec![]),
                    },
                );
                program.steps.insert(
                    1,
                    Step {
                        next: 2,
                        operation: Operation::Endint,
                    },
                );
            }
        });
        data.metadata["markers"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let mut player = world.id(index).unwrap();
        let mut map = world.id(0).unwrap();
        if matches!(continuation, KillCreatedReceiver) {
            let position = world.spatial(map).unwrap().local.position;
            map = world.create_object(map, "LOGIC", position, [0; 3]).unwrap();
        }
        let hud = cyrus_script::script::ScriptHud::default();
        let mut actors = vec![player, world.id(0).unwrap()];
        if matches!(continuation, KillCreatedReceiver) {
            actors.push(map);
        }
        let mut random = cyrus_game::random::EngineRandom::default();
        let mut positions = vec![];
        let mut crossed_distance_boundary = false;
        let distance = |world: &ScriptWorld| {
            let player = world
                .spatial(world.id(index).unwrap())
                .unwrap()
                .local
                .position;
            let receiver = world.spatial(world.id(0).unwrap()).unwrap().local.position;
            cyrus_script::script::rounded_distance(std::array::from_fn(|axis| {
                player[axis] - receiver[axis]
            }))
            .unwrap()
        };
        assert_eq!(distance(&world), 6656);
        // The player's own update has begun, as it has by the time a receiver's script runs.
        world.begin_player_movement_update(player, 0).unwrap();
        for round in 0..40 {
            let prepared = world
                .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &actors)
                .unwrap();
            let inputs = ScriptInputs {
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                player_collision: Some(PlayerCollisionSource {
                    scene: &scene,
                    animations: &data,
                    view: [10, 30],
                    actors: &actors,
                }),
                locomotion_geometry: Some(LocomotionGeometry {
                    ground: OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 4,
                        sidedness: false,
                    },
                    environment: ObstacleEnvironment {
                        selected_player: Some(player),
                        auto_grab: false,
                        item_system_ready: false,
                        shell_execution: false,
                        ticks: 1,
                    },
                }),
                ..Default::default()
            };
            if matches!(continuation, MissingCollision) && round > 0 {
                let position = world.spatial(player).unwrap().local.position;
                let result = world.inspect(0, 1)["globals"]["lineup_result"].clone();
                let outcome = world.advance(
                    map,
                    ScriptStage::Tasks,
                    ScriptInputs {
                        player_collision: None,
                        ..inputs
                    },
                    Some(&mut |_, _| Ok(())),
                    Some(&mut random),
                    20,
                );
                let Err(error) = outcome else {
                    assert_eq!(world.flags(), [12]);
                    continue;
                };
                assert!(
                    format!("{error:#}")
                        .contains("collision geometry bound to the selected player")
                );
                assert_eq!(world.spatial(player).unwrap().local.position, position);
                assert_eq!(world.inspect(0, 1)["globals"]["lineup_result"], result);
                assert_eq!(world.flags(), [12]);
                assert!(world.actors()[map.index].failure().is_some());
                assert!(
                    world
                        .advance(
                            map,
                            ScriptStage::Tasks,
                            inputs,
                            Some(&mut |_, _| Ok(())),
                            Some(&mut random),
                            20
                        )
                        .is_err()
                );
                assert!(
                    !world
                        .run_player_native_hook(player, &mut |_| {
                            panic!("failed alignment must not dispatch player input")
                        })
                        .unwrap()
                );
                world
                    .player_step_clear(
                        player,
                        inputs.locomotion_geometry.unwrap().ground,
                        cyrus_script::script::StepDirection::Forward,
                    )
                    .unwrap();
                assert_eq!(world.spatial(player).unwrap().local.position, position);
                assert_eq!(world.inspect(0, 1)["globals"]["lineup_result"], result);
                continue 'cases;
            }
            // The main thread runs the call once and resumes after the task completes.
            let stages: &[ScriptStage] = if round == 0 {
                &[ScriptStage::Body(cyrus_soup::Thread::Main)]
            } else {
                &[
                    ScriptStage::Tasks,
                    ScriptStage::Body(cyrus_soup::Thread::Main),
                ]
            };
            for &stage in stages {
                world
                    .advance(
                        map,
                        stage,
                        inputs,
                        Some(&mut |_, _| Ok(())),
                        Some(&mut random),
                        20,
                    )
                    .unwrap_or_else(|error| panic!("round {round}: {error:#}"));
            }
            if matches!(continuation, KillReceiver | KillCreatedReceiver) && round == 0 {
                assert_eq!(
                    world.actors()[map.index].runtime().unwrap().active_tasks(),
                    1
                );
                let position = world.spatial(player).unwrap().local.position;
                let result = world.inspect(0, 1)["globals"]["lineup_result"].clone();
                world
                    .advance(
                        map,
                        ScriptStage::Body(Thread::Interrupt),
                        inputs,
                        Some(&mut |_, _| Ok(())),
                        Some(&mut random),
                        20,
                    )
                    .unwrap();
                assert!(world.actors()[map.index].control_state().unwrap().killed);
                assert_eq!(
                    world.actors()[map.index].runtime().unwrap().active_tasks(),
                    0
                );
                if matches!(continuation, KillCreatedReceiver) {
                    for _ in 0..3 {
                        world
                            .update_created(inputs, &mut random, &mut |_, _| Ok(()), 20)
                            .unwrap();
                        assert!(world.actors()[map.index].runtime().is_none());
                        assert!(world.actors()[map.index].failure().is_none());
                        assert_eq!(world.spatial(player).unwrap().local.position, position);
                        assert_eq!(world.inspect(0, 1)["globals"]["lineup_result"], result);
                        assert_eq!(world.flags(), [12]);
                    }
                    continue 'cases;
                }
                // Killed map records remain addressable, but their cancelled task cannot move
                // the player or publish a successful lineup result on later task passes.
                assert_eq!(world.id(map.index).unwrap(), map);
                for _ in 0..3 {
                    world
                        .advance(
                            map,
                            ScriptStage::Tasks,
                            inputs,
                            Some(&mut |_, _| Ok(())),
                            Some(&mut random),
                            20,
                        )
                        .unwrap();
                    assert_eq!(world.spatial(player).unwrap().local.position, position);
                    assert_eq!(world.inspect(0, 1)["globals"]["lineup_result"], result);
                    assert_eq!(world.flags(), [12]);
                    assert!(world.actors()[map.index].failure().is_none());
                }
                continue 'cases;
            }
            if function == Native::PlayerLineUp && world.flags() != [7] {
                crossed_distance_boundary |= distance(&world) > lineup_distance.unwrap();
                let collision = cyrus_assets::collision::CollisionScene::default();
                let saved = world
                    .snapshot(&cyrus_script::script::WorldSnapshotSources {
                        identity_memo: None,
                        effects: None,
                        player: None,
                        scripts: &scripts,
                        animations: &data,
                        collision: &collision,
                    })
                    .unwrap();
                let saved = serde_json::to_value(saved).unwrap();
                let state = &saved["actors"][0]["tasks"]["tasks"][0]["state"]["LineUp"];
                assert_eq!(state["initialized"], true);
                assert_eq!(state["approach"], lineup_distance.unwrap() < 6656);
                assert_eq!(state["reference"], 0);
            }
            if matches!(continuation, MoveReference) && round == 0 {
                use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
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
                // One full forward step: with the `attr_step_type` 1 ramp, a smaller move would leave
                // the player between the arrival and movement thresholds, where alignment neither
                // snaps nor steps.
                let receiver = world.spatial(map).unwrap().local.position;
                saved["spatial"]["actors"][0]["local"]["position"] =
                    serde_json::json!([receiver[0], receiver[1], receiver[2] + 768]);
                let generation = world.generation() + 1;
                let globals = std::mem::take(world.globals_mut());
                world = ScriptWorld::from_snapshot(
                    serde_json::from_value(saved).unwrap(),
                    &sources,
                    WorldRestoreContext {
                        generation,
                        globals,
                        samples: &BTreeMap::new(),
                    },
                )
                .unwrap();
                player = world.id(index).unwrap();
                map = world.id(0).unwrap();
                actors = vec![player, map];
            }
            positions.push(world.spatial(player).unwrap().local.position);
            if world.flags() == [7] {
                break;
            }
        }
        assert_eq!(
            world.flags(),
            [7],
            "{function} {lineup_distance:?}: positions {positions:?}"
        );
        if lineup_distance == Some(6656) {
            assert!(
                crossed_distance_boundary,
                "equality mode must survive a later greater distance"
            );
        }
        let receiver = world.spatial(world.id(0).unwrap()).unwrap().local.position;
        let end = world.spatial(player).unwrap().local.position;
        if matches!(continuation, MoveReference) {
            assert_eq!([end[0], end[2]], [2560, 10496]);
            assert_eq!(
                world.actors()[map.index].runtime().unwrap().active_tasks(),
                0
            );
        }
        let target_z = if function == Native::PlayerLineUp {
            -z_offset
        } else {
            z_offset
        };
        assert_eq!([end[0], end[2]], [receiver[0], receiver[2] + target_z]);
    }
}

/// A receiver whose `PlayerFaceTalk(2048)` task runs against the walking-player fixture: the
/// player stands 6656 units away and the target is 2048 units in front of the receiver.
fn face_talk_fixture() -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    let (_, world, mut data, scene) = player_walking_fixture_with(|scripts| {
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::PlayerFaceTalk,
            param_count: 1,
            kind: CallKind::Task,
        });
        scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::ForwardWalk] = 3;
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
            wait(
                id,
                Native::PlayerFaceTalk,
                CallKind::Task,
                vec![Value::Integer { value: 2048 }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]
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
    });
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    (world, data, scene)
}

/// One scheduler round for the receiver: the player's collision and locomotion inputs, then the
/// receiver's task and main stages (the first round runs the call itself).
fn face_talk_round(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    round: usize,
) -> anyhow::Result<()> {
    scheduler_round(world, data, scene, round, false)
}

/// `face_talk_round`, optionally also running the selected player's own task stage (for a task
/// an object call left in the player's slots).
fn scheduler_round(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    round: usize,
    player_tasks: bool,
) -> anyhow::Result<()> {
    scheduler_round_with(world, data, scene, round, player_tasks, false)
}

/// `scheduler_round` with the caller's stage chosen: `ordinary` runs its interpreter entry (which
/// retains it as the shared source actor) instead of the body-only stage.
pub(super) fn scheduler_round_with(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    round: usize,
    player_tasks: bool,
    ordinary: bool,
) -> anyhow::Result<()> {
    scheduler_round_for(
        world,
        data,
        scene,
        (round, player_tasks, ordinary),
        Some(0),
        cyrus_script::script::ScriptStage::Tasks,
    )
}

/// One scheduler round whose caller is the given actor (`None`: only the player's tasks run).
/// `player_stage` is the stage the selected player's own update runs: `Tasks` polls its tasks only,
/// `OrdinaryMain` is the production wrapper that also ages its collision response.
fn scheduler_round_for(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    (round, player_tasks, ordinary): (usize, bool, bool),
    caller: Option<usize>,
    player_stage: cyrus_script::script::ScriptStage,
) -> anyhow::Result<()> {
    use cyrus_script::script::{
        LocomotionGeometry, ObstacleEnvironment, OrdinaryGroundQuery, PlayerCollisionSource,
        ScriptActorSource, ScriptInputs, ScriptStage,
    };
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    let map = world.id(caller.unwrap_or(0)).unwrap();
    let mut actors = vec![player, world.id(0).unwrap()];
    if map.index != 0 {
        actors.push(map);
    }
    let hud = cyrus_script::script::ScriptHud::default();
    let prepared = world
        .prepare_ordinary_collision_query(player, scene, data, [10, 30], &actors)
        .unwrap();
    let inputs = ScriptInputs {
        hud: Some(&hud),
        shell_execution: Some(false),
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        player_collision: Some(PlayerCollisionSource {
            scene,
            animations: data,
            view: [10, 30],
            actors: &actors,
        }),
        locomotion_geometry: Some(LocomotionGeometry {
            ground: OrdinaryGroundQuery {
                scene,
                animations: data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            environment: ObstacleEnvironment {
                selected_player: Some(player),
                auto_grab: false,
                item_system_ready: false,
                shell_execution: false,
                ticks: 1,
            },
        }),
        ..Default::default()
    };
    let mut random = cyrus_game::random::EngineRandom::default();
    let stages: &[ScriptStage] = if caller.is_none() {
        &[]
    } else if ordinary {
        &[ScriptStage::OrdinaryMain]
    } else if round == 0 {
        &[ScriptStage::Body(Thread::Main)]
    } else {
        &[ScriptStage::Tasks, ScriptStage::Body(Thread::Main)]
    };
    if caller.is_some_and(|caller| caller != 0) {
        // A created caller runs in the separate update, whose entry publishes the native source.
        world.update_created(inputs, &mut random, &mut |_, _| Ok(()), 20)?;
    }
    for &stage in stages
        .iter()
        .filter(|_| caller.is_none_or(|caller| caller == 0))
    {
        world.advance(
            map,
            stage,
            inputs,
            Some(&mut |_, _| Ok(())),
            Some(&mut random),
            20,
        )?;
    }
    if player_tasks {
        world.advance(
            player,
            player_stage,
            inputs,
            Some(&mut |_, _| Ok(())),
            Some(&mut random),
            20,
        )?;
    }
    Ok(())
}

#[test]
fn object_lineup_walks_the_receiver_to_the_callers_frame_and_ends_with_the_flag_cleared() {
    use cyrus_script::script::ScriptActorSource;
    // The map actor calls `Player.ObjectLineUp(0, 0, 2048)`: the receiver (the selected player)
    // walks to the point the offsets name from the caller and turns to the caller's heading plus a half turn.
    let (_, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::ObjectLineUp,
            param_count: 3,
            kind: CallKind::Task,
        });
        scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::ForwardWalk] = 3;
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
            Operation::ObjectCall {
                call: Call {
                    function_id: id,
                    function: Native::ObjectLineUp,
                    kind: CallKind::Task,
                    target: Object::Player,
                    // The offsets (rotated by the caller's zero heading) name the point 2048 units
                    // along +Z of the caller, ground the fixture covers (as for PlayerFaceTalk).
                    arguments: [0, 0, 2048].map(|value| Value::Integer { value }).to_vec(),
                },
            },
            set_flag(7),
            Operation::End { target: 0 },
        ]
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
    });
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    let mut finished = false;
    let mut trace = vec![];
    for round in 0..600 {
        scheduler_round_with(&mut world, &data, &scene, round, true, true)
            .unwrap_or_else(|error| panic!("round {round}: {error:#}"));
        // The object-targeted call returns at once: the caller continues while the player's own
        // slot runs the alignment.
        if round == 0 {
            assert_eq!(world.flags(), [7], "the caller continues at once");
        }
        let local = world.spatial(player).unwrap().local;
        trace.push((local.position, local.wrapped_angles()[1]));
        if world.actors()[index].runtime().unwrap().active_tasks() == 0 {
            finished = true;
            break;
        }
    }
    let caller_at = world.spatial(world.id(0).unwrap()).unwrap().local.position;
    assert!(
        finished,
        "the alignment completes; caller {caller_at:?}, trace {:?} .. {:?}",
        &trace[..3.min(trace.len())],
        &trace[trace.len().saturating_sub(3)..]
    );
    let caller = world.spatial(world.id(0).unwrap()).unwrap().local;
    let end = world.spatial(player).unwrap().local;
    assert_eq!(
        [end.position[0], end.position[2]],
        [caller.position[0], caller.position[2] + 2048]
    );
    assert_eq!(
        end.wrapped_angles()[1],
        (caller.wrapped_angles()[1] + 1024) & 2047,
        "the receiver ends facing the half-turn heading"
    );
    let control = world.actors()[index].control_state().unwrap();
    assert_eq!(
        control.script_control & 2,
        0,
        "the alignment flag is cleared"
    );
}

/// The walking-player fixture where the map actor calls `Player.ObjectPickupLineUp(angle, 0, 2048)`.
fn lineup_receiver_fixture(
    native: Native,
    angle: i32,
    caller_kills_itself: bool,
    forward_walk: u8,
    offset_z: i32,
) -> (
    Scripts,
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
    usize,
) {
    use cyrus_script::script::ScriptActorSource;
    let (scripts, world, mut data, scene) = player_walking_fixture_with(|scripts| {
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: native,
            param_count: 3,
            kind: CallKind::Task,
        });
        scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::ForwardWalk] = forward_walk;
        let kill_id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::KillMe,
            param_count: 0,
            kind: CallKind::Function,
        });
        let kill =
            caller_kills_itself.then(|| wait(kill_id, Native::KillMe, CallKind::Function, vec![]));
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
            Operation::ObjectCall {
                call: Call {
                    function_id: id,
                    function: native,
                    kind: CallKind::Task,
                    target: Object::Player,
                    // The unrotated offsets name the point 2048 units along +Z of the caller,
                    // ground the fixture covers (as for PlayerFaceTalk).
                    arguments: [angle, 0, offset_z]
                        .map(|value| Value::Integer { value })
                        .to_vec(),
                },
            },
            set_flag(7),
        ]
        .into_iter()
        .chain(kill)
        .chain([Operation::End { target: 0 }])
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
    });
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    (scripts, world, data, scene, index)
}

#[test]
fn object_pickup_lineup_arrives_completes_and_the_caller_continues() {
    // With an angle that puts the final heading on the player's own, the plain-mover fallback the
    // caller's height selects turns to it, arrives at the caller's X/Z plus the unrotated offsets
    // and the default animation completes the task in the player's own slot; the caller continued
    // at once.
    let (_, mut world, data, scene, index) =
        lineup_receiver_fixture(Native::ObjectPickupLineUp, 1024, false, 3, 2048);
    let player = world.id(index).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    let mut finished = false;
    for round in 0..600 {
        scheduler_round_with(&mut world, &data, &scene, round, true, true)
            .unwrap_or_else(|error| panic!("round {round}: {error:#}"));
        if round == 0 {
            assert_eq!(world.flags(), [7], "the caller continues at once");
            assert_eq!(
                world.actors()[index].runtime().unwrap().active_tasks(),
                1,
                "the receiver's own slot holds the task"
            );
        }
        if world.actors()[index].runtime().unwrap().active_tasks() == 0 {
            finished = true;
            break;
        }
    }
    let caller = world.spatial(world.id(0).unwrap()).unwrap().local.position;
    let end = world.spatial(player).unwrap().local.position;
    assert!(
        finished,
        "the task completes; player at {end:?}, caller {caller:?}"
    );
    assert_eq!([end[0], end[2]], [caller[0], caller[2] + 2048]);
}

#[test]
fn line_up_walks_the_receiver_to_the_retained_source_frame_and_the_caller_continues() {
    // LineUp is the close mover on the receiver: it turns to the final heading, steps toward the
    // rotated offset point of the source actor retained on the first invocation and completes
    // with the default animation, while the caller continued at once.
    let (_, mut world, data, scene, index) =
        lineup_receiver_fixture(Native::LineUp, 0, false, 3, 2048);
    let player = world.id(index).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    let mut finished = false;
    for round in 0..600 {
        scheduler_round_with(&mut world, &data, &scene, round, true, true)
            .unwrap_or_else(|error| panic!("round {round}: {error:#}"));
        if round == 0 {
            assert_eq!(world.flags(), [7], "the caller continues at once");
            assert_eq!(world.actors()[index].runtime().unwrap().active_tasks(), 1);
        }
        if world.actors()[index].runtime().unwrap().active_tasks() == 0 {
            finished = true;
            break;
        }
    }
    let caller = world.spatial(world.id(0).unwrap()).unwrap().local.position;
    let end = world.spatial(player).unwrap().local.position;
    assert!(
        finished,
        "the task completes; player at {end:?}, caller {caller:?}"
    );
    assert_eq!([end[0], end[2]], [caller[0], caller[2] + 2048]);
}

#[test]
fn object_parallel_lineup_arrives_completes_and_the_caller_continues() {
    // The plain mover on the receiver toward the caller's frame: angle 1024 puts the final
    // heading on the player's own, so it steps (no path test) and finishes at caller X/Z + offsets.
    let (_, mut world, data, scene, index) =
        lineup_receiver_fixture(Native::ObjectParallelLineUp, 1024, false, 3, 2048);
    let player = world.id(index).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    let mut finished = false;
    for round in 0..600 {
        scheduler_round_with(&mut world, &data, &scene, round, true, true)
            .unwrap_or_else(|error| panic!("round {round}: {error:#}"));
        if round == 0 {
            assert_eq!(world.flags(), [7], "the caller continues at once");
            assert_eq!(world.actors()[index].runtime().unwrap().active_tasks(), 1);
        }
        if world.actors()[index].runtime().unwrap().active_tasks() == 0 {
            finished = true;
            break;
        }
    }
    let caller = world.spatial(world.id(0).unwrap()).unwrap().local.position;
    let end = world.spatial(player).unwrap().local.position;
    assert!(
        finished,
        "the task completes; player at {end:?}, caller {caller:?}"
    );
    assert_eq!([end[0], end[2]], [caller[0], caller[2] + 2048]);
}

#[test]
fn a_departed_source_ends_the_receivers_lineup_with_an_explicit_error() {
    // A created caller starts the task on the player and is then killed and removed (the
    // original frees the record; a stale pointer is undefined there, so this build reports it).
    // LineUp retained the caller on its first invocation; the object natives read the shared
    // source, which still names the departed caller until another entry runs.
    for (native, angle) in [
        (Native::LineUp, 0),
        (Native::ObjectParallelLineUp, 1024),
        (Native::ObjectLineUp, 0),
    ] {
        let (_, mut world, data, scene, index) =
            lineup_receiver_fixture(native, angle, true, 3, 2048);
        let player = world.id(index).unwrap();
        world.begin_player_movement_update(player, 0).unwrap();
        let map = world.id(0).unwrap();
        let position = world.spatial(map).unwrap().local.position;
        let caller = world.create_object(map, "LOGIC", position, [0; 3]).unwrap();
        scheduler_round_for(
            &mut world,
            &data,
            &scene,
            (0, false, false),
            Some(caller.index),
            cyrus_script::script::ScriptStage::Tasks,
        )
        .unwrap();
        assert_eq!(world.actors()[index].runtime().unwrap().active_tasks(), 1);
        assert!(
            world.actors()[caller.index].runtime().is_none(),
            "the killed created caller is removed in its own update"
        );
        let tasks = world.actors()[index].runtime().unwrap().active_tasks();
        let error = scheduler_round_for(
            &mut world,
            &data,
            &scene,
            (4, true, false),
            None,
            cyrus_script::script::ScriptStage::Tasks,
        )
        .expect_err(&format!(
            "{native:?}: the departed source must end the task with an error (tasks {tasks})"
        ));
        assert!(
            format!("{error:#}").contains("retired created receiver"),
            "{native:?}: {error:#}"
        );
        let failure = world.actors()[index].failure().unwrap_or_default();
        assert!(
            failure.contains("retired created receiver"),
            "{native:?}: the player's runtime records the failure: {failure}"
        );
    }
}

/// Put a steep static ramp ahead of the lineup fixture's +Z walk, at the player's ground height
/// and covering Z 32 to 40 around X 10 in map units (the player stands at Z 30). The static
/// window never admits the map's first cell, so the placement sits in cell (1, 1) and its faces
/// reach back to the player's area with a bounds radius that admits it.
fn add_lineup_ramp(scene: &mut cyrus_script::player::data::PlayerScene, rise: f32) {
    use cyrus_assets::collision::{
        CollisionGeometry, CollisionModel, CollisionScene, CollisionSubobject, StaticPlacement,
    };
    use cyrus_collision::collision_world::StaticCollision;
    const ONE: i32 = 268435456;
    let ramp = CollisionModel {
        flags: 0,
        geometry: CollisionGeometry {
            version: Some(*b"v5.0"),
            radius: 1000.0,
            vertices: vec![
                [-610.0, rise, -488.0],
                [-410.0, rise, -488.0],
                [-410.0, rise, -480.0],
                [-610.0, rise, -480.0],
            ],
            // Normal Y of -100/256 classifies as steep ground.
            normals: vec![[0.0, -0.39, 0.92]],
            faces: vec![vec![0, 1, 2, 3]],
            textures: Some(vec![None]),
            subobjects: vec![CollisionSubobject {
                center: [-510.0, rise, -484.0],
                radius: 1000.0,
                extents: Some([1.0; 3]),
                faces: vec![0],
            }],
        },
    };
    scene.collision = CollisionScene {
        sphere_object_scale: Some(256),
        placements: vec![StaticPlacement {
            id: 1,
            model: "RAMP".into(),
            position: [520, -6, 520],
            rotation: [ONE, 0, 0, 0, ONE, 0, 0, 0, ONE],
        }],
        models: BTreeMap::from([("RAMP".into(), ramp)]),
        ..Default::default()
    };
    scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
}

#[test]
fn a_blocking_ramp_ends_the_lineup_with_return_state_six_without_moving_the_player() {
    // The steep ramp blocks the production forward step support query, so the player does not
    // move and the blocked-response marker is set. The plain mover keeps its step (the
    // destination stays straight ahead): nothing snaps the player and, once the production player
    // wrapper has aged the marker past eight updates, it abandons with return state six. The close
    // mover (`LineUp`) treats the mark as its fallback at once and abandons with state six on the
    // next poll. Neither moves the player onto the destination.
    // LineUp rotates its offsets by the source heading plus the angle, so the half-turn angle with
    // a negated offset puts the destination ahead of the player as for the parallel mover.
    for (native, angle, offset_z) in [
        (Native::ObjectParallelLineUp, 1024, 2048),
        (Native::LineUp, 1024, -2048),
    ] {
        let (_, mut world, data, mut scene, index) =
            lineup_receiver_fixture(native, angle, false, 3, offset_z);
        add_lineup_ramp(&mut scene, 0.0);
        let player = world.id(index).unwrap();
        world.begin_player_movement_update(player, 0).unwrap();
        let start = world.spatial(player).unwrap().local.position;
        let mut rounds = 0;
        while world.actors()[index].runtime().unwrap().active_tasks() != 0 || rounds == 0 {
            scheduler_round_for(
                &mut world,
                &data,
                &scene,
                (rounds, true, true),
                Some(0),
                cyrus_script::script::ScriptStage::OrdinaryMain,
            )
            .unwrap_or_else(|error| panic!("{native:?} round {rounds}: {error:#}"));
            rounds += 1;
            assert!(
                rounds < 400,
                "{native:?}: the blocked task must be abandoned"
            );
            if native == Native::ObjectParallelLineUp {
                assert_eq!(
                    world.spatial(player).unwrap().local.position,
                    start,
                    "{native:?}: the blocked player does not move"
                );
            }
        }
        if native == Native::ObjectParallelLineUp {
            assert!(
                rounds > 8,
                "{native:?}: abandoned only after the response aged ({rounds})"
            );
        }
        assert_eq!(
            serde_json::to_value(world.globals()).unwrap()["native_command_return"],
            6,
            "{native:?}: the abandon leaves return state six"
        );
    }
}

#[test]
fn a_lineup_step_off_the_supporting_ground_is_blocked_and_abandoned() {
    // The fixture's ground is a small triangle under the map actor. A destination past its apex
    // leads the walk off the ground: the slope check finds no surface at either position (and is
    // bypassed while the support-history counter runs), so the support query decides, finds
    // none and blocks the step. The player stops at the last supported position and the plain
    // mover abandons once the production wrapper has aged the response, with return state six.
    let (_, mut world, data, scene, index) =
        lineup_receiver_fixture(Native::ObjectParallelLineUp, 1024, false, 3, 6144);
    let player = world.id(index).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    let start = world.spatial(player).unwrap().local.position;
    let mut rounds = 0;
    while world.actors()[index].runtime().unwrap().active_tasks() != 0 || rounds == 0 {
        scheduler_round_for(
            &mut world,
            &data,
            &scene,
            (rounds, true, true),
            Some(0),
            cyrus_script::script::ScriptStage::OrdinaryMain,
        )
        .unwrap_or_else(|error| panic!("round {rounds}: {error:#}"));
        rounds += 1;
        assert!(rounds < 100, "the blocked task must be abandoned");
    }
    let end = world.spatial(player).unwrap().local.position;
    assert!(
        end[2] > start[2] && end[2] < start[2] + 6144,
        "the player stopped short of the destination: {start:?} to {end:?}"
    );
    assert_eq!(
        serde_json::to_value(world.globals()).unwrap()["native_command_return"],
        6
    );
}

#[test]
fn object_pickup_lineup_abandons_on_a_stale_collision_response_without_moving() {
    assert_stale_response_abandons(Native::ObjectPickupLineUp);
}

#[test]
fn object_parallel_lineup_abandons_on_a_stale_collision_response_without_moving() {
    assert_stale_response_abandons(Native::ObjectParallelLineUp);
}

fn assert_stale_response_abandons(native: Native) {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let (scripts, mut world, data, scene, index) =
        lineup_receiver_fixture(native, 1024, false, 3, 2048);
    let player = world.id(index).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    let start = world.spatial(player).unwrap().local.position;
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    // A nonzero collision-response mark older than eight updates: the plain mover abandons first.
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    saved["actors"][index]["control"]["body_response_flags"] = 1.into();
    saved["actors"][index]["control"]["collision_response_age"] = 9.into();
    let generation = world.generation() + 1;
    let mut world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation,
            globals: std::mem::take(world.globals_mut()),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let player = world.id(index).unwrap();
    for round in 0..6 {
        scheduler_round_with(&mut world, &data, &scene, round, true, true)
            .unwrap_or_else(|error| panic!("round {round}: {error:#}"));
    }
    assert_eq!(world.spatial(player).unwrap().local.position, start);
    assert_eq!(
        world.actors()[index].runtime().unwrap().active_tasks(),
        0,
        "the task completed by abandoning"
    );
    assert_eq!(
        serde_json::to_value(world.globals()).unwrap()["native_command_return"],
        6,
        "the abandon leaves return state six"
    );
}

#[test]
fn a_receivers_player_face_talk_walks_the_selected_player_in_front_of_it_and_faces_it() {
    use cyrus_script::script::ScriptActorSource;
    let (mut world, data, scene) = face_talk_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    let mut trace = vec![];
    // The player first turns to its final heading (a half turn, eight units a round), then walks.
    for round in 0..400 {
        face_talk_round(&mut world, &data, &scene, round)
            .unwrap_or_else(|error| panic!("round {round}: {error:#}"));
        let local = world.spatial(player).unwrap().local;
        trace.push((local.position, local.wrapped_angles()[1]));
        if world.flags() == [7] {
            break;
        }
    }
    assert_eq!(
        world.flags(),
        [7],
        "the task completes and the caller continues: {:?}",
        &trace[trace.len().saturating_sub(3)..]
    );
    let receiver = world.spatial(world.id(0).unwrap()).unwrap().local.position;
    let end = world.spatial(player).unwrap().local.position;
    // The target is the receiver's position plus (0, 0, 2048) through its identity orientation;
    // the player ends facing the receiver's heading turned a half turn, with the alignment
    // flag cleared.
    assert_eq!([end[0], end[2]], [receiver[0], receiver[2] + 2048]);
    assert_eq!(
        world.spatial(player).unwrap().local.wrapped_angles()[1],
        1024
    );
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .script_control
            & 2,
        0
    );
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn player_face_talk_reports_the_selected_player_diagnostic_without_continuing() {
    use cyrus_script::script::ScriptActorSource;
    let (mut world, data, scene) = face_talk_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    world.set_attribute_for_test(player, Attribute::Slave, 1);
    world.begin_player_movement_update(player, 0).unwrap();
    let position = world.spatial(player).unwrap().local.position;
    let error = face_talk_round(&mut world, &data, &scene, 0).unwrap_err();
    assert!(
        format!("{error:#}").contains("original diagnostic 137"),
        "{error:#}"
    );
    assert_eq!(
        world.flags(),
        [12],
        "the failed call withholds the continuation"
    );
    assert_eq!(world.spatial(player).unwrap().local.position, position);
}

#[test]
fn initialized_lineup_references_are_validated_before_session_replacement() {
    use cyrus_game::{
        preferences::{PreferenceKey, PreferenceStore, Preferences},
        random::EngineRandom,
    };
    use cyrus_script::{
        clock::SimulationClock,
        player::input::PlayerToggles,
        script::{
            LocomotionGeometry, LogicalSessionRestoreInputs, LogicalSessionSnapshot,
            ObstacleEnvironment, OrdinaryGroundQuery, PlayerCollisionSource, ScriptActors,
            ScriptHud, ScriptInputs, ScriptStage, WorldSnapshotSources,
        },
    };
    struct Store;
    impl PreferenceStore for Store {
        fn load(&self, _: PreferenceKey) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
        fn save(&self, _: PreferenceKey, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
        scripts.lineup_distance = Some(i32::MAX);
        let id = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: Native::PlayerLineUp,
            param_count: 3,
            kind: CallKind::Task,
        });
        scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::ForwardWalk] = 3;
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = wait(
            id,
            Native::PlayerLineUp,
            CallKind::Task,
            vec![
                Value::Integer { value: 1024 },
                Value::Integer { value: 0 },
                Value::Integer { value: -2048 },
            ],
        );
        let kill_id = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: Native::KillMe,
            param_count: 0,
            kind: CallKind::Function,
        });
        let mut created = scripts.actors["LOGIC"].clone();
        created.code_origin += 1000;
        created.header_index += 100;
        created.instances = 1;
        created.namespace = None;
        created.instance_variables = vec![vec![0]];
        created.program.entry = 3;
        created.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: wait(kill_id, Native::KillMe, CallKind::Function, vec![]),
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::Endint,
                },
            ),
            (
                3,
                Step {
                    next: 4,
                    operation: Operation::End { target: 3 },
                },
            ),
        ]);
        scripts.actors.insert("REFERENCE".into(), created);
    });
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let player = world.selected_player().unwrap();
    let receiver = world.id(0).unwrap();
    let actors = [player, receiver];
    let preferences = Preferences::new(Store);
    let asset = inventory_asset(&[]);
    let hud = ScriptHud::default();
    hud.configure(&asset, &preferences);
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &actors)
        .unwrap();
    let inputs = ScriptInputs {
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        hud: Some(&hud),
        shell_execution: Some(false),
        player_collision: Some(PlayerCollisionSource {
            scene: &scene,
            animations: &data,
            view: [10, 30],
            actors: &actors,
        }),
        locomotion_geometry: Some(LocomotionGeometry {
            ground: OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            environment: ObstacleEnvironment {
                selected_player: Some(player),
                auto_grab: false,
                item_system_ready: false,
                shell_execution: false,
                ticks: 1,
            },
        }),
        ..Default::default()
    };
    world.begin_player_movement_update(player, 0).unwrap();
    let mut random = EngineRandom::from_state(42);
    world
        .advance(
            receiver,
            ScriptStage::Body(Thread::Main),
            inputs,
            Some(&mut |_, _| Ok(())),
            Some(&mut random),
            20,
        )
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    let retired_reference = world
        .create_object(receiver, "REFERENCE", [0; 3], [0; 3])
        .unwrap();
    world
        .advance(
            retired_reference,
            ScriptStage::Body(Thread::Interrupt),
            inputs,
            Some(&mut |_, _| Ok(())),
            Some(&mut random),
            20,
        )
        .unwrap();
    world
        .update_created(inputs, &mut random, &mut |_, _| Ok(()), 20)
        .unwrap();
    let live_reference = world
        .create_object(receiver, "REFERENCE", [0; 3], [0; 3])
        .unwrap();
    let pending_reference = world
        .create_object(receiver, "REFERENCE", [0; 3], [0; 3])
        .unwrap();

    world
        .advance(
            pending_reference,
            ScriptStage::Body(Thread::Interrupt),
            inputs,
            Some(&mut |_, _| Ok(())),
            Some(&mut random),
            20,
        )
        .unwrap();
    assert!(world.actors()[live_reference.index].runtime().is_some());
    assert!(
        world.actors()[pending_reference.index]
            .control_state()
            .unwrap()
            .killed
    );
    assert!(world.actors()[pending_reference.index].runtime().is_some());
    assert!(world.actors()[retired_reference.index].runtime().is_none());
    assert!(world.spatial(retired_reference).is_err());
    let before = world.inspect(0, 64);
    let collision = Default::default();
    let sources = BTreeMap::from([(
        scripts.world,
        WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        },
    )]);
    let mut registry = ScriptActors::default();
    registry.current = Some(world);
    let saved = serde_json::to_value(
        registry
            .snapshot_session(
                &sources,
                &hud,
                &SimulationClock::new(scripts.clock).unwrap(),
                &random,
                PlayerToggles {
                    walk_mode: true,
                    auto_defend: false,
                },
            )
            .unwrap(),
    )
    .unwrap();
    let task = &saved["registry"]["worlds"][0]["actors"][0]["tasks"];
    assert_eq!(task["tasks"][0]["state"]["LineUp"]["initialized"], true);
    assert_eq!(task["tasks"][0]["state"]["LineUp"]["approach"], false);
    assert_eq!(task["tasks"][0]["state"]["LineUp"]["reference"], 0);
    let restore = |document| {
        serde_json::from_value::<LogicalSessionSnapshot>(document)
            .unwrap()
            .plan(LogicalSessionRestoreInputs {
                sources: &sources,
                last_generation: 100,
                hud: &asset,
                preferences: &preferences,
                bios_tick: 100,
            })
            .unwrap()
            .finish(&(0..16).map(|slot| (slot, slot)).collect())
    };
    for (reference, expected) in [
        (9999, "stale or absent saved owner"),
        (player.index, "lineup reference is the selected player"),
        (retired_reference.index, "retired created receiver"),
    ] {
        let mut invalid = saved.clone();
        invalid["registry"]["worlds"][0]["actors"][0]["tasks"]["tasks"][0]["state"]["LineUp"]["reference"] =
            reference.into();
        let error = restore(invalid)
            .err()
            .expect("invalid retained reference must reject before session replacement");
        let message = format!("{error:#}");
        assert!(
            message.contains(expected)
                && message.contains("actor 0 saved task references")
                && message.contains("native PlayerLineUp token"),
            "{message}"
        );
        assert_eq!(registry.current.as_ref().unwrap().inspect(0, 64), before);
    }
    let alternate = registry
        .current
        .as_ref()
        .unwrap()
        .actors()
        .iter()
        .position(|actor| {
            matches!(
                actor.source,
                cyrus_script::script::ScriptActorSource::AlternatePlayer
            )
        })
        .unwrap();
    let mut swapped = saved.clone();
    swapped["registry"]["globals"]["player_swapped"] = true.into();
    swapped["registry"]["worlds"][0]["actors"][0]["tasks"]["tasks"][0]["state"]["LineUp"]["reference"] =
        alternate.into();
    let error = restore(swapped)
        .err()
        .expect("swapped selected-player reference must reject");
    assert!(
        format!("{error:#}").contains("lineup reference is the selected player"),
        "{error:#}"
    );
    let mut nonselected = saved.clone();
    nonselected["registry"]["worlds"][0]["actors"][0]["tasks"]["tasks"][0]["state"]["LineUp"]["reference"] =
        alternate.into();
    assert!(
        restore(nonselected).is_ok(),
        "the unselected player role remains a valid reference"
    );
    for reference in [live_reference, pending_reference] {
        let mut valid = saved.clone();
        valid["registry"]["worlds"][0]["actors"][0]["tasks"]["tasks"][0]["state"]["LineUp"]["reference"] =
            reference.index.into();
        let restored =
            restore(valid).expect("allocated created references remain valid until cleanup");
        let world = restored.actors.current.unwrap();
        assert!(world.spatial(world.id(reference.index).unwrap()).is_ok());
        assert_eq!(registry.current.as_ref().unwrap().inspect(0, 64), before);
    }
    let restored = restore(saved.clone()).unwrap();
    let restored_world = restored.actors.current.unwrap();
    assert_eq!(restored_world.generation(), 101);
    let roundtrip =
        serde_json::to_value(restored_world.snapshot(&sources[&scripts.world]).unwrap()).unwrap();
    assert_eq!(&roundtrip["actors"][0]["tasks"], task);
}
