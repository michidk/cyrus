//! World-local environmental effects follow preservation's SOUP pool and lifetime rules.
//!
//! Lifetime updates release slots without erasing their retained drawing state.
//! Scene drawing separately applies alternate-path Y movement before admission,
//! including released records while another slot remains active. Snapshots retain
//! both active and released records; older active-only snapshots still restore.
//! Hosts call `ScriptWorld::draw_environment_effects` once per actual scene draw.
//! Source: preservation's SOUP environmental effect frame-submission contract.
//!
//! Animation actor events execute at their command, before subsequent sounds and
//! water/death pool allocations. Playback borrows actor-local inputs separately
//! from the world's effect pool and camera so earlier effects survive later errors.
//! See preservation's [actor-event rules](https://michidk.github.io/redguard-preservation/engine/animation.html#actor-events).
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_assets::effects::Effects;
use cyrus_assets::surface::Surface;
use cyrus_world::heading::wrap;
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
    speed: i32,
    active: bool,
}

impl EnvironmentalEffect {
    /// Descriptor bit selecting the alternate drawing path: the retained frame is drawn
    /// explicitly at half geometry scale, and the record rises by `nominal_ticks + 2` per draw pass.
    pub const ALTERNATE_DRAW: u32 = 0x20000;
    /// Descriptor bit selecting caller scale 90 instead of 256 on the ordinary path.
    pub const REDUCED_SCALE: u32 = 0x10000;
}

impl EffectsSnapshot {
    /// The saved active type-two records, in slot order.
    pub(super) fn light_effects(&self) -> impl Iterator<Item = &LightEffect> {
        self.lights.iter().flatten().filter(|light| light.active)
    }
}

/// One modulated parameter of a type-two light effect: its starting value, the current value
/// the scene reads, and the amplitude, period and phase that retime the next random value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightChannel {
    pub start: i32,
    pub current: i32,
    pub amplitude: i32,
    pub period: i32,
    pub phase: i32,
}

impl LightChannel {
    fn new(value: i32, amplitude: i32) -> Self {
        Self {
            start: value,
            current: value,
            amplitude,
            period: 1,
            phase: 0,
        }
    }

    /// One update: a zero amplitude leaves the channel alone; otherwise the phase adds the ticks
    /// and, once it reaches the period, restarts and draws `start + Rnd(amplitude) - amplitude/2`.
    fn modulate(
        &mut self,
        ticks: u32,
        random: &mut cyrus_game::random::EngineRandom,
    ) -> Result<()> {
        if self.amplitude <= 0 {
            return Ok(());
        }
        self.phase = self.phase.wrapping_add(ticks as i32);
        if self.phase >= self.period {
            self.phase = 0;
            self.current = self
                .start
                .wrapping_add(random.rnd(self.amplitude)?)
                .wrapping_sub(self.amplitude >> 1);
        }
        Ok(())
    }
}

/// One retained type-two (light) effect record. A released record keeps its fields, including the
/// position `AddLightEffect` never writes, for the next allocation of the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightEffect {
    /// The record's XYZ, which `AddLightEffect` does not set: zero in a fresh pool.
    pub position: [i32; 3],
    /// Signed ticks left; the record ends at or below zero.
    pub lifetime: i32,
    pub active: bool,
    /// The channel whose low word is the light radius; the light submits only while it is positive.
    pub radius: LightChannel,
    /// The second channel, modulated before the radius channel; scene submission ignores it.
    pub secondary: LightChannel,
}

/// Slot continuation only; metadata and capacity come from the exact installed source.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct EffectsSnapshot {
    source: String,
    slots: Vec<Option<EffectSnapshot>>,
    #[serde(default)]
    lights: Vec<Option<LightEffect>>,
}
#[derive(Clone, Serialize, Deserialize)]
struct EffectSnapshot {
    #[serde(default = "active_by_default", skip_serializing_if = "is_active")]
    active: bool,
    descriptor: u32,
    position: [i32; 3],
    frame: i32,
    #[serde(default = "default_speed")]
    speed: i32,
}

