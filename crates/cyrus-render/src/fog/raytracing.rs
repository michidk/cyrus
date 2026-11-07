//! Hardware ray-traced lighting through Bevy Solari, selected by `Area::RayTracing`.
//!
//! **Modern only.** The original shades models and terrain with its own fog, clipping and
//! palette arithmetic in custom forward shaders; Solari lights a deferred G-buffer and traces
//! standard PBR materials. While the area is Modern and the GPU supports ray queries, each
//! opaque world mesh gets a stand-in entity that Solari can use (a mesh with exactly the
//! vertex layout it accepts, and a plain `StandardMaterial` that opts into deferred rendering),
//! and the shader-driven originals stop drawing. Flats, water, sky, alpha-masked or
//! chroma-keyed meshes and the HUD keep their forward shaders and draw over the lit scene.
//! The sun, the ambient colour and the object lights come from the world's environment.
use bevy::prelude::*;

/// Whether this build and GPU can trace rays.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub enum RayTracingSupport {
    /// The build lacks ray tracing, or no renderer exists (headless, browser).
    #[default]
    Unavailable,
    /// The adapter lacks these wgpu features.
    MissingFeatures(String),
    /// Ray queries are available.
    Available,
}

impl RayTracingSupport {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// A world mesh's ray-traced stand-in. It has no parent, so scene preparation ignores it.
#[derive(Component)]
pub(crate) struct Proxy;

#[cfg(feature = "raytracing")]
mod lights;
#[cfg(feature = "raytracing")]
mod plugin;
#[cfg(feature = "raytracing")]
mod proxies;
#[cfg(feature = "raytracing")]
pub use plugin::RayTracingPlugin;
