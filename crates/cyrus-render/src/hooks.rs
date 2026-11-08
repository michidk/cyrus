//! What the presentation offers a provider such as a mod, without knowing what it will do with
//! it: named vertex sets it may displace and named scene values it may read. Each publisher
//! documents the names it writes; nothing here is specific to one of them.
use bevy::prelude::*;
use std::collections::HashMap;

/// The animated water vertices of the terrain in the camera window, in exported scene units.
/// Displacing them replaces the original table wave on screen.
pub const WATER: &str = "water";
/// The loaded world's wave parameters `[amplitude, speed, frequency]`, in WLD units.
pub const WATER_WAVE: &str = "water-wave";

/// Vertex sets a provider may move, by name.
#[derive(Resource, Default)]
pub struct VertexSets(pub HashMap<String, VertexSet>);

/// One published vertex set and the offsets a provider answered for it.
#[derive(Default)]
pub struct VertexSet {
    /// Changes whenever `positions` are republished.
    pub generation: u32,
    /// Base positions of the vertices.
    pub positions: Vec<[f32; 3]>,
    /// Offsets in the positions' units for `generation`, one per position. The publisher takes
    /// them every frame, so a provider that stops writing hands the vertices back.
    pub offsets: Option<(u32, Vec<[f32; 3]>)>,
    /// The publisher's own index of each position.
    pub(crate) vertices: Vec<usize>,
}

/// Values the presentation publishes for providers, by name, and its clock.
#[derive(Resource, Default)]
pub struct SceneValues {
    pub values: HashMap<String, Vec<f32>>,
    /// Presentation clock in seconds.
    pub seconds: f32,
}