fn default_speed() -> i32 {
    1
}

fn active_by_default() -> bool {
    true
}
fn is_active(active: &bool) -> bool {
    *active
}

/// Fixed-capacity slots in original retained-list traversal order.
pub struct EnvironmentalEffects {
    source: String,
    disabled: bool,
    images: BTreeMap<u16, (u32, i32)>,
    slots: Vec<Option<EnvironmentalEffect>>,
    /// Type-two records; they share the pool's capacity with the type-one slots.
    lights: Vec<Option<LightEffect>>,
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
            lights: Vec::new(),
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
                        active: effect.active,
                        descriptor: effect.descriptor,
                        position: effect.position,
                        frame: effect.frame,
                        speed: effect.speed,
                    })
                })
                .collect(),
            lights: self.lights.clone(),
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
        ensure!(
            saved.lights.len() <= pool.slots.len(),
            "saved light effect slot count exceeds the pool"
        );
        pool.lights = saved.lights;
        ensure!(
            pool.slots
                .iter()
                .flatten()
                .filter(|effect| effect.active)
                .count()
                + pool
                    .lights
                    .iter()
                    .flatten()
                    .filter(|light| light.active)
                    .count()
                <= pool.slots.len()
                || pool.slots.is_empty(),
            "saved effects exceed the pool capacity"
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
                if saved.active {
                    saved.frame == -1
                        || (target == 0 && saved.frame == 0)
                        || (target > 0 && saved.frame <= target)
                } else {
                    target > 0 && saved.frame > target
                },
                "saved environmental effect slot {index}: frame {} disagrees with lifetime target {target} and active state",
                saved.frame
            );
            pool.slots[index] = Some(EnvironmentalEffect {
                descriptor: saved.descriptor,
                position: saved.position,
                frame: saved.frame,
                target,
                speed: saved.speed,
                active: saved.active,
            });
        }
        Ok(pool)
    }
    /// Active effects in retained pool order, with stable slot indices until world replacement.
    pub fn active(&self) -> impl Iterator<Item = (usize, &EnvironmentalEffect)> {
        self.slots.iter().enumerate().filter_map(|(slot, effect)| {
            effect
                .as_ref()
                .filter(|effect| effect.active)
                .map(|effect| (slot, effect))
        })
    }
    pub fn is_empty(&self) -> bool {
        self.active().next().is_none()
    }
    fn spawn(&mut self, descriptor: u32, point: [i32; 3]) -> Result<()> {
        self.spawn_with_speed(descriptor, point, 1, 0, None)
    }

    /// Allocates the first free record and places it at `point` (Q8). A zero `spread` lowers the
    /// point by half the image height; a nonzero one instead adds `rnd(spread >> 7) - (spread >> 8)`
    /// per axis (x, y, z), drawn after the record and image are found, and lowers nothing. A spread
    /// below 128 gives a nonpositive bound, which the original's reduction never leaves.
    fn spawn_with_speed(
        &mut self,
        descriptor: u32,
        point: [i32; 3],
        speed: i32,
        spread: i32,
        random: Option<&mut cyrus_game::random::EngineRandom>,
    ) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|effect| effect.as_ref().is_none_or(|effect| !effect.active))
        else {
            return Ok(());
        };
        let &(height, target) = self.images.get(&(descriptor as u16)).with_context(|| {
            format!(
                "environment effect texture {} unavailable",
                descriptor as u16
            )
        })?;
        let position = if spread == 0 {
            let mut position = point.map(|v| v >> 8);
            position[1] = point[1].wrapping_sub((height as i32).wrapping_mul(128)) >> 8;
            position
        } else {
            let random =
                random.context("AddFlatEffect: shared engine random stream unavailable")?;
            let bound = spread >> 7;
            ensure!(
                bound > 0,
                "AddFlatEffect: spread {spread} gives a nonpositive random bound, which the original never leaves"
            );
            let mut position = [0; 3];
            for (axis, value) in position.iter_mut().enumerate() {
                *value =
                    (point[axis] >> 8).wrapping_add(random.rnd(bound)?.wrapping_sub(spread >> 8));
            }
            position
        };
        *slot = Some(EnvironmentalEffect {
            descriptor,
            position,
            frame: 0,
            target,
            speed,
            active: true,
        });
        Ok(())
    }
    /// `AddLightEffect`: allocate a type-two record in the first free light slot while the pool
    /// has a free record. The record's position is not written (a reused slot keeps its old one);
    /// `d` and `f` start the two channels, `e` and `g` are their amplitudes and `h` the lifetime.
    /// Returns the record's one-based slot, zero when the pool is disabled or full.
    fn add_light(&mut self, [d, e, f, g, h]: [i32; 5]) -> Result<i32> {
        if self.disabled {
            return Ok(0);
        }
        ensure!(
            e >= 0 && g >= 0,
            "AddLightEffect: a negative modulation amplitude has no recovered behavior"
        );
        let active = self
            .slots
            .iter()
            .flatten()
            .filter(|effect| effect.active)
            .count()
            + self
                .lights
                .iter()
                .flatten()
                .filter(|light| light.active)
                .count();
        if active >= self.slots.len() {
            return Ok(0);
        }
        self.lights.resize(self.slots.len(), None);
        let Some(slot) = self
            .lights
            .iter()
            .position(|light| light.is_none_or(|light| !light.active))
        else {
            return Ok(0);
        };
        let position = self.lights[slot].map_or([0; 3], |light| light.position);
        self.lights[slot] = Some(LightEffect {
            position,
            lifetime: h,
            active: true,
            radius: LightChannel::new(d, e),
            secondary: LightChannel::new(f, g),
        });
        Ok(slot as i32 + 1)
    }
    /// Lifetime then modulation of the type-two records: a record at or below zero ends before
    /// either channel advances; the `f`/`g` channel advances before the `d`/`e` channel.
    fn advance_lights(
        &mut self,
        ticks: u32,
        random: &mut cyrus_game::random::EngineRandom,
    ) -> Result<()> {
        for light in self
            .lights
            .iter_mut()
            .flatten()
            .filter(|light| light.active)
        {
            light.lifetime = light.lifetime.wrapping_sub(ticks as i32);
            if light.lifetime <= 0 {
                light.active = false;
                continue;
            }
            light.secondary.modulate(ticks, random)?;
            light.radius.modulate(ticks, random)?;
        }
        Ok(())
    }
    /// Active type-two records in slot order.
    pub fn light_effects(&self) -> impl Iterator<Item = &LightEffect> {
        self.lights.iter().flatten().filter(|light| light.active)
    }
    fn prepare_draw(&mut self, ticks: u32) {
        self.prepare_draws(ticks, 1);
    }
    fn prepare_draws(&mut self, ticks: u32, draws: u32) {
        if self.disabled || self.is_empty() {
            return;
        }
        for effect in self.slots.iter_mut().flatten() {
            if effect.descriptor & EnvironmentalEffect::ALTERNATE_DRAW != 0 {
                effect.position[1] = effect.position[1]
                    .wrapping_sub(ticks.wrapping_add(draws.wrapping_mul(2)) as i32);
            }
        }
    }
    fn advance(&mut self, ticks: u32) {
        for slot in &mut self.slots {
            if let Some(effect) = slot
                && effect.active
            {
                // Effect frames advance by nominal ticks multiplied by the retained animation speed.
                if effect.descriptor as u16 != 0 && effect.frame != -1 && effect.target != 0 {
                    effect.frame = effect
                        .frame
                        .wrapping_add((ticks as i32).wrapping_mul(effect.speed));
                    if effect.frame > effect.target {
                        effect.active = false;
                    }
                }
            }
        }
    }
}
/// One authored actor event (animation opcode eleven) with the emitting actor's stored sound
/// surface and local Q8 position when its command ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorEvent {
    pub actor: ScriptActorId,
    pub flags: u32,
    pub surface: Option<cyrus_assets::surface::Surface>,
    pub position: Option<[i32; 3]>,
}

