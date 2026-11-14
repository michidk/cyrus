//! Owned rigid assemblies over scene-coordinate geometry. Physics never changes gameplay state.
use crate::{ActorId, Pose, Vec3, bindings::cyrus::mods::components as raw};

/// Request-local body reference. Only bodies inserted in this assembly can be referenced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyIndex(u32);

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Sphere(f32),
    Capsule { half_height: f32, radius: f32 },
    Box(Vec3),
}

#[derive(Clone, Debug)]
pub struct BodyDesc {
    /// World scene pose, unit quaternion.
    pub pose: Pose,
    pub shape: Shape,
    /// Positive mass. Scene distance units, seconds and this mass unit define impulse units.
    pub mass: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub velocity: Vec3,
    /// Constant world acceleration, supplied by the guest, including gravity if desired.
    pub acceleration: Vec3,
    pub impulse: Vec3,
}

#[derive(Clone, Debug)]
pub struct JointDesc {
    pub bodies: [BodyIndex; 2],
    /// World rest frame. Translation is the shared anchor. Angular limits are relative to this frame.
    pub frame: Pose,
    /// Ordered radians for X, Y and Z. Linear motion is locked on all axes.
    pub limits: [[f32; 2]; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct VertexWeight {
    pub body: BodyIndex,
    pub weight: f32,
}

/// Bind a currently published attachment to an assembly body. Indices expire with the model snapshot.
#[derive(Clone, Copy, Debug)]
pub struct AttachmentBinding {
    pub index: usize,
    pub body: BodyIndex,
}

/// Validated atomically by the host. 1..32 bodies, <=64 joints and <=20000 source vertices.
/// Weights have 1..4 entries per source vertex, finite and nonnegative, summing to one.
#[derive(Default, Clone, Debug)]
pub struct Assembly {
    bodies: Vec<BodyDesc>,
    pub joints: Vec<JointDesc>,
    pub attachments: Vec<AttachmentBinding>,
    pub weights: Vec<Vec<VertexWeight>>,
}
impl Assembly {
    pub fn body(&mut self, body: BodyDesc) -> BodyIndex {
        let index = BodyIndex(self.bodies.len() as u32);
        self.bodies.push(body);
        index
    }
    pub fn bodies(&self) -> &[BodyDesc] {
        &self.bodies
    }
    fn encode(&self) -> String {
        let vector = |v: Vec3| [v.x, v.y, v.z];
        let bodies = self.bodies.iter().map(|body| {
            let shape = match body.shape {
                Shape::Sphere(radius) => serde_json::json!({"Sphere":radius}),
                Shape::Capsule {half_height,radius} => serde_json::json!({"Capsule":{"half_height":half_height,"radius":radius}}),
                Shape::Box(extents) => serde_json::json!({"Box":vector(extents)}),
            };
            serde_json::json!({"pose":body.pose.to_list(),"shape":shape,"mass":body.mass,
                "linear_damping":body.linear_damping,"angular_damping":body.angular_damping,
                "velocity":vector(body.velocity),"acceleration":vector(body.acceleration),"impulse":vector(body.impulse)})
        }).collect::<Vec<_>>();
        let joints = self.joints.iter().map(|joint| serde_json::json!({"bodies":joint.bodies.map(|i|i.0),"frame":joint.frame.to_list(),"limits":joint.limits})).collect::<Vec<_>>();
        let weights = self
            .weights
            .iter()
            .map(|weights| {
                weights
                    .iter()
                    .map(|w| (w.body.0, w.weight))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        serde_json::json!({"bodies":bodies,"joints":joints,"weights":weights,"attachments":self.attachments.iter().map(|a|serde_json::json!({"index":a.index,"body":a.body.0})).collect::<Vec<_>>()}).to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimError {
    Invalid,
    Unavailable,
    AlreadyClaimed,
    Budget,
    UnsupportedHost,
}

pub struct Physics {
    pub(crate) raw: raw::Physics,
    pub(crate) id: &'static str,
}
impl Physics {
    /// Current source geometry in model-local scene axes. Read only when fitting a new binding.
    pub fn vertices(&self, actor: ActorId) -> Vec<Vec3> {
        self.raw
            .vertices(actor.0)
            .chunks_exact(3)
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect()
    }
    /// Source geometry key and world pose captured at the death commitment.
    pub fn model(&self, actor: ActorId) -> Option<String> {
        let model = self.raw.model(actor.0);
        (!model.is_empty()).then_some(model)
    }
    pub fn origin(&self, actor: ActorId) -> Option<Pose> {
        Pose::from_list(&self.raw.origin(actor.0))
    }
    pub fn reference(&self, actor: ActorId) -> Vec<Vec3> {
        self.raw
            .reference(actor.0)
            .chunks_exact(3)
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect()
    }
    pub fn attachments(&self, actor: ActorId) -> Vec<Pose> {
        self.raw
            .attachments(actor.0)
            .chunks_exact(7)
            .filter_map(Pose::from_list)
            .collect()
    }
    pub fn claim(&self, actor: ActorId, assembly: &Assembly) -> Result<(), ClaimError> {
        match self.raw.claim(self.id, actor.0, &assembly.encode()) {
            0 => Ok(()),
            1 => Err(ClaimError::Invalid),
            2 => Err(ClaimError::Unavailable),
            3 => Err(ClaimError::AlreadyClaimed),
            4 => Err(ClaimError::Budget),
            _ => Err(ClaimError::UnsupportedHost),
        }
    }
    /// Drop the assembly, solver objects and pose claim. Another owner cannot release it.
    pub fn release(&self, actor: ActorId) -> bool {
        self.raw.release(self.id, actor.0)
    }
    /// Batch world body poses in insertion order. Empty means stale or unowned.
    pub fn transforms(&self, actor: ActorId) -> Vec<Pose> {
        self.raw
            .transforms(self.id, actor.0)
            .chunks_exact(7)
            .filter_map(Pose::from_list)
            .collect()
    }
}
