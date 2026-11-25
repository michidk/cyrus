use super::*;

#[test]
fn ordinary_actor_overlap_reads_current_local_pose_and_excludes_self() {
    use cyrus_script::script::{ScriptActorId, ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::MoveByAxis,
        param_count: 3,
        kind: CallKind::Task,
    });
    let mut steps = BTreeMap::new();
    for (offset, delta, target) in [(0, 83 * 256, 2), (2, 256, 2)] {
        steps.insert(
            offset,
            Step {
                next: offset + 1,
                operation: wait(
                    1,
                    Native::MoveByAxis,
                    CallKind::Multitask,
                    vec![
                        Value::Integer { value: 0 },
                        Value::Integer { value: delta },
                        Value::Integer { value: 1 },
                    ],
                ),
            },
        );
        steps.insert(
            offset + 1,
            Step {
                next: offset + 2,
                operation: Operation::End { target },
            },
        );
    }
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    assert_eq!(
        world.query_ordinary_actor_overlap(id, id, &data).unwrap(),
        0
    );
    assert_eq!(
        world
            .query_ordinary_actor_overlap(id, other, &data)
            .unwrap(),
        84
    );
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    for expected in [1, 0] {
        world
            .advance(
                other,
                ScriptStage::Body(Thread::Main),
                inputs,
                None,
                None,
                20,
            )
            .unwrap();
        // The model presentation still has its initial pose. Overlap follows
        // current logical coordinates independently of stored face geometry.
        assert_eq!(
            world
                .model_presentation(other)
                .unwrap()
                .unwrap()
                .pose
                .position[0],
            10 * 256
        );
        assert_eq!(
            world
                .query_ordinary_actor_overlap(id, other, &data)
                .unwrap(),
            expected
        );
    }
    assert!(
        world
            .query_ordinary_actor_overlap(
                id,
                ScriptActorId {
                    generation: 2,
                    index: 0
                },
                &data
            )
            .is_err()
    );
}

#[test]
fn ordinary_initial_overlap_shape_preserves_fractional_rotated_centers() {
    use cyrus_assets::{animation::Animations, collision::ActorCollisionModels};
    use cyrus_collision::actor_models::ActorModelCollision;
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
        placement.angles = [256, 0, 0];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "ravc_count":0,"mpsz_bounds_primary":0,"raex":{"unknown_12":0,"v_vertex":0,"v_size":0}}],
            "mpsz_entries":[{"total_extent":[48,128,49],"pos_extent":[24,62,24],
                "neg_extent":[24,66,25],"center_offset":[0,0,0]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let models = ActorModelCollision::new(&ActorCollisionModels {
        object_count: 2,
        collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
        ..Default::default()
    })
    .unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &models).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 50;
    let id = world.id(0).unwrap();
    world
        .prepare_ordinary_collision_shape(id, &data, &scene)
        .unwrap();
    let spheres = world.ordinary_overlap_spheres(id, &data).unwrap();
    assert_eq!(spheres.len(), 2);
    assert_eq!(spheres[0].radius, 24.0);
    assert_eq!(spheres[0].center[0], 10.0);
    assert!((spheres[0].center[1] - (-9.698486)).abs() < 0.00001);
    assert!((spheres[0].center[2] - 59.698486).abs() < 0.00001);
    assert_ne!(spheres[0].center[1], spheres[0].center[1].round());
    data.metadata["actors"][0]["ravc_count"] = 1.into();
    assert!(
        world
            .ordinary_overlap_spheres(id, &data)
            .unwrap_err()
            .to_string()
            .contains("RAVC records unavailable")
    );
}

#[test]
fn ordinary_overlap_weights_preserve_static_pair_and_small_contact_rules() {
    use cyrus_script::script::{OrdinaryOverlapMotion as Motion, ordinary_overlap_motion};
    for (weight, other, distance, expected) in [
        (128, Some(128), 0, Motion::NoContact),
        (0, Some(0), 2, Motion::Touch),
        (0, None, 2, Motion::Rollback),
        (
            128,
            None,
            2,
            Motion::Separate {
                actor: 767,
                other: 0,
            },
        ),
        (0, Some(0), 3, Motion::Rollback),
        (
            128,
            Some(128),
            10,
            Motion::Separate {
                actor: 1535,
                other: 1535,
            },
        ),
        (
            128,
            None,
            10,
            Motion::Separate {
                actor: 2815,
                other: 0,
            },
        ),
        (
            0,
            Some(128),
            10,
            Motion::Separate {
                actor: 0,
                other: 1107,
            },
        ),
        (
            128,
            Some(0),
            10,
            Motion::Separate {
                actor: 1107,
                other: 0,
            },
        ),
        (
            255,
            Some(255),
            10,
            Motion::Separate {
                actor: 1535,
                other: 1535,
            },
        ),
        (
            128,
            None,
            16384,
            Motion::Separate {
                actor: -4194049,
                other: 0,
            },
        ),
    ] {
        assert_eq!(
            ordinary_overlap_motion(weight, other, distance).unwrap(),
            expected,
            "{weight}/{other:?}/{distance}"
        );
    }
    assert!(ordinary_overlap_motion(128, Some(128), -1).is_err());
}