/// Event bits that spawn an effect, in evaluation order: (bit, required stored surface,
/// descriptor). The surfaces are lava (5), water (1) twice and sand (6); descriptor 0x2500 is also
/// the one the landing check requests through the same effect point.
const ACTOR_EVENT_EFFECTS: [(u32, Surface, u32); 4] = [
    (1, Surface::LAVA, 0x2580),
    (2, Surface::WATER, 0x2500),
    (4, Surface::WATER, 0x2500),
    (8, Surface::SAND, 0x2700),
];
/// Event bit that adds a camera shake impulse, after the effect bits.
const ACTOR_EVENT_SHAKE: u32 = 0x10;

// The effect helper uses the opposite angular convention to script query bearings.
pub(super) fn death_effect_point(point: [i32; 3], camera: [i32; 3]) -> [i32; 3] {
    let bearing = super::super::spatial::query_bearing(
        point[0].wrapping_sub(camera[0]),
        point[2].wrapping_sub(camera[2]),
    );
    displaced(
        point,
        usize::from(wrap(bearing.wrapping_neg().wrapping_sub(1024))),
        2048.0,
    )
}

/// Move the horizontal axes along rotation-table `index` (x from the sample, z from the sample a
/// quarter turn on), with the original's float stores and integer conversion.
fn displaced(point: [i32; 3], index: usize, distance: f64) -> [i32; 3] {
    let mut result = point;
    for (axis, sample) in [(0, index), (2, index + 512)] {
        let value = (f64::from(point[axis] as f32)
            + distance * f64::from(cyrus_world::transform::rotation_sample(sample)))
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

/// Actor-local inputs retained while animation playback mutably owns its cursor.
/// Capturing absent inputs is allowed: zero, unknown and surface-mismatched bits
/// never query collision, camera or pool services.
pub(super) struct ActorEventContext {
    actor: ScriptActorId,
    surface: Option<cyrus_assets::surface::Surface>,
    position: Option<[i32; 3]>,
    height: Option<i32>,
    camera_ready: bool,
}

/// Event owners borrowed separately from a combat receiver's actor and globals.
pub(super) struct ActorEventDispatch<'a> {
    pub context: ActorEventContext,
    pub camera: Option<&'a mut super::super::camera::CameraState>,
    pub effects: &'a mut Option<EnvironmentalEffects>,
}

impl ActorEventDispatch<'_> {
    pub(super) fn dispatch(&mut self, flags: u32) -> Result<()> {
        self.context
            .dispatch(flags, self.camera.as_deref_mut(), self.effects)
    }
}

