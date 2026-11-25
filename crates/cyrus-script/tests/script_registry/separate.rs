use super::*;

#[test]
fn is_shell_script_returns_the_execution_context_only_while_a_shell_exists() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let run = |spawn: bool, context: Option<bool>| {
        let mut scripts = random_scripts(Native::IsShellScript, vec![], Object::Me);
        scripts.definition.functions[1].param_count = 0;
        let mut shell = scripts.actors["LOGIC"].clone();
        shell.code_origin += 1000;
        shell.header_index += 100;
        scripts.actors.insert("LATER".into(), shell);
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        if spawn {
            world.spawn_shell("LATER").unwrap();
        }
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    shell_execution: context,
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .map(|_| world.actors()[0].runtime().unwrap().variables()[0])
    };
    assert_eq!(run(false, Some(true)).unwrap(), 0, "no shell in the list");
    assert_eq!(run(true, Some(false)).unwrap(), 0, "not in shell context");
    assert_eq!(run(true, Some(true)).unwrap(), 1);
    let error = run(true, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("IsShellScript: shell context unavailable"),
        "{error:#}"
    );
}

#[test]
fn separate_updates_retain_locals_and_tasks_without_ordinary_control_or_attributes() {
    use cyrus_script::script::ScriptInputs;
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::SCRIPT_ONLY;
        instance.map_position = [600, 20, 600];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    for (first, second) in [(5, 10), (6, 11)] {
        world
            .update_separate(
                [600, 600],
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [first]);
        assert_eq!(world.actors()[1].runtime().unwrap().variables(), [second]);
        for actor in world.actors() {
            assert!(actor.control_state().is_none());
            assert!(actor.attributes().bytes().iter().all(|value| *value == 0));
            assert!(actor.failure().is_none());
        }
    }
    let mut scripts = wait_cache(vec![
        wait(
            1,
            Native::Wait,
            CallKind::Task,
            vec![Value::Integer { value: 100 }],
        ),
        Operation::End { target: 0 },
    ]);
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::SCRIPT_ONLY;
        instance.map_position = [600, 20, 600];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    for _ in 0..3 {
        world
            .update_separate(
                [600, 600],
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        for actor in world.actors() {
            assert_eq!(actor.runtime().unwrap().active_tasks(), 1);
        }
    }
}

#[test]
fn separate_kill_me_releases_the_actor_runtime_and_owned_sounds() {
    use cyrus_script::script::{ScriptActorSource, ScriptInputs, ScriptSound};
    let mut scripts = random_scripts(Native::KillMe, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.instances.truncate(1);
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::SCRIPT_ONLY;
        instance.map_position = [600, 20, 600];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let mut sounds = Vec::new();
    world
        .update_separate(
            [600, 600],
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |actor, sound| {
                sounds.push((actor, sound));
                Ok(())
            },
            100,
        )
        .unwrap();
    assert!(matches!(
        world.actors()[0].source,
        ScriptActorSource::Map(_)
    ));
    assert!(world.actors()[0].runtime().is_none());
    assert_eq!(sounds.len(), 1);
    assert!(
        sounds
            .iter()
            .all(|(_, sound)| matches!(sound, ScriptSound::StopOwned))
    );
    world
        .update_separate(
            [600, 600],
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |actor, sound| {
                sounds.push((actor, sound));
                Ok(())
            },
            100,
        )
        .unwrap();
    assert!(world.actors()[0].failure().is_none());
    assert_eq!(sounds.len(), 1);
}

#[test]
fn shell_scripts_run_without_a_map_object_and_retain_locals() {
    use cyrus_script::script::{ScriptActorSource, ScriptInputs};
    let mut scripts = cache();
    scripts.instances.clear();
    scripts.shells = vec!["LOGIC".into()];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(world.actors().len(), 1);
    assert!(matches!(
        world.actors()[0].source,
        ScriptActorSource::Shell(_)
    ));
    for expected in [5, 6] {
        world
            .update_shells(
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        let actor = &world.actors()[0];
        assert_eq!(actor.runtime().unwrap().variables(), [expected]);
        assert!(actor.control_state().is_none());
        assert!(actor.failure().is_none());
    }
}

#[test]
fn separate_attribute_writes_are_temporary_and_activation_does_not_bypass_view_selection() {
    use cyrus_script::script::ScriptInputs;
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: Native::GetAttribute,
            param_count: 1,
            kind: CallKind::Function,
        },
        Function {
            name: Native::SetAttribute,
            param_count: 2,
            kind: CallKind::Function,
        },
    ]);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::Active] = 99;
    let get = attribute_call(1, Native::GetAttribute, 0, None);
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Flag { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(get),
                        },
                        rest: vec![],
                    },
                },
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::Call {
                    call: attribute_call(2, Native::SetAttribute, 0, Some(7)),
                },
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::SCRIPT_ONLY;
        instance.map_position = [600, 20, 600];
        instance.active = 1;
    }
    scripts.instances[1].map_position = [60000, 20, 60000];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(
        world
            .separate_actors([600, 600])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect::<Vec<_>>(),
        [0]
    );
    for _ in 0..2 {
        world
            .update_separate(
                [600, 600],
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert_eq!(world.flags()[0], 0);
        assert_eq!(world.actors()[0].attributes()[Attribute::Active], 0);
        assert!(
            world.actors()[0].failure().is_none(),
            "{:?}",
            world.actors()[0].failure()
        );
    }
}

