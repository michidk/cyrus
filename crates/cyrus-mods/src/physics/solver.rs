//! Fixed-step visual rigid bodies and read-only installed world collision.
use bevy::prelude::*;
use cyrus_script::player::data::PlayerScene;
use rapier3d::prelude as rp;
pub(super) fn vector(value: [f32; 3]) -> rp::Vector {
    rp::Vector::new(value[0], value[1], value[2])
}
pub(super) fn pose(value: [f32; 7]) -> rp::Pose {
    rp::Pose::from_parts(
        vector([value[0], value[1], value[2]]),
        rp::Rotation::from_xyzw(value[3], value[4], value[5], value[6]).normalize(),
    )
}
pub(super) fn matrix(value: &rp::Pose) -> Mat4 {
    Mat4::from_rotation_translation(
        Quat::from_xyzw(
            value.rotation.x,
            value.rotation.y,
            value.rotation.z,
            value.rotation.w,
        ),
        Vec3::new(
            value.translation.x,
            value.translation.y,
            value.translation.z,
        ),
    )
}

pub(super) struct Simulation {
    pub(super) pipeline: rp::PhysicsPipeline,
    pub(super) islands: rp::IslandManager,
    pub(super) broad: rp::BroadPhaseBvh,
    pub(super) narrow: rp::NarrowPhase,
    pub(super) bodies: rp::RigidBodySet,
    pub(super) colliders: rp::ColliderSet,
    pub(super) joints: rp::ImpulseJointSet,
    pub(super) multibodies: rp::MultibodyJointSet,
    pub(super) soft_bodies: rp::SoftBodySet,
    pub(super) ccd: rp::CCDSolver,
    pub(super) accumulator: f64,
    /// Pairs of one assembly's bodies that touch or overlap in the claimed pose, stored in both
    /// orders. They never collide; every other pair of assembly bodies does, so limbs cannot fold
    /// through the torso.
    pub(super) resting: std::collections::HashSet<(rp::RigidBodyHandle, rp::RigidBodyHandle)>,
}

/// Bodies closer than this in the claimed pose are treated as touching, in world units.
pub(super) const REST_GAP: f32 = 0.05;

struct RestingPairs<'a>(&'a std::collections::HashSet<(rp::RigidBodyHandle, rp::RigidBodyHandle)>);
impl rp::PhysicsHooks for RestingPairs<'_> {
    fn filter_contact_pair(&self, context: &rp::PairFilterContext) -> Option<rp::SolverFlags> {
        match (context.rigid_body1, context.rigid_body2) {
            (Some(a), Some(b)) if self.0.contains(&(a, b)) => None,
            _ => Some(rp::SolverFlags::COMPUTE_RIGID_IMPULSES),
        }
    }
}
impl Default for Simulation {
    fn default() -> Self {
        Self {
            pipeline: rp::PhysicsPipeline::new(),
            islands: rp::IslandManager::new(),
            broad: rp::BroadPhaseBvh::new(),
            narrow: rp::NarrowPhase::new(),
            bodies: rp::RigidBodySet::new(),
            colliders: rp::ColliderSet::new(),
            joints: rp::ImpulseJointSet::new(),
            multibodies: rp::MultibodyJointSet::new(),
            soft_bodies: rp::SoftBodySet::default(),
            ccd: rp::CCDSolver::new(),
            accumulator: 0.,
            resting: Default::default(),
        }
    }
}
impl Simulation {
    pub(super) fn advance(&mut self, delta: f64) {
        const STEP: f64 = 1. / 120.;
        self.accumulator = (self.accumulator + delta).min(STEP * 8.);
        let params = rp::IntegrationParameters {
            dt: STEP as f32,
            ..Default::default()
        };
        while self.accumulator >= STEP {
            self.pipeline.step(
                vector([0., 0., 0.]),
                &params,
                &mut self.islands,
                &mut self.broad,
                &mut self.narrow,
                &mut self.bodies,
                &mut self.colliders,
                &mut self.joints,
                &mut self.multibodies,
                &mut self.soft_bodies,
                &mut self.ccd,
                &RestingPairs(&self.resting),
                &(),
            );
            self.accumulator -= STEP;
        }
    }
    pub(super) fn remove(&mut self, handles: &[rp::RigidBodyHandle]) {
        self.resting
            .retain(|(a, b)| !handles.contains(a) && !handles.contains(b));
        for &handle in handles {
            self.bodies.remove(
                handle,
                &mut self.islands,
                &mut self.colliders,
                &mut self.joints,
                &mut self.multibodies,
                &mut self.soft_bodies,
                true,
            );
        }
    }
    pub(super) fn world(scene: &PlayerScene) -> anyhow::Result<Self> {
        let mut sim = Self::default();
        let convert = |[x, y, z]: [f32; 3]| vector([-x / 20., -y / 20., (z - 65_535.996) / 20.]);
        // Static source polygons, independent of the player's admitted neighbourhood.
        for placement in &scene.collision.placements {
            let model = scene
                .collision
                .models
                .get(&placement.model)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "world {} static model {} collision unavailable",
                        scene.world,
                        placement.model
                    )
                })?;
            let vertices = model
                .geometry
                .vertices
                .iter()
                .map(|point| {
                    let rotation = placement.rotation.map(|v| v as f32 / 268_435_456.);
                    let p = Vec3::from_array(*point);
                    let world = Mat3::from_cols_array(&rotation).transpose() * p
                        + Vec3::from_array(placement.position.map(|v| v as f32));
                    convert(world.to_array())
                })
                .collect();
            let triangles = model
                .geometry
                .faces
                .iter()
                .flat_map(|face| {
                    (1..face.len().saturating_sub(1))
                        .map(move |i| [face[0] as u32, face[i] as u32, face[i + 1] as u32])
                })
                .collect::<Vec<_>>();
            if !triangles.is_empty() {
                sim.colliders.insert(
                    rp::ColliderBuilder::trimesh(vertices, triangles)?
                        .friction(0.8)
                        .build(),
                );
            }
        }
        if let Some(terrain) = &scene.terrain {
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            for row in 2..255_i64 {
                for column in 0..255_i64 {
                    let corners = [
                        (column, row),
                        (column + 1, row),
                        (column + 1, row - 1),
                        (column, row - 1),
                    ];
                    let points = corners.map(|(x, z)| {
                        let position = [x * 256 * 256, 0, z * 256 * 256];
                        terrain.sample_dry(position).map(|support| {
                            convert([x as f32 * 256., support.height as f32, z as f32 * 256.])
                        })
                    });
                    if let [Some(a), Some(b), Some(c), Some(d)] = points {
                        let start = vertices.len() as u32;
                        vertices.extend([a, b, c, d]);
                        indices
                            .extend([[start, start + 1, start + 2], [start, start + 2, start + 3]]);
                    }
                }
            }
            if !indices.is_empty() {
                sim.colliders.insert(
                    rp::ColliderBuilder::trimesh(vertices, indices)?
                        .friction(0.8)
                        .build(),
                );
            }
        }
        Ok(sim)
    }
}
