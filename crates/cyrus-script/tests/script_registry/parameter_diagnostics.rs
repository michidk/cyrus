//! PrintParms, LogParms and PrintStringParm format the receiver's script name and their arguments.
use super::*;
use cyrus_script::script::{ScriptInputs, ScriptStage};

fn name_of(world: &ScriptWorld) -> String {
    world.actors()[0].name().to_owned()
}

#[test]
fn print_parms_prints_the_name_and_eight_signed_integers_on_the_first_free_console_line() {
    let arguments = [1, -2, 3, i32::MIN, i32::MAX, 0, 7, -8];
    let world = run_native_world_with(&[(Native::PrintParms, &arguments)], true).unwrap();
    let lines = world.globals().printed_lines();
    assert_eq!(
        lines,
        [format!(
            "{} 1, -2, 3, {}, {}, 0, 7, -8",
            name_of(&world),
            i32::MIN,
            i32::MAX
        )]
    );
    // The call returns zero and a second call takes the next line.
    let twice = run_native_world_with(
        &[
            (Native::PrintParms, &arguments),
            (Native::PrintParms, &[0; 8]),
        ],
        true,
    )
    .unwrap();
    assert_eq!(twice.globals().printed_lines().len(), 2);
    assert_eq!(twice.actors()[0].runtime().unwrap().variables()[0], 0);
}

#[test]
fn log_parms_returns_zero_without_a_console_line_or_state_change() {
    let before = run_native_world_with(&[], true).unwrap();
    let world =
        run_native_world_with(&[(Native::LogParms, &[1, 2, 3, 4, 5, 6, 7, 8])], true).unwrap();
    assert!(world.globals().printed_lines().is_empty());
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.position,
        before
            .spatial(before.id(0).unwrap())
            .unwrap()
            .local
            .position
    );
    assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 0);
}

#[test]
fn a_parameter_diagnostic_with_the_wrong_argument_count_is_rejected_when_the_world_is_prepared() {
    for native in [Native::PrintParms, Native::LogParms] {
        let error = run_native_world_with(&[(native, &[1, 2, 3])], true)
            .err()
            .expect("seven or fewer arguments are refused");
        assert!(format!("{error:#}").contains(native.name()), "{error:#}");
    }
}

#[test]
fn print_string_parm_prints_the_name_and_the_evaluated_string() {
    let run = |text: &str| {
        let mut scripts = cache();
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::PrintStringParm,
            param_count: 1,
            kind: CallKind::Function,
        });
        let logic = scripts.actors.get_mut("LOGIC").unwrap();
        logic.program.strings = vec![text.into()];
        logic.program.steps = [
            Operation::Call {
                call: Call {
                    function_id: id,
                    function: Native::PrintStringParm,
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
            .unwrap();
        world
    };
    for text in ["hello", "", "two words, comma"] {
        let world = run(text);
        assert_eq!(
            world.globals().printed_lines(),
            [format!("{} {text}", name_of(&world))]
        );
    }
}
