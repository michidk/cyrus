//! `Move`, `sMove`, `Rotate` and `sRotate` through the public task stages. Expectations come from the
//! four handlers' arithmetic: all take (x, y, z, duration) and continue while `duration - 1` exceeds
//! the elapsed input; `Rotate` and `sRotate` are one routine.
use super::*;
use cyrus_script::script::{ScriptInputs, ScriptStage};

fn int(value: i32) -> Value {
    Value::Integer { value }
}

/// Actor 0 starts `function(x, y, z, duration)` as a task; returns the world before the first run.
fn world(function: Native, operands: [i32; 4], angles: [i32; 3]) -> ScriptWorld {
    let mut scripts = wait_cache(vec![
        wait(1, function, CallKind::Task, operands.map(int).to_vec()),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions[1] = Function {
        name: function,
        param_count: 4,
        kind: CallKind::Task,
    };
    scripts.instances[0].angles = angles.map(|angle| angle as u32);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world
}

fn run(world: &mut ScriptWorld, stage: ScriptStage, ticks: u32) {
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            stage,
            ScriptInputs {
                nominal_ticks: Some(ticks),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
}

fn pose(world: &ScriptWorld) -> ([i32; 3], [i32; 3]) {
    let spatial = world.spatial(world.id(0).unwrap()).unwrap();
    (spatial.local.position, spatial.local.angles)
}

fn active(world: &ScriptWorld) -> usize {
    world.actors()[0].runtime().unwrap().active_tasks()
}

/// Runs the task to completion and returns the pose after the initial call and each task pass.
fn poses(world: &mut ScriptWorld, ticks: u32) -> Vec<([i32; 3], [i32; 3])> {
    let mut out = vec![];
    run(world, ScriptStage::Body(Thread::Main), ticks);
    out.push(pose(world));
    for _ in 0..16 {
        if active(world) == 0 {
            break;
        }
        run(world, ScriptStage::Tasks, ticks);
        out.push(pose(world));
    }
    out
}

#[test]
fn move_adds_each_operand_times_the_nominal_ticks_until_the_duration_elapses() {
    let mut world = world(Native::Move, [10, 0, -5, 3], [0; 3]);
    let start = pose(&world).0;
    let seen = poses(&mut world, 3);
    // Elapsed 0, 1 and 2 each add (30, 0, -15); the third invocation reaches duration - 1.
    let step = |count: i32| [start[0] + 30 * count, start[1], start[2] - 15 * count];
    assert_eq!(
        seen.iter()
            .map(|(position, _)| *position)
            .collect::<Vec<_>>(),
        [step(1), step(2), step(3)]
    );
    assert_eq!(active(&world), 0);
}

#[test]
fn smove_adds_the_quantised_step_whatever_the_tick_count() {
    let mut world = world(Native::SMove, [10, 0, -5, 3], [0; 3]);
    let start = pose(&world).0;
    // (10 << 8) / 3 >> 8 is 3 and (-5 << 8) / 3 >> 8 is -2, once per invocation.
    let seen = poses(&mut world, 7);
    let step = |count: i32| [start[0] + 3 * count, start[1], start[2] - 2 * count];
    assert_eq!(
        seen.iter()
            .map(|(position, _)| *position)
            .collect::<Vec<_>>(),
        [step(1), step(2), step(3)]
    );
}

#[test]
fn rotate_and_srotate_are_one_routine_that_stores_the_angle_unmasked() {
    for function in [Native::Rotate, Native::SRotate] {
        // Starting at 2040, x gains (80 << 8) / 3 * elapsed >> 8 per invocation: 0, 26, 53. The sum
        // is stored without masking to 2047 and z loses the matching magnitude.
        let mut world = world(function, [80, 0, -40, 3], [2040, 300, 50]);
        let seen = poses(&mut world, 3);
        assert_eq!(
            seen.iter().map(|(_, angles)| *angles).collect::<Vec<_>>(),
            [[2040, 300, 50], [2066, 300, 37], [2119, 300, 11]],
            "{function}"
        );
        assert_eq!(active(&world), 0);
    }
}

#[test]
fn a_zero_duration_has_no_successful_result() {
    for function in [Native::Move, Native::SMove, Native::Rotate, Native::SRotate] {
        let mut world = world(function, [1, 0, 0, 0], [0; 3]);
        let id = world.id(0).unwrap();
        let result = world.advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        );
        if function == Native::Move {
            // Move never divides and `duration - 1` is already below the elapsed input, so the
            // handler completes at once; the slot is allocated regardless and a later pass drops it.
            result.unwrap();
            assert_eq!(active(&world), 1);
            run(&mut world, ScriptStage::Tasks, 1);
            assert_eq!(active(&world), 0);
        } else {
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains("division by zero"), "{function}: {error}");
        }
    }
}

/// Actor 0 starts `function(axis, delta, duration)` as a task.
fn axis_world(function: Native, operands: [i32; 3]) -> ScriptWorld {
    let mut scripts = wait_cache(vec![
        wait(1, function, CallKind::Task, operands.map(int).to_vec()),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions[1] = Function {
        name: function,
        param_count: 3,
        kind: CallKind::Task,
    };
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world
}

#[test]
fn smove_along_axis_spreads_the_step_over_the_duration_in_frames() {
    // frames = (duration << 8) / frame_scale = 3 and the step is ((delta << 8) / frames) >> 8 = 3,
    // rotated by the receiver's angles (none here) and added on every invocation.
    for (axis, index) in [(0, 0), (1, 1), (2, 2), (9, 2)] {
        let mut world = axis_world(Native::SMoveAlongAxis, [axis, 10, 3]);
        let start = pose(&world).0;
        let seen = poses(&mut world, 5);
        let at = |count: i32| {
            let mut position = start;
            position[index] += 3 * count;
            position
        };
        assert_eq!(
            seen.iter()
                .map(|(position, _)| *position)
                .collect::<Vec<_>>(),
            [at(1), at(2), at(3)],
            "axis {axis}"
        );
    }
}

#[test]
fn move_along_axis_moves_until_its_running_sum_reaches_the_delta_without_moving_then() {
    // delta 2560 over duration 10 at frame scale 256: (2560 / 10 >> 8) * 256 = 256 per
    // invocation. The sum reaches the delta on the tenth invocation, which completes without
    // moving, so nine steps are taken.
    let mut world = axis_world(Native::MoveAlongAxis, [0, 2560, 10]);
    let start = pose(&world).0;
    let seen = poses(&mut world, 1);
    assert_eq!(seen.len(), 10);
    let last = seen.last().unwrap().0;
    assert_eq!(last[0] - start[0], 9 * 256);
    assert_eq!(
        seen[8].0[0], last[0],
        "the completing invocation does not move"
    );
    assert_eq!(active(&world), 0);
}

#[test]
fn smove_along_axis_without_a_frame_scale_step_has_no_successful_result() {
    let mut world = axis_world(Native::SMoveAlongAxis, [0, 10, 0]);
    let id = world.id(0).unwrap();
    let error = world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("division by zero"),
        "{error:#}"
    );
}
