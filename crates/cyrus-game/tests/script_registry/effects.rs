use super::*;
use cyrus_assets::effects::{EffectImage, Effects};
use cyrus_game::{
    clock::ClockStep,
    random::EngineRandom,
    script::{
        ActorEvent, ScriptActorSource, ScriptHud, ScriptInputs, ScriptSound, ScriptStage,
        WorldSnapshotSources, camera::CameraPose,
    },
};

fn source(capacity: u32, disabled: bool) -> Effects {
    Effects {
        ambient_brightness: Some(128),
        capacity,
        disabled,
        images: BTreeMap::from([(
            0x2500,
            EffectImage {
                name: "synthetic-death".into(),
                size: [1, 4],
                texture_scale: 256,
                delay_ms: 71,
                flags: 1,
                alpha: 151,
                frames: vec![vec![255; 16]; 4],
            },
        )]),
    }
}

#[test]
fn routed_fatal_health_emits_retained_effects_and_roundtrips_live_slots() {
    for (capacity, disabled) in [(2, false), (1, false), (0, false), (2, true)] {
        let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
            for (index, name, value) in [(3, "health", 40), (105, "skip_death", 1)] {
                scripts.definition.attributes[index] = name.into();
                scripts
                    .actors
                    .get_mut("CYRUS")
                    .unwrap()
                    .attributes
                    .insert(name.into(), value);
            }
            let function_id = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: "SetHealth".into(),
                param_count: 1,
                kind: CallKind::Function,
            });
            scripts
                .actors
                .get_mut("LOGIC")
                .unwrap()
                .program
                .steps
                .get_mut(&0)
                .unwrap()
                .operation = Operation::ObjectCall {
                call: Call {
                    function_id,
                    function: "SetHealth".into(),
                    kind: CallKind::Function,
                    target: Object::Player,
                    arguments: vec![Value::Integer { value: 0 }],
                },
            };
        });
        data.metadata["markers"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        let player = world
            .id(world
                .actors()
                .iter()
                .position(|actor| matches!(actor.source, ScriptActorSource::Player))
                .unwrap())
            .unwrap();
        world
            .advance(
                player,
                ScriptStage::Entry,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap();
        world.set_actor_surface(player, 2).unwrap();
        world
            .initialize_camera_pose(CameraPose {
                position: [0; 3],
                angles: [0; 3],
            })
            .unwrap();
        let hud = ScriptHud::default();
        let mut random = EngineRandom::from_state(42);
        world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    hud: Some(&hud),
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    death_suppressed: Some(false),
                    ..Default::default()
                },
                None,
                Some(&mut random),
                100,
            )
            .unwrap();
        assert_eq!(
            world
                .globals()
                .selected_health()
                .unwrap()
                .death()
                .unwrap()
                .mode(),
            2
        );
        let prepared = world
            .prepare_ordinary_collision_query(
                player,
                &scene,
                &data,
                [10, 30],
                &[player, world.id(0).unwrap()],
            )
            .unwrap();
        let effects = source(capacity, disabled);
        world.prepare_environment_effects(&effects).unwrap();
        let mut sounds = Vec::new();
        for _ in 0..8 {
            world
                .advance_player_controller(
                    player,
                    cyrus_game::script::OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 4,
                        sidedness: false,
                    },
                    ClockStep {
                        nominal_ticks: 0,
                        frame_scale: 256,
                        fine_ticks: 0,
                    },
                    ScriptInputs {
                        hud: Some(&hud),
                        ..Default::default()
                    },
                    &mut random,
                    &mut |id, sound| {
                        assert_eq!(id, player);
                        sounds.push(sound);
                        Ok(())
                    },
                )
                .unwrap();
        }
        let active = world
            .environmental_effects()
            .unwrap()
            .active()
            .map(|(slot, effect)| (slot, effect.descriptor, effect.position, effect.frame))
            .collect::<Vec<_>>();
        if disabled || capacity == 0 {
            assert!(active.is_empty());
        } else {
            assert_eq!(active.len(), capacity as usize);
            assert_eq!(
                active,
                [(0, 0x2500, [8, 22, 23], 0), (1, 0x2500, [5, 22, 24], 0)][..capacity as usize]
            );
            let before = active.clone();
            world.prepare_environment_effects(&effects).unwrap();
            assert_eq!(
                world
                    .environmental_effects()
                    .unwrap()
                    .active()
                    .map(|(slot, effect)| (slot, effect.descriptor, effect.position, effect.frame))
                    .collect::<Vec<_>>(),
                before
            );
            let mut changed = effects.clone();
            changed.images.get_mut(&0x2500).unwrap().alpha = 152;
            assert!(world.prepare_environment_effects(&changed).is_err());
            let sources = WorldSnapshotSources {
                effects: Some(&effects),
                player: None,
                scripts: &scripts,
                animations: &data,
                collision: &scene.collision,
            };
            let missing = WorldSnapshotSources {
                effects: None,
                ..sources
            };
            let error = world.snapshot(&missing).err().unwrap();
            assert!(
                format!("{error:#}").contains("world effect snapshot source unavailable"),
                "{error:#}"
            );
            let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
            let samples = BTreeMap::new();
            let context = || cyrus_game::script::WorldRestoreContext {
                generation: world.generation() + 1,
                globals: cyrus_game::script::GameplayGlobals::default(),
                samples: &samples,
            };
            for field in ["source", "slots", "descriptor", "frame", "active"] {
                let mut corrupt = saved.clone();
                match field {
                    "source" => corrupt["effects"]["source"] = "wrong-source".into(),
                    "slots" => {
                        corrupt["effects"]["slots"]
                            .as_array_mut()
                            .unwrap()
                            .push(serde_json::Value::Null);
                    }
                    "descriptor" => corrupt["effects"]["slots"][0]["descriptor"] = 1.into(),
                    "frame" => corrupt["effects"]["slots"][0]["frame"] = 4.into(),
                    "active" => corrupt["effects"]["slots"][0]["active"] = false.into(),
                    _ => unreachable!(),
                }
                assert!(
                    ScriptWorld::from_snapshot(
                        serde_json::from_value(corrupt).unwrap(),
                        &sources,
                        context()
                    )
                    .is_err(),
                    "accepted corrupt {field}"
                );
                assert_eq!(
                    serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap(),
                    saved
                );
            }
            assert!(
                ScriptWorld::from_snapshot(
                    serde_json::from_value(saved.clone()).unwrap(),
                    &missing,
                    context()
                )
                .is_err()
            );
            let changed_sources = WorldSnapshotSources {
                effects: Some(&changed),
                ..sources
            };
            assert!(
                ScriptWorld::from_snapshot(
                    serde_json::from_value(saved.clone()).unwrap(),
                    &changed_sources,
                    context()
                )
                .is_err()
            );
            let mut restored = ScriptWorld::from_snapshot(
                serde_json::from_value(saved.clone()).unwrap(),
                &sources,
                context(),
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(restored.snapshot(&sources).unwrap()).unwrap()["effects"],
                saved["effects"]
            );
            if capacity == 2 {
                let mut draw_saved = saved.clone();
                draw_saved["effects"]["slots"][0]["descriptor"] = 0x22500.into();
                draw_saved["effects"]["slots"][0]["frame"] = 3.into();
                let mut drawing = ScriptWorld::from_snapshot(
                    serde_json::from_value(draw_saved).unwrap(),
                    &sources,
                    context(),
                )
                .unwrap();
                drawing.advance_environment_effects(1);
                assert_eq!(
                    drawing
                        .draw_environment_effects(7)
                        .map(|(slot, effect)| (slot, effect.position))
                        .collect::<Vec<_>>(),
                    [(1, [5, 22, 24])]
                );
                let captured = serde_json::to_value(drawing.snapshot(&sources).unwrap()).unwrap();
                assert_eq!(
                    captured["effects"]["slots"][0]["position"],
                    serde_json::json!([8, 13, 23])
                );
                let mut drawing = ScriptWorld::from_snapshot(
                    serde_json::from_value(captured).unwrap(),
                    &sources,
                    cyrus_game::script::WorldRestoreContext {
                        generation: drawing.generation() + 1,
                        ..context()
                    },
                )
                .unwrap();
                assert_eq!(drawing.draw_environment_effects(0).count(), 1);
                let captured = serde_json::to_value(drawing.snapshot(&sources).unwrap()).unwrap();
                assert_eq!(
                    captured["effects"]["slots"][0]["position"],
                    serde_json::json!([8, 11, 23])
                );
                drawing.advance_environment_effects(3);
                assert_eq!(drawing.draw_environment_effects(7).count(), 0);
                assert_eq!(
                    serde_json::to_value(drawing.snapshot(&sources).unwrap()).unwrap()["effects"]["slots"]
                        [0]["position"],
                    serde_json::json!([8, 11, 23])
                );
            }
            restored.advance_environment_effects(3);
            assert!(
                restored
                    .environmental_effects()
                    .unwrap()
                    .active()
                    .all(|(_, effect)| effect.frame == 3)
            );
            restored.advance_environment_effects(1);
            assert!(restored.environmental_effects().unwrap().is_empty());
            world.advance_environment_effects(3);
            assert!(
                world
                    .environmental_effects()
                    .unwrap()
                    .active()
                    .all(|(_, effect)| effect.frame == 3)
            );
            world.advance_environment_effects(1);
            assert!(world.environmental_effects().unwrap().is_empty());
        }
        assert_eq!(random.state(), 2681529084);
        assert_eq!(sounds.len(), 2);
        assert!(matches!(
            sounds[0],
            ScriptSound::Positional {
                rate_offset_hz: -28,
                ..
            }
        ));
        assert!(matches!(
            sounds[1],
            ScriptSound::Positional {
                rate_offset_hz: -12,
                ..
            }
        ));
        assert!(sounds.iter().all(|sound| matches!(
            sound,
            ScriptSound::Positional {
                effect: 71,
                volume: 0,
                position: [2560, -1536, 7680],
                ..
            }
        )));
    }
}

