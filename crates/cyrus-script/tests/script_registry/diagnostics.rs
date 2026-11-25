use super::*;

#[test]
fn spare_natives_ignore_their_arguments_add_one_diagnostic_line_and_return_zero() {
    use Native::*;
    // (native, argument count): the Spare1Parm7 record carries eight although the handler
    // cleans seven; the dispatch uses the record's own count.
    let spares = [
        (Spare0Parm0, 0),
        (Spare0Parm1, 1),
        (Spare0Parm2, 2),
        (Spare0Parm3, 3),
        (Spare0Parm4, 4),
        (Spare0Parm5, 5),
        (Spare0Parm6, 6),
        (Spare0Parm7, 7),
        (Spare1Parm0, 0),
        (Spare1Parm1, 1),
        (Spare1Parm2, 2),
        (Spare1Parm3, 3),
        (Spare1Parm4, 4),
        (Spare1Parm5, 5),
        (Spare1Parm6, 6),
        (Spare1Parm7, 8),
    ];
    for (native, count) in spares {
        let arguments: Vec<i32> = (0..count).map(|value| value * 7 + 3).collect();
        let world = run_native_world_with(&[(native, &arguments)], true).unwrap();
        let id = world.id(0).unwrap();
        let before = run_native_world_with(&[], true).unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            before
                .spatial(before.id(0).unwrap())
                .unwrap()
                .local
                .position,
            "{native}: no actor state changes"
        );
        let lines = world.globals().printed_lines();
        assert_eq!(lines.len(), 1, "{native}");
        assert!(lines[0].ends_with("Spare function"), "{native}: {lines:?}");
    }
}

#[test]
fn native_failure_preserves_written_flags_and_restores_runtime() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::PlayerLineUp,
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: set_flag(73),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: wait(1, Native::PlayerLineUp, CallKind::Function, vec![]),
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
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let error = world.step(id, Thread::Main, 20).unwrap_err();
    // Without a selected player the lineup owner cannot be built, so the native fails.
    assert!(
        format!("{error:#}").contains("lineup selected player unavailable"),
        "{error:#}"
    );
    assert_eq!(world.flags(), [73]);
    assert!(world.actors()[0].runtime().unwrap().failure().is_some());
    assert!(world.step(id, Thread::Main, 20).is_err());
    assert_eq!(world.flags(), [73]);
}

#[test]
fn alternate_main_retains_native_source_before_decoding_strings() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let disable = Operation::Call {
        call: Call {
            function_id: 4,
            function: Native::DisableTasks,
            kind: CallKind::Function,
            target: Object::Me,
            arguments: vec![],
        },
    };
    let decode = Operation::Call {
        call: Call {
            function_id: 5,
            function: Native::SetAiType,
            kind: CallKind::Function,
            target: Object::Me,
            arguments: vec![Value::String { index: 1 }],
        },
    };
    let mut scripts = routed_cache(
        vec![Operation::End { target: 0 }],
        vec![
            disable,
            Operation::Yield,
            decode,
            Operation::End { target: 0 },
        ],
    );
    scripts.definition.functions.extend([
        Function {
            name: Native::DisableTasks,
            param_count: 0,
            kind: CallKind::Function,
        },
        Function {
            name: Native::SetAiType,
            param_count: 1,
            kind: CallKind::Function,
        },
    ]);
    scripts.actors.get_mut("RECEIVER").unwrap().program.strings =
        vec!["receiver".into(), "receiver-only string".into()];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 41).unwrap();
    let source = world.id(0).unwrap();
    let alternate = world.id(1).unwrap();
    // Direct body execution sets the alternate flag without assigning an interpreter source.
    world.step(alternate, Thread::Main, 20).unwrap();
    world
        .advance(
            source,
            ScriptStage::Entry,
            ScriptInputs::default(),
            None,
            None,
            20,
        )
        .unwrap();
    let error = world
        .advance(
            alternate,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("retained source string 1 absent"),
        "{error:#}"
    );
    let globals = serde_json::to_value(world.globals()).unwrap();
    assert_eq!(
        globals["native_source"],
        serde_json::json!({"Actor":{"generation":41,"index":0}})
    );
}

#[test]
fn a_watched_native_is_logged_with_its_caller_and_unwatched_ones_are_not() {
    use cyrus_script::script::{NativeCall, ScriptInputs};
    let run = |watch: &[Native]| {
        let (mut world, _, _) = script_jump_fixture(0, |_| {});
        world.watch_natives(watch.iter().copied().collect());
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
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
        world.take_native_calls()
    };
    assert_eq!(
        run(&[Native::Jump]),
        [NativeCall {
            function: Native::Jump,
            actor: "LOGIC".to_owned(),
            integers: vec![],
        }]
    );
    assert!(run(&[Native::SetAttribute]).is_empty());
    assert!(run(&[]).is_empty());
}

#[test]
fn first_sticky_script_failure_is_logged_once_with_world_and_script() {
    struct Capture;
    thread_local! {
        static LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    impl log::Log for Capture {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() == log::Level::Error
        }
        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                LINES.with(|lines| lines.borrow_mut().push(record.args().to_string()));
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Capture = Capture;
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Error);
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let name = world.actors()[0].name().to_owned();
    let world_id = scripts.world;
    let error = world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            cyrus_script::script::ScriptInputs::default(),
            None,
            None,
            0,
        )
        .unwrap_err();
    let reason = format!("{error:#}");
    let mut actors = cyrus_script::script::ScriptActors::default();
    actors.current = Some(world);
    actors.report_runtime_error(format!("actor {name}: {reason}"));
    actors.report_runtime_error("later failure must not replace the cause".into());
    LINES.with(|lines| {
        let lines = lines.borrow();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains(&format!("world Some({world_id})")));
        assert!(lines[0].contains(&format!("actor {name}")));
        assert!(lines[0].contains(&reason));
        assert!(!lines[0].contains("later failure"));
    });
}
