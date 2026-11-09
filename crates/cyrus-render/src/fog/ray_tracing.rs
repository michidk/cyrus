//! Ray-traced lighting through Bevy Solari, selected by `Area::RayTracing` on a GPU with ray
//! queries.
//!
//! **Modern only.** Solari traces only `StandardMaterial` meshes with exactly the position, normal,
//! UV 0 and tangent layout, and shades them in a deferred G-buffer, while the original's materials
//! are forward shaders with their own layout. So while Solari lights the scene, each opaque world
//! mesh gets a deferred stand-in entity in that layout, and the original's draw stops. Flats, water,
//! sky, alpha-masked or chroma-keyed meshes and the HUD keep their own shaders and draw over the
//! traced scene. The sun, the ambient colour and the object lights come from the world's
//! environment, calibrated as the raster modern lighting is; object lights become small emissive
//! spheres, since Solari lights only with the sun, the environment and emissive meshes. The traced
//! pixels get the scene fog in a pass of their own (`haze`). Where ray tracing is unavailable, the
//! `lighting` area applies.
use bevy::prelude::*;
use cyrus_game::fidelity::{Area, Fidelity, Mode};

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

/// Replaces the real-time ray-traced lighting with Solari's path tracer, which accumulates a
/// converged reference image while the camera stays still. A development aid for judging the
/// real-time lighting and its denoiser; it has no effect unless Solari lights the scene. Present
/// only where `RayTracingPlugin` is.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RayTracingReference(pub bool);

/// A world mesh's traced stand-in. It has no parent, so scene preparation ignores it.
#[derive(Component)]
pub(crate) struct StandIn;

/// Whether Solari lights the scene: the area is Modern and the GPU can trace.
pub(crate) fn ray_traced(fidelity: &Fidelity, support: &RayTracingSupport) -> bool {
    fidelity.mode(Area::RayTracing) == Mode::Modern && support.is_available()
}

/// Whether models and terrain draw their world-space meshes instead of the original's per-frame
/// camera-space preparation. Modern lighting needs them for shadows and the depth prepass, and the
/// traced stand-ins are built from them.
pub(crate) fn world_space_geometry(
    fidelity: &Fidelity,
    support: Option<&RayTracingSupport>,
) -> bool {
    fidelity.mode(Area::Lighting) == Mode::Modern
        || support.is_some_and(|support| ray_traced(fidelity, support))
}

/// Whether the terrain shader moves the modern waves instead of the CPU rewriting the terrain mesh.
/// It needs the world-space terrain of modern lighting; a traced scene keeps the waves in the mesh,
/// because its stand-ins are built from that mesh.
pub(crate) fn shader_waves(fidelity: &Fidelity, support: Option<&RayTracingSupport>) -> bool {
    fidelity.mode(Area::Water) == Mode::Modern
        && fidelity.mode(Area::Lighting) == Mode::Modern
        && !support.is_some_and(|support| ray_traced(fidelity, support))
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

    fn fidelity(lighting: Mode, ray_tracing: Mode) -> Fidelity {
        let mut fidelity = Fidelity::default();
        fidelity.set(Area::Lighting, lighting);
        fidelity.set(Area::RayTracing, ray_tracing);
        fidelity
    }

    #[test]
    fn ray_tracing_needs_the_area_and_the_hardware() {
        let modern = fidelity(Mode::Faithful, Mode::Modern);
        assert!(ray_traced(&modern, &RayTracingSupport::Available));
        assert!(!ray_traced(&modern, &RayTracingSupport::Unavailable));
        assert!(!ray_traced(
            &modern,
            &RayTracingSupport::MissingFeatures("EXPERIMENTAL_RAY_QUERY".into())
        ));
        assert!(!ray_traced(
            &fidelity(Mode::Modern, Mode::Faithful),
            &RayTracingSupport::Available
        ));
    }

    #[test]
    fn world_space_geometry_follows_either_modern_tier() {
        let available = Some(&RayTracingSupport::Available);
        let unavailable = Some(&RayTracingSupport::Unavailable);
        assert!(!world_space_geometry(
            &fidelity(Mode::Faithful, Mode::Faithful),
            available
        ));
        assert!(world_space_geometry(
            &fidelity(Mode::Modern, Mode::Faithful),
            unavailable
        ));
        assert!(world_space_geometry(
            &fidelity(Mode::Faithful, Mode::Modern),
            available
        ));
        // Without the hardware the faithful geometry stays.
        assert!(!world_space_geometry(
            &fidelity(Mode::Faithful, Mode::Modern),
            unavailable
        ));
    }
}
