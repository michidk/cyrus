//! World-local environmental effects follow preservation's SOUP pool and lifetime rules.
use super::ScriptWorld;
use anyhow::{Context, Result, ensure};
use cyrus_assets::effects::Effects;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One retained type-one effect. Positions are whole original map units, Y downward.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentalEffect {
    /// Lower sixteen bits select the image; upper bits select drawing behavior.
    pub descriptor: u32,
    pub position: [i32; 3],
    /// Lifetime frame, independent of the image's automatic playback frame.
    pub frame: i32,
    target: i32,
}

/// Slot continuation only; metadata and capacity come from the exact installed source.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct EffectsSnapshot {
    source: String,
    slots: Vec<Option<EffectSnapshot>>,
}
#[derive(Clone, Serialize, Deserialize)]
struct EffectSnapshot {
    descriptor: u32,
    position: [i32; 3],
    frame: i32,
}

/// Fixed-capacity slots in original retained-list traversal order.
pub struct EnvironmentalEffects {
    source: String,
    disabled: bool,
    images: BTreeMap<u16, (u32, i32)>,
    slots: Vec<Option<EnvironmentalEffect>>,
}
impl EnvironmentalEffects {
    fn prepare(source: &Effects) -> Result<Self> {
        source.validate()?;
        let mut slots = Vec::new();
        if !source.disabled {
            slots
                .try_reserve_exact(source.capacity as usize)
                .context("environment effect pool allocation failed")?;
            slots.resize(source.capacity as usize, None);
        }
        Ok(Self {
            source: Self::identity(source)?,
            disabled: source.disabled,
            images: source
                .images
                .iter()
                .map(|(&id, image)| {
                    Ok((
                        id,
                        (
                            image.size[1],
                            i32::try_from(image.frames.len() - 1)
                                .context("environment effect frame count outside signed range")?,
                        ),
                    ))
                })
                .collect::<Result<_>>()?,
            slots,
        })
    }
    fn identity(source: &Effects) -> Result<String> {
        Ok(blake3::hash(&serde_json::to_vec(source)?)
            .to_hex()
            .to_string())
    }
    pub(super) fn snapshot(&self, source: &Effects) -> Result<EffectsSnapshot> {
        ensure!(
            self.source == Self::identity(source)?,
            "environmental effect capture source changed"
        );
        Ok(EffectsSnapshot {
            source: self.source.clone(),
            slots: self
                .slots
                .iter()
                .map(|effect| {
                    effect.as_ref().map(|effect| EffectSnapshot {
                        descriptor: effect.descriptor,
                        position: effect.position,
                        frame: effect.frame,
                    })
                })
                .collect(),
        })
    }
    pub(super) fn from_snapshot(saved: EffectsSnapshot, source: &Effects) -> Result<Self> {
        let mut pool = Self::prepare(source)?;
        ensure!(
            saved.source == pool.source,
            "saved environmental effect source changed"
        );
        ensure!(
            saved.slots.len() == pool.slots.len(),
            "saved environmental effect slot count differs"
        );
        for (index, saved) in saved.slots.into_iter().enumerate() {
            let Some(saved) = saved else { continue };
            let &(_, target) = pool
                .images
                .get(&(saved.descriptor as u16))
                .with_context(|| {
                    format!(
                        "saved environmental effect slot {index}: texture {} unavailable",
                        saved.descriptor as u16
                    )
                })?;
            ensure!(
                saved.frame == -1
                    || (target == 0 && saved.frame == 0)
                    || (target > 0 && saved.frame <= target),
                "saved environmental effect slot {index}: frame {} exceeds lifetime target {target}",
                saved.frame
            );
            pool.slots[index] = Some(EnvironmentalEffect {
                descriptor: saved.descriptor,
                position: saved.position,
                frame: saved.frame,
                target,
            });
        }
        Ok(pool)
    }
    /// Active effects in retained pool order, with stable slot indices until world replacement.
    pub fn active(&self) -> impl Iterator<Item = (usize, &EnvironmentalEffect)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(slot, effect)| effect.as_ref().map(|effect| (slot, effect)))
    }
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }
    fn spawn(&mut self, descriptor: u32, point: [i32; 3]) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        let Some(slot) = self.slots.iter_mut().find(|effect| effect.is_none()) else {
            return Ok(());
        };
        let &(height, target) = self.images.get(&(descriptor as u16)).with_context(|| {
            format!(
                "environment effect texture {} unavailable",
                descriptor as u16
            )
        })?;
        let mut position = point;
        position[1] = position[1].wrapping_sub((height as i32).wrapping_mul(128));
        *slot = Some(EnvironmentalEffect {
            descriptor,
            position: position.map(|v| v >> 8),
            frame: 0,
            target,
        });
        Ok(())
    }
    fn advance(&mut self, ticks: u32) {
        for slot in &mut self.slots {
            if let Some(effect) = slot {
                // The selected-death constructor always has reference zero and speed one.
                if effect.frame != -1 && effect.target != 0 {
                    effect.frame = effect.frame.wrapping_add(ticks as i32);
                    if effect.frame > effect.target {
                        *slot = None;
                    }
                }
            }
        }
    }
}
// The effect helper uses the opposite angular convention to script query bearings.
pub(super) fn death_effect_point(point: [i32; 3], camera: [i32; 3]) -> [i32; 3] {
    let bearing = super::super::spatial::query_bearing(
        point[0].wrapping_sub(camera[0]),
        point[2].wrapping_sub(camera[2]),
    );
    let index = usize::from(bearing.wrapping_neg().wrapping_sub(1024) & 2047);
    let mut result = point;
    for (axis, sample) in [(0, index), (2, index + 512)] {
        let value = (f64::from(point[axis] as f32)
            + 2048.0 * f64::from(cyrus_world::transform::rotation_sample(sample)))
            as f32;
        // The original integer conversion returns signed minimum on overflow.
        result[axis] = if f64::from(value) >= f64::from(i32::MIN) && f64::from(value) < 2147483648.0
        {
            value as i32
        } else {
            i32::MIN
        };
    }
    result
}

