//! Retained actor light controls follow preservation's script light lifecycle.
use cyrus_assets::script::Instance;
use cyrus_assets::script::PlacementType;
use cyrus_soup::Native;
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
    /// Authored base strengths; absent when the original producer leaves them uninitialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starting_rgb: Option<[i16; 3]>,
    /// Script flicker parameters (`LightFlicker`): first value, phase, second value.
    #[serde(default)]
    pub flicker: [u16; 3],
    /// Script size-effect parameters (`LightSize`), laid out like `flicker`: first, phase, second.
    #[serde(default)]
    pub size: [u16; 3],
}

impl LightState {
    pub fn effect(
        record: &mut Option<Self>,
        position: [i32; 3],
        function: Native,
        arguments: &[cyrus_soup::Argument],
        command: &mut crate::script::NativeCommandState,
        ticks: Option<u32>,
        random: Option<&mut cyrus_game::random::EngineRandom>,
    ) -> anyhow::Result<bool> {
        if function == Native::FxFlickerOnOff {
            let [bound] = arguments else {
                anyhow::bail!("FxFlickerOnOff requires one argument");
            };
            Self::flicker_radius(record, position, bound.integer()?, random)?;
            return Ok(false);
        }
        if function == Native::FxFlickerDim {
            let [amount] = arguments else {
                anyhow::bail!("FxFlickerDim requires one argument");
            };
            let bound = amount.integer()?.wrapping_abs();
            anyhow::ensure!(
                bound > 0,
                "FxFlickerDim requires a nonzero representable absolute bound"
            );
            let light = record.get_or_insert_with(|| Self::dynamic(position));
            let base = light.starting_rgb.ok_or_else(|| {
                anyhow::anyhow!("FxFlickerDim starting RGB unavailable for this light record")
            })?;
            let random =
                random.ok_or_else(|| anyhow::anyhow!("FxFlickerDim shared random unavailable"))?;
            let decrement = random.rnd(bound)? as i16;
            light.rgb = base.map(|channel| channel.wrapping_sub(decrement).max(-255));
            return Ok(false);
        }
        anyhow::ensure!(
            function == Native::FxPhase,
            "unsupported light effect {function}"
        );
        let values = arguments
            .iter()
            .map(cyrus_soup::Argument::integer)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let [a, b, c, d, e, f, span] = values.as_slice() else {
            anyhow::bail!("FxPhase requires seven arguments");
        };
        let light = record.get_or_insert_with(|| Self::dynamic(position));
        if *span == 0 {
            return Ok(true);
        }
        let ticks = ticks.ok_or_else(|| anyhow::anyhow!("FxPhase nominal ticks unavailable"))?;
        command.continuation = command.continuation.wrapping_add(ticks);
        if (command.continuation as i32) > *span {
            command.continuation = 0;
            command.continuation_direction = command.continuation_direction.wrapping_add(1) & 1;
        }
        let (start, end) = if command.continuation_direction != 0 {
            ([*a, *b, *c], [*d, *e, *f])
        } else {
            ([*d, *e, *f], [*a, *b, *c])
        };
        let mut rgb = [0; 3];
        for axis in 0..3 {
            let difference = end[axis]
                .wrapping_shl(8)
                .wrapping_sub(start[axis].wrapping_shl(8));
            let slope = difference
                .checked_div(*span)
                .ok_or_else(|| anyhow::anyhow!("FxPhase signed division overflow unsupported"))?;
            let delta = slope.wrapping_mul(command.continuation as i32) >> 8;
            rgb[axis] = start[axis].wrapping_add(delta) as i16;
        }
        light.rgb = rgb;
        Ok(false)
    }
    pub fn flicker_radius(
        record: &mut Option<Self>,
        position: [i32; 3],
        bound: i32,
        random: Option<&mut cyrus_game::random::EngineRandom>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            bound > 0,
            "FxFlickerOnOff requires a positive bound, got {bound}"
        );
        let random =
            random.ok_or_else(|| anyhow::anyhow!("FxFlickerOnOff shared random unavailable"))?;
        let light = record.get_or_insert_with(|| Self::dynamic(position));
        if random.rnd(bound)? == 0 {
            light.radius = if light.radius == 0 {
                light.starting_radius
            } else {
                0
            };
        }
        Ok(())
    }

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
            starting_rgb: (placement.object_type == PlacementType::COLOURED_LIGHT)
                .then_some(placement.light_rgb),
            flicker: [0; 3],
            size: [0; 3],
        }
    }

    /// A script-created record: disabled and unlocked, intensity 256, radius 100, no offset.
    /// Its starting colour is unavailable because this producer does not initialize it.
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
            starting_rgb: None,
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

    pub(super) fn maintain_light(
        &mut self,
        ticks: u32,
        random: &mut cyrus_game::random::EngineRandom,
    ) -> anyhow::Result<()> {
        let Some(light) = &mut self.light else {
            return Ok(());
        };
        if light.enabled || light.locked {
            for (function, amplitude) in [
                (Native::LightFlicker, light.flicker[0] as i16),
                (Native::LightSize, light.size[0] as i16),
            ] {
                anyhow::ensure!(
                    amplitude >= 0,
                    "{function} negative amplitude {amplitude} unsupported"
                );
            }
        }
        if light.locked {
            light.enabled = true;
        }
        if !light.enabled {
            light.inactive_ticks = light.inactive_ticks.wrapping_add(ticks as u8);
            if light.inactive_ticks > 32 {
                self.light = None;
            }
            return Ok(());
        }
        for (words, starting, current) in [
            (
                &mut light.flicker,
                light.starting_intensity,
                &mut light.intensity,
            ),
            (&mut light.size, light.starting_radius, &mut light.radius),
        ] {
            if words[0] == 0 {
                continue;
            }
            words[1] = words[1].wrapping_add(ticks as u16);
            if words[1] as i16 >= words[2] as i16 {
                words[1] = 0;
                let offset = random.rnd(i32::from(words[0]))? - i32::from(words[0] / 2);
                *current = starting.wrapping_add(offset as i16);
            }
        }
        light.radius = light.radius.max(1);
        Ok(())
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
                if placement.object_type != PlacementType::COLOURED_LIGHT {
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
