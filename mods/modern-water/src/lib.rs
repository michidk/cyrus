//! Modern water: crossing sharpened waves, ripples, shore foam and wet sand on the terrain. The
//! effects live in this mod's `water_surface.wesl`, which replaces the game's neutral one; this
//! code turns them on through the "water-surface" shader input. Where the terrain shader cannot
//! move the waves (Lighting is not Modern, which draws the terrain a different way), it moves the
//! "water" vertex set itself.
mod bindings {
    wasvy_macros::guest_bindings!({
        path: ["../../crates/cyrus-mods/wit", "wit"],
        world: "cyrus:modern-water/guest",
        with: {
            "wasvy:ecs/app@0.0.11": generate,
            "cyrus:mods/components@0.1.0": generate,
        }
    });
}

use bindings::cyrus::mods::components::{Scene, Uniforms, Vertices};
use bindings::wasvy::ecs::app::{App, Query, QueryFor, Schedule, System};

/// The vertex set, scene values and shader input this mod uses.
const WATER: &str = "water";
const WATER_WAVE: &str = "water-wave";
const WATER_CELL: &str = "water-cell";
const LIGHTING_MODERN: &str = "lighting-modern";
const WATER_SURFACE: &str = "water-surface";

/// Each wave's direction, wavelength in grid cells, speed in cells per second and share of the
/// amplitude. The same four waves as `water_surface.wesl`.
const WAVES: [([f32; 2], f32, f32, f32); 4] = [
    ([0.940, 0.342], 11.0, 1.10, 0.20),
    ([0.643, -0.766], 7.0, 0.95, 0.13),
    ([-0.259, 0.966], 5.0, 0.80, 0.09),
    ([0.996, -0.087], 4.0, 0.70, 0.08),
];

struct ModernWater;

impl bindings::Guest for ModernWater {
    fn setup(app: App) {
        let waves = System::new("waves");
        waves.add_query(&[
            QueryFor::Mut(Vertices::type_path()),
            QueryFor::Ref(Scene::type_path()),
            QueryFor::Mut(Uniforms::type_path()),
        ]);
        app.add_systems(&Schedule::Update, &[&waves]);
    }

    fn waves(query: Query) {
        while let Some(row) = query.iter() {
            let vertices = Vertices::new(row.component(0));
            let scene = Scene::new(row.component(1));
            let uniforms = Uniforms::new(row.component(2));
            // Without water in the world there is nothing to change.
            let Some(&amplitude) = scene.values(WATER_WAVE).first() else {
                continue;
            };
            let amplitude = amplitude / 20.0;
            let cell = scene.values(WATER_CELL).first().copied().unwrap_or(0.0);
            let lighting = scene.values(LIGHTING_MODERN).first().copied() == Some(1.0);
            if lighting && amplitude != 0.0 && cell > 0.0 {
                uniforms.set(WATER_SURFACE, &[1.0, 0.0, amplitude, cell]);
                continue;
            }
            uniforms.set(WATER_SURFACE, &[1.0, 0.0, 0.0, 0.0]);
            let generation = vertices.generation(WATER);
            let seconds = scene.seconds();
            let offsets: Vec<f32> = vertices
                .positions(WATER)
                .chunks_exact(3)
                .flat_map(|p| [0.0, height(amplitude, cell, p[0], p[2], seconds), 0.0])
                .collect();
            if !offsets.is_empty() {
                vertices.displace(WATER, generation, &offsets);
            }
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

bindings::export!(ModernWater with_types_in bindings);
