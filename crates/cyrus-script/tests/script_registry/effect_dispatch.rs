use super::*;

#[test]
fn add_flat_effect_retains_native_speed_and_scatters_by_the_spread() {
    use cyrus_assets::effects::{EffectImage, Effects};
    let mut effects = Effects {
        ambient_brightness: Some(128),
        capacity: 2,
        disabled: false,
        images: BTreeMap::from([(
            0x2380,
            EffectImage {
                name: "brazier".into(),
                size: [1, 4],
                texture_scale: 256,
                delay_ms: 71,
                flags: 1,
                alpha: 151,
                frames: vec![vec![255; 16]; 4],
            },
        )]),
    };
    effects.images.insert(0, effects.images[&0x2380].clone());
    let spawn = |spread: i32, speed: i32, descriptor: i32, seed: Option<u32>| {
        let mut world = {
            let mut scripts = random_scripts(Native::Rnd, vec![], Object::Me);
            scripts.definition.functions.push(Function {
                name: Native::AddFlatEffect,
                param_count: 6,
                kind: CallKind::Function,
            });
            let id = u16::try_from(scripts.definition.functions.len() - 1).unwrap();
            scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
                assign_call(
                    1,
                    id,
                    Native::AddFlatEffect,
                    &[2560, 5120, 7680, spread, descriptor, speed],
                )
                .operation,
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
            scripts.content_id = scripts.content_identity().unwrap();
            ScriptWorld::prepare(&scripts, 1).unwrap()
        };
        world.prepare_environment_effects(&effects).unwrap();
        let id = world.id(0).unwrap();
        let mut random = seed.map(cyrus_game::random::EngineRandom::from_state);
        world
            .advance(
                id,
                cyrus_script::script::ScriptStage::OrdinaryMain,
                cyrus_script::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                random.as_mut(),
                100,
            )
            .map(|()| world.actors()[0].failure().map(str::to_owned))
            .map(|failure| {
                let count = world
                    .environmental_effects()
                    .map_or(0, |effects| effects.active().count());
                let position = world
                    .environmental_effects()
                    .unwrap()
                    .active()
                    .next()
                    .map(|(_, effect)| effect.position);
                world.advance_environment_effects(1);
                let frame = world
                    .environmental_effects()
                    .unwrap()
                    .active()
                    .next()
                    .map(|(_, effect)| effect.frame);
                (failure, count, frame, position)
            })
    };
    for speed in [1, 2] {
        let (failure, count, frame, _) = spawn(0, speed, 0x2380, None).unwrap();
        assert!(failure.is_none(), "{failure:?}");
        assert_eq!(count, 1);
        assert_eq!(frame, Some(speed));
    }
    for descriptor in [0, 0x20000] {
        let (failure, count, frame, _) = spawn(0, 3, descriptor, None).unwrap();
        assert!(failure.is_none(), "{failure:?}");
        assert_eq!(count, 1);
        assert_eq!(frame, Some(0));
    }
    // A nonzero spread adds `rnd(spread >> 7) - (spread >> 8)` per axis in x, y, z order to the
    // point's whole map units and no longer lowers the point by half the image height.
    let (_, _, _, unspread) = spawn(0, 1, 0x2380, None).unwrap();
    assert_eq!(unspread.unwrap()[0], 10);
    let mut expected = cyrus_game::random::EngineRandom::from_state(7);
    let draws = [(); 3].map(|_| expected.rnd(8).unwrap());
    let (failure, count, _, scattered) = spawn(0x400, 1, 0x2380, Some(7)).unwrap();
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(count, 1);
    assert_eq!(
        scattered.unwrap(),
        [10 + draws[0] - 4, 20 + draws[1] - 4, 30 + draws[2] - 4]
    );
    // Without the shared random stream the spread stops the script; a spread below 128 gives a
    // bound the original's reduction never leaves.
    let error = spawn(0x400, 1, 0x2380, None).unwrap_err();
    assert!(format!("{error:#}").contains("random stream unavailable"));
    let error = spawn(5, 1, 0x2380, Some(7)).unwrap_err();
    assert!(format!("{error:#}").contains("nonpositive random bound"));
}

