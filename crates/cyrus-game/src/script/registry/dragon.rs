//! `DragonBreath` and `DragonBreathTask`: the dragon's smoke while it speaks, its fireball and the
//! item swap when it dies. The task form runs the same update every frame until it returns zero.
use super::GameHost;
use crate::script::spatial::query_bearing;
use anyhow::{Context, Result, ensure};
use cyrus_collision::distance::rounded_distance;

/// Death selector that marks a killed receiver.
const KILLED_SELECTOR: u8 = 8;
/// Combat flag that lets the dragon pick a target.
const COMBAT_FLAG_TARGETING: u8 = 4;
/// Smoke effect descriptor.
const SMOKE_EFFECT: u32 = 0x32480;
/// Installed script of the fireball.
const FIREBALL: &str = "DRAGBALL";
/// Distance under which the dragon does not breathe, in Q8 units.
const MINIMUM_RANGE: i32 = 0x19000;
/// Item the dragon's death takes from the player, and the item it gives in exchange.
const DEATH_ITEM_TAKEN: u16 = 0x4b;
const DEATH_ITEM_GIVEN: u16 = 1;
/// The fireball leaves pitched this far above the line between the two grip vertices.
const FIREBALL_PITCH_OFFSET: i32 = 0x78;

/// The fireball's pitch, yaw and roll for the grip-to-grip vector: aimed along it, as `shootPlayer`
/// aims (the original's table bearings are the negation of the shared query bearing), with the yaw
/// turned a quarter circle and the pitch lowered by [`FIREBALL_PITCH_OFFSET`].
fn fireball_angles(delta: [i32; 3]) -> Result<[u16; 3]> {
    let yaw = (i32::from(query_bearing(delta[0], delta[2])) + 0x400) & 0x7ff;
    let horizontal = rounded_distance([delta[0], 0, delta[2]])?;
    let elevation = i32::from(query_bearing(horizontal, delta[1])).wrapping_neg() & 0x7ff;
    let pitch = (elevation - 0x200) & 0x7ff;
    let pitch = (FIREBALL_PITCH_OFFSET - pitch) & 0x7ff;
    Ok([pitch as u16, yaw as u16, 0])
}

impl GameHost<'_, '_, '_, '_> {
    /// One of the receiver's RAEX attachment vertices (`grip0`, `grip1`, `scabbard0`, `scabbard1`)
    /// rotated by the receiver's orientation, relative to its position.
    pub(super) fn attachment_offset(&mut self, native: &str, field: &str) -> Result<[i32; 3]> {
        let data = self
            .inputs
            .animations
            .with_context(|| format!("{native} animation data unavailable"))?
            .0;
        let id = self.registry.id(self.index)?;
        let index = super::raex_word(
            self.registry.actors[self.index].control.as_ref(),
            data.actor(self.registry.actors[self.index].name())?,
            field,
        )
        .with_context(|| format!("{native}: RAEX {field} unavailable"))?;
        let vertex = if self.registry.updates.dummy_model_manager {
            [0; 3]
        } else {
            self.registry
                .actor_animation_source_vertex(id, data, i32::from(index))?
        };
        self.registry.spatial(id)?.local.matrix.transform(vertex)
    }