pub(super) fn overlap_response_fixture(
    weight: u8,
    other_weight: u8,
    offset: i32,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    overlap_response_fixture_with(weight, other_weight, offset, 8)
}

pub(super) fn overlap_response_fixture_with(
    weight: u8,
    other_weight: u8,
    offset: i32,
    collide: u8,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    overlap_response_fixture_state(weight, other_weight, offset, collide, LivingState::ORDINARY)
}

fn overlap_response_fixture_state(
    weight: u8,
    other_weight: u8,
    offset: i32,
    collide: u8,
    living_state: LivingState,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    overlap_response_fixture_tweaked(weight, other_weight, offset, collide, living_state, |_| {})
}

pub(super) fn overlap_response_fixture_tweaked(
    weight: u8,
    other_weight: u8,
    offset: i32,
    collide: u8,
    living_state: LivingState,
    tweak: impl FnOnce(&mut Scripts),
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    overlap_response_fixture_edited(
        weight,
        other_weight,
        offset,
        collide,
        living_state,
        tweak,
        &[],
    )
}

fn overlap_response_fixture_edited(
    weight: u8,
    other_weight: u8,
    offset: i32,
    collide: u8,
    living_state: LivingState,
    tweak: impl FnOnce(&mut Scripts),
    control_edits: &[(&str, serde_json::Value)],
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::MoveByAxis,
        param_count: 3,
        kind: CallKind::Task,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables = vec![vec![4]];
    actor.attributes[Attribute::Weight] = weight;
    actor.attributes[Attribute::Collide] = collide;
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(
                    1,
                    Native::MoveByAxis,
                    CallKind::Multitask,
                    vec![
                        Value::Integer { value: 0 },
                        Value::Integer {
                            value: offset * 256,
                        },
                        Value::Integer { value: 1 },
                    ],
                ),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    let mut other = actor.clone();
    other.instance_variables = vec![vec![9]];
    other.attributes[Attribute::Weight] = other_weight;
    scripts.actors.insert("OTHER".into(), other);
    scripts.instances[1].actor = "OTHER".into();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    tweak(&mut scripts);
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, data, _) = static_model_fixture(scripts.clone());
    if living_state != LivingState::ORDINARY || !control_edits.is_empty() {
        let collision = Default::default();
        let sources = cyrus_script::script::WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        saved["actors"][0]["control"]["living_state"] = living_state.byte().into();
        for (field, value) in control_edits {
            saved["actors"][0]["control"][*field] = value.clone();
        }
        world = ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            cyrus_script::script::WorldRestoreContext {
                generation: 2,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
    }
    let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
    world.refresh_model_presentations(&data, &ids).unwrap();
    world
        .advance(
            ids[1],
            cyrus_script::script::ScriptStage::Body(Thread::Main),
            cyrus_script::script::ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    (world, data)
}

#[test]
fn ordinary_overlap_response_moves_logical_actors_and_preserves_model_records() {
    use cyrus_script::script::OrdinaryOverlapMotion as Motion;
    // Inferred from initialized quarter-turn residuals and stored forward translation.
    for (
        weight,
        other_weight,
        expected_actor,
        expected_z,
        expected_other,
        actor_distance,
        other_distance,
    ) in [
        (128, 128, 1025, 7679, 23039, 1535, 1535),
        (128, 0, 1453, 7679, 21504, 1107, 0),
        (0, 128, 2560, 7680, 22611, 0, 1107),
    ] {
        let (mut world, data) = overlap_response_fixture(weight, other_weight, 74);
        let id = world.id(0).unwrap();
        let other = world.id(1).unwrap();
        assert_eq!(
            world
                .resolve_ordinary_actor_overlap(id, other, &data)
                .unwrap(),
            Motion::Separate {
                actor: actor_distance,
                other: other_distance
            }
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [expected_actor, 5120, expected_z]
        );
        assert_eq!(
            world.spatial(other).unwrap().local.position,
            [expected_other, 5120, 7680]
        );
        for actor in [id, other] {
            assert_eq!(
                world
                    .model_presentation(actor)
                    .unwrap()
                    .unwrap()
                    .pose
                    .position,
                [2560, 5120, 7680]
            );
            assert_eq!(
                world.actors()[actor.index]
                    .control_state()
                    .unwrap()
                    .last_collide
                    .bits(),
                0x400
            );
        }
        assert_eq!(
            world.actors()[id.index]
                .control_state()
                .unwrap()
                .overlap_contact_identifier,
            37
        );
        assert_eq!(
            world.actors()[other.index]
                .control_state()
                .unwrap()
                .overlap_contact_identifier,
            0
        );
    }
}

#[test]
fn ordinary_overlap_touch_ends_pass_and_immovable_pair_rolls_back_without_new_actor_mark() {
    use cyrus_script::script::{OrdinaryOverlapMotion as Motion, ScriptInputs, ScriptStage};
    let (mut world, data) = overlap_response_fixture(0, 0, 83);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    let before = [
        world.spatial(id).unwrap().local.position,
        world.spatial(other).unwrap().local.position,
    ];
    assert_eq!(
        world
            .resolve_ordinary_actor_overlap(id, other, &data)
            .unwrap(),
        Motion::Touch
    );
    assert_eq!(
        [
            world.spatial(id).unwrap().local.position,
            world.spatial(other).unwrap().local.position
        ],
        before
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide
            .bits(),
        0x400
    );
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .last_collide
            .bits(),
        0x400
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .overlap_contact_identifier,
        37
    );
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .overlap_contact_identifier,
        36
    );
    let (mut world, data) = overlap_response_fixture(0, 0, 74);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world.snapshot_spatial(id).unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.spatial(id).unwrap().local.position[0], 21504);
    assert_eq!(
        world
            .resolve_ordinary_actor_overlap(id, other, &data)
            .unwrap(),
        Motion::Rollback
    );
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 5120, 7680]
    );
    assert_eq!(
        world.spatial(other).unwrap().local.position,
        [21504, 5120, 7680]
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide
            .bits(),
        0
    );
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .last_collide
            .bits(),
        0x400
    );
}

