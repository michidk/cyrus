//! Modern lighting: the world lit by real-time PBR shading, selected by `Area::Lighting`.
//!
//! **Modern only.** The original shades models and terrain with its own fog, clipping and
//! palette arithmetic in custom forward shaders. While the area is Modern, each opaque world
//! mesh gets a stand-in entity (a mesh without the original's baked colours and projection
//! attributes, and a plain `StandardMaterial`), and the shader-driven originals stop drawing.
//! The stand-ins are shaded with Bevy's forward PBR: a cascaded shadow-mapped sun, the ambient
//! colour as an environment light and the object lights as point lights, with screen-space
//! ambient occlusion, contact shadows, anti-aliasing and bloom on the camera. Flats, water,
//! sky, alpha-masked or chroma-keyed meshes and the HUD keep their forward shaders and draw
//! over the lit scene. The sun, the ambient colour and the object lights come from the
//! world's environment.
use bevy::prelude::*;

/// A world mesh's lit stand-in. It has no parent, so scene preparation ignores it.
#[derive(Component)]
pub(crate) struct Proxy;

mod lights;
mod plugin;
mod proxies;
pub use plugin::ModernLightingPlugin;