#[test]
fn separate_disabled_tasks_preserve_locals_and_maintain_authored_lights() {
    use cyrus_script::script::ScriptInputs;
    for object_type in [PlacementType::SCRIPT_ONLY, PlacementType::COLOURED_LIGHT] {
        let mut scripts = cache();
        scripts.updates.task_system = false;
        for instance in &mut scripts.instances {
            instance.object_type = object_type;
            instance.map_position = [600, 20, 600];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world
            .update_separate(
                [600, 600],
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        for (index, actor) in world.actors().iter().enumerate() {
            assert_eq!(
                actor.runtime().unwrap().variables(),
                [if index == 0 { 4 } else { 9 }]
            );
            assert!(actor.control_state().is_none());
            assert!(actor.failure().is_none());
        }
    }
}

#[test]
fn a_shell_started_after_preparation_survives_a_snapshot_roundtrip() {
    use cyrus_script::script::{ScriptActorSource, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    let mut shell = scripts.actors["LOGIC"].clone();
    shell.code_origin += 1000;
    shell.header_index += 100;
    scripts.actors.insert("LATER".into(), shell);
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, data, _) = static_model_fixture(scripts.clone());
    let census = world.actors().len();
    world.spawn_shell("LATER").unwrap();
    assert_eq!(world.actors().len(), census + 1);
    assert!(world.spawn_shell("ABSENT").is_err());
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    let restored = ScriptWorld::from_snapshot(
        serde_json::from_value(saved.clone()).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 9,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(restored.actors().len(), census + 1);
    assert!(matches!(
        &restored.actors()[census].source,
        ScriptActorSource::Shell(name) if name == "LATER"
    ));
    // Only shells may exceed the installed census.
    let mut forged = saved;
    forged["actors"][census]["source"] = serde_json::json!("map");
    assert!(
        ScriptWorld::from_snapshot(
            serde_json::from_value(forged).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 9,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .is_err()
    );
}

#[test]
fn open_shell_starts_the_named_script_regardless_of_case_and_rejects_unknown_names() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let open = |name: &str| {
        let mut scripts = cache();
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::OpenShell,
            param_count: 1,
            kind: CallKind::Function,
        });
        let mut shell = scripts.actors["LOGIC"].clone();
        shell.code_origin += 1000;
        shell.header_index += 100;
        scripts.actors.insert("ITEMLIST".into(), shell);
        let logic = scripts.actors.get_mut("LOGIC").unwrap();
        logic.program.strings = vec![name.into()];
        logic.program.steps = [
            Operation::Call {
                call: Call {
                    function_id: id,
                    function: Native::OpenShell,
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![Value::String { index: 0 }],
                },
            },
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
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .map(|()| world.running_shells("ITEMLIST"))
    };
    assert_eq!(open("itemlist").unwrap(), 1);
    assert!(open("nothing").is_err());
}