#[test]
fn actor_events_spawn_surface_effects_in_bit_order_and_shake_the_camera() {
    let (_scripts, mut world, data, scene) = player_walking_fixture_with(|_| {});
    let player = world
        .id(world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap())
        .unwrap();
    world
        .advance(
            player,
            ScriptStage::Entry,
            ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    world
        .initialize_camera_pose(CameraPose {
            position: [0; 3],
            angles: [0; 3],
        })
        .unwrap();
    world
        .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player])
        .unwrap();
    let mut effects = source(8, false);
    let image = effects.images[&0x2500].clone();
    for descriptor in [0x2580, 0x2700] {
        effects.images.insert(descriptor, image.clone());
    }
    world.prepare_environment_effects(&effects).unwrap();
    let event = |flags, surface| ActorEvent {
        actor: player,
        flags,
        surface,
        position: Some([2560, 0, 5120]),
    };
    // A stored surface that no set bit names spawns nothing, and the shake bit needs no surface.
    world
        .apply_actor_events(&[event(0x0f, Some(0)), event(0x10, None)])
        .unwrap();
    assert!(world.environmental_effects().unwrap().is_empty());
    assert_eq!(world.camera().unwrap().shake, 32);
    // Water takes both of its bits, lava and sand one each, in emission order.
    world
        .apply_actor_events(&[event(6, Some(1)), event(1, Some(5)), event(8, Some(6))])
        .unwrap();
    let active = world
        .environmental_effects()
        .unwrap()
        .active()
        .map(|(_, effect)| (effect.descriptor, effect.position[1]))
        .collect::<Vec<_>>();
    // The vertical point is the actor's extent plus 1536 Q8, less half the image height (512).
    let height = world
        .ordinary_collision_shape(player)
        .unwrap()
        .positive_extent[1];
    let y = height + 4;
    assert_eq!(active, [(0x2500, y), (0x2500, y), (0x2580, y), (0x2700, y)]);
    // A surface bit without a known stored surface fails instead of skipping.
    assert!(world.apply_actor_events(&[event(1, None)]).is_err());
}

