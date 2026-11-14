//! Modern water: crossing sharpened waves, ripples, shore foam and wet sand on the terrain. The
//! effects live in this mod's `terrain_surface.wesl`, which replaces the game's neutral one; this
//! code turns them on through the terrain surface effect slot. The terrain shader moves the
//! waves, so they show only where the Lighting area is Modern, which draws the terrain that way.
use cyrus_mod_sdk::{Cx, Lighting, Mod, TerrainSurface};

struct ModernWater;

impl Mod for ModernWater {
    const ID: &'static str = "modern-water";

    fn frame(cx: &Cx) {
        // Without water in the world, or under the original lighting, there is nothing to change.
        let Some(water) = cx.environment.water() else {
            return;
        };
        if cx.environment.lighting() != Lighting::Modern {
            return;
        }
        // The world's amplitude in world file units; the shader works in scene units. The waves
        // move when the world has any amplitude and a grid to put them on.
        let amplitude = water.amplitude / 20.0;
        let waves = amplitude != 0.0 && water.cell > 0.0;
        cx.effects.terrain_surface(TerrainSurface {
            effects: true,
            amplitude: if waves { amplitude } else { 0.0 },
            cell: if waves { water.cell } else { 0.0 },
        });
    }
}

cyrus_mod_sdk::export_mod!(ModernWater);