impl ActorEventContext {
    pub(super) fn capture(world: &ScriptWorld, actor: ScriptActorId) -> Self {
        Self {
            actor,
            surface: world.actors[actor.index].surface,
            position: super::animation_sound_position(world.spatial.as_ref(), actor.index),
            height: world.actors[actor.index]
                .collision
                .as_ref()
                .map(|shape| shape.positive_extent[1]),
            camera_ready: world
                .camera
                .as_ref()
                .is_some_and(|camera| world.spatial(camera.camera_actor).is_ok()),
        }
    }

    pub(super) fn dispatch(
        &self,
        flags: u32,
        mut camera: Option<&mut super::super::camera::CameraState>,
        effects: &mut Option<EnvironmentalEffects>,
    ) -> Result<()> {
        for (bit, surface, descriptor) in ACTOR_EVENT_EFFECTS {
            if flags & bit == 0 {
                continue;
            }
            let stored = self.surface.with_context(|| {
                format!(
                    "actor {} event: stored surface unavailable",
                    self.actor.index
                )
            })?;
            if stored != surface {
                continue;
            }
            let position = self.position.with_context(|| {
                format!(
                    "actor {} event: logical position unavailable",
                    self.actor.index
                )
            })?;
            let camera = camera
                .as_ref()
                .context("actor event published camera unavailable")?;
            ensure!(self.camera_ready, "actor event camera pose not initialized");
            let mut point = death_effect_point(position, camera.current.position);
            let height = self
                .height
                .context("ordinary collision shape not prepared")?;
            point[1] = point[1]
                .wrapping_add(height.wrapping_mul(256))
                .wrapping_add(1536);
            effects
                .as_mut()
                .context("actor event environmental effect spawning unavailable")?
                .spawn(descriptor, point)?;
        }
        if flags & ACTOR_EVENT_SHAKE != 0 {
            camera
                .as_mut()
                .context("actor event camera session unavailable")?
                .add_shake();
        }
        Ok(())
    }
}

