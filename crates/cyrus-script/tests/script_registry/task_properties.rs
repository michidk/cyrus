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
    // The second image frame is `first + progress + 1` with the progress before the pass adds its
    // ticks, so writing the progress word as 2 puts the overlay at offset 3, where the untouched
    // task, whose first pass left 3, draws offset 4.
    let written = gxa_world(Some(2));
    assert_eq!(locals(&written)[2], 2, "the write reads back");
    assert_eq!(offset(&written), Some(3));
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

/// Both actors run the same program: an `AnimateGXA` multitask, then reads of the `master` actor's
/// slot zero timer and status into locals 0 and 1 and a write of its pause byte.
fn pause_neighbour_world() -> ScriptWorld {
    let mut scripts = random_scripts(Native::AnimateGXA, vec![], Object::Me);
    scripts.definition.functions[1].kind = CallKind::Multitask;
    scripts.definition.functions[1].param_count = 4;
    scripts.definition.references = vec![
        Property::TaskTimer0,
        Property::TaskStatus0,
        Property::TaskPause0,
    ];
    let master = Object::Named { string: 2 };
    let slot = |reference| Place::Property {
        object: master.clone(),
        reference,
    };
    let assign = |destination, first| Operation::Assign {
        destination,
        formula: Formula {
            first,
            rest: vec![],
        },
    };
    let operations = vec![
        wait(
            1,
            Native::AnimateGXA,
            CallKind::Multitask,
            vec![Value::String { index: 1 }, int(2), int(5), int(100)],
        ),
        assign(Place::Local { index: 0 }, Value::Read { place: slot(0) }),
        assign(Place::Local { index: 1 }, Value::Read { place: slot(1) }),
        assign(slot(2), int(1)),
        Operation::End { target: 0 },
    ];
    let logic = scripts.actors.get_mut("LOGIC").unwrap();
    logic.program.strings = vec![
        "logic".into(),
        "system\\stmap01.gxa".into(),
        "master".into(),
    ];
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
    // Actor 1 answers to `master`.
    let mut second = logic.clone();
    second.namespace = Some("master".into());
    second.header_index = 1;
    second.instances = 1;
    second.instance_variables = vec![vec![0; 3]];
    logic.instances = 1;
    logic.instance_variables.truncate(1);
    scripts.instances[1].actor = "MASTER".into();
    scripts.actors.insert("MASTER".into(), second);
    scripts.content_id = scripts.content_identity().unwrap();
    ScriptWorld::prepare(&scripts, 1).unwrap()
}

#[test]
fn another_actors_slot_fields_are_read_and_written_through_its_own_scheduler_state() {
    let mut world = pause_neighbour_world();
    let inputs = ScriptInputs {
        frame_scale: Some(256),
        nominal_ticks: Some(1),
        ..Default::default()
    };
    let (me, master) = (world.id(0).unwrap(), world.id(1).unwrap());
    // The master runs first: its own slot is served by its running interpreter.
    world
        .advance(master, ScriptStage::OrdinaryMain, inputs, None, None, 100)
        .unwrap();
    // Then actor 0 reads the master's timer (one: allocated, not yet passed) and status (zero: a
    // multitask) and pauses the master's slot, not its own.
    world
        .advance(me, ScriptStage::OrdinaryMain, inputs, None, None, 100)
        .unwrap();
    assert_eq!(locals(&world)[..2], [1, 0]);
    let paused = |index: usize| world.actors()[index].runtime().unwrap().task_slots()[0].3;
    assert!(paused(1), "the write reached the master's slot");
    assert!(!paused(0));
    // A paused slot is skipped by the task pass: the master's timer stays at one.
    world
        .advance(master, ScriptStage::Tasks, inputs, None, None, 100)
        .unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().task_slots()[0].4, 1);
}
