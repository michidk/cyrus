//! `size.psize.v*` and `size.msize.v*` are the extents of the actor's collision shape.
use super::*;
use cyrus_script::script::{
    ActivationInputs, PlayerCollisionSource, ScriptActorSource, ScriptInputs, ScriptStage,
};

/// The selected player runs one property write on its first ordinary update; returns the world
/// with the write applied, plus what restoring a snapshot of it gives.
fn write_extent(
    property: Property,
    value: i32,
) -> (
    ScriptWorld,
    ScriptWorld,
    cyrus_script::player::data::PlayerScene,
    cyrus_assets::animation::Animations,
    Scripts,
) {
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
    (world, restored, scene, data, scripts)
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
            let (world, restored, ..) = write_extent(property, value);
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
        let (_, restored, ..) = write_extent(Property::SizePsizeVy, value);
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
        let (_, restored, ..) = write_extent(Property::SizeMsizeVx, value);
        // The matrix goes through single precision: the largest word rounds up out of range, which
        // the transform rejects with its contextual error.
        if value == i32::MAX {
            let error = ordinary_bounds_segments(player_shape(&restored), Matrix::IDENTITY, [0; 3])
                .unwrap_err();
            assert!(
                format!("{error:#}").contains("cannot convert to i32"),
                "{error:#}"
            );
            continue;
        }
        let segments =
            ordinary_bounds_segments(player_shape(&restored), Matrix::IDENTITY, [0; 3]).unwrap();
        assert_eq!(segments[0][1][0], value, "bounds endpoint, msize.x={value}");
        assert_eq!(segments[1][0][0], value, "bounds endpoint, msize.x={value}");
    }
}

/// A box whose near face lies 30 map units in front of the player at (522, 542).
fn add_wall(scene: &mut cyrus_script::player::data::PlayerScene) {
    use cyrus_assets::collision::{
        CollisionGeometry, CollisionModel, CollisionSubobject, StaticPlacement,
    };
    use cyrus_collision::collision_world::StaticCollision;
    scene.collision.placements.push(StaticPlacement {
        id: 300,
        model: "WALL".into(),
        position: [512, 0, 512],
        rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
    });
    scene.collision.models.insert(
        "WALL".into(),
        CollisionModel {
            flags: 0,
            geometry: CollisionGeometry {
                version: Some(*b"v5.0"),
                radius: 300.,
                vertices: vec![
                    [-100., -200., 60.],
                    [100., -200., 60.],
                    [100., 200., 60.],
                    [-100., 200., 60.],
                    [-100., -200., 160.],
                    [100., -200., 160.],
                    [100., 200., 160.],
                    [-100., 200., 160.],
                ],
                normals: vec![
                    [0., 0., -1.],
                    [0., -1., 0.],
                    [0., 0., 1.],
                    [0., 1., 0.],
                    [-1., 0., 0.],
                    [1., 0., 0.],
                ],
                faces: vec![
                    vec![0, 3, 2, 1],
                    vec![0, 1, 5, 4],
                    vec![4, 5, 6, 7],
                    vec![3, 7, 6, 2],
                    vec![0, 4, 7, 3],
                    vec![1, 2, 6, 5],
                ],
                textures: Some(vec![None; 6]),
                subobjects: vec![CollisionSubobject {
                    center: [0., 0., 100.],
                    radius: 300.,
                    extents: Some([200.; 3]),
                    faces: (0..6).collect(),
                }],
            },
        },
    );
    scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
}

/// Primary body contacts of the selected player at (522, 542) against a box whose near face lies
/// 30 map units in front of it, after the script wrote `size.pos.vz = center_z`.
fn body_contacts_with_center(center_z: i32) -> (usize, usize) {
    body_contacts(center_z, None)
}

/// [`body_contacts_with_center`] with an optional cached link position on the player.
fn body_contacts(center_z: i32, link: Option<[i32; 3]>) -> (usize, usize) {
    use cyrus_script::script::OrdinaryResidualQuery;
    let (world, restored, mut scene, data, _) = write_extent(Property::SizePosVz, center_z);
    add_wall(&mut scene);
    let count = |mut world: ScriptWorld| {
        let index = world
            .actors()
            .iter()
            .position(|a| matches!(a.source, ScriptActorSource::Player))
            .unwrap();
        let player = world.id(index).unwrap();
        let map = world.id(0).unwrap();
        world
            .place_selected_player([522 * 256, -1536, 542 * 256], 0)
            .unwrap();
        world.refresh_model_presentations(&data, &[player]).unwrap();
        if let Some(link) = link {
            world.set_link_position_for_test(player, link);
        }
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [522, 542], &[player, map])
            .unwrap();
        world
            .query_ordinary_residual_body(
                player,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                },
            )
            .unwrap()
            .len()
    };
    (count(world), count(restored))
}

#[test]
fn center_offset_moves_the_residual_body_query_origin() {
    let (centered, centered_restored) = body_contacts_with_center(0);
    let (forward, forward_restored) = body_contacts_with_center(30);
    let (back, _) = body_contacts_with_center(-30);
    assert_eq!(
        centered, 0,
        "the wall is out of reach of the unshifted body"
    );
    assert!(
        forward > 0,
        "a +30 center offset puts the body on the wall face"
    );
    assert_eq!(back, 0, "a -30 center offset moves it away");
    assert_eq!(
        (centered_restored, forward_restored),
        (centered, forward),
        "restore"
    );
}

#[test]
fn a_linked_actors_residual_body_query_reads_its_link_position() {
    // The wall touches the body at the written center offset; a link position far away moves the
    // query origin there, as the original's query-position selector does.
    let (plain, _) = body_contacts(30, None);
    let (linked, _) = body_contacts(30, Some([-4000 * 256, 0, -4000 * 256]));
    assert!(plain > 0);
    assert_eq!(linked, 0, "the linked body is nowhere near the wall");
}

/// An armed actor's wall avoidance, with the player standing 12 map units from the wall face
/// and facing it: the front probe finds the wall and the actor steps aside and back.
fn avoidance_after(marks: u32) -> ([i32; 3], (i32, u32, i32)) {
    use cyrus_script::script::OrdinaryGroundQuery;
    let (_, mut world, mut scene, data, _) = write_extent(Property::SizePosVz, 0);
    add_wall(&mut scene);
    let index = world
        .actors()
        .iter()
        .position(|a| matches!(a.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    let map = world.id(0).unwrap();
    world
        .place_selected_player([522 * 256, -1536, 560 * 256], 0)
        .unwrap();
    world.refresh_model_presentations(&data, &[player]).unwrap();
    world.set_combat_record_for_test(player);
    world.set_last_collide_for_test(player, marks);
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [522, 560], &[player, map])
        .unwrap();
    world
        .avoid_walls(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            1,
            256,
        )
        .unwrap();
    let combat = world.actors()[index]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    (
        world.spatial(player).unwrap().local.position,
        (combat.avoid_side, combat.avoid_mask, combat.avoid_hold),
    )
}

#[test]
fn an_armed_actor_steps_aside_and_back_from_a_wall_its_front_probe_finds() {
    let start = [522 * 256, -1536, 560 * 256];
    // Without a collision-response mark or a running hold timer the pass does nothing.
    assert_eq!(avoidance_after(0), (start, (0, 0, 0)));
    // A mark starts the probe: the front point meets the wall (mask 1), the first side is 2, and the
    // actor moves 10 map units along its heading rotated by -0x200, then 10 along 0x400.
    let (position, state) = avoidance_after(1);
    assert_eq!(state, (2, 1, 0x10));
    assert_eq!(position, [start[0] + 2560, start[1], start[2] - 2560]);
}
