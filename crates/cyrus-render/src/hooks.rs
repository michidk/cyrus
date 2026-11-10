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

/// The grid cell size of the terrain in world units, as a one-value scene value.
pub const WATER_CELL: &str = "water-cell";
/// One while the Lighting area is Modern, else zero, as a one-value scene value.
pub const LIGHTING_MODERN: &str = "lighting-modern";

/// The water-surface shader input: `[effects, 0, amplitude, cell]`. `effects` above zero turns on
/// the shader's shore foam, wet sand and ripples; a positive `amplitude` also makes the terrain
/// shader move the waves, `cell` being the grid cell size in world units, so the mesh keeps its
/// base grid. Read as `fog.water` by the terrain and screen-pass shaders.
pub const WATER_SURFACE: &str = "water-surface";

/// Four-value shader inputs a provider writes, by name. The presentation reads each name it
/// documents every frame, so a provider that stops writing hands the shaders their built-in
/// behaviour back.
#[derive(Resource, Default)]
pub struct ShaderInputs(pub HashMap<String, [f32; 4]>);

impl ShaderInputs {
    /// The `WATER_SURFACE` input, all zero when no provider writes it.
    pub fn water(&self) -> Vec4 {
        Vec4::from_array(self.0.get(WATER_SURFACE).copied().unwrap_or_default())
    }

    /// Whether the terrain shader moves the waves.
    pub fn shader_waves(&self) -> bool {
        let water = self.water();
        water.x > 0.0 && water.z > 0.0
    }
}
