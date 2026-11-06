//! TurnToAngle and TurnToPos turn the selected player like any receiver and rebuild its matrix.
use super::*;
use cyrus_game::script::{ScriptActorId, ScriptHud, ScriptInputs, ScriptSound};

fn turn_world(native: Native, arguments: Vec<i32>) -> ScriptWorld {
    let call = Call {
        function_id: 4,
        function: native,
        kind: CallKind::Function,
        target: Object::Player,
        arguments: arguments
            .iter()
            .map(|&value| Value::Integer { value })
            .collect(),
    };
    let mut scripts = wait_cache(vec![
        Operation::Assign {
            destination: Place::Flag { index: 0 },
            formula: Formula {
                first: Value::ObjectCall {
                    call: Box::new(call),
                },
                rest: vec![],
            },
        },
        Operation::Yield,
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: native,
        param_count: arguments.len() as u8,
        kind: CallKind::Function,
    });
    add_standalone_receivers(&mut scripts);
    scripts.actors.get_mut("LOGIC").unwrap().program.strings = vec!["logic".into()];
    for instance in &mut scripts.instances {
        instance.object_type = cyrus_assets::script::PlacementType::MODEL;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(2).unwrap();
    world
        .initialize_player_spatial(player, [4096, 0, 8192], [0, 300, 0])
        .unwrap();
    world
}

fn run(world: &mut ScriptWorld, with_sounds: bool) -> anyhow::Result<()> {
    let hud = ScriptHud::default();
    let mut sounds = |_: ScriptActorId, _: ScriptSound| Ok(());
    world.advance(
        world.id(0).unwrap(),
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            hud: Some(&hud),
            shell_execution: Some(false),
            ..Default::default()
        },
        with_sounds.then_some(
            &mut sounds as &mut dyn FnMut(ScriptActorId, ScriptSound) -> anyhow::Result<()>,
        ),
        None,
        20,
    )
}

#[test]
fn turn_to_angle_turns_the_selected_player_and_rebuilds_its_orientation() {
    // (argument, yaw after, returned value)
    for (target, after, returned) in [(300, 300, 0), (304, 304, 0), (1000, 308, 700)] {
        let mut world = turn_world(Native::TurnToAngle, vec![target]);
        let player = world.id(2).unwrap();
        run(&mut world, true).unwrap();
        let local = world.spatial(player).unwrap().local;
        assert_eq!(local.angles[1], after, "target {target}");
        assert_eq!(world.flags()[0], returned, "target {target}");
        assert_eq!(
            local.matrix,
            cyrus_world::transform::Matrix::orientation(local.wrapped_angles()),
            "the matrix is rebuilt from the stored angles, target {target}"
        );
    }
}

#[test]
fn turn_to_angle_equality_needs_no_sounds_but_a_turn_without_them_fails_before_changing_anything() {
    // Equality returns before any service is needed.
    let mut world = turn_world(Native::TurnToAngle, vec![300]);
    run(&mut world, false).unwrap();
    assert_eq!(world.flags()[0], 0);
    // A turn needs the synchronous sound service; without it nothing is stored.
    let mut world = turn_world(Native::TurnToAngle, vec![1000]);
    let player = world.id(2).unwrap();
    let before = world.spatial(player).unwrap().local;
    let error = format!("{:#}", run(&mut world, false).unwrap_err());
    assert!(error.contains("synchronous sounds"), "{error}");
    assert_eq!(world.spatial(player).unwrap().local.angles, before.angles);
}

#[test]
fn turn_to_pos_snaps_or_turns_the_selected_player() {
    // A point within the facing threshold snaps the exact X and Z; a far point +Z turns.
    let mut world = turn_world(Native::TurnToPos, vec![4096 + 100, 8192 - 50]);
    let player = world.id(2).unwrap();
    run(&mut world, true).unwrap();
    let local = world.spatial(player).unwrap().local;
    assert_eq!([local.position[0], local.position[2]], [4196, 8142]);
    assert_eq!(local.angles[1], 300);
    assert_eq!(world.flags()[0], 0);

    let mut world = turn_world(Native::TurnToPos, vec![4096, 8192 + 1_000_000]);
    let player = world.id(2).unwrap();
    run(&mut world, true).unwrap();
    let local = world.spatial(player).unwrap().local;
    assert_eq!(
        local.position,
        [4096, 0, 8192],
        "a turn does not move the player"
    );
    assert_eq!(local.angles[1], 292);
    assert_eq!(world.flags()[0], -300);
    assert_eq!(
        local.matrix,
        cyrus_world::transform::Matrix::orientation(local.wrapped_angles())
    );
}