#[test]
fn animation_stage_commits_actor_events_before_a_later_sound_failure() {
    use cyrus_world::animation::AnimationSound;
    let (mut world, mut data, models) = static_model_fixture({
        let mut scripts = cache();
        add_standalone_receivers(&mut scripts);
        scripts
    });
    let actor = world.id(0).unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = serde_json::json!("animation_table");
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":0},
            {"opcode":11,"value":22},
            {"opcode":4,"sound_param":9,"volume_shift":0},
            {"opcode":0,"frame_reference":1},{"opcode":1}
        ]
    }]);
    world
        .initialize_map_animation(actor, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.set_actor_surface(actor, 1).unwrap();
    world
        .initialize_camera_pose(CameraPose {
            position: [0; 3],
            angles: [0; 3],
        })
        .unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    world
        .prepare_ordinary_collision_query(actor, &scene, &data, [10, 30], &[actor])
        .unwrap();
    world
        .prepare_environment_effects(&source(4, false))
        .unwrap();
    world
        .advance(
            actor,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| panic!("initial frame selected before event commands")),
            None,
            100,
        )
        .unwrap();
    let mut emitted = Vec::new();
    let error = world
        .advance(
            actor,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |receiver, sound| {
                assert_eq!(receiver, actor);
                emitted.push(sound);
                anyhow::bail!("later sound failure witness")
            }),
            None,
            100,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("later sound failure witness"));
    assert!(matches!(
        emitted.as_slice(),
        [ScriptSound::Animation {
            sound: AnimationSound::Play { effect: 9, .. },
            ..
        }]
    ));
    assert_eq!(world.camera().unwrap().shake, 32);
    let active = world
        .environmental_effects()
        .unwrap()
        .active()
        .map(|(_, effect)| effect.descriptor)
        .collect::<Vec<_>>();
    assert_eq!(active, [0x2500, 0x2500]);
    assert!(world.actors()[actor.index].failure().is_some());
}

