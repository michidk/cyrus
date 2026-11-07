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

fn player_shape(world: &ScriptWorld) -> &cyrus_script::script::OrdinaryCollisionShape {
    let index = world
        .actors()
        .iter()
        .position(|a| matches!(a.source, ScriptActorSource::Player))
        .unwrap();
    world
        .ordinary_collision_shape(world.id(index).unwrap())
        .unwrap()
}

#[test]
fn extent_writes_replace_the_shape_extents_and_survive_a_restore() {
    // The fixture's extents are 12, 24, 12 on both sides. Writes are raw signed words.
    for value in [77, 0, -5, i32::MIN, i32::MAX] {
        for (property, negative, axis) in [
            (Property::SizePsizeVy, false, 1),
            (Property::SizeMsizeVx, true, 0),
        ] {
            let (world, restored) = write_extent(property, value);
            for world in [&world, &restored] {
                let shape = player_shape(world);
                let (written, other) = if negative {
                    (shape.negative_extent, shape.positive_extent)
                } else {
                    (shape.positive_extent, shape.negative_extent)
                };
                assert_eq!(other, [12, 24, 12], "{property}={value}: opposite side");
                let mut expected = [12, 24, 12];
                expected[axis] = value;
                assert_eq!(written, expected, "{property}={value}");
            }
        }
    }
}

#[test]
fn probes_consume_the_written_extents_with_32_bit_wrapping() {
    use cyrus_collision::ground::GroundProbe;
    use cyrus_script::script::ordinary_bounds_segments;
    use cyrus_world::transform::Matrix;
    let position = [100 * 256, 40 * 256, 200 * 256];
    for value in [77, -5, i32::MAX] {
        // The vertical ground probe is centered on the actor origin plus the positive Y extent.
        let (_, restored) = write_extent(Property::SizePsizeVy, value);
        let extent = player_shape(&restored).positive_extent[1];
        let center = 40_i32.wrapping_add(value);
        let probe = GroundProbe::new(position.map(i64::from), extent, 500).unwrap();
        assert_eq!(
            probe.endpoints(),
            (
                [100, center.wrapping_sub(500), 200],
                [100, center.wrapping_add(500), 200]
            ),
            "ground probe, psize.y={value}"
        );
        // The state-fifteen bounds diagonals use the negative X extent as stored, without negation.
        let (_, restored) = write_extent(Property::SizeMsizeVx, value);
        let segments =
            ordinary_bounds_segments(player_shape(&restored), Matrix::IDENTITY, [0; 3]).unwrap();
        // The matrix goes through single precision: the largest word rounds up out of range and
        // converts to the x87 integer indefinite.
        let expected = if value == i32::MAX { i32::MIN } else { value };
        assert_eq!(
            segments[0][1][0], expected,
            "bounds endpoint, msize.x={value}"
        );
        assert_eq!(
            segments[1][0][0], expected,
            "bounds endpoint, msize.x={value}"
        );
    }
}
