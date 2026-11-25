use super::*;

#[test]
fn native_flag_helpers_keep_type_behavior_numeric_values_and_latches_separate() {
    let native = |id, name: Native| Value::Call {
        call: Box::new(Call {
            function_id: id,
            kind: CallKind::Function,
            target: Object::Me,
            function: name,
            arguments: vec![Value::Integer { value: 0 }],
        }),
    };
    let assign = |index, value| Operation::Assign {
        destination: Place::Local { index },
        formula: Formula {
            first: value,
            rest: vec![],
        },
    };
    for (kind, initial, after_two, after_third, final_latch) in [
        ("BOOL", 0, 1, 0, 1),
        ("FLIPFLOP", 0, 0, 1, 1),
        ("COUNTER", 2, 0, 0, 0),
        ("NUMBER", 7, 7, 7, 1),
    ] {
        let mut scripts = cache();
        scripts.definition.flags[0].flag_type = kind.into();
        scripts.definition.flags[0].initial_value = initial;
        scripts.definition.functions.extend(
            [
                Native::SetGlobalFlag,
                Native::ResetGlobalFlag,
                Native::TestGlobalFlag,
            ]
            .map(|name| Function {
                name,
                param_count: 1,
                kind: CallKind::Function,
            }),
        );
        let ops = vec![
            assign(0, native(1, Native::SetGlobalFlag)),
            assign(0, native(1, Native::SetGlobalFlag)),
            assign(1, native(3, Native::TestGlobalFlag)),
            assign(
                2,
                Value::Read {
                    place: Place::Flag { index: 0 },
                },
            ),
            Operation::Yield,
            assign(0, native(2, Native::ResetGlobalFlag)),
            assign(1, native(3, Native::TestGlobalFlag)),
            assign(
                2,
                Value::Read {
                    place: Place::Flag { index: 0 },
                },
            ),
            Operation::Yield,
            assign(0, native(1, Native::SetGlobalFlag)),
            assign(1, native(3, Native::TestGlobalFlag)),
            assign(
                2,
                Value::Read {
                    place: Place::Flag { index: 0 },
                },
            ),
            Operation::End { target: 0 },
        ];
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.program.variables = vec![0; 3];
        actor.instance_variables = vec![vec![0; 3]; 2];
        actor.program.steps = ops
            .into_iter()
            .enumerate()
            .map(|(offset, operation)| {
                (
                    offset as u32,
                    Step {
                        next: offset as u32 + 1,
                        operation,
                    },
                )
            })
            .collect();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [0, 1, after_two],
            "{kind}"
        );
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [0, 0, after_two],
            "{kind}"
        );
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [0, final_latch, after_third],
            "{kind}"
        );
    }
}

fn flag_snapshot_scripts() -> Scripts {
    let mut scripts = cache();
    scripts.definition.flags = [("BOOL", 0), ("FLIPFLOP", 0), ("COUNTER", 2), ("NUMBER", -3)]
        .into_iter()
        .enumerate()
        .map(|(index, (kind, initial))| Flag {
            name: format!("Flag{index}"),
            flag_type: kind.into(),
            initial_value: initial,
        })
        .collect();
    scripts.definition.functions.push(Function {
        name: Native::SetGlobalFlag,
        param_count: 1,
        kind: CallKind::Function,
    });
    let operations = (0..4)
        .map(|index| Operation::Call {
            call: Call {
                function_id: 1,
                function: Native::SetGlobalFlag,
                target: Object::Me,
                kind: CallKind::Function,
                arguments: vec![Value::Integer { value: index }],
            },
        })
        .chain(std::iter::once(Operation::End { target: 0 }));
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = operations
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
    scripts
}

#[test]
fn global_flag_snapshot_preserves_native_latches_without_executing_scripts() {
    use cyrus_script::script::GlobalFlagsSnapshot;
    let scripts = flag_snapshot_scripts();
    let mut source = ScriptWorld::prepare(&scripts, 1).unwrap();
    source
        .step(source.id(0).unwrap(), Thread::Main, 20)
        .unwrap();
    assert_eq!(source.flags(), [1, 1, 1, -3]);
    let bytes = serde_json::to_vec(&source.snapshot_flags()).unwrap();
    let snapshot: GlobalFlagsSnapshot = serde_json::from_slice(&bytes).unwrap();
    let mut restored = ScriptWorld::prepare(&scripts, 99).unwrap();
    restored.restore_flags(snapshot).unwrap();
    assert_eq!(restored.flags(), [1, 1, 1, -3]);
    assert_eq!(restored.actors()[0].runtime().unwrap().variables(), [4]);
    assert_eq!(restored.actors()[1].runtime().unwrap().variables(), [9]);
    restored
        .step(restored.id(0).unwrap(), Thread::Main, 20)
        .unwrap();
    // A restored Bool latch prevents a second toggle; FlipFlop still toggles.
    assert_eq!(restored.flags(), [1, 0, 0, -3]);
    assert_eq!(source.flags(), [1, 1, 1, -3]);
}

