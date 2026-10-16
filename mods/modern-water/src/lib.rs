//! Modern water waves: two smooth crossing sine waves around the original mean height.
mod bindings {
    wasvy_macros::guest_bindings!({
        path: ["../../crates/cyrus-mods/wit", "wit"],
        world: "cyrus:modern-water/guest",
        with: {
            "wasvy:ecs/app@0.0.9": generate,
            "cyrus:mods/components@0.1.0": generate,
        }
    });
}

use bindings::cyrus::mods::components::Water;
use bindings::wasvy::ecs::app::{App, Query, QueryFor, Schedule, System};
use std::cell::RefCell;

/// Exported scene units per WLD grid cell (256 engine units, 20 per scene unit).
const CELL_SIZE: f32 = 256.0 / 20.0;

thread_local! {
    /// Horizontal positions for the published generation; they change only when the camera window moves.
    static POSITIONS: RefCell<(Option<u32>, Vec<[f32; 2]>)> = const { RefCell::new((None, Vec::new())) };
}

struct ModernWater;

impl bindings::Guest for ModernWater {
    fn setup(app: App) {
        let waves = System::new("waves");
        waves.add_query(&[QueryFor::Mut(Water::type_path())]);
        app.add_systems(&Schedule::Update, &[&waves]);
    }

    fn waves(query: Query) {
        while let Some(row) = query.iter() {
            let water = Water::new(row.component(0));
            let generation = water.generation();
            let amplitude = water.parameters().first().copied().unwrap_or(0.0);
            let seconds = water.seconds();
            let heights: Vec<f32> = POSITIONS.with_borrow_mut(|(cached, positions)| {
                if *cached != Some(generation) {
                    *positions = water
                        .positions()
                        .chunks_exact(3)
                        .map(|p| [p[0], p[2]])
                        .collect();
                    *cached = Some(generation);
                }
                positions
                    .iter()
                    .map(|&[x, z]| height(amplitude, x, z, seconds))
                    .collect()
            });
            if !heights.is_empty() {
                water.answer(generation, &heights);
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