impl ScriptWorld {
    /// Bind the exact installed source before model or script effect allocation.
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
    /// Absent until a host has bound its installed source; independent of GPU readiness.
    pub fn environmental_effects(&self) -> Option<&EnvironmentalEffects> {
        self.effects.as_ref()
    }
    /// Prepare one actual scene draw, independently of lifetime updates.
    /// Alternate records move upward by nominal ticks plus two, including free
    /// retained records while the pool has an active slot. Call once per draw.
    /// Returned records remain in pool order; image admission belongs to rendering.
    /// Verified: preservation's environmental effect frame-submission contract.
    pub fn draw_environment_effects(
        &mut self,
        nominal_ticks: u32,
    ) -> impl Iterator<Item = (usize, &EnvironmentalEffect)> {
        if let Some(effects) = &mut self.effects {
            effects.prepare_draw(nominal_ticks);
        }
        self.effects.iter().flat_map(EnvironmentalEffects::active)
    }
    /// Apply the published nominal-tick and per-draw position terms separately.
    /// Presentation hosts may normalize draw cadence without deferring nominal
    /// movement across a later birth, release or slot reuse. `draws` can be zero;
    /// callers must supply each interval before the next object/lifetime update.
    /// This advances neither lifetime nor image playback. Active slots stay ordered.
    pub fn prepare_environment_effect_draws(
        &mut self,
        nominal_ticks: u32,
        draws: u32,
    ) -> impl Iterator<Item = (usize, &EnvironmentalEffect)> {
        if let Some(effects) = &mut self.effects {
            effects.prepare_draws(nominal_ticks, draws);
        }
        self.effects.iter().flat_map(EnvironmentalEffects::active)
    }
    pub(super) fn spawn_environment_effect(
        &mut self,
        descriptor: u32,
        point: [i32; 3],
    ) -> Result<()> {
        self.spawn_environment_effect_with_speed(descriptor, point, 1, 0, None)
    }

