//! PlayerForceTalk places the selected player through the receiver's orientation matrix.
use super::*;
use cyrus_game::script::ScriptInputs;

fn force_talk_world(distance: i32, angles: [u16; 3], target: Object) -> ScriptWorld {
    let call = Call {
        function_id: 4,
        function: Native::PlayerForceTalk,
        kind: CallKind::Function,
        target: target.clone(),
        arguments: vec![Value::Integer { value: distance }],
    };
    let operation = if matches!(target, Object::Me) {
        Operation::Call { call }
    } else {
        Operation::ObjectCall { call }
    };
    let mut scripts = wait_cache(vec![
        operation,
        set_flag(91),
        Operation::Yield,
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: Native::PlayerForceTalk,
        param_count: 1,
        kind: CallKind::Function,
    });
    add_standalone_receivers(&mut scripts);
    scripts.actors.get_mut("LOGIC").unwrap().program.strings = vec!["logic".into()];
    for instance in &mut scripts.instances {
        instance.object_type = 1;
    }
    scripts.instances[0].angles = angles.map(u32::from);
    scripts.instances[0].map_position = [10, 0, 20];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(2).unwrap();
    world
        .initialize_player_spatial(player, [123, 456, 789], [0, 300, 0])
        .unwrap();
    world
}

fn run(world: &mut ScriptWorld) -> anyhow::Result<()> {
    world.advance(
        world.id(0).unwrap(),
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        20,
    )
}

#[test]
fn player_force_talk_moves_the_player_to_the_receiver_plus_the_rotated_distance() {
    for (distance, angles) in [
        (512, [0; 3]),
        (-300, [0; 3]),
        (512, [0, 512, 0]),
        (700, [0, 1500, 0]),
    ] {
        let mut world = force_talk_world(distance, angles, Object::Me);
        let (receiver, player) = (world.id(0).unwrap(), world.id(2).unwrap());
        let source = world.spatial(receiver).unwrap().local;
        let rotated = source.matrix.transform([0, 0, distance]).unwrap();
        let before = world.spatial(player).unwrap().local;
        run(&mut world).unwrap();
        let after = world.spatial(player).unwrap().local;
        assert_eq!(
            after.position,
            [
                source.position[0].wrapping_add(rotated[0]),
                before.position[1],
                source.position[2].wrapping_add(rotated[2]),
            ],
            "distance {distance} angles {angles:?}"
        );
        assert_eq!(after.angles, before.angles, "the orientation is unchanged");
        assert_eq!(
            world.flags()[0],
            91,
            "the call returned and the caller continued"
        );
        if angles == [0; 3] {
            // An unrotated receiver moves the player straight along Z.
            assert_eq!(rotated, [0, 0, distance]);
        }
    }
}

#[test]
fn player_force_talk_accepts_the_selected_player_as_its_own_receiver() {
    let mut world = force_talk_world(256, [0; 3], Object::Player);
    let player = world.id(2).unwrap();
    let before = world.spatial(player).unwrap().local;
    let rotated = before.matrix.transform([0, 0, 256]).unwrap();
    run(&mut world).unwrap();
    let after = world.spatial(player).unwrap().local;
    assert_eq!(
        after.position,
        [
            before.position[0].wrapping_add(rotated[0]),
            before.position[1],
            before.position[2].wrapping_add(rotated[2]),
        ]
    );
}
