//! Retained actor light controls follow preservation's script light lifecycle.
use cyrus_assets::script::Instance;
use serde::{Deserialize, Serialize};

/// Logical light state stays with its actor across world retention and replacement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct LightState {
    pub locked: bool,
    pub enabled: bool,
    pub inactive_ticks: u8,
    pub offset: [i16; 3],
    pub position: [i32; 3],
    pub starting_intensity: i16,
    pub intensity: i16,
    pub starting_radius: i16,
    pub radius: i16,
    pub rgb: [i16; 3],
}

impl LightState {
    pub fn authored(placement: &Instance) -> Self {
        Self {
            locked: true,
            enabled: true,
            inactive_ticks: 0,
            offset: [0; 3],
            position: placement.map_position.map(|value| value as i32),
            starting_intensity: placement.light_intensity,
            intensity: placement.light_intensity,
            starting_radius: placement.light_radius,
            radius: placement.light_radius,
            rgb: placement.light_rgb,
        }
    }
}

impl super::ScriptActor {
    pub(super) fn maintain_light(&mut self, ticks: u32) {
        let Some(light) = &mut self.light else {
            return;
        };
        if light.locked {
            light.enabled = true;
        }
        if !light.enabled {
            light.inactive_ticks = light.inactive_ticks.wrapping_add(ticks as u8);
            if light.inactive_ticks > 32 {
                self.light = None;
            }
        }
    }
}

impl super::ScriptWorld {
    /// Enabled authored object lights in original map order. Runtime mutation and
    /// region selection remain separate from this retained-state projection.
    pub fn object_lights(&self) -> Vec<cyrus_assets::environment::LocalLight> {
        self.actors
            .iter()
            .filter_map(|actor| {
                let placement = actor.placement()?;
                if placement.object_type != 6 {
                    return None;
                }
                let light = actor.light.as_ref()?;
                light.rendered()
            })
            .collect()
    }
}

impl LightState {
    pub(super) fn rendered(&self) -> Option<cyrus_assets::environment::LocalLight> {
        self.enabled
            .then_some(cyrus_assets::environment::LocalLight {
                position: self.position,
                radius: self.radius,
                rgb: self.rgb,
            })
    }
}
