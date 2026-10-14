//! Original actor shape construction, including horizontal and extra-sphere rules.
use cyrus_collision::actor_shape::{ActorBounds, BodySphere, Orientation};

#[test]
fn static_vertex_bounds_round_to_even_and_scale_only_horizontal_extents() {
    let vertices = [[2.5, 2.5, 5.5], [-5.5, -4.5, -7.5]];
    let bounds = ActorBounds::from_vertices(&vertices, 128, 128).unwrap();
    assert_eq!(bounds.positive_extent(), [1, 2, 3]);
    let shape = bounds.prepare(0, 0).unwrap();
    assert_eq!(shape.orientation, Orientation::Z);
    assert_eq!(
        shape.body,
        [BodySphere {
            center: [0, 0, 0],
            radius: 2
        }]
    );
    assert_eq!(
        ActorBounds::from_vertices(&vertices, 0, 0)
            .unwrap()
            .positive_extent(),
        [2, 2, 6]
    );
    assert!(ActorBounds::from_vertices(&[[f32::NAN, 0.0, 0.0]], 256, 256).is_err());
    assert!(ActorBounds::from_vertices(&[[2147483648.0, 0.0, 0.0]], 256, 256).is_err());
}

#[test]
fn upright_installed_dimensions_keep_the_additional_sphere_separate() {
    let shape = ActorBounds::new([24, 62, 24], [24, 66, 25], 0)
        .unwrap()
        .prepare(50, 1)
        .unwrap();
    assert_eq!(shape.orientation, Orientation::Upright);
    assert_eq!(
        shape.body,
        vec![
            BodySphere {
                center: [0, -42, 0],
                radius: 24
            },
            BodySphere {
                center: [0, -12, 0],
                radius: 24
            },
        ]
    );
    assert_eq!(
        shape.additional,
        BodySphere {
            center: [0, -220, 0],
            radius: 24
        }
    );
}

#[test]
fn x_dominant_shape_repeats_authored_centers_and_copies_the_first_sphere() {
    let shape = ActorBounds::new([50, 10, 15], [50, 10, 15], 128)
        .unwrap()
        .prepare(10, 1)
        .unwrap();
    assert_eq!(shape.orientation, Orientation::X);
    assert_eq!(
        shape.body,
        vec![
            BodySphere {
                center: [-40, 0, 0],
                radius: 5
            };
            6
        ]
    );
    assert_eq!(
        shape.additional,
        BodySphere {
            center: [-40, 0, 0],
            radius: 5
        }
    );
}

#[test]
fn z_dominant_shape_advances_unscaled_centers() {
    let shape = ActorBounds::new([10, 15, 50], [10, 15, 50], 128)
        .unwrap()
        .prepare(10, 1)
        .unwrap();
    assert_eq!(shape.orientation, Orientation::Z);
    assert_eq!(
        shape.body,
        vec![
            BodySphere {
                center: [0, -5, -40],
                radius: 5
            },
            BodySphere {
                center: [0, -5, -25],
                radius: 5
            },
            BodySphere {
                center: [0, -5, -10],
                radius: 5
            },
            BodySphere {
                center: [0, -5, 5],
                radius: 5
            },
            BodySphere {
                center: [0, -5, 20],
                radius: 5
            },
            BodySphere {
                center: [0, -5, 35],
                radius: 5
            },
        ]
    );
    assert_eq!(
        shape.additional,
        BodySphere {
            center: [0, -5, -40],
            radius: 5
        }
    );
}

#[test]
fn horizontal_post_height_equality_does_not_reduce_the_radius() {
    let shape = ActorBounds::new([10, 15, 50], [10, 15, 50], -1)
        .unwrap()
        .prepare(30, 0)
        .unwrap();
    assert_eq!(
        shape.body,
        vec![
            BodySphere {
                center: [0, -30, -35],
                radius: 15
            },
            BodySphere {
                center: [0, -30, -13],
                radius: 15
            },
            BodySphere {
                center: [0, -30, 9],
                radius: 15
            },
            BodySphere {
                center: [0, -30, 31],
                radius: 15
            },
        ]
    );
}

#[test]
fn dominant_axis_ties_are_upright() {
    let shape = ActorBounds::new([20; 3], [20; 3], 256)
        .unwrap()
        .prepare(50, 0)
        .unwrap();
    assert_eq!(shape.orientation, Orientation::Upright);
    assert_eq!(
        shape.body,
        vec![
            BodySphere {
                center: [0, 0, 0],
                radius: 20
            },
            BodySphere {
                center: [0, -50, 0],
                radius: 20
            },
        ]
    );
}

#[test]
fn invalid_authored_bounds_fail_at_the_boundary() {
    assert!(
        ActorBounds::new([-1, 0, 0], [0; 3], 256)
            .unwrap_err()
            .to_string()
            .contains("axis 0")
    );
    assert!(
        ActorBounds::new([i32::MAX, 0, 0], [1, 0, 0], 256)
            .unwrap_err()
            .to_string()
            .contains("axis 0")
    );
}