#[test]
fn map_animation_initialization_keeps_a_shake_before_a_sound_failure() {
    use cyrus_world::animation::AnimationSound;
    let (mut world, mut data, _) = static_model_fixture({
        let mut scripts = cache();
        add_standalone_receivers(&mut scripts);
        scripts
    });
    let actor = world.id(0).unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = serde_json::json!("animation_table");
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":11,"value":16},
            {"opcode":4,"sound_param":9,"volume_shift":0},
            {"opcode":0,"frame_reference":0},{"opcode":1}
        ]
    }]);
    world
        .initialize_camera_pose(CameraPose {
            position: [0; 3],
            angles: [0; 3],
        })
        .unwrap();
    let mut emitted = Vec::new();
    let error = world
        .initialize_map_animation(actor, &data, 0, &mut |_, sound| {
            emitted.push(sound);
            anyhow::bail!("initialization sound failure witness")
        })
        .unwrap_err();
    assert!(format!("{error:#}").contains("initialization sound failure witness"));
    assert!(matches!(
        emitted.as_slice(),
        [ScriptSound::Animation {
            sound: AnimationSound::Play { effect: 9, .. },
            ..
        }]
    ));
    assert_eq!(world.camera().unwrap().shake, 32);
    assert!(world.actors()[actor.index].failure().is_some());
}

#[test]
fn animation_events_without_matching_bits_need_no_external_sound_or_effect_services() {
    for flags in [0, 0x10000, 15] {
        let (mut world, mut data, _) = static_model_fixture({
            let mut scripts = cache();
            add_standalone_receivers(&mut scripts);
            scripts
        });
        let actor = world.id(0).unwrap();
        let definition = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "LOGIC")
            .unwrap();
        definition["model_load_path"] = serde_json::json!("animation_table");
        definition["animation_groups"] = serde_json::json!([{
            "group_index":0,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":11,"value":flags},
                {"opcode":0,"frame_reference":0},
                {"opcode":11,"value":flags},
                {"opcode":0,"frame_reference":1},{"opcode":1}
            ]
        }]);
        world.set_actor_surface(actor, 0).unwrap();
        world
            .initialize_map_animation(actor, &data, 0, &mut |_, _| {
                panic!("actor event reached audio adapter")
            })
            .unwrap();
        world
            .advance(
                actor,
                ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap();
        world
            .advance(
                actor,
                ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap();
        assert_eq!(world.animation_frame(actor).unwrap(), Some(1));
    }
}

