//! Modern water waves: two smooth crossing sine waves around the original mean height, moving
//! the game's "water" vertex set.
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

use bindings::cyrus::mods::components::{Scene, Vertices};
use bindings::wasvy::ecs::app::{App, Query, QueryFor, Schedule, System};

/// Exported scene units per WLD grid cell (256 engine units, 20 per scene unit).
const CELL_SIZE: f32 = 256.0 / 20.0;
/// The vertex set and the scene value this mod uses.
const WATER: &str = "water";
const WATER_WAVE: &str = "water-wave";

struct ModernWater;

impl bindings::Guest for ModernWater {
    fn setup(app: App) {
        let waves = System::new("waves");
        waves.add_query(&[
            QueryFor::Mut(Vertices::type_path()),
            QueryFor::Ref(Scene::type_path()),
        ]);
        app.add_systems(&Schedule::Update, &[&waves]);
    }

    fn waves(query: Query) {
        while let Some(row) = query.iter() {
            let vertices = Vertices::new(row.component(0));
            let scene = Scene::new(row.component(1));
            let generation = vertices.generation(WATER);
            let amplitude = scene.values(WATER_WAVE).first().copied().unwrap_or(0.0);
            let seconds = scene.seconds();
            let offsets: Vec<f32> = vertices
                .positions(WATER)
                .chunks_exact(3)
                .flat_map(|p| [0.0, height(amplitude, p[0], p[2], seconds), 0.0])
                .collect();
            if !offsets.is_empty() {
                vertices.displace(WATER, generation, &offsets);
            }
        }
    }
}

/// Vertical offset in scene units; `amplitude` is the world's WLD wave amplitude.
fn height(amplitude: f32, x: f32, z: f32, seconds: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let phase = (x + z * 0.35) / (CELL_SIZE * 6.0) * tau - seconds * 0.8;
    let cross = (z - x * 0.6) / (CELL_SIZE * 2.5) * tau - seconds * 1.3;
    amplitude / 20.0 * (0.5 + 0.35 * phase.sin() + 0.15 * cross.sin())
}

bindings::export!(ModernWater with_types_in bindings);