    pub(super) fn spawn_environment_effect_with_speed(
        &mut self,
        descriptor: u32,
        point: [i32; 3],
        speed: i32,
        spread: i32,
        random: Option<&mut cyrus_game::random::EngineRandom>,
    ) -> Result<()> {
        self.effects
            .as_mut()
            .context("environmental effect spawning unavailable")?
            .spawn_with_speed(descriptor, point, speed, spread, random)
    }
    /// Apply explicitly supplied inspection events in order. Automatic animation
    /// dispatch uses the borrowed command handler and never queues these records.
    /// Each surface-gated bit spawns its effect at
    /// the actor's position (the position captured with the event, else the current one) only when
    /// the actor's stored surface matches; the shake bit adds one camera shake impulse. A bit that
    /// needs unavailable surface, camera, extent or effect-pool state fails instead of skipping.
    pub fn apply_actor_events(&mut self, events: &[ActorEvent]) -> Result<()> {
        for event in events {
            self.resolve(event.actor)?;
            let mut context = ActorEventContext::capture(self, event.actor);
            context.surface = event.surface;
            context.position = event.position.or(context.position);
            context.dispatch(event.flags, self.camera.as_mut(), &mut self.effects)?;
        }
        Ok(())
    }
    /// `AddLightEffect(a, b, c, d, e, f, g, h)`: the first three arguments and the receiver do
    /// not affect the record. Returns its one-based slot, zero when the pool cannot allocate.
    pub(super) fn add_light_effect(&mut self, arguments: [i32; 8]) -> Result<i32> {
        let [_, _, _, d, e, f, g, h] = arguments;
        self.effects
            .as_mut()
            .context("light effect spawning unavailable")?
            .add_light([d, e, f, g, h])
    }
    /// World-loop stage for the type-two records, run beside the lifetime stage with the shared
    /// random generator their modulation draws from.
    pub fn advance_light_effects(
        &mut self,
        ticks: u32,
        random: &mut cyrus_game::random::EngineRandom,
    ) -> Result<()> {
        match &mut self.effects {
            Some(effects) => effects.advance_lights(ticks, random),
            None => Ok(()),
        }
    }
    /// World-loop lifetime stage after object/code-object updates. The scene already
    /// retained its effect submissions; expiry does not retract those packets.
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
            ambient_brightness: Some(128),
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
    fn draw_moves_retained_free_records_and_restores_their_continuation() -> Result<()> {
        let source = source(4);
        let mut effects = EnvironmentalEffects::prepare(&source)?;
        effects.spawn(0x22500, [256, 0, 512])?;
        effects.advance(1);
        effects.spawn(0x12500, [768, 0, 1024])?;
        effects.advance(3);
        assert_eq!(
            effects.active().map(|(slot, _)| slot).collect::<Vec<_>>(),
            [1]
        );
        effects.prepare_draw(0);
        assert_eq!(effects.slots[0].as_ref().unwrap().position, [1, -4, 2]);
        assert_eq!(effects.active().next().unwrap().1.position, [3, -2, 4]);
        let saved = serde_json::to_value(effects.snapshot(&source)?)?;
        assert_eq!(saved["slots"][0]["active"], false);
        assert!(saved["slots"][1].get("active").is_none());
        let mut restored =
            EnvironmentalEffects::from_snapshot(serde_json::from_value(saved)?, &source)?;
        for pool in [&mut effects, &mut restored] {
            pool.prepare_draw(u32::MAX);
            assert_eq!(pool.slots[0].as_ref().unwrap().position, [1, -5, 2]);
            pool.advance(1);
            assert!(pool.is_empty());
            pool.prepare_draw(7);
            assert_eq!(pool.slots[0].as_ref().unwrap().position, [1, -5, 2]);
            pool.spawn(0x22500, [1280, 0, 1536])?;
            pool.prepare_draw(7);
            assert_eq!(pool.active().next().unwrap().1.position, [5, -11, 6]);
        }
        assert_eq!(
            serde_json::to_value(effects.snapshot(&source)?)?,
            serde_json::to_value(restored.snapshot(&source)?)?
        );
        Ok(())
    }

    #[test]
    fn draw_wraps_y_and_invalid_free_snapshots_reject() -> Result<()> {
        let source = source(4);
        let mut effects = EnvironmentalEffects::prepare(&source)?;
        effects.spawn(0x32500, [0; 3])?;
        effects.slots[0].as_mut().unwrap().position[1] = i32::MIN;
        effects.prepare_draw(0);
        assert_eq!(effects.active().next().unwrap().1.position[1], i32::MAX - 1);
        let saved = serde_json::to_value(effects.snapshot(&source)?)?;
        let mut invalid = saved.clone();
        invalid["slots"][0]["active"] = false.into();
        assert!(
            EnvironmentalEffects::from_snapshot(serde_json::from_value(invalid)?, &source).is_err()
        );
        let restored =
            EnvironmentalEffects::from_snapshot(serde_json::from_value(saved)?, &source)?;
        assert_eq!(
            restored.active().next().unwrap().1.position[1],
            i32::MAX - 1
        );
        Ok(())
    }