#[test]
fn created_effects_maintain_before_scripts_and_preserve_hidden_phases_after_restore() {
    use cyrus_script::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    for restore in [false, true] {
        for hidden in [false, true] {
            let mut calls: Vec<(Native, &[i32])> = vec![
                (Native::Light, &[256, 100]),
                (Native::LightFlicker, &[18, 1]),
                (Native::LightSize, &[8, 1]),
                (Native::FlatAnimate, &[129, 1, 1]),
            ];
            if hidden {
                calls.push((Native::HideMe, &[]));
            }
            let mut scripts = light_maintenance_scripts(PlacementType::LIGHT, 1, &calls);
            scripts.texture_frames.insert(1, BTreeMap::from([(1, 4)]));
            let (scripts, mut world, data) = light_maintenance_world(scripts);
            world.prepare_spatial().unwrap();
            let created = world
                .create_object(world.id(0).unwrap(), "LOGIC", [0; 3], [0; 3])
                .unwrap();
            let inputs = ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            };
            let mut random = cyrus_game::random::EngineRandom::from_state(1);
            world
                .update_created(inputs, &mut random, &mut |_, _| Ok(()), 100)
                .unwrap();
            let first_flat = world.actors()[created.index].flat_state().unwrap();
            let first_light = world.actors()[created.index].light_state().unwrap();
            assert_eq!(first_flat["frame"], 0);
            assert_eq!(random.state(), 1, "new effects wait for the next update");
            if restore {
                let collision = Default::default();
                let sources = WorldSnapshotSources {
                    identity_memo: None,
                    effects: None,
                    player: None,
                    scripts: &scripts,
                    animations: &data,
                    collision: &collision,
                };
                let saved = world.snapshot(&sources).unwrap();
                let globals = std::mem::take(world.globals_mut());
                world = ScriptWorld::from_snapshot(
                    saved,
                    &sources,
                    WorldRestoreContext {
                        generation: 2,
                        globals,
                        samples: &BTreeMap::new(),
                    },
                )
                .unwrap();
            }
            world
                .update_created(inputs, &mut random, &mut |_, _| Ok(()), 100)
                .unwrap();
            let actor = &world.actors()[created.index];
            assert!(actor.failure().is_none(), "{:?}", actor.failure());
            if hidden {
                assert_eq!(actor.flat_state().unwrap(), first_flat);
                assert_eq!(actor.light_state().unwrap(), first_light);
                assert_eq!(random.state(), 1);
            } else {
                assert_eq!(actor.flat_state().unwrap()["frame"], 1);
                assert_ne!(actor.light_state().unwrap(), first_light);
                assert_eq!(random.state(), 2524885223);
            }
        }
    }
}

fn animate_gxa_scripts(first: i32, cycle: i32, count: i32) -> Scripts {
    let mut scripts = random_scripts(Native::AnimateGXA, vec![], Object::Me);
    scripts.definition.functions[1].kind = CallKind::Task;
    scripts.definition.functions[1].param_count = 4;
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.strings = vec!["logic".into(), "system\\stmap01.gxa".into()];
    actor.program.steps.get_mut(&0).unwrap().operation = wait(
        1,
        Native::AnimateGXA,
        CallKind::Task,
        vec![
            Value::String { index: 1 },
            Value::Integer { value: first },
            Value::Integer { value: cycle },
            Value::Integer { value: count },
        ],
    );
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn animate_gxa_keeps_its_loaded_image_and_progress_in_the_task_slot_across_a_restore() {
    use cyrus_script::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    let scripts = animate_gxa_scripts(2, 5, 100);
    let (_, _, animations) = revisit_test_app(&scripts);
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    // Each step: the second slot's offset is the progress before the update; the progress then
    // gains the ticks and wraps by the cycle of 5 while above it.
    for (step, (stage, ticks, offset)) in [
        (ScriptStage::OrdinaryMain, 3, 1),
        (ScriptStage::Tasks, 3, 4),
        (ScriptStage::Tasks, 1, 2),
        (ScriptStage::Tasks, 4, 3),
        (ScriptStage::Tasks, 1, 2),
    ]
    .into_iter()
    .enumerate()
    {
        let inputs = |nominal_ticks| ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(nominal_ticks),
            ..Default::default()
        };
        // A restored world has no retained native source until an interpreter entry runs, as
        // in the original, so the frame's ordinary entry precedes the task stage.
        if step > 0 {
            world
                .advance(
                    world.id(0).unwrap(),
                    ScriptStage::OrdinaryMain,
                    inputs(0),
                    None,
                    None,
                    20,
                )
                .unwrap();
        }
        world
            .advance(world.id(0).unwrap(), stage, inputs(ticks), None, None, 20)
            .unwrap();
        let overlay = world.gxa_overlay().unwrap();
        assert_eq!(overlay.name, "system\\stmap01.gxa");
        assert_eq!((overlay.frame, overlay.second_offset), (2, Some(offset)));
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        // The drawn slots are cleared by the main loop each frame, so they are not saved; the
        // task slot's words are.
        let snapshot = world.snapshot(&sources).unwrap();
        world = ScriptWorld::from_snapshot(
            snapshot,
            &sources,
            WorldRestoreContext {
                generation: world.generation() + 1,
                globals: Default::default(),
                samples: &Default::default(),
            },
        )
        .unwrap();
        assert!(world.gxa_overlay().is_none());
    }
}