pub(super) fn static_overlap_scene(world: u32) -> cyrus_script::player::data::PlayerScene {
    use cyrus_assets::collision::{
        CollisionGeometry, CollisionModel, CollisionScene, StaticPlacement,
    };
    use cyrus_collision::collision_world::StaticCollision;
    let (_, _, models) = static_model_fixture(cache());
    let mut scene = ordinary_collision_scene(world, models);
    scene.collision = CollisionScene {
        sphere_object_scale: Some(256),
        placements: [604, 605]
            .into_iter()
            .enumerate()
            .map(|(id, x)| StaticPlacement {
                id: id as u32,
                model: "SPHERE".into(),
                position: [x, 20, 30],
                rotation: [0; 9],
            })
            .collect(),
        models: BTreeMap::from([(
            "SPHERE".into(),
            CollisionModel {
                flags: 1,
                geometry: CollisionGeometry {
                    version: Some(*b"v5.0"),
                    radius: 42.0,
                    vertices: vec![],
                    normals: vec![],
                    faces: vec![],
                    textures: None,
                    subobjects: vec![],
                },
            },
        )]),
        ..Default::default()
    };
    scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
    scene
}

#[test]
fn ordinary_static_overlap_keeps_reverse_placement_order_and_tiny_response() {
    use cyrus_collision::candidates::{ActorCandidate, CandidateOwner, ModelInstance};
    use cyrus_script::script::{
        OrdinaryOverlapMotion as Motion, OrdinaryOverlapPass as Pass, ScriptInputs, ScriptStage,
    };
    for weight in [128, 0] {
        let (mut world, data) = overlap_response_fixture(weight, 0, 512);
        let id = world.id(0).unwrap();
        world.snapshot_spatial(id).unwrap();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position[0], 133632);
        let scene = static_overlap_scene(world.world);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [522, 30], &[id])
            .unwrap();
        let candidate = ActorCandidate {
            owner: CandidateOwner::Static(1),
            model: Some(ModelInstance::StaticPlacement(1)),
            collision_flags: 1,
        };
        if weight == 128 {
            assert_eq!(
                world
                    .resolve_ordinary_overlaps(id, &scene, &data, &prepared)
                    .unwrap(),
                Pass::Complete {
                    needs_polygon_repair: true
                }
            );
            assert_eq!(
                world.spatial(id).unwrap().local.position,
                [133121, 5120, 7680]
            );
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .last_collide
                    .bits(),
                0x400
            );
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .overlap_contact_identifier,
                0
            );
        } else {
            assert_eq!(
                world
                    .resolve_ordinary_static_overlap(id, &scene, &data, &prepared, &candidate)
                    .unwrap(),
                Motion::Rollback
            );
            assert_eq!(
                world.spatial(id).unwrap().local.position,
                [2560, 5120, 7680]
            );
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .last_collide
                    .bits(),
                0
            );
        }
        let foreign = ActorCandidate {
            owner: CandidateOwner::Static(2),
            model: Some(ModelInstance::StaticPlacement(2)),
            collision_flags: 1,
        };
        assert!(
            world
                .resolve_ordinary_static_overlap(id, &scene, &data, &prepared, &foreign)
                .is_err()
        );
    }
}

