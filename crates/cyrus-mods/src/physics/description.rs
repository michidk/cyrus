//! Atomic validation of owned assembly requests at the guest boundary.
use bevy::prelude::*;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Assembly {
    pub(super) bodies: Vec<Body>,
    pub(super) joints: Vec<Joint>,
    pub(super) weights: Vec<Vec<(usize, f32)>>,
    #[serde(default)]
    pub(super) attachments: Vec<Attachment>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Attachment {
    pub(super) index: usize,
    pub(super) body: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Body {
    pub(super) pose: [f32; 7],
    pub(super) shape: Shape,
    pub(super) mass: f32,
    pub(super) linear_damping: f32,
    pub(super) angular_damping: f32,
    pub(super) velocity: [f32; 3],
    pub(super) acceleration: [f32; 3],
    pub(super) impulse: [f32; 3],
}
#[derive(Deserialize)]
pub(super) enum Shape {
    Sphere(f32),
    Capsule { half_height: f32, radius: f32 },
    Box([f32; 3]),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Joint {
    pub(super) bodies: [usize; 2],
    pub(super) frame: [f32; 7],
    pub(super) limits: [[f32; 2]; 3],
}

fn finite(values: &[f32], maximum: f32) -> bool {
    values.iter().all(|v| v.is_finite() && v.abs() <= maximum)
}
fn valid_pose(pose: &[f32; 7]) -> bool {
    finite(pose, 1e6)
        && (Quat::from_xyzw(pose[3], pose[4], pose[5], pose[6]).length_squared() - 1.).abs() < 0.001
}
impl Assembly {
    pub(super) fn validate(&self, vertices: usize) -> bool {
        !self.bodies.is_empty()
            && self.bodies.len() <= 32
            && self.joints.len() <= 64
            && vertices <= 20_000
            && self.weights.len() == vertices
            && self.bodies.iter().all(|body| {
                valid_pose(&body.pose)
                    && body.mass.is_finite()
                    && (0.001..=10000.).contains(&body.mass)
                    && [body.linear_damping, body.angular_damping]
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=100.).contains(v))
                    && finite(&body.acceleration, 1000.)
                    && finite(&body.velocity, 1000.)
                    && finite(&body.impulse, 10000.)
                    && match body.shape {
                        Shape::Sphere(radius) => {
                            radius.is_finite() && (0.001..=100.).contains(&radius)
                        }
                        Shape::Capsule {
                            half_height,
                            radius,
                        } => [half_height, radius]
                            .iter()
                            .all(|v| v.is_finite() && (0.001..=100.).contains(v)),
                        Shape::Box(extents) => extents
                            .iter()
                            .all(|v| v.is_finite() && (0.001..=100.).contains(v)),
                    }
            })
            && self.joints.iter().all(|joint| {
                joint.bodies[0] != joint.bodies[1]
                    && joint.bodies.iter().all(|&i| i < self.bodies.len())
                    && valid_pose(&joint.frame)
                    && joint
                        .limits
                        .iter()
                        .all(|limit| finite(limit, std::f32::consts::PI) && limit[0] <= limit[1])
            })
            && self.weights.iter().all(|weights| {
                !weights.is_empty()
                    && weights.len() <= 4
                    && weights.iter().all(|&(body, weight)| {
                        body < self.bodies.len()
                            && weight.is_finite()
                            && (0.0..=1.).contains(&weight)
                    })
                    && (weights.iter().map(|w| w.1).sum::<f32>() - 1.).abs() < 0.001
            })
    }
}
