//! `task_variable*` are the four 32-bit words each of the actor's six task slots retains.
use super::*;
use cyrus_script::script::{ScriptInputs, ScriptStage};

fn int(value: i32) -> Value {
    Value::Integer { value }
}

fn property(reference: u16) -> Place {
    Place::Property {
        object: Object::Me,
        reference,
    }
}

/// Actor 0 starts `AnimateGXA` as a multitask (so the body continues), optionally writes
/// `task_variable3[0]`, then reads the slot's first three words into locals 0 to 2. Slot zero's
/// first word is the loaded image resource, its third the animation progress.
fn gxa_world(write_progress: Option<i32>) -> ScriptWorld {
    let mut scripts = random_scripts(Native::AnimateGXA, vec![], Object::Me);
    scripts.definition.functions[1].kind = CallKind::Multitask;
    scripts.definition.functions[1].param_count = 4;
    scripts.definition.references = vec![
        Property::TaskVariable0,
        Property::TaskVariable2_0,
        Property::TaskVariable3_0,
    ];
    let assign = |destination, first| Operation::Assign {
        destination,
        formula: Formula {
            first,
            rest: vec![],
        },
    };
    let mut operations = vec![wait(
        1,
        Native::AnimateGXA,
        CallKind::Multitask,
        vec![Value::String { index: 1 }, int(2), int(5), int(100)],
    )];
    if let Some(value) = write_progress {
        operations.push(assign(property(2), int(value)));
    }
    for (local, reference) in [(0, 0), (1, 1), (2, 2)] {
        operations.push(assign(
            Place::Local { index: local },
            Value::Read {
                place: property(reference),
            },
        ));
    }
    operations.push(Operation::End { target: 0 });
    let logic = scripts.actors.get_mut("LOGIC").unwrap();
    logic.program.strings = vec!["logic".into(), "system\\stmap01.gxa".into()];
    logic.program.variables = vec![0; 3];
    logic.instance_variables = vec![vec![0; 3]; 2];
    logic.program.steps = operations
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
    let inputs = |ticks| ScriptInputs {
        frame_scale: Some(256),
        nominal_ticks: Some(ticks),
        ..Default::default()
    };
    let id = world.id(0).unwrap();
    world
        .advance(id, ScriptStage::OrdinaryMain, inputs(3), None, None, 100)
        .unwrap();
    world
        .advance(id, ScriptStage::Tasks, inputs(3), None, None, 100)
        .unwrap();
    world
}

fn locals(world: &ScriptWorld) -> Vec<i32> {
    world.actors()[0]
        .runtime()
        .unwrap()
        .variables()
        .iter()
        .map(|value| value.integer().unwrap())
        .collect()
}

#[test]
fn a_script_write_to_a_task_slot_word_changes_the_native_task_that_resumes_from_it() {
    let untouched = gxa_world(None);
    let offset = |world: &ScriptWorld| world.gxa_overlay().unwrap().second_offset;
    assert_eq!(offset(&untouched), Some(4), "the task's own progress");
    let reads = locals(&untouched);
    assert_ne!(reads[0], 0, "the loaded image resource is the first word");
    assert_eq!(reads[1], 0, "no pause is pending");
    // The progress word is the task's retained progress; writing it before the next pass moves the
    // overlay the task draws.
    let written = gxa_world(Some(2));
    assert_eq!(locals(&written)[2], 2, "the write reads back");
    assert_ne!(offset(&written), offset(&untouched));
}

#[test]
fn task_words_round_trip_per_slot_with_their_widths_and_the_packed_point_command() {
    let mut scripts = cache();
    let words = [
        Property::TaskVariable1,
        Property::TaskVariable2_1,
        Property::TaskVariable3_1,
        Property::TaskVariable4_1,
        Property::TaskVariable4_0,
    ];
    scripts.definition.references = words.to_vec();
    let writes = [-5, 0x7fff_ffff, i32::MIN, 0x0907_1234u32 as i32];
    let mut operations: Vec<Operation> = writes
        .iter()
        .enumerate()
        .map(|(reference, value)| Operation::Assign {
            destination: property(reference as u16),
            formula: Formula {
                first: int(*value),
                rest: vec![],
            },
        })
        .collect();
    for (local, reference) in (0..5).enumerate() {
        operations.push(Operation::Assign {
            destination: Place::Local { index: local as u8 },
            formula: Formula {
                first: Value::Read {
                    place: property(reference),
                },
                rest: vec![],
            },
        });
    }
    operations.push(Operation::End { target: 0 });
    let logic = scripts.actors.get_mut("LOGIC").unwrap();
    logic.program.variables = vec![0; 5];
    logic.instance_variables = vec![vec![0; 5]; 2];
    logic.program.steps = operations
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
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    // Whole 32-bit words per slot; slot zero's packed word was never written.
    assert_eq!(locals(&world), [-5, 0x7fff_ffff, i32::MIN, 0x0907_1234, 0]);
}