impl ScriptWorld {
    /// Bind the exact installed source after its presentation host has prepared it.
    /// Rebinding preserves live slots and rejects changed source bytes within a world.
    pub fn prepare_environment_effects(&mut self, source: &Effects) -> Result<()> {
        if let Some(effects) = &self.effects {
            ensure!(
                effects.source == EnvironmentalEffects::identity(source)?,
                "world {}: environmental effect source changed",
                self.world
            );
        } else {
            self.effects = Some(
                EnvironmentalEffects::prepare(source)
                    .with_context(|| format!("world {} environmental effects", self.world))?,
            );
        }
        Ok(())
    }
    /// Absent until an effect presentation host has bound its installed source.
    pub fn environmental_effects(&self) -> Option<&EnvironmentalEffects> {
        self.effects.as_ref()
    }
    pub(super) fn spawn_death_effect(&mut self, descriptor: u32, point: [i32; 3]) -> Result<()> {
        self.effects
            .as_mut()
            .context("selected death environmental effect spawning unavailable")?
            .spawn(descriptor, point)
    }
    /// World-loop effect stage, after object/code-object updates and before scene drawing.
    pub fn advance_environment_effects(&mut self, ticks: u32) {
        if let Some(effects) = &mut self.effects {
            effects.advance(ticks);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_assets::effects::EffectImage;

    fn source(frame_count: usize) -> Effects {
        Effects {
            capacity: 2,
            disabled: false,
            images: BTreeMap::from([(
                0x2500,
                EffectImage {
                    name: "synthetic-effect".into(),
                    size: [1, 4],
                    texture_scale: 256,
                    delay_ms: 71,
                    flags: 1,
                    alpha: 151,
                    frames: vec![vec![255; 16]; frame_count],
                },
            )]),
        }
    }

    #[test]
    fn expired_slots_are_reused_in_retained_order() {
        let source = source(4);
        let mut effects = EnvironmentalEffects::prepare(&source).unwrap();
        effects.spawn(0x2500, [256, 0, -1]).unwrap();
        effects.advance(1);
        effects.spawn(0x12500, [512, 0, -257]).unwrap();
        effects.advance(3);
        assert_eq!(
            effects
                .active()
                .map(|(slot, effect)| (slot, effect.frame))
                .collect::<Vec<_>>(),
            [(1, 3)]
        );
        effects = EnvironmentalEffects::from_snapshot(effects.snapshot(&source).unwrap(), &source)
            .unwrap();
        effects.spawn(0x2500, [768, 0, -513]).unwrap();
        assert_eq!(
            effects
                .active()
                .map(|(slot, effect)| (slot, effect.descriptor, effect.position))
                .collect::<Vec<_>>(),
            [(0, 0x2500, [3, -2, -3]), (1, 0x12500, [2, -2, -2])]
        );
        effects.advance(1);
        assert_eq!(
            effects
                .active()
                .map(|(slot, effect)| (slot, effect.frame))
                .collect::<Vec<_>>(),
            [(0, 1)]
        );
    }

    #[test]
    fn single_frame_and_sentinel_effects_do_not_advance() {
        let mut effects = EnvironmentalEffects::prepare(&source(1)).unwrap();
        effects.spawn(0x2500, [0; 3]).unwrap();
        effects.advance(u32::MAX);
        assert_eq!(effects.active().next().unwrap().1.frame, 0);
        let mut effects = EnvironmentalEffects::prepare(&source(4)).unwrap();
        effects.spawn(0x2500, [0; 3]).unwrap();
        effects.slots[0].as_mut().unwrap().frame = -1;
        effects.advance(12);
        assert_eq!(effects.active().next().unwrap().1.frame, -1);
    }

    #[test]
    fn original_effect_position_vectors_preserve_y_and_float_store_rounding() {
        for (point, camera, expected) in [
            ([2560, -1536, 7680], [0; 3], [1917, -1536, 5735]),
            ([10000, 71, 0], [0; 3], [7952, 71, 0]),
            (
                [16777217, -19, -16777217],
                [2000, 900, 3000],
                [16775772, -19, -16775763],
            ),
        ] {
            assert_eq!(death_effect_point(point, camera), expected);
        }
    }
}
