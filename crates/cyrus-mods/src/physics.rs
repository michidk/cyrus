//! Bounded owner-scoped rigid assemblies and source-vertex bindings. No gameplay collision writes.
use crate::{Catalog, actors::Actors};
use bevy::prelude::*;
use cyrus_assets::animation::Animations;
use cyrus_render::pose_override::{PoseOverride, VertexWeights};
use cyrus_script::{
    player::data::{PlayerScene, PlayerSceneRequest},
    script::{ScriptActors, ScriptBinding},
};
use rapier3d::prelude as rp;
mod description;
mod solver;
use description::{Assembly, Shape};
use solver::{Simulation, matrix, pose, vector};
use std::collections::{BTreeMap, HashMap};
use wasvy::prelude::*;

const MAX_BODIES: usize = 1024;
const OWNER_BODIES: usize = 256;
const OWNER_VERTICES: usize = 100_000;
const MAX_VERTICES: usize = 400_000;
const MAX_REQUEST: usize = 2_000_000;

#[derive(Clone, Copy, Reflect, PartialEq, Eq)]
struct Lease {
    instance: Entity,
    revision: u64,
}
struct Model {
    node: Entity,
    transform: Mat4,
    vertices: Vec<f32>,
    reference: Vec<f32>,
    logical: cyrus_script::script::ScriptActorId,
    revision: u64,
    name: String,
    source: Handle<Animations>,
    frame: usize,
    attachments: Vec<(Entity, Mat4)>,
}
struct Claim {
    logical: cyrus_script::script::ScriptActorId,
    revision: u64,
    name: String,
    owner: String,
    lease: Lease,
    node: Entity,
    bodies: Vec<rp::RigidBodyHandle>,
    inverse_rest: Vec<Mat4>,
    weights: std::sync::Arc<[VertexWeights]>,
    model_transform: Mat4,
    source: Handle<Animations>,
    frame: usize,
    attachments: Vec<(Entity, usize, Mat4)>,
}

