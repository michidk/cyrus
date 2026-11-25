use super::*;

pub(super) fn random_scripts(function: Native, arguments: Vec<Value>, target: Object) -> Scripts {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: function,
        param_count: 1,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.strings = vec!["logic".into()];
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(Call {
                                function_id: 1,
                                function,
                                kind: CallKind::Function,
                                target,
                                arguments,
                            }),
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
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn random_stream_is_shared_by_routed_actors_and_survives_world_replacement() {
    use cyrus_game::random::EngineRandom;
    use cyrus_script::script::ScriptStage;
    let scripts = random_scripts(
        Native::QuickRnd,
        vec![Value::Integer { value: -1 }],
        Object::Named { string: 0 },
    );
    let mut random = EngineRandom::default();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    for (index, expected) in [(1, 16838), (0, 5758)] {
        world
            .advance(
                world.id(index).unwrap(),
                ScriptStage::Body(Thread::Main),
                Default::default(),
                None,
                Some(&mut random),
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[index].runtime().unwrap().variables(),
            [expected]
        );
    }
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [16838]);
    let mut replacement = ScriptWorld::prepare(&scripts, 2).unwrap();
    replacement
        .advance(
            replacement.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            Default::default(),
            None,
            Some(&mut random),
            20,
        )
        .unwrap();
    assert_eq!(
        replacement.actors()[0].runtime().unwrap().variables(),
        [10113]
    );
    assert_eq!(random.state(), 662824084);
}

#[test]
fn random_handler_requires_shared_state_and_invalid_bounds_fail_stickily() {
    use cyrus_game::random::EngineRandom;
    use cyrus_script::script::ScriptStage;
    for (function, bound, supplied, message) in [
        (
            Native::QuickRnd,
            127,
            false,
            "shared engine random stream unavailable",
        ),
        (Native::Rnd, 0, true, "positive bound"),
        (Native::Rnd, -1, true, "positive bound"),
    ] {
        let scripts = random_scripts(function, vec![Value::Integer { value: bound }], Object::Me);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let mut random = EngineRandom::default();
        let id = world.id(0).unwrap();
        let error = world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                Default::default(),
                None,
                supplied.then_some(&mut random),
                20,
            )
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains(message), "{error}");
        assert!(world.actors()[0].failure().is_some());
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
        assert_eq!(random.state(), 1);
        assert!(
            world
                .advance(
                    id,
                    ScriptStage::Body(Thread::Main),
                    Default::default(),
                    None,
                    Some(&mut random),
                    20
                )
                .is_err()
        );
        assert_eq!(random.state(), 1);
    }
}

#[test]
fn rnd_script_masks_before_reducing_and_consumes_one_draw() {
    use cyrus_game::random::EngineRandom;
    use cyrus_script::script::ScriptStage;
    let scripts = random_scripts(Native::Rnd, vec![Value::Integer { value: 3 }], Object::Me);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let mut random = EngineRandom::default();
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            Default::default(),
            None,
            Some(&mut random),
            20,
        )
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [2]);
    assert_eq!(random.state(), 1103527590);
}
