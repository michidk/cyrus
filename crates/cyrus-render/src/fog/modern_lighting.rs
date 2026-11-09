//! Modern lighting: the world lit by real-time PBR shading, selected by `Area::Lighting`.
//!
//! **Modern only.** The original shades models and terrain with its own vertex lighting, fog,
//! clipping and palette arithmetic in custom forward shaders. While the area is Modern, those
//! same materials compile with the `MODERN_LIGHTING` shader define: they keep their texture
//! sampling, chroma keys and scene fog, and replace the vertex lighting with Bevy's PBR lighting
//! (a cascaded shadow-mapped sun, whose cascades draw shadow-only twins of the casters
//! (`shadows`), the ambient colour as an environment light and the object lights as point
//! lights), with screen-space ambient occlusion, contact shadows, anti-aliasing and bloom on the
//! camera. Static scene models whose remaining material values are equal draw through one shared
//! material (`sharing`), so the camera's passes batch them. The models draw their full
//! world-space meshes and the terrain its unprojected mesh, instead of the original's per-frame
//! camera-space preparation, so they can cast shadows and fill the depth prepass. Water is glossy;
//! flats and the sky keep the original's shading. The sun, the ambient colour and the object
//! lights come from the world's environment.
//!
//! The day/night cycle (`day_night`) moves and tints these lights by the hour, and `effects`
//! holds the optional effects: tonemapping and grading, automatic exposure, god rays, the sky
//! glow, water reflections and the lens flare (`post`), swaying plants (`foliage`), depth of
//! field and motion blur. At night the player carries a candle (`candle`).
mod candle;
mod day_night;
mod effects;
mod foliage;
mod glow;
mod lights;
mod plugin;
mod post;
mod shadows;
mod sharing;
pub use day_night::{DayNight, Daylight, SkyLight};
pub use effects::{Effect, ModernEffects, Tonemapper};
pub use plugin::ModernLightingPlugin;
