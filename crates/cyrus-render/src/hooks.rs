//! What the presentation offers a provider such as a mod, without knowing what it will do with
//! it: facts about the loaded world it may read and named effect slots it may fill. Effects run in
//! the shaders (a provider replaces a shader library and fills its slot); the presentation never
//! hands out its meshes.
use bevy::prelude::*;
use std::collections::HashMap;

/// Facts about the loaded world, published every frame for providers to read.
#[derive(Resource, Default)]
pub struct WorldFacts {
    /// Presentation clock in seconds.
    pub seconds: f32,
    /// Whether the Lighting area is Modern.
    pub modern_lighting: bool,
    /// The loaded world's water; `None` when it has none.
    pub water: Option<WaterFacts>,
}

/// How the loaded world's water moves, as its installed environment describes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterFacts {
    /// Wave amplitude, speed and frequency in the world file's units.
    pub amplitude: f32,
    pub speed: f32,
    pub frequency: f32,
    /// The terrain grid cell size in world units.
    pub cell: f32,
}

/// The terrain surface slot: `[effects, 0, amplitude, cell]`. `effects` above zero turns on the
/// surface library's shore foam, wet sand and ripples; a positive `amplitude` also makes the
/// terrain shader move the waves, `cell` being the grid cell size in world units, so the mesh keeps
/// its base grid. The terrain and screen-pass shaders read it as `fog.water`.
pub const TERRAIN_SURFACE: &str = "terrain-surface";

/// Four-value parameter blocks a provider writes into named effect slots. The presentation reads
/// each slot it documents every frame, so a provider that stops writing hands the shaders their
/// built-in behaviour back.
#[derive(Resource, Default)]
pub struct EffectSlots(pub HashMap<String, [f32; 4]>);

impl EffectSlots {
    /// The `TERRAIN_SURFACE` slot, all zero when no provider writes it.
    pub fn terrain_surface(&self) -> Vec4 {
        Vec4::from_array(self.0.get(TERRAIN_SURFACE).copied().unwrap_or_default())
    }

    /// Whether the terrain shader moves the waves.
    pub fn shader_waves(&self) -> bool {
        let surface = self.terrain_surface();
        surface.x > 0.0 && surface.z > 0.0
    }
}