#[test]
fn automatic_model_phase_keeps_initialization_events_when_later_admission_fails() {
    use bevy::prelude::*;
    use cyrus_assets::{
        animation::Animations,
        collision::{ModelVisibility, VisibilityNode},
    };
    use cyrus_game::script::{ScriptActors, ScriptStatus, ScriptView};
    use cyrus_world::animation::AnimationRequest;
    let mut scripts = cache();
    scripts.instances.truncate(1);
    add_standalone_receivers(&mut scripts);
    for placement in &mut scripts.instances {
        placement.object_type = 1;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, mut data, _) = static_model_fixture(scripts.clone());
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = serde_json::json!("animation_table");
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":11,"value":16},{"opcode":0,"frame_reference":0},{"opcode":1}
        ]
    }]);
    world
        .initialize_camera_pose(CameraPose {
            position: [0; 3],
            angles: [0; 3],
        })
        .unwrap();
    world
        .prepare_model_admission(
            &ModelVisibility {
                center: [0; 3],
                half_extent: 100,
                nodes: vec![VisibilityNode {
                    children: [None; 8],
                    objects: vec![60000],
                }],
            },
            1,
        )
        .unwrap();
    let (mut app, prepared, _) = revisit_test_app(&scripts);
    app.add_message::<cyrus_world::animation::AnimationTick>()
        .add_message::<cyrus_world::sound::SoundCue>();
    let source = prepared.animations.clone();
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .insert(source.id(), data)
        .unwrap();
    app.insert_resource(prepared)
        .insert_resource(AnimationRequest(source.clone()))
        .insert_resource(ScriptView {
            source,
            position: [0; 3],
        });
    app.update();
    app.world_mut().resource_mut::<ScriptActors>().current = Some(world);
    app.insert_resource(State::new(cyrus_game::AppState::InGame));
    app.update();
    app.update();
    let status = format!("{:?}", app.world().resource::<ScriptStatus>());
    assert!(
        status.contains("PVO selected object outside prepared tables"),
        "{status}"
    );
    let actors = app.world().resource::<ScriptActors>();
    let world = actors.current.as_ref().unwrap();
    assert_eq!(
        world.camera().unwrap().shake,
        32,
        "the later model-stage error must retain the earlier authored event"
    );
}

#[test]
fn selected_death_animation_events_allocate_before_the_death_controller_effect() {
    let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
        for (index, name, value) in [(3, "health", 40), (105, "skip_death", 1)] {
            scripts.definition.attributes[index] = name.into();
            scripts
                .actors
                .get_mut("CYRUS")
                .unwrap()
                .attributes
                .insert(name.into(), value);
        }
        let function_id = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: "SetHealth".into(),
            param_count: 1,
            kind: CallKind::Function,
        });
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = Operation::ObjectCall {
            call: Call {
                function_id,
                function: "SetHealth".into(),
                kind: CallKind::Function,
                target: Object::Player,
                arguments: vec![Value::Integer { value: 0 }],
            },
        };
    });
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "CYRUS")
        .unwrap()["animation_groups"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "group_index":134,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":11,"value":1},{"opcode":0,"frame_reference":0},{"opcode":1}
            ]
        }));
    world = replace_health_test_animation(world, &scripts, &mut data, index, 0, |_| {});
    let player = world.id(index).unwrap();
    let timing = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(player, ScriptStage::Entry, timing, None, None, 100)
        .unwrap();
    world.set_actor_surface(player, 2).unwrap();
    world
        .initialize_camera_pose(CameraPose {
            position: [0; 3],
            angles: [0; 3],
        })
        .unwrap();
    let hud = ScriptHud::default();
    let mut random = EngineRandom::from_state(42);
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                hud: Some(&hud),
                death_suppressed: Some(false),
                ..timing
            },
            None,
            Some(&mut random),
            100,
        )
        .unwrap();
    assert_eq!(
        world
            .globals()
            .selected_health()
            .unwrap()
            .death()
            .unwrap()
            .mode(),
        2
    );
    // The retained death mode chooses its own effect; the current surface gates the
    // authored animation event independently.
    world.set_actor_surface(player, 5).unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player])
        .unwrap();
    let mut effects = source(1, false);
    effects
        .images
        .insert(0x2580, effects.images[&0x2500].clone());
    world.prepare_environment_effects(&effects).unwrap();
    let mut sounds = Vec::new();
    for _ in 0..8 {
        world
            .advance_player_controller(
                player,
                cyrus_game::script::OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                ClockStep {
                    nominal_ticks: 0,
                    frame_scale: 256,
                    fine_ticks: 0,
                },
                ScriptInputs {
                    hud: Some(&hud),
                    ..Default::default()
                },
                &mut random,
                &mut |_, sound| {
                    sounds.push(sound);
                    Ok(())
                },
            )
            .unwrap();
    }
    assert!(
        sounds
            .iter()
            .any(|sound| matches!(sound, ScriptSound::Positional { effect: 71, .. })),
        "death effect admission was not exercised"
    );
    let slots = world
        .environmental_effects()
        .unwrap()
        .active()
        .map(|(slot, effect)| (slot, effect.descriptor))
        .collect::<Vec<_>>();
    assert_eq!(
        slots,
        [(0, 0x2580)],
        "the authored event must allocate before the later death effect exhausts the shared pool"
    );
    assert!(!sounds.iter().any(|sound| matches!(
        sound,
        ScriptSound::Animation {
            sound: cyrus_world::animation::AnimationSound::Event { .. },
            ..
        }
    )));
}

