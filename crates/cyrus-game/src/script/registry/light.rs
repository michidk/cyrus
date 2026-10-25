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
    /// Script flicker parameters (`LightFlicker`): first value, phase, second value.
    #[serde(default)]
    pub flicker: [u16; 3],
    /// Script size-effect parameters (`LightSize`), laid out like `flicker`: first, phase, second.
    #[serde(default)]
    pub size: [u16; 3],
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
            flicker: [0; 3],
            size: [0; 3],
        }
    }

    /// A script-created record: disabled and unlocked, intensity 256, radius 100, no offset.
    /// Position and colour maintenance for such lights is not implemented.
    pub fn dynamic(position: [i32; 3]) -> Self {
        Self {
            locked: false,
            enabled: false,
            inactive_ticks: 0,
            offset: [0; 3],
            position,
            starting_intensity: 256,
            intensity: 256,
            starting_radius: 100,
            radius: 100,
            rgb: [0; 3],
            flicker: [0; 3],
            size: [0; 3],
        }
    }
}

impl super::ScriptActor {
    /// The actor's retained light record as inspection JSON, when it has one.
    pub fn light_state(&self) -> Option<serde_json::Value> {
        self.light
            .as_ref()
            .map(|light| serde_json::to_value(light).expect("light state serializes"))
    }

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
            .enumerate()
            .filter_map(|(index, actor)| {
                let placement = actor.placement()?;
                if placement.object_type != 6 {
                    return None;
                }
                let id = self.id(index).ok()?;
                let light = actor.light.as_ref()?;
                match self.spatial(id) {
                    Ok(spatial) => light.rendered_at(spatial),
                    Err(_) => light.rendered(),
                }
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

    pub(super) fn rendered_at(
        &self,
        spatial: &crate::script::spatial::ActorSpatial,
    ) -> Option<cyrus_assets::environment::LocalLight> {
        self.rendered_pose(spatial.effective.unwrap_or(spatial.local))
    }

    pub(super) fn rendered_pose(
        &self,
        pose: crate::script::spatial::Pose,
    ) -> Option<cyrus_assets::environment::LocalLight> {
        if !self.enabled {
            return None;
        }
        let rotated = pose.matrix.transform(self.offset.map(i32::from)).ok()?;
        Some(cyrus_assets::environment::LocalLight {
            position: std::array::from_fn(|axis| {
                pose.position[axis].wrapping_add(rotated[axis]) >> 8
            }),
            radius: self.radius,
            rgb: self.rgb,
        })
    }
}
