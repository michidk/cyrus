//! Ray-traced lighting through Bevy Solari: a switch under Modern lighting, on by default, that
//! traces on a GPU with ray queries. Builds without the `raytracing` feature have neither the
//! switch nor the tracer.
//!
//! **Modern only.** Solari traces only `StandardMaterial` meshes with exactly the position, normal,
//! UV 0 and tangent layout, and shades them in a deferred G-buffer, while the original's materials
//! are forward shaders with their own layout. So while Solari lights the scene, each opaque world
//! mesh gets a deferred stand-in entity in that layout, and the original's draw stops. Flats, water,
//! sky, alpha-masked or chroma-keyed meshes and the HUD keep their own shaders and draw over the
//! traced scene. The sun, the ambient colour and the object lights come from the world's
//! environment, calibrated as the raster modern lighting is; object lights become small emissive
//! spheres, since Solari lights only with the sun, the environment and emissive meshes. The traced
//! pixels get the scene fog in a pass of their own (`haze`). Where ray tracing is off or
//! unavailable, the raster modern lighting applies.
use bevy::prelude::*;
use cyrus_game::fidelity::{Area, Fidelity, Mode};

/// Whether the GPU can trace rays.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RayTracingSupport {
    /// No renderer exists, or it has not been inspected yet.
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

/// The ray-tracing switch and what the GPU supports. Present only where `RayTracingPlugin` is, so
/// its absence means the build has no ray tracing.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct RayTracing {
    /// Traces while `Area::Lighting` is Modern; on by default.
    pub enabled: bool,
    pub support: RayTracingSupport,
}

impl Default for RayTracing {
    fn default() -> Self {
        Self {
            enabled: true,
            support: RayTracingSupport::default(),
        }
    }
}

/// Replaces the real-time ray-traced lighting with Solari's path tracer, which accumulates a
/// converged reference image while the camera stays still. A development aid for judging the
/// real-time lighting and its denoiser; it has no effect unless Solari lights the scene. Present
/// only where `RayTracingPlugin` is.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RayTracingReference(pub bool);

/// A world mesh's traced stand-in. It has no parent, so scene preparation ignores it.
#[derive(Component)]
pub(crate) struct StandIn;

/// Whether Solari lights the scene: lighting is Modern, the switch is on and the GPU can trace.
pub fn ray_traced(fidelity: &Fidelity, ray_tracing: &RayTracing) -> bool {
    fidelity.mode(Area::Lighting) == Mode::Modern
        && ray_tracing.enabled
        && ray_tracing.support.is_available()
}

/// [`ray_traced`] for a build or app that may lack ray tracing.
pub(crate) fn traced_if_built(fidelity: &Fidelity, ray_tracing: Option<&RayTracing>) -> bool {
    ray_tracing.is_some_and(|ray_tracing| ray_traced(fidelity, ray_tracing))
}

#[cfg(feature = "raytracing")]
mod haze;
#[cfg(feature = "raytracing")]
mod lights;
#[cfg(feature = "raytracing")]
mod plugin;
#[cfg(feature = "raytracing")]
mod proxies;
#[cfg(feature = "raytracing")]
pub use plugin::RayTracingPlugin;

#[cfg(test)]
mod tests {
    use super::*;

    fn switch(enabled: bool, support: RayTracingSupport) -> RayTracing {
        RayTracing { enabled, support }
    }

    #[test]
    fn ray_tracing_needs_modern_lighting_the_switch_and_the_hardware() {
        let mut modern = Fidelity::default();
        modern.set(Area::Lighting, Mode::Modern);
        assert!(RayTracing::default().enabled);
        assert!(ray_traced(
            &modern,
            &switch(true, RayTracingSupport::Available)
        ));
        assert!(!ray_traced(
            &modern,
            &switch(false, RayTracingSupport::Available)
        ));
        assert!(!ray_traced(
            &modern,
            &switch(true, RayTracingSupport::Unavailable)
        ));
        assert!(!ray_traced(
            &modern,
            &switch(
                true,
                RayTracingSupport::MissingFeatures("EXPERIMENTAL_RAY_QUERY".into())
            )
        ));
        // Faithful lighting never traces, whatever the switch says.
        assert!(!ray_traced(
            &Fidelity::default(),
            &switch(true, RayTracingSupport::Available)
        ));
        assert!(!traced_if_built(&modern, None));
    }
}