#[test]
fn ordinary_overlap_pass_stops_at_first_tiny_pair_before_unavailable_later_shape() {
    use cyrus_assets::{
        animation::{AnimationModel, Animations},
        collision::ActorCollisionModels,
    };
    use cyrus_collision::actor_models::ActorModelCollision;
    use cyrus_script::script::OrdinaryOverlapPass as Pass;
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Weight] = 128;
    let mut later = scripts.actors["LOGIC"].clone();
    later.instances = 1;
    later.instance_variables = vec![vec![4]];
    scripts.actors.insert("LATER".into(), later);
    let mut placement = scripts.instances[0].clone();
    placement.object_id = 2;
    placement.actor = "LATER".into();
    scripts.instances.push(placement);
    for (placement, x) in scripts.instances.iter_mut().zip([10, 33, 20]) {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [x, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": (["LOGIC", "LATER"].map(|name| serde_json::json!({
                "script_name":name, "model_load_path":"animation_table",
                "ravc_count": if name == "LATER" {1} else {0},
                "raan_entries":[{"file_path":"MOVING.3D", "frame_count":1}],
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}))),
            "mpob_objects": scripts.instances.iter().map(|p| serde_json::json!({"script_name":p.actor,"object_type":1})).collect::<Vec<_>>()}),
        models: BTreeMap::from([(
            "MOVING".into(),
            AnimationModel {
                radius: Some(42.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i16".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        )]),
        sections: Default::default(),
        unsupported: Default::default(),
    };
    let models = ActorModelCollision::new(&ActorCollisionModels {
        object_count: 3,
        collision_flags: BTreeMap::from([(0, 0), (1, 0), (2, 0)]),
        ..Default::default()
    })
    .unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &models).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let ids = [
        world.id(0).unwrap(),
        world.id(1).unwrap(),
        world.id(2).unwrap(),
    ];
    for id in ids {
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        world
            .prepare_ordinary_collision_shape(id, &data, &scene)
            .unwrap();
    }
    world.refresh_model_presentations(&data, &ids).unwrap();
    assert!(world.ordinary_overlap_spheres(ids[2], &data).is_err());
    let first = world
        .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
        .unwrap();
    assert_eq!(
        world
            .resolve_ordinary_overlaps(ids[0], &scene, &data, &first)
            .unwrap(),
        Pass::Touch
    );
    let reversed = world
        .prepare_ordinary_collision_query(
            ids[0],
            &scene,
            &data,
            [10, 30],
            &[ids[0], ids[2], ids[1]],
        )
        .unwrap();
    assert!(
        world
            .resolve_ordinary_overlaps(ids[0], &scene, &data, &reversed)
            .unwrap_err()
            .to_string()
            .contains("RAVC records unavailable")
    );
}

#[test]
fn a_falling_actor_lands_on_a_bouncy_actor_through_the_overlap_pass() {
    use cyrus_assets::{
        animation::{AnimationModel, Animations},
        collision::ActorCollisionModels,
    };
    use cyrus_collision::actor_models::ActorModelCollision;
    use cyrus_script::script::OrdinaryOverlapPass as Pass;
    for (misc_group, publish_model) in [(true, false), (true, true), (false, true)] {
        let mut scripts = cache();
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Weight] = 128;
        let mut other = scripts.actors["LOGIC"].clone();
        other.instances = 1;
        other.instance_variables = vec![vec![4]];
        other.attributes[Attribute::BouncePower] = 140;
        scripts.actors.insert("BOUNCY".into(), other);
        scripts.instances[1].actor = "BOUNCY".into();
        scripts.updates.fall_landing = Some(cyrus_assets::script::FallLanding {
            death_height: 568,
            hurt_height: 300,
            hurt_zap: 10,
            bounce_height: Some(100),
        });
        for (placement, x) in scripts.instances.iter_mut().zip([10, 16]) {
            placement.object_type = PlacementType::MODEL;
            placement.map_position = [x, 20, 30];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let group = |index: u16| {
            serde_json::json!({"group_index":index,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]})
        };
        let data = Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": (["LOGIC", "BOUNCY"].map(|name| serde_json::json!({
                "script_name":name, "model_load_path":"animation_table",
                "ravc_count": 0,
                "raan_entries":[{"file_path":"MOVING.3D", "frame_count":1}],
                "animation_groups": if misc_group { vec![group(0), group(10)] } else { vec![group(0)] }}))),
            "mpob_objects": scripts.instances.iter().map(|p| serde_json::json!({"script_name":p.actor,"object_type":1})).collect::<Vec<_>>()}),
            models: BTreeMap::from([(
                "MOVING".into(),
                AnimationModel {
                    radius: Some(42.0),
                    base_positions: vec![],
                    base_normals: vec![],
                    faces: vec![],
                    corner_normals: vec![],
                    encoding: "animated_i16".into(),
                    positions: vec![vec![]],
                    normals: vec![vec![]],
                },
            )]),
            sections: Default::default(),
            unsupported: Default::default(),
        };
        let models = ActorModelCollision::new(&ActorCollisionModels {
            object_count: 2,
            collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
            ..Default::default()
        })
        .unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        world.prepare_model_resources(&data, &models).unwrap();
        let scene = ordinary_collision_scene(world.world, models);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        for id in ids {
            world
                .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
                .unwrap();
            world
                .prepare_ordinary_collision_shape(id, &data, &scene)
                .unwrap();
        }
        if publish_model {
            world.refresh_model_presentations(&data, &ids).unwrap();
        }
        assert_eq!(
            world.model_presentation(ids[1]).unwrap().is_some(),
            publish_model
        );
        world.set_actor_surface(ids[0], Surface::UNKNOWN).unwrap();
        let y = world.spatial(ids[0]).unwrap().local.position[1];
        world.set_falling_for_test(ids[0], JumpMotion::STANDING, y - 101 * 256);
        world.set_action_words_for_test(ids[0], 0x13, 0x21);
        if !publish_model {
            assert_eq!(
                world
                    .resolve_ordinary_actor_overlap(ids[0], ids[1], &data)
                    .unwrap(),
                cyrus_script::script::OrdinaryOverlapMotion::Landing { admitted: false }
            );
            assert_eq!(
                world.actors()[0].control_state().unwrap().falling_state,
                JumpMotion::STANDING
            );
            assert_eq!(world.actors()[0].support_actor(), None);
            world
                .set_model_support(
                    ids[0],
                    cyrus_script::script::ModelSupportOwners {
                        actor: Some(ids[1]),
                        static_placement: None,
                    },
                )
                .unwrap();
            world.set_living_state_for_test(ids[0], LivingState::LANDED_ON_ACTOR);
            assert!(!world.bounce_handler_prefix_for_test(ids[0]).unwrap());
            assert_eq!(world.actors()[0].support_actor(), None);
            assert_eq!(
                world.actors()[0].control_state().unwrap().living_state,
                LivingState::ORDINARY
            );
            eprintln!(
                "actor landing misc_group={misc_group} model=false: admitted=false falling=1 stale_support_cleared=true"
            );
            continue;
        }
        let query = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
            .unwrap();
        assert_eq!(
            world
                .resolve_ordinary_overlaps(ids[0], &scene, &data, &query)
                .unwrap(),
            Pass::Landed {
                other: ids[1],
                admitted: true
            }
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.actions.pending, control.actions.accepted),
            (0, 0),
            "an admitted landing clears both action words"
        );
        assert_eq!(
            (
                control.living_state,
                control.falling_state,
                control.queued_motion
            ),
            (
                LivingState::LANDED_ON_ACTOR,
                JumpMotion::NONE,
                JumpMotion::NONE
            )
        );
        assert!(
            control
                .last_collide
                .contains(CollisionFlags::LANDED_ON_ACTOR)
        );
        assert_eq!(world.actors()[0].support_actor(), Some(ids[1]));
        assert!(!world.bounce_handler_prefix_for_test(ids[0]).unwrap());
        assert_eq!(world.actors()[0].support_actor(), Some(ids[1]));
        assert_eq!(
            world.actors()[0].control_state().unwrap().living_state,
            LivingState::LANDED_ON_ACTOR
        );
        eprintln!(
            "actor landing misc_group={misc_group} model={publish_model}: admitted=true living=27 falling=0 actions=0 support=1"
        );
        // Living state 27 refuses a second landing; the pass still ends, and nothing else changes.
        world.set_falling_for_test(ids[0], JumpMotion::STANDING, y - 101 * 256);
        world.set_action_words_for_test(ids[0], 0x13, 0x21);
        let query = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
            .unwrap();
        assert_eq!(
            world
                .resolve_ordinary_overlaps(ids[0], &scene, &data, &query)
                .unwrap(),
            Pass::Landed {
                other: ids[1],
                admitted: false
            }
        );
        assert_eq!(
            world.actors()[0].control_state().unwrap().falling_state,
            JumpMotion::STANDING
        );
        let words = world.actors()[0].control_state().unwrap().actions;
        assert_eq!(
            (words.pending, words.accepted),
            (0x13, 0x21),
            "a refused landing leaves the action words"
        );
        // A shorter fall than the installed bounce height takes the weighted response instead.
        world.set_falling_for_test(ids[0], JumpMotion::STANDING, y - 99 * 256);
        let query = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
            .unwrap();
        assert!(matches!(
            world
                .resolve_ordinary_overlaps(ids[0], &scene, &data, &query)
                .unwrap(),
            Pass::Complete { .. }
        ));
    }
}