#[test]
fn initialization_events_allocate_before_water_model_effects_in_the_shared_pool() {
    for capacity in [1, 2] {
        let (scripts, world, mut data, scene) = player_walking_fixture();
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "CYRUS")
            .unwrap()["animation_groups"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "group_index":140,"anim_id":1,"anim_type":0,"commands":[
                    {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}
                ]
            }));
        let mut world =
            replace_health_test_animation(world, &scripts, &mut data, index, 140, |saved| {
                saved["actors"][index]["model"]["current"] = serde_json::Value::Null;
                saved["actors"][index]["model"]["position"] =
                    serde_json::json!([10.0_f32.to_bits(), 75.0_f32.to_bits(), 30.0_f32.to_bits()]);
            });
        let player = world.id(index).unwrap();
        world
            .prepare_ordinary_collision_shape(player, &data, &scene)
            .unwrap();
        world
            .advance(
                player,
                ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap();
        world
            .prepare_model_admission(&Default::default(), scene.collision.placements.len())
            .unwrap();
        world.set_actor_surface(player, 2).unwrap();
        world
            .initialize_camera_pose(CameraPose {
                position: [0; 3],
                angles: [0; 3],
            })
            .unwrap();
        let mut effects = source(capacity, false);
        effects
            .images
            .insert(0x2580, effects.images[&0x2500].clone());
        world.prepare_environment_effects(&effects).unwrap();
        let actor = world.id(0).unwrap();
        world
            .prepare_ordinary_collision_shape(actor, &data, &scene)
            .unwrap();
        world.set_actor_surface(actor, 5).unwrap();
        let definition = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "LOGIC")
            .unwrap();
        definition["model_load_path"] = serde_json::json!("animation_table");
        definition["raan_entries"] = serde_json::json!([{"file_path":"PLAYER.3D","frame_count":1}]);
        definition["animation_groups"] = serde_json::json!([{
            "group_index":0,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":11,"value":1},{"opcode":0,"frame_reference":0},{"opcode":1}
            ]
        }]);
        world
            .initialize_map_animation(actor, &data, 0, &mut |_, _| {
                panic!("actor event reached audio adapter")
            })
            .unwrap();
        world.set_actor_surface(actor, 0).unwrap();
        let hud = ScriptHud::default();
        world
            .refresh_automatic_models(
                &data,
                [0; 3],
                Some(&scene),
                ScriptInputs {
                    hud: Some(&hud),
                    ..Default::default()
                },
                Some(&mut EngineRandom::from_state(42)),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert!(
            world.actors()[index].failure().is_none(),
            "{:?}",
            world.actors()[index].failure()
        );
        assert_eq!(
            world.model_position(player).unwrap(),
            Some([10.0, 58.0, 30.0])
        );
        let slots = world
            .environmental_effects()
            .unwrap()
            .active()
            .map(|(slot, effect)| (slot, effect.descriptor))
            .collect::<Vec<_>>();
        assert_eq!(slots, [(0, 0x2580), (1, 0x2500)][..capacity as usize]);
    }
}
