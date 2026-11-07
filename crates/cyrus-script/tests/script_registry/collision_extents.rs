//! `size.psize.v*` and `size.msize.v*` are the extents of the actor's collision shape.
use super::*;
use cyrus_script::script::{
    ActivationInputs, PlayerCollisionSource, ScriptActorSource, ScriptInputs, ScriptStage,
};

/// The selected player runs one property write on its first ordinary update; returns the world
/// with the write applied, plus what restoring a snapshot of it gives.
fn write_extent(property: Property, value: i32) -> (ScriptWorld, ScriptWorld) {
    let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
        let reference = u16::try_from(scripts.definition.references.len()).unwrap();
        scripts.definition.references.push(property);
        scripts
            .actors
            .get_mut("CYRUS")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = Operation::Assign {
            destination: Place::Property {
                object: Object::Me,
                reference,
            },
            formula: Formula {
                first: Value::Integer { value },
                rest: vec![],
            },
        };
    });
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    let map = world.id(0).unwrap();
    world
        .prepare_ordinary_collision_shape(player, &data, &scene)
        .unwrap();
    let activation = ActivationInputs {
        player,
        actors: BTreeMap::new(),
        labels: BTreeMap::new(),
        lockout: 0,
        dialogue_open: false,
        lineup_result: 0,
        use_held: false,
        item_system_ready: false,
        shell_execution: false,
        render_disabled: false,
    };
    world
        .advance(
            player,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                activation: Some(&activation),
                player_collision: Some(PlayerCollisionSource {
                    scene: &scene,
                    animations: &data,
                    view: [522, 542],
                    actors: &[player, map],
                }),
                shell_execution: Some(false),
                fine_ticks: Some(0),
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            100,
        )
        .unwrap();
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let saved = movement_snapshot(&world, &scripts, &data);
    let globals = std::mem::take(world.globals_mut());
    let restored = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &cyrus_script::script::WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &Default::default(),
        },
        cyrus_script::script::WorldRestoreContext {
            generation: world.generation() + 1,
            globals,
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    (world, restored)
}

#[test]
fn extent_writes_replace_the_shape_extents_and_survive_a_restore() {
    // The fixture's extents are 12, 24, 12 on both sides.
    for (property, negative, axis) in [
        (Property::SizePsizeVy, false, 1),
        (Property::SizeMsizeVx, true, 0),
    ] {
        let (world, restored) = write_extent(property, 77);
        for world in [&world, &restored] {
            let id = world
                .id(world
                    .actors()
                    .iter()
                    .position(|a| matches!(a.source, ScriptActorSource::Player))
                    .unwrap())
                .unwrap();
            let shape = world.ordinary_collision_shape(id).unwrap();
            let (written, other) = if negative {
                (shape.negative_extent, shape.positive_extent)
            } else {
                (shape.positive_extent, shape.negative_extent)
            };
            assert_eq!(written[axis], 77, "{property}");
            assert_eq!(other, [12, 24, 12], "the opposite side is untouched");
            let mut expected = [12, 24, 12];
            expected[axis] = 77;
            assert_eq!(written, expected, "other axes are untouched");
        }
    }
}