#[test]
fn grounded_overlap_admits_every_living_state_and_preserves_support_contact_order() {
    use cyrus_script::script::{ModelSupportOwners, OrdinaryOverlapMotion as Motion};
    for state in (0..=255).map(LivingState::from_byte) {
        for (offset, supported) in [(74, false), (74, true), (83, true)] {
            let (mut world, data) = overlap_response_fixture_state(128, 128, offset, 8, state);
            let id = world.id(0).unwrap();
            let other = world.id(1).unwrap();
            if supported {
                world
                    .set_model_support(
                        id,
                        ModelSupportOwners {
                            actor: Some(other),
                            static_placement: None,
                        },
                    )
                    .unwrap();
            }
            let positions = [
                world.spatial(id).unwrap().local.position,
                world.spatial(other).unwrap().local.position,
            ];
            let bypass = supported && state.is_bounce() && offset == 74;
            assert_eq!(
                world
                    .resolve_ordinary_actor_overlap(id, other, &data)
                    .unwrap(),
                if offset == 83 {
                    Motion::Touch
                } else if bypass {
                    Motion::NoContact
                } else {
                    Motion::Separate {
                        actor: 1535,
                        other: 1535,
                    }
                },
                "state {state}, offset {offset}, support {supported}"
            );
            let own = world.actors()[0].control_state().unwrap();
            let other_control = world.actors()[1].control_state().unwrap();
            assert_eq!(own.last_collide.bits(), if bypass { 0 } else { 0x400 });
            assert_eq!(other_control.last_collide.bits(), 0x400);
            if bypass || offset == 83 {
                assert_eq!(
                    [
                        world.spatial(id).unwrap().local.position,
                        world.spatial(other).unwrap().local.position
                    ],
                    positions
                );
            }
            if offset == 83 {
                assert_eq!(
                    (
                        own.overlap_contact_identifier,
                        other_control.overlap_contact_identifier
                    ),
                    (37, 36)
                );
            } else if bypass {
                assert_eq!(
                    (
                        own.overlap_contact_identifier,
                        other_control.overlap_contact_identifier
                    ),
                    (0, 0)
                );
            }
        }
    }
}

