//! Modern lighting: the world lit by real-time PBR shading, optionally ray traced through
//! Bevy Solari. `Area::Lighting` selects the raster tier and `Area::RayTracing` the
//! ray-traced tier.
//!
//! **Modern only.** The original shades models and terrain with its own fog, clipping and
//! palette arithmetic in custom forward shaders. While either area is Modern, each opaque
//! world mesh gets a stand-in entity (a mesh with exactly the vertex layout Solari accepts,
//! and a plain `StandardMaterial`), and the shader-driven originals stop drawing. The raster
//! tier shades the stand-ins with Bevy's forward PBR: a cascaded shadow-mapped sun, the
//! ambient colour as an environment light and the object lights as point lights. The
//! ray-traced tier makes the stand-ins deferred and lights them with Solari when the GPU
//! supports ray queries. Flats, water, sky, alpha-masked or chroma-keyed meshes and the HUD
//! keep their forward shaders and draw over the lit scene. The sun, the ambient colour and
//! the object lights come from the world's environment.
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

/// How the stand-ins are lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Tier {
    /// Forward PBR with shadow maps.
    Raster,
    /// Solari ray-traced lighting.
    Traced,
}

impl Tier {
    /// The tier the fidelity settings select, if any. Ray tracing wins where the GPU supports it.
    pub(crate) fn select(fidelity: &Fidelity, support: &RayTracingSupport) -> Option<Self> {
        if fidelity.mode(Area::RayTracing) == Mode::Modern && support.is_available() {
            Some(Self::Traced)
        } else if fidelity.mode(Area::Lighting) == Mode::Modern {
            Some(Self::Raster)
        } else {
            None
        }
    }
}

/// A world mesh's lit stand-in. It has no parent, so scene preparation ignores it.
#[derive(Component)]
pub(crate) struct Proxy;

mod lights;
mod plugin;
mod proxies;
pub use plugin::ModernLightingPlugin;
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
    fn faithful_settings_select_no_tier() {
        let fidelity = fidelity(Mode::Faithful, Mode::Faithful);
        assert_eq!(Tier::select(&fidelity, &RayTracingSupport::Available), None);
    }

    #[test]
    fn modern_lighting_selects_the_raster_tier() {
        let fidelity = fidelity(Mode::Modern, Mode::Faithful);
        assert_eq!(
            Tier::select(&fidelity, &RayTracingSupport::Unavailable),
            Some(Tier::Raster)
        );
    }

    #[test]
    fn ray_tracing_needs_support_and_falls_back_to_raster() {
        let fidelity = fidelity(Mode::Modern, Mode::Modern);
        assert_eq!(
            Tier::select(&fidelity, &RayTracingSupport::Available),
            Some(Tier::Traced)
        );
        assert_eq!(
            Tier::select(
                &fidelity,
                &RayTracingSupport::MissingFeatures(String::new())
            ),
            Some(Tier::Raster)
        );
    }

    #[test]
    fn ray_tracing_alone_without_support_selects_nothing() {
        let fidelity = fidelity(Mode::Faithful, Mode::Modern);
        assert_eq!(
            Tier::select(&fidelity, &RayTracingSupport::Unavailable),
            None
        );
    }
}
