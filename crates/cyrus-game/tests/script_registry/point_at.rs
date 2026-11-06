//! PointAt turns its receiver toward the actor whose script runs the call.
use super::*;
use cyrus_game::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};

/// Actor 0 runs `target.PointAt()`; its own stored model position is `caller` and the selected
/// player's (actor 2) is `player`, both in map units.
fn point_world(target: Object, caller: [f32; 3], player: [f32; 3]) -> ScriptWorld {
    let call = Call {
        function_id: 4,
        function: Native::PointAt,
        kind: CallKind::Function,
        target: target.clone(),
        arguments: vec![],
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
        name: Native::PointAt,
        param_count: 0,
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
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": scripts.actors.keys().map(|name| serde_json::json!({"script_name":name,"model_load_path":"none","mpsz_bounds_primary":-1})).collect::<Vec<_>>() }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    // The stored model coordinates are restored state: place them through a snapshot.
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for (index, position) in [(0, caller), (2, player)] {
        saved["actors"][index]["model"]["position"] = serde_json::json!(position.map(f32::to_bits));
    }
    let mut world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 8,
            globals: Default::default(),
            samples: &Default::default(),
        },
    )
    .unwrap();
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

fn yaw(world: &ScriptWorld, index: usize) -> i32 {
    world
        .spatial(world.id(index).unwrap())
        .unwrap()
        .local
        .angles[1]
}

#[test]
fn point_at_turns_the_routed_receiver_toward_the_calling_actor() {
    // Differences are caller minus receiver, truncated toward zero. Yaw is the negated bearing:
    // the angles follow the original's bearing helper (axis cases, equal magnitudes per quadrant
    // and fractions that truncate away).
    for (difference, expected) in [
        ([0.0, 0.0, 5.0], 0),
        ([0.0, 0.0, -5.0], 1024),
        ([5.0, 0.0, 0.0], 1536),
        ([-5.0, 0.0, 0.0], 512),
        ([5.0, 0.0, 5.0], 1792),
        ([-5.0, 0.0, 5.0], 256),
        ([5.0, 0.0, -5.0], 1280),
        ([-5.0, 0.0, -5.0], 768),
        // The fractions truncate toward zero before the angle: (5.9, -0.9, 5.9) is (5, 0, 5).
        ([5.9, 0.0, 5.9], 1792),
        ([-0.9, 0.0, 5.0], 0),
        ([5.0, 0.0, -0.9], 1536),
    ] {
        let receiver_position = [100.0, 7.0, -40.0];
        let caller_position = [0, 1, 2].map(|axis| receiver_position[axis] + difference[axis]);
        let mut world = point_world(Object::Player, caller_position, receiver_position);
        let before = world.spatial(world.id(2).unwrap()).unwrap().local;
        run(&mut world).unwrap();
        assert_eq!(yaw(&world, 2), expected, "difference {difference:?}");
        let after = world.spatial(world.id(2).unwrap()).unwrap().local;
        // Only the yaw field is written: no other angle, position or matrix changes.
        assert_eq!(after.position, before.position);
        assert_eq!(after.matrix, before.matrix);
        assert_eq!(
            [after.angles[0], after.angles[2]],
            [before.angles[0], before.angles[2]]
        );
        assert_eq!(
            world.flags()[0],
            91,
            "the call returned and the caller continued"
        );
        assert_eq!(yaw(&world, 0), 0, "the caller is untouched");
    }
}

#[test]
fn point_at_on_the_running_actor_itself_has_no_difference_and_yaws_to_zero() {
    let mut world = point_world(Object::Me, [3.0, 0.0, 9.0], [0.0; 3]);
    run(&mut world).unwrap();
    // Zero differences select the bearing helper's zero-X branch with a nonnegative Z.
    assert_eq!(yaw(&world, 0), 0);
}