#[test]
fn grounded_static_overlap_uses_weighted_response_for_every_living_state() {
    use cyrus_collision::candidates::{ActorCandidate, CandidateOwner, ModelInstance};
    use cyrus_script::script::{OrdinaryOverlapMotion as Motion, ScriptInputs, ScriptStage};
    for state in (0..=255).map(LivingState::from_byte) {
        let (mut world, data) = overlap_response_fixture_state(128, 0, 512, 8, state);
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        let scene = static_overlap_scene(world.world);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [522, 30], &[id])
            .unwrap();
        let candidate = ActorCandidate {
            owner: CandidateOwner::Static(1),
            model: Some(ModelInstance::StaticPlacement(1)),
            collision_flags: 1,
        };
        assert_eq!(
            world
                .resolve_ordinary_static_overlap(id, &scene, &data, &prepared, &candidate)
                .unwrap(),
            Motion::Separate {
                actor: 511,
                other: 0
            },
            "state {state}"
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [133121, 5120, 7680]
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            0x400
        );
    }
}

#[test]
fn standalone_player_combat_retains_installed_contact_profile() {
    use cyrus_assets::animation::CombatContactProfile;
    let (scripts, _, mut data, _) = player_walking_fixture();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let player = world
        .actors()
        .iter()
        .position(|actor| actor.name() == "CYRUS")
        .unwrap();
    let id = world.id(player).unwrap();
    for (vertex, size, radius) in [(-7, 100, 25600), (9, -1, -256)] {
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let definition = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "CYRUS")
            .unwrap();
        definition["raex"]["v_vertex"] = vertex.into();
        definition["raex"]["v_size"] = size.into();
        world
            .initialize_player_animation(id, &data, 0, &mut |_| Ok(()))
            .unwrap();
        assert_eq!(
            world.actors()[player]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .contact_profile,
            CombatContactProfile { vertex, radius }
        );
    }
}

