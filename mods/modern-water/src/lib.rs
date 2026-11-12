//! Modern water: crossing sharpened waves, ripples, shore foam and wet sand on the terrain. The
//! effects live in this mod's `water_surface.wesl`, which replaces the game's neutral one; this
//! code turns them on through the "water-surface" shader input. Where the terrain shader cannot
//! move the waves (Lighting is not Modern, which draws the terrain a different way), it moves the
//! "water" vertex set itself.
use cyrus_mod_sdk::published::{scene_value, shader_input, vertex_set};
use cyrus_mod_sdk::{Cx, Mod, Vec3};

/// Each wave's direction, wavelength in grid cells, speed in cells per second and share of the
/// amplitude. The same four waves as `water_surface.wesl`.
const WAVES: [([f32; 2], f32, f32, f32); 4] = [
    ([0.940, 0.342], 11.0, 1.10, 0.20),
    ([0.643, -0.766], 7.0, 0.95, 0.13),
    ([-0.259, 0.966], 5.0, 0.80, 0.09),
    ([0.996, -0.087], 4.0, 0.70, 0.08),
];

struct ModernWater;

impl Mod for ModernWater {
    const ID: &'static str = "modern-water";

    fn frame(cx: &Cx) {
        // Without water in the world there is nothing to change.
        let Some(amplitude) = cx.scene.value(scene_value::WATER_WAVE) else {
            return;
        };
        let amplitude = amplitude / 20.0;
        let cell = cx.scene.value(scene_value::WATER_CELL).unwrap_or(0.0);
        let lighting = cx.scene.value(scene_value::LIGHTING_MODERN) == Some(1.0);
        if lighting && amplitude != 0.0 && cell > 0.0 {
            cx.uniforms
                .set(shader_input::WATER_SURFACE, [1.0, 0.0, amplitude, cell]);
            return;
        }
        cx.uniforms
            .set(shader_input::WATER_SURFACE, [1.0, 0.0, 0.0, 0.0]);
        let generation = cx.vertices.generation(vertex_set::WATER);
        let seconds = cx.scene.seconds();
        let offsets: Vec<Vec3> = cx
            .vertices
            .positions(vertex_set::WATER)
            .iter()
            .map(|p| Vec3::new(0.0, height(amplitude, cell, p.x, p.z, seconds), 0.0))
            .collect();
        if !offsets.is_empty() {
            cx.vertices
                .displace(vertex_set::WATER, generation, &offsets);
        }
    }
}

/// Vertical offset in scene units at an open-sea position in scene units: crossing sharpened
/// waves around the original mean height. `amplitude` is the world's wave amplitude in scene
/// units, `cell` the grid cell size.
fn height(amplitude: f32, cell: f32, x: f32, z: f32, seconds: f32) -> f32 {
    use std::f32::consts::TAU;
    let height = WAVES
        .iter()
        .fold(0.5, |height, &(direction, length, speed, share)| {
            let k = TAU / (length * cell);
            let phase = (x * direction[0] + z * direction[1]) * k - seconds * speed * TAU / length;
            let s = phase.sin();
            height + share * s * (1.0 - 0.25 * (1.0 - s.abs()))
        });
    amplitude * height
}

cyrus_mod_sdk::export_mod!(ModernWater);
