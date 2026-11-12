//! Modern water: crossing sharpened waves, ripples, shore foam and wet sand on the terrain. The
//! effects live in this mod's `water_surface.wesl`, which replaces the game's neutral one; this
//! code turns them on through the "water-surface" shader input. The terrain shader moves the
//! waves, so they show only where the Lighting area is Modern, which draws the terrain that way.
use cyrus_mod_sdk::published::{scene_value, shader_input};
use cyrus_mod_sdk::{Cx, Mod};

struct ModernWater;

impl Mod for ModernWater {
    const ID: &'static str = "modern-water";

    fn frame(cx: &Cx) {
        // Without water in the world, or under the original lighting, there is nothing to change.
        let Some(amplitude) = cx.scene.value(scene_value::WATER_WAVE) else {
            return;
        };
        if cx.scene.value(scene_value::LIGHTING_MODERN) != Some(1.0) {
            return;
        }
        // The world's amplitude in WLD units; the shader works in scene units.
        let amplitude = amplitude / 20.0;
        let cell = cx.scene.value(scene_value::WATER_CELL).unwrap_or(0.0);
        // Effects on; the waves move when the world has any amplitude and a grid to put them on.
        let waves = if amplitude != 0.0 && cell > 0.0 {
            [amplitude, cell]
        } else {
            [0.0, 0.0]
        };
        cx.uniforms
            .set(shader_input::WATER_SURFACE, [1.0, 0.0, waves[0], waves[1]]);
    }
}

cyrus_mod_sdk::export_mod!(ModernWater);