#[test]
fn global_flag_snapshot_retains_raw_values_and_mixed_latches() {
    let scripts = flag_snapshot_scripts();
    let mut source = ScriptWorld::prepare(&scripts, 1).unwrap();
    for (index, value) in [i32::MIN, -17, i32::MIN, i32::MAX].into_iter().enumerate() {
        source
            .assign_global_flag(index, value, index % 2 == 0)
            .unwrap();
    }
    let snapshot =
        serde_json::from_value(serde_json::to_value(source.snapshot_flags()).unwrap()).unwrap();
    let mut restored = ScriptWorld::prepare(&scripts, 2).unwrap();
    restored.restore_flags(snapshot).unwrap();
    assert_eq!(restored.flags(), [i32::MIN, -17, i32::MIN, i32::MAX]);
    for index in 0..4 {
        assert_eq!(restored.global_flag_is_set(index).unwrap(), index % 2 == 0);
    }
    restored
        .step(restored.id(0).unwrap(), Thread::Main, 20)
        .unwrap();
    assert_eq!(restored.flags(), [i32::MIN, 0, i32::MAX, i32::MAX]);
    for index in 0..4 {
        assert!(restored.global_flag_is_set(index).unwrap());
    }
}

#[test]
fn global_flag_snapshot_rejects_incompatible_schema_before_any_write() {
    use serde_json::json;
    let scripts = flag_snapshot_scripts();
    let source = ScriptWorld::prepare(&scripts, 1).unwrap();
    let snapshot = serde_json::to_value(source.snapshot_flags()).unwrap();
    let mut destination = ScriptWorld::prepare(&scripts, 2).unwrap();
    for index in 0..4 {
        destination
            .assign_global_flag(index, 40 + index as i32, true)
            .unwrap();
    }
    let before = serde_json::to_value(destination.snapshot_flags()).unwrap();
    let mut cases = vec![];
    let mut version = snapshot.clone();
    version["version"] = json!(2);
    cases.push(version);
    let mut short = snapshot.clone();
    short["flags"].as_array_mut().unwrap().pop();
    cases.push(short);
    let mut long = snapshot.clone();
    long["flags"]
        .as_array_mut()
        .unwrap()
        .push(snapshot["flags"][0].clone());
    cases.push(long);
    let mut reordered = snapshot.clone();
    reordered["flags"].as_array_mut().unwrap().swap(0, 3);
    cases.push(reordered);
    for (field, value) in [
        ("name", json!("Other")),
        ("flag_type", json!("BOOL")),
        ("initial_value", json!(30)),
    ] {
        let mut changed = snapshot.clone();
        changed["flags"][3]["definition"][field] = value;
        cases.push(changed);
    }
    for corrupt in cases {
        let error = destination
            .restore_flags(serde_json::from_value(corrupt).unwrap())
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("world 7 global flag restoration") && message.contains("snapshot"),
            "{message}"
        );
        assert_eq!(
            serde_json::to_value(destination.snapshot_flags()).unwrap(),
            before
        );
    }
}

#[test]
fn global_flag_snapshot_empty_schema_and_malformed_records_are_explicit() {
    use cyrus_script::script::GlobalFlagsSnapshot;
    use serde_json::json;
    let mut scripts = flag_snapshot_scripts();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let snapshot = serde_json::to_value(world.snapshot_flags()).unwrap();
    for (field, invalid) in [("value", json!(2147483648u64)), ("latched", json!(1))] {
        let mut corrupt = snapshot.clone();
        corrupt["flags"][0][field] = invalid;
        assert!(serde_json::from_value::<GlobalFlagsSnapshot>(corrupt).is_err());
    }
    let mut missing = snapshot.clone();
    missing["flags"][0]
        .as_object_mut()
        .unwrap()
        .remove("latched");
    assert!(serde_json::from_value::<GlobalFlagsSnapshot>(missing).is_err());
    scripts.definition.flags.clear();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut empty = ScriptWorld::prepare(&scripts, 2).unwrap();
    empty
        .restore_flags(
            serde_json::from_value(serde_json::to_value(empty.snapshot_flags()).unwrap()).unwrap(),
        )
        .unwrap();
    assert!(empty.flags().is_empty());
    assert!(
        empty
            .restore_flags(serde_json::from_value(snapshot).unwrap())
            .is_err()
    );
    assert!(empty.flags().is_empty());
}

#[test]
fn global_flag_snapshot_round_trips_the_installed_schema() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (global flag snapshots)");
        return;
    };
    let definition =
        rgpre::import::soup_def::try_load_soup_def(&install.root).expect("installed SOUP386.DEF");
    assert!(!definition.flags.is_empty());
    let mut scripts = flag_snapshot_scripts();
    scripts.definition.flags = definition
        .flags
        .into_iter()
        .map(|flag| Flag {
            name: flag.name,
            flag_type: flag.flag_type,
            initial_value: flag.initial_value,
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut source = ScriptWorld::prepare(&scripts, 1).unwrap();
    let values: Vec<_> = (0..scripts.definition.flags.len())
        .map(|index| i32::MIN + index as i32)
        .collect();
    for (index, value) in values.iter().enumerate() {
        source
            .assign_global_flag(index, *value, index % 2 == 0)
            .unwrap();
    }
    let snapshot =
        serde_json::from_slice(&serde_json::to_vec(&source.snapshot_flags()).unwrap()).unwrap();
    let mut restored = ScriptWorld::prepare(&scripts, 2).unwrap();
    restored.restore_flags(snapshot).unwrap();
    assert_eq!(restored.flags(), values);
    for index in 0..values.len() {
        assert_eq!(restored.global_flag_is_set(index).unwrap(), index % 2 == 0);
    }
}