// Saved session owners establish the fixture without adding runtime setters.
fn selected_overlap_fixture(
    lineup: Option<i32>,
    lockout: Option<u16>,
    other_weight: u8,
    offset: i32,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    use cyrus_game::{
        preferences::{PreferenceKey, PreferenceStore, Preferences},
        random::EngineRandom,
    };
    use cyrus_script::{
        clock::SimulationClock,
        player::input::PlayerToggles,
        script::{
            LogicalSessionRestoreInputs, LogicalSessionSnapshot, ScriptActors, ScriptHud,
            WorldSnapshotSources,
        },
    };
    struct Store;
    impl PreferenceStore for Store {
        fn load(&self, _: PreferenceKey) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
        fn save(&self, _: PreferenceKey, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let (scripts, mut world, mut data, mut scene) = player_walking_fixture();
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let player = world.selected_player().unwrap();
    world
        .prepare_ordinary_collision_shape(player, &data, &scene)
        .unwrap();
    let collision = Default::default();
    let sources = BTreeMap::from([(
        scripts.world,
        WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        },
    )]);
    let asset = inventory_asset(&[]);
    let preferences = Preferences::new(Store);
    let hud = ScriptHud::default();
    hud.configure(&asset, &preferences);
    let mut actors = ScriptActors::default();
    actors.current = Some(world);
    let mut saved = serde_json::to_value(
        actors
            .snapshot_session(
                &sources,
                &hud,
                &SimulationClock::new(scripts.clock).unwrap(),
                &EngineRandom::from_state(42),
                PlayerToggles {
                    walk_mode: true,
                    auto_defend: false,
                },
            )
            .unwrap(),
    )
    .unwrap();
    saved["registry"]["globals"]["lineup_result"] = serde_json::to_value(lineup).unwrap();
    saved["registry"]["globals"]["lockout"] = serde_json::to_value(lockout).unwrap();
    let saved_world = &mut saved["registry"]["worlds"][0];
    for (index, position, weight) in [
        (player.index, [133632, 5120, 7680], 128),
        (0, [133632, 5120, 7680 + offset * 256], other_weight),
    ] {
        for attribute in [Attribute::Weight, Attribute::ReservedWeight] {
            saved_world["actors"][index]["attributes"][attribute.index()] = weight.into();
        }
        let spatial = &mut saved_world["spatial"]["actors"][index];
        spatial["local"]["position"] = serde_json::json!(position);
        if spatial["effective"].is_object() {
            spatial["effective"]["position"] = serde_json::json!(position);
        }
        spatial["previous_local_position"] =
            serde_json::json!([position[0] - 256, position[1], position[2]]);
    }
    let mut world = serde_json::from_value::<LogicalSessionSnapshot>(saved)
        .unwrap()
        .plan(LogicalSessionRestoreInputs {
            sources: &sources,
            last_generation: 10,
            hud: &asset,
            preferences: &preferences,
            bios_tick: 100,
        })
        .unwrap()
        .finish(&(0..16).map(|slot| (slot, slot)).collect())
        .unwrap()
        .actors
        .current
        .unwrap();
    let player = world.selected_player().unwrap();
    world.refresh_model_presentations(&data, &[player]).unwrap();
    let definition = scene.definition.take();
    scene = static_overlap_scene(world.world);
    scene.definition = definition;
    scene.collision.placements[0].position = [574, 20, 30];
    scene.static_collision =
        cyrus_collision::collision_world::StaticCollision::new(&scene.collision).unwrap();
    (world, data, scene)
}

#[test]
fn selected_overlap_mobility_protects_both_pair_roles_without_changing_weights() {
    use cyrus_script::script::OrdinaryOverlapMotion as Motion;
    for (lineup, lockout) in [(0, 0), (1, 0), (-1, 0), (0, 2), (1, 1)] {
        for player_updates in [true, false] {
            let (mut world, data, _) =
                selected_overlap_fixture(Some(lineup), Some(lockout), 128, 50);
            let player = world.selected_player().unwrap();
            let other = world.id(0).unwrap();
            let before = world.spatial(player).unwrap().local.position;
            let (id, target) = if player_updates {
                (player, other)
            } else {
                (other, player)
            };
            let response = world
                .resolve_ordinary_actor_overlap(id, target, &data)
                .unwrap();
            let Motion::Separate { actor, other } = response else {
                panic!("{response:?}")
            };
            if lineup != 0 || lockout != 0 {
                assert_eq!(if player_updates { actor } else { other }, 0);
                assert!(if player_updates { other } else { actor } > 0);
                assert_eq!(world.spatial(player).unwrap().local.position, before);
            } else {
                assert!(actor > 0 && other > 0);
                assert_ne!(world.spatial(player).unwrap().local.position, before);
            }
            assert_eq!(
                world.actors()[player.index].attributes()[Attribute::Weight],
                128
            );
            assert_eq!(world.actors()[0].attributes()[Attribute::Weight], 128);
        }
    }
}

#[test]
fn selected_overlap_lineup_preserves_position_but_lockout_alone_rolls_back() {
    use cyrus_collision::candidates::{ActorCandidate, CandidateOwner, ModelInstance};
    use cyrus_script::script::OrdinaryOverlapMotion as Motion;
    for (lineup, lockout) in [(1, 0), (-1, 2), (0, 1), (0, 2)] {
        for static_contact in [false, true] {
            let (mut world, data, scene) =
                selected_overlap_fixture(Some(lineup), Some(lockout), 0, 50);
            let player = world.selected_player().unwrap();
            let current = world.spatial(player).unwrap().local.position;
            let previous = world.spatial(player).unwrap().previous_local_position;
            assert_ne!(current, previous);
            let response = if static_contact {
                let prepared = world
                    .prepare_ordinary_collision_query(player, &scene, &data, [522, 30], &[player])
                    .unwrap();
                let candidate = ActorCandidate {
                    owner: CandidateOwner::Static(0),
                    model: Some(ModelInstance::StaticPlacement(0)),
                    collision_flags: 1,
                };
                world
                    .resolve_ordinary_static_overlap(player, &scene, &data, &prepared, &candidate)
                    .unwrap()
            } else {
                world
                    .resolve_ordinary_actor_overlap(player, world.id(0).unwrap(), &data)
                    .unwrap()
            };
            assert_eq!(response, Motion::Rollback);
            assert_eq!(
                world.spatial(player).unwrap().local.position,
                if lineup != 0 { current } else { previous }
            );
            assert_eq!(
                world.actors()[player.index].attributes()[Attribute::Weight],
                128
            );
        }
    }
}

#[test]
fn selected_overlap_small_contacts_precede_unavailable_global_owners() {
    use cyrus_script::script::OrdinaryOverlapMotion as Motion;
    let (mut world, data, _) = selected_overlap_fixture(None, None, 0, 52);
    let player = world.selected_player().unwrap();
    let other = world.id(0).unwrap();
    assert_eq!(
        world
            .query_ordinary_actor_overlap(player, other, &data)
            .unwrap(),
        2
    );
    let position = world.spatial(player).unwrap().local.position;
    assert_eq!(
        world
            .resolve_ordinary_actor_overlap(player, other, &data)
            .unwrap(),
        Motion::Touch
    );
    assert_eq!(world.spatial(player).unwrap().local.position, position);
    for index in [player.index, other.index] {
        assert_ne!(
            world.actors()[index]
                .control_state()
                .unwrap()
                .last_collide
                .bits()
                & 0x400,
            0
        );
    }
    let (mut world, data, _) = selected_overlap_fixture(None, Some(0), 128, 50);
    let player = world.selected_player().unwrap();
    let before = world.spatial(player).unwrap().local.position;
    let error = world
        .resolve_ordinary_actor_overlap(player, world.id(0).unwrap(), &data)
        .unwrap_err();
    assert!(format!("{error:#}").contains("overlap lineup result unavailable"));
    assert_eq!(world.spatial(player).unwrap().local.position, before);
}

#[test]
fn selected_overlap_protection_leaves_other_actor_rollback_and_unlocked_static_motion_intact() {
    use cyrus_collision::candidates::{ActorCandidate, CandidateOwner, ModelInstance};
    use cyrus_script::script::OrdinaryOverlapMotion as Motion;
    let (mut world, data, _) = selected_overlap_fixture(Some(1), Some(0), 0, 50);
    let player = world.selected_player().unwrap();
    let other = world.id(0).unwrap();
    let previous = world.spatial(other).unwrap().previous_local_position;
    assert_eq!(
        world
            .resolve_ordinary_actor_overlap(other, player, &data)
            .unwrap(),
        Motion::Rollback
    );
    assert_eq!(world.spatial(other).unwrap().local.position, previous);

    let (mut world, data, scene) = selected_overlap_fixture(Some(0), Some(0), 0, 50);
    let player = world.selected_player().unwrap();
    let before = world.spatial(player).unwrap().local.position;
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [522, 30], &[player])
        .unwrap();
    let candidate = ActorCandidate {
        owner: CandidateOwner::Static(0),
        model: Some(ModelInstance::StaticPlacement(0)),
        collision_flags: 1,
    };
    assert_eq!(
        world
            .resolve_ordinary_static_overlap(player, &scene, &data, &prepared, &candidate)
            .unwrap(),
        Motion::Separate {
            actor: 767,
            other: 0
        }
    );
    assert_ne!(world.spatial(player).unwrap().local.position, before);
}

