//! Public actor overlap contracts exercise ordered and rounded world shapes.
use cyrus_collision::actor_overlap::{OverlapSphere, actor_separation, static_separation};

fn sphere(center: [f32; 3], radius: f32) -> OverlapSphere {
    OverlapSphere { center, radius }
}

#[test]
fn overlap_is_strict_in_three_dimensions_and_separates_horizontally() {
    let a = [sphere([0.0; 3], 5.0)];
    assert_eq!(
        actor_separation(&a, &[sphere([6.0, 8.0, 0.0], 5.0)]).unwrap(),
        0
    );
    assert_eq!(
        actor_separation(&a, &[sphere([6.0, 7.0, 0.0], 5.0)]).unwrap(),
        4
    );
    assert_eq!(
        actor_separation(&a, &[sphere([0.0, 9.0, 0.0], 5.0)]).unwrap(),
        10
    );
}

#[test]
fn summed_radius_and_center_differences_truncate_before_nearest_distance() {
    let a = [sphere([0.0; 3], 2.25)];
    assert_eq!(
        actor_separation(&a, &[sphere([3.5, 0.0, 0.0], 2.25)]).unwrap(),
        1
    );
    assert_eq!(
        actor_separation(&a, &[sphere([2.5, 0.0, 0.0], 2.25)]).unwrap(),
        2
    );
    assert_eq!(
        actor_separation(&a, &[sphere([2.5, 0.0, 0.0], 3.25)]).unwrap(),
        3
    );
}

#[test]
fn first_pair_wins_and_spheres_after_thirty_two_do_not_participate() {
    let left = [sphere([0.0; 3], 5.0), sphere([8.0, 0.0, 0.0], 5.0)];
    let right = [sphere([8.0, 0.0, 0.0], 5.0)];
    assert_eq!(actor_separation(&left, &right).unwrap(), 2);
    assert_eq!(actor_separation(&[left[1], left[0]], &right).unwrap(), 10);
    let mut many = vec![sphere([100.0; 3], 1.0); 32];
    many.push(sphere([8.0, 0.0, 0.0], 5.0));
    assert_eq!(actor_separation(&many, &right).unwrap(), 0);
    assert_eq!(actor_separation(&right, &many).unwrap(), 0);
}

#[test]
fn invalid_participating_shapes_report_errors_and_empty_shapes_miss() {
    let a = [sphere([0.0; 3], 1.0)];
    assert_eq!(actor_separation(&[], &a).unwrap(), 0);
    assert!(actor_separation(&a, &[sphere([f32::NAN, 0.0, 0.0], 1.0)]).is_err());
    assert!(actor_separation(&a, &[sphere([0.0; 3], -1.0)]).is_err());
    assert!(actor_separation(&a, &[sphere([f32::MAX, 0.0, 0.0], 1.0)]).is_err());
}

#[test]
fn static_placement_truncates_radii_independently_of_actor_pair_conversion() {
    let actor = [sphere([0.0; 3], 2.5)];
    let placement = sphere([4.0, 0.0, 0.0], 2.5);
    assert_eq!(static_separation(&actor, placement).unwrap(), 0);
    assert_eq!(actor_separation(&actor, &[placement]).unwrap(), 1);
    assert_eq!(
        static_separation(&actor, sphere([2.0, 1.0, 0.0], 2.5)).unwrap(),
        2
    );
}

#[test]
fn extended_difference_retains_a_small_offset_below_an_integer_boundary() {
    let actor = [sphere([1.0, 0.0, 0.0], 0.5)];
    let other = [sphere([1.0e-17, 0.0, 0.0], 0.5)];
    assert_eq!(actor_separation(&actor, &other).unwrap(), 1);
    assert_eq!(
        actor_separation(&actor, &[sphere([0.0; 3], 0.5)]).unwrap(),
        0
    );
}

#[test]
fn static_overlap_sphere_requires_retained_actor_style_identity() {
    use cyrus_assets::collision::{
        CollisionGeometry, CollisionModel, CollisionScene, StaticPlacement,
    };
    use cyrus_collision::{
        candidates::{ActorCandidate, CandidateOwner, ModelInstance},
        collision_world::StaticCollision,
    };
    let scene = CollisionScene {
        placements: vec![StaticPlacement {
            id: 0,
            model: "SPHERE".into(),
            position: [600, 25, 600],
            rotation: [0; 9],
        }],
        models: std::collections::BTreeMap::from([(
            "SPHERE".into(),
            CollisionModel {
                flags: 1,
                geometry: CollisionGeometry {
                    version: Some(*b"v5.0"),
                    radius: 42.5,
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
    let statics = StaticCollision::new(&scene).unwrap();
    let valid = ActorCandidate {
        owner: CandidateOwner::Static(0),
        model: Some(ModelInstance::StaticPlacement(0)),
        collision_flags: 1,
    };
    let sphere = statics.static_actor_overlap_sphere(&valid).unwrap();
    assert_eq!(sphere.center, [600.0, 25.0, 600.0]);
    assert_eq!(sphere.radius, 42.5);
    for candidate in [
        ActorCandidate {
            model: None,
            ..valid
        },
        ActorCandidate {
            owner: CandidateOwner::Actor(0),
            ..valid
        },
        ActorCandidate {
            model: Some(ModelInstance::Runtime(0)),
            ..valid
        },
        ActorCandidate {
            collision_flags: 0,
            ..valid
        },
    ] {
        assert!(statics.static_actor_overlap_sphere(&candidate).is_err());
    }
}