    #[test]
    fn normalized_draw_terms_do_not_carry_tick_debt_across_birth_or_reuse() -> Result<()> {
        let mut effects = EnvironmentalEffects::prepare(&source(4))?;
        effects.spawn(0x22500, [0; 3])?;
        effects.prepare_draws(3, 0);
        assert_eq!(effects.active().next().unwrap().1.position[1], -5);
        // A later birth must not inherit the previous interval's nominal ticks.
        effects.spawn(0x22500, [0; 3])?;
        effects.prepare_draws(0, 1);
        assert_eq!(effects.slots[0].as_ref().unwrap().position[1], -7);
        assert_eq!(effects.slots[1].as_ref().unwrap().position[1], -4);
        effects.slots[0].as_mut().unwrap().active = false;
        effects.prepare_draws(5, 0);
        assert_eq!(effects.slots[0].as_ref().unwrap().position[1], -12);
        effects.spawn(0x22500, [0; 3])?;
        effects.prepare_draws(0, 1);
        assert_eq!(effects.slots[0].as_ref().unwrap().position[1], -4);
        assert_eq!(effects.slots[1].as_ref().unwrap().position[1], -11);
        Ok(())
    }

    #[test]
    fn submission_precedes_same_loop_birth_updates_and_keeps_packets_after_expiry() -> Result<()> {
        let mut effects = EnvironmentalEffects::prepare(&source(4))?;
        // The scene's first submission sees no effect. Object work then creates it.
        effects.prepare_draw(1);
        assert_eq!(effects.active().count(), 0);
        effects.spawn(0x22500, [2560, 0, 7680])?;
        effects.advance(1);
        assert_eq!(effects.active().next().unwrap().1.frame, 1);
        // Next scene submission sees the preceding loop's settled lifetime frame.
        effects.prepare_draw(3);
        let packet = effects.active().next().unwrap().1.clone();
        assert_eq!(packet.frame, 1);
        assert_eq!(packet.position, [10, -7, 30]);
        effects.advance(3);
        assert_eq!(effects.active().count(), 0);
        assert_eq!(packet.frame, 1);
        assert_eq!(packet.position, [10, -7, 30]);
        Ok(())
    }

    #[test]
    fn effect_speed_scales_frame_advance_and_survives_snapshot() -> Result<()> {
        let source = source(12);
        let mut effects = EnvironmentalEffects::prepare(&source)?;
        effects.spawn_with_speed(0x2500, [0; 3], 3, 0, None)?;
        effects.advance(2);
        assert_eq!(effects.active().next().unwrap().1.frame, 6);
        let saved = effects.snapshot(&source)?;
        let mut restored = EnvironmentalEffects::from_snapshot(saved, &source)?;
        restored.advance(1);
        assert_eq!(restored.active().next().unwrap().1.frame, 9);
        restored.advance(1);
        assert_eq!(restored.active().count(), 0);
        Ok(())
    }

    #[test]
    fn zero_texture_effects_keep_their_frame_even_with_drawing_flags() -> Result<()> {
        let mut source = source(4);
        let image = source.images.remove(&0x2500).unwrap();
        source.images.insert(0, image);
        for descriptor in [0, EnvironmentalEffect::ALTERNATE_DRAW] {
            let mut effects = EnvironmentalEffects::prepare(&source)?;
            effects.spawn_with_speed(descriptor, [0; 3], 3, 0, None)?;
            effects.advance(2);
            let effect = effects.active().next().unwrap().1;
            assert_eq!(effect.frame, 0);
            let saved = effects.snapshot(&source)?;
            let mut restored = EnvironmentalEffects::from_snapshot(saved, &source)?;
            restored.advance(2);
            assert_eq!(restored.active().next().unwrap().1.frame, 0);
        }
        Ok(())
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