#[test]
fn selected_overlap_fixture_retains_mobility_through_ordinary_updates() {
    use cyrus_script::{
        player::input::PlayerInput,
        script::{OrdinaryInputs, PlayerControllerInput, ScriptHud, ScriptInputs},
    };
    for weight in [0, 128] {
        let (mut world, data, scene) = selected_overlap_fixture(Some(0), Some(0), weight, 80);
        let player = world.selected_player().unwrap();
        let other = world.id(0).unwrap();
        let held = PlayerInput::default();
        let hud = ScriptHud::default();
        for (id, mask) in [(player, 11), (other, 0)] {
            world
                .set_actor_attribute((id).index, (Attribute::Collide).index(), mask)
                .unwrap();
            world
                .set_actor_attribute((id).index, (Attribute::ReservedCollide).index(), mask)
                .unwrap();
        }
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        for _ in 0..2 {
            world
                .refresh_model_presentations(&data, &[other, player])
                .unwrap();
            world
                .update_ordinary(
                    &data,
                    [522, 30],
                    OrdinaryInputs {
                        scripts: ScriptInputs {
                            hud: Some(&hud),
                            player_controller: Some(PlayerControllerInput {
                                held: &held,
                                walk_mode: false,
                                auto_defend: false,
                                steer: None,
                                claim: None,
                                probes: None,
                            }),
                            nominal_ticks: Some(1),
                            frame_scale: Some(256),
                            fine_ticks: Some(8),
                            step_fps: Some(12),
                            shell_execution: Some(false),
                            ..Default::default()
                        },
                        scene: Some(&scene),
                    },
                    &mut random,
                    &mut |_, _| Ok(()),
                    100,
                )
                .unwrap();
            for id in [player, other] {
                assert!(world.actors()[id.index].failure().is_none());
            }
            assert_eq!(
                world.actors()[player.index].attributes()[Attribute::Weight],
                128
            );
            assert_eq!(
                world.actors()[other.index].attributes()[Attribute::Weight],
                weight
            );
        }
    }
}