    /// One update: true while the receiver keeps breathing, false once it is dead.
    pub(super) fn breathe(&mut self) -> Result<bool> {
        let id = self.registry.id(self.index)?;
        let control = self.registry.actors[self.index]
            .control
            .as_ref()
            .context("DragonBreath: receiver control unavailable")?;
        if control.death_selector == KILLED_SELECTOR {
            self.swap_death_item()?;
            return Ok(false);
        }
        let ticks = self
            .inputs
            .nominal_ticks
            .context("DragonBreath nominal ticks unavailable")?;
        let scale = self
            .inputs
            .frame_scale
            .context("DragonBreath frame scale unavailable")?;
        ensure!(scale > 0, "DragonBreath frame scale must be positive");
        let random = self
            .random
            .as_deref_mut()
            .context("DragonBreath shared engine random stream unavailable")?;
        if random.rnd((0x1000 / scale) as i32)? == 0 && self.speaking(id)? {
            let random = self.random.as_deref_mut().expect("checked above");
            let field = if random.quick_rnd(1) != 0 {
                "scabbard0"
            } else {
                "scabbard1"
            };
            let rotated = self.attachment_offset("DragonBreath", field)?;
            let position = self.registry.spatial(id)?.local.position;
            let point = std::array::from_fn(|axis| rotated[axis].wrapping_add(position[axis]));
            self.registry
                .spawn_environment_effect(SMOKE_EFFECT, point)?;
        }
        let cooldown = &mut self.registry.globals.dragon_breath_cooldown;
        if *cooldown != 0 {
            *cooldown = cooldown.saturating_sub(ticks);
            return Ok(true);
        }
        let combat = self.registry.actors[self.index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .context("DragonBreath: receiver combat state unavailable")?;
        let Some(target) = combat
            .target
            .filter(|_| combat.flags & COMBAT_FLAG_TARGETING != 0)
        else {
            return Ok(true);
        };
        let own = self.registry.spatial(id)?.local.position;
        let goal = self.registry.spatial(target)?.local.position;
        if rounded_distance(std::array::from_fn(|axis| {
            goal[axis].wrapping_sub(own[axis])
        }))? < MINIMUM_RANGE
        {
            return Ok(true);
        }
        let grip0 = self.attachment_offset("DragonBreath", "grip0")?;
        let grip1 = self.attachment_offset("DragonBreath", "grip1")?;
        let delta: [i32; 3] = std::array::from_fn(|axis| grip1[axis].wrapping_sub(grip0[axis]));
        let angles = fireball_angles(delta)?;
        let position = std::array::from_fn(|axis| grip0[axis].wrapping_add(own[axis]));
        self.registry
            .create_object(id, FIREBALL, position, angles)?;
        let random = self.random.as_deref_mut().expect("checked above");
        self.registry.globals.dragon_breath_cooldown = random.rnd(0x20)? as u32 + 0xc;
        Ok(true)
    }

    /// Whether a dialogue line owned by `id` is active.
    fn speaking(&self, id: super::ScriptActorId) -> Result<bool> {
        Ok(self
            .registry
            .globals
            .dialogue
            .as_ref()
            .context("DragonBreath: dialogue records unavailable")?
            .active_speakers()
            .any(|speaker| speaker == id))
    }

    /// The dragon's death: a player holding the taken item gets the given item, held and selected,
    /// in its place.
    fn swap_death_item(&mut self) -> Result<()> {
        let hud = self
            .inputs
            .hud
            .context("DragonBreath: inventory host unavailable")?;
        if hud.item_count(DEATH_ITEM_TAKEN)? == 0 {
            return Ok(());
        }
        let player = self
            .registry
            .selected_player_index()
            .context("DragonBreath: selected player unavailable")?;
        self.add_session_item(DEATH_ITEM_GIVEN, 1)?;
        self.registry.actors[player].attributes[23] = DEATH_ITEM_GIVEN as u8;
        hud.remove_item(DEATH_ITEM_TAKEN)?;
        hud.select_item(DEATH_ITEM_GIVEN)
    }
}

#[cfg(test)]
mod tests {
    use super::fireball_angles;

    #[test]
    fn the_fireball_yaw_follows_the_grip_vector_a_quarter_circle_off_its_bearing() {
        for (dx, dz) in [(0, 4096), (4096, 0), (-3000, 2000), (-1500, -2500)] {
            let [_, yaw, roll] = fireball_angles([dx, 0, dz]).unwrap();
            let bearing = (-f64::from(dx).atan2(f64::from(dz)) * 2048.0 / std::f64::consts::TAU)
                .rem_euclid(2048.0);
            let expected = (bearing + 1024.0).rem_euclid(2048.0);
            let gap = (f64::from(yaw) - expected).abs();
            assert!(gap.min(2048.0 - gap) <= 2.0, "yaw {yaw} against {expected}");
            assert_eq!(roll, 0);
        }
    }
}