/// Generic owned rigid assemblies. WIT JSON is validated before any solver or render allocation.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Physics {
    owners: BTreeMap<String, Lease>,
    #[reflect(ignore)]
    models: BTreeMap<u64, Model>,
    #[reflect(ignore)]
    claims: BTreeMap<u64, Claim>,
    #[reflect(ignore)]
    simulation: Option<Simulation>,
    #[reflect(ignore)]
    retired: Vec<Entity>,
    world: Option<(u32, u64)>,
    #[reflect(ignore)]
    captures: BTreeMap<u64, (String, usize, Mat4)>,
}
impl Physics {
    fn retire_invalid(&mut self) -> Vec<Entity> {
        let retired = self
            .claims
            .iter()
            .filter(|(token, c)| {
                self.owners.get(&c.owner) != Some(&c.lease)
                    || !self.models.get(token).is_some_and(|m| {
                        m.node == c.node && m.name == c.name && m.revision == c.revision
                    })
            })
            .map(|(&token, _)| token)
            .collect::<Vec<_>>();
        let mut nodes = Vec::new();
        for token in retired {
            let claim = self.claims.remove(&token).expect("retiring claim");
            if let Some(sim) = &mut self.simulation {
                sim.remove(&claim.bodies);
            }
            nodes.push(claim.node);
        }
        nodes
    }
}
#[wasvy::methods]
impl Physics {
    pub fn model(&mut self, actor: u64) -> String {
        self.models
            .get(&actor)
            .map(|m| m.name.clone())
            .unwrap_or_default()
    }
    pub fn origin(&mut self, actor: u64) -> Vec<f32> {
        self.models
            .get(&actor)
            .map(|m| {
                let (_, q, p) = m.transform.to_scale_rotation_translation();
                vec![p.x, p.y, p.z, q.x, q.y, q.z, q.w]
            })
            .unwrap_or_default()
    }
    pub fn reference(&mut self, actor: u64) -> Vec<f32> {
        self.models
            .get(&actor)
            .map(|m| m.reference.clone())
            .unwrap_or_default()
    }
    pub fn attachments(&mut self, actor: u64) -> Vec<f32> {
        self.models
            .get(&actor)
            .map(|model| {
                model
                    .attachments
                    .iter()
                    .flat_map(|(_, matrix)| {
                        let (_, q, p) = matrix.to_scale_rotation_translation();
                        [p.x, p.y, p.z, q.x, q.y, q.z, q.w]
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn vertices(&mut self, actor: u64) -> Vec<f32> {
        self.models
            .get(&actor)
            .map(|m| m.vertices.clone())
            .unwrap_or_default()
    }
    /// 0 accepted, 1 invalid, 2 unavailable/stale/not visible dead, 3 claimed, 4 budget.
    pub fn claim(&mut self, owner: String, actor: u64, description: String) -> u32 {
        let (Some(&lease), Some(model), Some(sim)) = (
            self.owners.get(&owner),
            self.models.get(&actor),
            self.simulation.as_mut(),
        ) else {
            return 2;
        };
        if description.len() > MAX_REQUEST {
            return 1;
        }
        let Ok(desc) = serde_json::from_str::<Assembly>(&description) else {
            return 1;
        };
        if !desc.validate(model.vertices.len() / 3)
            || desc.attachments.len() > model.attachments.len()
            || desc.attachments.iter().enumerate().any(|(i, a)| {
                a.index >= model.attachments.len()
                    || a.body >= desc.bodies.len()
                    || desc.attachments[..i].iter().any(|b| b.index == a.index)
            })
        {
            return 1;
        }
        if self.claims.contains_key(&actor) {
            return 3;
        }
        let count = self
            .claims
            .values()
            .map(|claim| claim.bodies.len())
            .sum::<usize>();
        let owned = self
            .claims
            .values()
            .filter(|claim| claim.owner == owner)
            .map(|claim| claim.bodies.len())
            .sum::<usize>();
        let vertices = self
            .claims
            .values()
            .map(|claim| claim.weights.len())
            .sum::<usize>();
        let owned_vertices = self
            .claims
            .values()
            .filter(|claim| claim.owner == owner)
            .map(|claim| claim.weights.len())
            .sum::<usize>();
        if count + desc.bodies.len() > MAX_BODIES
            || owned + desc.bodies.len() > OWNER_BODIES
            || vertices + desc.weights.len() > MAX_VERTICES
            || owned_vertices + desc.weights.len() > OWNER_VERTICES
        {
            return 4;
        }
        let mut bodies = Vec::new();
        let mut inverse_rest = Vec::new();
        for body in &desc.bodies {
            let position = pose(body.pose);
            inverse_rest.push(matrix(&position).inverse());
            let handle = sim.bodies.insert(
                rp::RigidBodyBuilder::dynamic()
                    .pose(position)
                    .linvel(vector(body.velocity))
                    .linear_damping(body.linear_damping)
                    .angular_damping(body.angular_damping)
                    .ccd_enabled(true)
                    .build(),
            );
            let collider = match body.shape {
                Shape::Sphere(radius) => rp::ColliderBuilder::ball(radius),
                Shape::Capsule {
                    half_height,
                    radius,
                } => rp::ColliderBuilder::capsule_y(half_height, radius),
                Shape::Box([x, y, z]) => rp::ColliderBuilder::cuboid(x, y, z),
            }
            .mass(body.mass)
            .friction(0.8)
            .collision_groups(rp::InteractionGroups::new(
                rp::Group::GROUP_2,
                rp::Group::GROUP_1,
                rp::InteractionTestMode::And,
            ));
            sim.colliders
                .insert_with_parent(collider, handle, &mut sim.bodies);
            sim.bodies[handle].add_force(vector(body.acceleration) * body.mass, true);
            sim.bodies[handle].apply_impulse(vector(body.impulse), true);
            bodies.push(handle);
        }
        for joint in desc.joints {
            let [a, b] = joint.bodies;
            let frame = pose(joint.frame);
            let mut builder = rp::GenericJointBuilder::new(rp::JointAxesMask::LIN_AXES)
                .local_frame1(sim.bodies[bodies[a]].position().inverse() * frame)
                .local_frame2(sim.bodies[bodies[b]].position().inverse() * frame)
                .contacts_enabled(false);
            for (axis, limits) in [
                rp::JointAxis::AngX,
                rp::JointAxis::AngY,
                rp::JointAxis::AngZ,
            ]
            .into_iter()
            .zip(joint.limits)
            {
                builder = builder.limits(axis, limits);
            }
            sim.joints.insert(bodies[a], bodies[b], builder, true);
        }
        self.claims.insert(
            actor,
            Claim {
                logical: model.logical,
                revision: model.revision,
                name: model.name.clone(),
                owner,
                lease,
                node: model.node,
                bodies,
                inverse_rest,
                weights: desc
                    .weights
                    .into_iter()
                    .map(VertexWeights)
                    .collect::<Vec<_>>()
                    .into(),
                model_transform: model.transform,
                source: model.source.clone(),
                frame: model.frame,
                attachments: desc
                    .attachments
                    .iter()
                    .map(|binding| {
                        let (entity, transform) = model.attachments[binding.index];
                        (entity, binding.body, transform)
                    })
                    .collect(),
            },
        );
        0
    }
    pub fn release(&mut self, owner: String, actor: u64) -> bool {
        if !self
            .claims
            .get(&actor)
            .is_some_and(|claim| claim.owner == owner)
        {
            return false;
        }
        let claim = self.claims.remove(&actor).expect("owned claim");
        self.retired.push(claim.node);
        if let Some(sim) = &mut self.simulation {
            sim.remove(&claim.bodies);
        }
        true
    }
    pub fn transforms(&mut self, owner: String, actor: u64) -> Vec<f32> {
        let Some(claim) = self.claims.get(&actor).filter(|c| c.owner == owner) else {
            return vec![];
        };
        let Some(sim) = &self.simulation else {
            return vec![];
        };
        claim
            .bodies
            .iter()
            .flat_map(|&body| {
                let p = sim.bodies[body].position();
                [
                    p.translation.x,
                    p.translation.y,
                    p.translation.z,
                    p.rotation.x,
                    p.rotation.y,
                    p.rotation.z,
                    p.rotation.w,
                ]
            })
            .collect()
    }
}

#[allow(clippy::too_many_arguments)] // Ownership, logical actors, geometry and Wasm asset lifetimes are separate Bevy inputs.
pub(crate) fn publish(
    mut commands: Commands,
    mut host: Query<(&Actors, &mut Physics)>,
    catalog: Res<Catalog>,
    actors: Option<Res<ScriptActors>>,
    nodes: Query<(Entity, &ScriptBinding, &GlobalTransform)>,
    attachments: Query<(Entity, &cyrus_render::WeaponRoot, &GlobalTransform)>,
    mods: Query<&wasvy::prelude::Mod>,
    animations: Option<Res<Assets<Animations>>>,
    scenes: Option<Res<Assets<PlayerScene>>>,
    scene: Option<Res<PlayerSceneRequest>>,
    mut events: MessageReader<AssetEvent<ModAsset>>,
    mut revisions: Local<HashMap<AssetId<ModAsset>, u64>>,
) {
    for event in events.read() {
        if let AssetEvent::LoadedWithDependencies { id } = event {
            *revisions.entry(*id).or_default() += 1;
        }
    }
    let Ok((actors_api, mut host)) = host.single_mut() else {
        return;
    };
    let current = actors.as_deref().and_then(|a| a.current.as_ref());
    let world = current.map(|w| (w.world, w.generation()));
    if host.world != world {
        for claim in host.claims.values() {
            commands.entity(claim.node).try_remove::<PoseOverride>();
        }
        host.claims.clear();
        host.captures.clear();
        host.simulation = None;
        host.world = world;
    }
    host.owners = catalog
        .entries
        .iter()
        .filter(|e| e.active && !e.gone)
        .filter_map(|e| {
            let instance = e.instance?;
            let asset = mods.get(instance).ok()?.asset().id();
            Some((
                e.id.clone(),
                Lease {
                    instance,
                    revision: revisions.get(&asset).copied().unwrap_or_default(),
                },
            ))
        })
        .collect();
    host.captures
        .retain(|token, _| actors_api.visible_dead(*token));
    for index in 0..actors_api.count() {
        let Some(&token) = actors_api.death(index).first() else {
            continue;
        };
        let model = actors_api.eventmodel(index);
        let frames = actors_api.eventframe(index);
        let values = actors_api.eventpose(index);
        if let (Some(&frame), &[x, y, z, qx, qy, qz, qw]) = (frames.first(), values.as_slice()) {
            if !model.is_empty() {
                host.captures.insert(
                    token,
                    (
                        model,
                        frame as usize,
                        Mat4::from_rotation_translation(
                            Quat::from_xyzw(qx, qy, qz, qw),
                            Vec3::new(x, y, z),
                        ),
                    ),
                );
            }
        }
    }
    host.models.clear();
    if let (Some(current), Some(animations)) = (current, animations.as_deref()) {
        for token in actors_api.ids() {
            if !actors_api.visible_dead(token) {
                continue;
            }
            let Some(logical) = actors_api.logical(token) else {
                continue;
            };
            let Some((node, binding, _transform)) =
                nodes.iter().find(|(_, b, _)| b.actor == logical)
            else {
                continue;
            };
            let Some(data) = animations.get(&binding.source) else {
                continue;
            };
            let Some((model, frame, captured_transform)) = host.captures.get(&token).cloned()
            else {
                continue;
            };
            let Some(positions) = data.models.get(&model).and_then(|m| m.positions.get(frame))
            else {
                continue;
            };
            if positions.len() > 20_000 || current.actor_snapshot(logical).is_err() {
                continue;
            }
            let vertices = positions
                .iter()
                .flat_map(|[x, y, z]| [-x / 20., -y / 20., z / 20.])
                .collect();
            host.models.insert(
                token,
                Model {
                    node,
                    transform: captured_transform,
                    vertices,
                    reference: data.models[&model]
                        .positions
                        .first()
                        .unwrap_or(positions)
                        .iter()
                        .flat_map(|[x, y, z]| [-x / 20., -y / 20., z / 20.])
                        .collect(),
                    logical,
                    revision: current
                        .actor_snapshot(logical)
                        .expect("validated actor")
                        .revision,
                    name: model,
                    source: binding.source.clone(),
                    frame,
                    attachments: attachments
                        .iter()
                        .filter(|(_, root, _)| root.owner == node)
                        .map(|(entity, _, transform)| (entity, transform.to_matrix()))
                        .collect(),
                },
            );
        }
    }
    for node in host.retire_invalid() {
        commands.entity(node).try_remove::<PoseOverride>();
    }
    if host.simulation.is_none() && !host.owners.is_empty() {
        if let Some(scene) = scene
            .as_deref()
            .and_then(|request| scenes.as_deref()?.get(&request.0))
            .filter(|s| Some(s.world) == current.map(|w| w.world))
        {
            match Simulation::world(scene) {
                Ok(sim) => host.simulation = Some(sim),
                Err(error) => error!("mod physics world {}: {error:#}", scene.world),
            }
        }
    }
}

pub(crate) fn update(
    mut commands: Commands,
    mut host: Query<&mut Physics>,
    nodes: Query<&GlobalTransform, Without<cyrus_render::WeaponRoot>>,
    actors: Option<Res<ScriptActors>>,
    time: Res<Time>,
    catalog: Res<Catalog>,
    mut attachments: Query<(&mut Transform, &mut GlobalTransform), With<cyrus_render::WeaponRoot>>,
) {
    let Ok(mut host) = host.single_mut() else {
        return;
    };
    let retired = host
        .claims
        .iter()
        .filter(|(_, claim)| {
            !catalog.entries.iter().any(|entry| {
                entry.id == claim.owner
                    && entry.active
                    && !entry.gone
                    && entry.instance == Some(claim.lease.instance)
            }) || !actors
                .as_deref()
                .and_then(|a| a.current.as_ref())
                .and_then(|w| w.actor_snapshot(claim.logical).ok())
                .is_some_and(|s| {
                    s.dead
                        && s.presence == cyrus_script::script::ActorPresence::Visible
                        && s.revision == claim.revision
                })
        })
        .map(|(&token, _)| token)
        .collect::<Vec<_>>();
    for token in retired {
        let claim = host.claims.remove(&token).expect("retiring visual claim");
        if let Some(sim) = &mut host.simulation {
            sim.remove(&claim.bodies);
        }
        commands.entity(claim.node).try_remove::<PoseOverride>();
    }
    for node in host.retired.drain(..) {
        commands.entity(node).try_remove::<PoseOverride>();
    }
    let Physics {
        simulation, claims, ..
    } = &mut *host;
    let Some(sim) = simulation else {
        return;
    };
    sim.advance(time.delta_secs_f64());
    for (&token, claim) in claims.iter() {
        let Ok(transform) = nodes.get(claim.node) else {
            continue;
        };
        let inverse = transform.to_matrix().inverse();
        let matrices = claim
            .bodies
            .iter()
            .zip(&claim.inverse_rest)
            .map(|(&body, rest)| {
                inverse * matrix(sim.bodies[body].position()) * *rest * claim.model_transform
            })
            .collect();
        for &(entity, body, rest_transform) in &claim.attachments {
            if let Ok((mut local, mut global)) = attachments.get_mut(entity) {
                let deformed = matrix(sim.bodies[claim.bodies[body]].position())
                    * claim.inverse_rest[body]
                    * rest_transform;
                *local = Transform::from_matrix(deformed);
                *global = GlobalTransform::from(deformed);
            }
        }
        commands.entity(claim.node).insert(PoseOverride {
            key: token,
            matrices,
            weights: claim.weights.clone(),
            source: Some((claim.source.clone(), claim.name.clone(), claim.frame)),
        });
    }
}

#[cfg(test)]
mod tests;
