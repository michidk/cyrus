//! The selected player's armed attack input: per-key tap latches and the combo move start.
//! Rules follow preservation's engine/player.md "Armed attack input".
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::player::input::PlayerInput;
use anyhow::{Context, Result, bail};

/// One key's quick-press counter. The release timer counts frames since the last release; a press
/// inside the tap window raises the count, a longer gap restarts it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TapLatch {
    timer: i32,
    pressed: bool,
    count: i32,
}

impl TapLatch {
    /// One world-loop iteration with the key's held state; `window` is the tap window in frames.
    fn update(&mut self, held: bool, window: i32) {
        if !held {
            self.timer = self.timer.wrapping_add(1);
            self.pressed = false;
            if self.timer > window {
                self.count = 0;
            }
        } else if !self.pressed {
            self.pressed = true;
            if self.timer > 0 && self.timer < window {
                self.count = self.count.wrapping_add(1);
            } else {
                self.count = 0;
            }
            self.timer = 0;
        }
    }

    /// Whether the press count equals `count`; `consume` clears the latch on a match.
    fn matches(&mut self, count: i32, consume: bool) -> bool {
        if self.count != count {
            return false;
        }
        if consume {
            *self = Self::default();
        }
        true
    }
}

/// The armed player's look-key settings: SYSTEM.INI `sheath_sword_delay`, COMBAT.INI
/// `cyrus_defend_interval` and the automatic-defence toggle.
#[derive(Clone, Copy, Debug)]
pub(super) struct StanceSettings {
    pub(super) sheath_delay: i32,
    pub(super) defend_interval: i32,
    pub(super) auto_defend: bool,
    /// Nominal ticks of this controller iteration.
    pub(super) nominal_ticks: i32,
}

/// The two latches the armed attack selection reads, plus the pending jump attack flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AttackLatches {
    action: TapLatch,
    forward: TapLatch,
    jump_attack_pending: bool,
    /// Nominal ticks the look and action keys have been held together while armed.
    sheath_timer: i32,
    /// Nominal ticks since the last defence stance began; accumulates every iteration.
    defend_counter: i32,
}

impl AttackLatches {
    /// The tap window: `tap_time` frames scaled by the frame scale, at least six iterations.
    fn window(tap_time: i32, frame_scale: i32) -> i32 {
        (tap_time.wrapping_shl(8) / frame_scale.max(1)).max(6)
    }

    pub(super) fn update(
        &mut self,
        held: &PlayerInput,
        tap_time: i32,
        frame_scale: i32,
        nominal_ticks: i32,
    ) {
        self.defend_counter = self.defend_counter.wrapping_add(nominal_ticks);
        let window = Self::window(tap_time, frame_scale);
        self.action.update(held.action, window);
        self.forward.update(held.forward, window);
    }
}

impl ScriptWorld {
    /// Advance the tap latches once per controller iteration with the held keys.
    pub(super) fn update_attack_latches(
        &mut self,
        id: ScriptActorId,
        held: &PlayerInput,
        tap_time: i32,
        frame_scale: i32,
        nominal_ticks: i32,
    ) {
        if let Some(movement) = self.actors[id.index].player_movement.as_mut() {
            movement
                .latches
                .update(held, tap_time, frame_scale, nominal_ticks);
        }
    }

    /// Drawing and sheathing predicates of the selected player.
    fn player_drawing_sheathing(
        &self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
    ) -> Result<(bool, bool)> {
        let actor = &self.actors[id.index];
        let control = actor
            .control
            .as_ref()
            .context("player stance control unavailable")?;
        let (_, drawing, sheathing) = super::super::activation::weapon_predicates(
            control.living_state,
            actor
                .animation
                .as_ref()
                .map(|animation| super::super::ActivationAnimation {
                    group: animation.playback.current_group(),
                }),
            control.combat_death_default.is_some(),
            control.weapon_drawn,
            actor.attributes[110],
            inputs
                .hud
                .map(|hud| hud.inventory_ready())
                .transpose()?
                .unwrap_or(false),
            inputs.shell_execution.unwrap_or(false),
        );
        Ok((drawing, sheathing))
    }

    /// The armed player's look-key handling, before the attack keys. Holding the look and action
    /// keys together counts nominal ticks and sheathes the sword once they exceed the sheathing
    /// delay; a drawing or sheathing sword ends the iteration; with the action key released the
    /// look key starts the defence stance once `defend_interval` ticks passed since the last one
    /// (the automatic defence only when an attacker's strike window fits a defence), and a jump
    /// attack still pending while falling starts. Returns whether the iteration ends here; with the
    /// action key held and no sheathing it does not, and the attack keys follow.
    pub(super) fn control_player_stance(
        &mut self,
        id: ScriptActorId,
        held: &PlayerInput,
        settings: StanceSettings,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let nominal_ticks = settings.nominal_ticks;
        let movement = self.actors[id.index]
            .player_movement
            .as_mut()
            .context("player stance latches unavailable")?;
        if held.action && held.look {
            movement.latches.sheath_timer =
                movement.latches.sheath_timer.wrapping_add(nominal_ticks);
            if movement.latches.sheath_timer > settings.sheath_delay {
                self.sheath_selected_player(inputs, random, sounds)?;
            }
            return Ok(true);
        }
        movement.latches.sheath_timer = 0;
        let (drawing, sheathing) = self.player_drawing_sheathing(id, inputs)?;
        if drawing || sheathing {
            return Ok(true);
        }
        if held.action {
            return Ok(false);
        }
        let counter = self.actors[id.index]
            .player_movement
            .as_ref()
            .expect("validated latches")
            .latches
            .defend_counter;
        if held.look && counter > settings.defend_interval {
            let started = if settings.auto_defend {
                self.auto_defend_player(id, inputs, sounds)?
            } else {
                self.start_player_stance(id, inputs, sounds)?;
                true
            };
            if started {
                let actor = &mut self.actors[id.index];
                let combat = actor
                    .control
                    .as_mut()
                    .and_then(|control| control.combat.as_mut())
                    .context("player stance combat unavailable")?;
                combat.flags = (combat.flags & !3) | 2;
                actor
                    .player_movement
                    .as_mut()
                    .expect("validated latches")
                    .latches
                    .defend_counter = 0;
            }
        }
        let latches = self.actors[id.index]
            .player_movement
            .as_ref()
            .expect("validated latches")
            .latches;
        let falling = self.actors[id.index]
            .control
            .as_ref()
            .is_some_and(|control| control.falling_state == 1);
        if falling && latches.jump_attack_pending {
            let started = self.begin_player_move(id, 6, inputs, random, sounds)?;
            let latches = &mut self.actors[id.index]
                .player_movement
                .as_mut()
                .expect("validated latches")
                .latches;
            if started {
                latches.jump_attack_pending = false;
            }
            latches.action.matches(1, true);
        }
        Ok(true)
    }

    /// Actors, in ordinary list order, whose combat target is `id`, whose attacker-selection
    /// state is one and that are not dead.
    fn player_attackers(&self, id: ScriptActorId) -> Result<Vec<ScriptActorId>> {
        let traversal = self
            .actor_lists
            .as_ref()
            .and_then(|lists| lists.retained_selected())
            .context("player attackers: ordinary traversal unavailable")?
            .to_vec();
        let mut attackers = Vec::new();
        for candidate in traversal {
            if candidate == id {
                continue;
            }
            let combat = self.actors[self.resolve(candidate)?]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref());
            if combat.is_some_and(|combat| {
                combat.target == Some(id) && combat.attacker_selection_state == 1
            }) && !self.query_is_dead(candidate, Some(id))?
            {
                attackers.push(candidate);
            }
        }
        Ok(attackers)
    }

    /// The defence stance: with no move selected and the sword neither drawing nor sheathing, the
    /// pending defence index becomes zero, or the counter-attack reference of the first attacker
    /// that is in the player's forward arc and mid-strike, the selected move becomes nine and the
    /// defence executor runs.
    fn start_player_stance(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let (drawing, sheathing) = self.player_drawing_sheathing(id, inputs)?;
        let combat = self.actors[id.index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .context("player stance combat unavailable")?;
        if drawing || sheathing || combat.action != -1 {
            return Ok(());
        }
        let mut index = 0;
        if let Some(&attacker) = self.player_attackers(id)?.first() {
            let own = self.spatial(id)?.local;
            let other = self.spatial(attacker)?.local;
            let striking = self.actors[attacker.index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_some_and(|combat| combat.decision.attack_state == 1);
            if striking
                && crate::script::decision::ordinary_defence_forward_arc(
                    own.position,
                    other.position,
                    i32::from(own.angles[1]),
                )
            {
                index = self
                    .current_combat_motion(attacker)?
                    .defense
                    .context("attacker counter-attack reference unavailable")?
                    .wrapping_sub(1);
            }
        }
        let combat = self.actors[id.index]
            .control
            .as_mut()
            .and_then(|control| control.combat.as_mut())
            .expect("admitted combat");
        combat.retained_action_indices[0] = index;
        combat.action = 9;
        self.execute_combat_attack(id, 0, inputs, sounds)?;
        Ok(())
    }

    /// The automatic defence: the first attacker in the player's forward arc that holds intent
    /// bit one, is mid-strike and whose strike window (from its first collision frame and first
    /// defence frame) lies inside the active window of the player's matching defence starts the
    /// stance. Whether one did.
    fn auto_defend_player(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        for attacker in self.player_attackers(id)? {
            let own = self.spatial(id)?.local;
            let other = self.spatial(attacker)?.local;
            let combat = self.actors[attacker.index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("attacker combat unavailable")?;
            if !crate::script::decision::ordinary_defence_forward_arc(
                own.position,
                other.position,
                i32::from(own.angles[1]),
            ) || combat.flags & 1 == 0
                || combat.decision.attack_state != 1
            {
                continue;
            }
            let motion = self.current_combat_motion(attacker)?;
            let cursor = self.actors[attacker.index]
                .animation
                .as_ref()
                .context("attacker animation unavailable")?
                .playback
                .current_command_cursor()?
                .context("attacker command cursor unavailable")?;
            let gap_near = motion.first_collide_frame.wrapping_sub(cursor);
            let gap_far = motion
                .first_defend_frame
                .context("attacker first_defend_frame unavailable")?
                .wrapping_sub(cursor);
            let counter = self.current_combat_counter_attack(attacker)?;
            let slot = usize::try_from(counter).context("negative counter defense slot")?;
            let record = self.actors[id.index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("player stance combat unavailable")?
                .defense_indices
                .get(slot)
                .copied()
                .flatten()
                .with_context(|| format!("player defense slot {slot}: unpopulated"))?;
            let defense = self
                .combat_defenses
                .get(usize::from(record))
                .with_context(|| format!("player defense record {record}: unavailable"))?;
            let start = defense
                .first_collide_frame
                .context("player defense first_collide_frame unavailable")?;
            let end = start.wrapping_add(
                defense
                    .collide_duration
                    .context("player defense collide_duration unavailable")?,
            );
            if (start < gap_far || gap_far <= 0)
                && end > gap_far
                && start < gap_near
                && end > gap_near
            {
                self.start_player_stance(id, inputs, sounds)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The reach test before a combo move starts. A start proceeds when an actor with a combat
    /// record in the move's arc and range is attackable (attribute 97 zero, or lifecycle selector 6),
    /// and otherwise unless an actor in that arc and range is a protected one (attribute 97 nonzero,
    /// selector neither 6 nor 8): then the player speaks the opponent-unarmed taunt (type 11 when the
    /// target's attribute 118 is nonzero, else 10) and the move does not start. The forward move (3)
    /// tests the protected actor against an arc six times as wide. Arc and range come from the first
    /// motion of the move's own attack slot; the receiver itself is not excluded.
    fn attack_reach_admits(
        &mut self,
        id: ScriptActorId,
        kind: i32,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let slot = usize::try_from(kind).context("attack reach: negative move")?;
        // An unpopulated slot is refused by the move's own start gate.
        let Some(attack) = self.combat_attack(id, slot)? else {
            return Ok(true);
        };
        let motion = attack
            .motions
            .first()
            .with_context(|| format!("attack reach: slot {slot} first motion unavailable"))?;
        let arc = motion
            .arc
            .with_context(|| format!("attack reach: slot {slot} arc unavailable"))?;
        let (center, near, far) = (
            motion.arc_center.unwrap_or(0),
            motion.min_range,
            motion.max_range,
        );
        let own_position = self.query_position(id)?;
        let own = self.spatial(id)?.local;
        let traversal = self
            .actor_lists
            .as_ref()
            .and_then(|lists| lists.retained_selected())
            .context("attack reach: ordinary traversal unavailable")?
            .iter()
            .copied()
            // The inactive player role stays in the list without a pose and takes no part.
            .filter(|&other| self.spatial(other).is_ok())
            .collect::<Vec<_>>();
        let in_reach = |world: &Self, other: ScriptActorId, scale: f32| -> Result<bool> {
            let other_position = world.query_position(other)?;
            let distance =
                cyrus_collision::distance::rounded_distance(std::array::from_fn(|axis| {
                    other_position[axis].wrapping_sub(own_position[axis])
                }))?;
            Ok(distance > near
                && distance < far
                && crate::script::decision::defence_arc_from_positions_scaled(
                    own.position,
                    world.spatial(other)?.local.position,
                    i32::from(own.angles[1]),
                    arc,
                    center,
                    scale,
                ))
        };
        for &other in &traversal {
            let index = self.resolve(other)?;
            let actor = &self.actors[index];
            let Some(control) = actor.control.as_ref() else {
                continue;
            };
            if control.combat.is_some()
                && in_reach(self, other, 1.0)?
                && (actor.attributes[97] == 0 || control.death_selector == 6)
            {
                return Ok(true);
            }
        }
        for &other in &traversal {
            let index = self.resolve(other)?;
            let actor = &self.actors[index];
            let selector = actor.control.as_ref().map(|control| control.death_selector);
            if matches!(selector, Some(6 | 8))
                || actor.attributes[97] == 0
                || !in_reach(self, other, if kind == 3 { 6.0 } else { 1.0 })?
            {
                continue;
            }
            let trigger = if actor.attributes[118] != 0 { 11 } else { 10 };
            self.request_combat_taunt(id, trigger, 0, inputs, random, sounds)?;
            return Ok(false);
        }
        Ok(true)
    }

    /// The armed player's action key after movement dispatch (jump key not held): pick a combo move
    /// from the latches and start it through the defence start once the reach test admits it.
    pub(super) fn control_player_attack(
        &mut self,
        id: ScriptActorId,
        held: &PlayerInput,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let actor = &self.actors[id.index];
        let control = actor
            .control
            .as_ref()
            .context("player attack control unavailable")?;
        let Some(combat) = control.combat.as_ref() else {
            return Ok(());
        };
        if held.jump || !held.action || matches!(control.living_state, 20 | 21) {
            return Ok(());
        }
        // Only a first, second or third tap, or an idle record, may continue the combo.
        if !matches!(combat.action, -1..=1) {
            return Ok(());
        }
        let airborne = control.queued_motion != 0 || control.falling_state == 1;
        let walking_jump = control.directional_state[0] & 3 != 0;
        let falling = control.falling_state == 1;
        let mut latches = self.actors[id.index]
            .player_movement
            .as_ref()
            .context("player attack latches unavailable")?
            .latches;
        let chosen = if airborne || latches.jump_attack_pending {
            // A jump attack starts only once the player is falling, and consumes the action latch.
            latches.jump_attack_pending = true;
            if falling {
                latches.jump_attack_pending = false;
                latches.action.matches(latches.action.count, true);
                Some(6)
            } else {
                if !airborne {
                    latches.jump_attack_pending = false;
                }
                None
            }
        } else if walking_jump {
            latches.action.matches(latches.action.count, true);
            None
        } else if latches.forward.matches(1, true) {
            Some(4)
        } else if held.forward {
            Some(3)
        } else if latches.action.matches(2, true) {
            Some(2)
        } else if latches.action.matches(1, false) {
            Some(1)
        } else if latches.action.matches(0, false) {
            Some(0)
        } else {
            latches.action.matches(latches.action.count, true);
            None
        };
        self.actors[id.index]
            .player_movement
            .as_mut()
            .expect("validated latches")
            .latches = latches;
        let Some(kind) = chosen else {
            return Ok(());
        };
        self.begin_player_move(id, kind, inputs, random, sounds)?;
        Ok(())
    }

    /// The reach test, then the move start through the defence start. Whether the move started.
    fn begin_player_move(
        &mut self,
        id: ScriptActorId,
        kind: i32,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        if !self.attack_reach_admits(id, kind, inputs, random, sounds)? {
            return Ok(false);
        }
        let weapon = super::WeaponInputs {
            selected_player: id,
            item_system_ready: inputs
                .hud
                .map(|hud| hud.inventory_ready())
                .transpose()?
                .unwrap_or(false),
            shell_execution: inputs.shell_execution.unwrap_or(false),
        };
        let clock = inputs
            .combat_clock_sample
            .context("selected combat BIOS-clock sample unavailable")? as i32;
        self.start_combat_defence(
            id,
            kind,
            weapon,
            clock,
            inputs,
            random,
            // The lunge is a forward step of attribute 16; it requests no animation and so no sound.
            |world, id, _distance, _mode| {
                world.step_player(id, "Lunge", inputs, &mut |_, _| {
                    bail!("selected-player lunge sound unsupported")
                })
            },
            sounds,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(latch: &mut TapLatch, frames: &[bool], window: i32) {
        for &held in frames {
            latch.update(held, window);
        }
    }

    #[test]
    fn the_defence_counter_accumulates_nominal_ticks_every_iteration() {
        let mut latches = AttackLatches::default();
        let held = PlayerInput::default();
        latches.update(&held, 12, 256, 2);
        latches.update(&held, 12, 256, 3);
        assert_eq!(latches.defend_counter, 5);
    }

    #[test]
    fn quick_presses_count_and_a_long_gap_restarts() {
        let mut latch = TapLatch::default();
        tap(&mut latch, &[true], 12);
        assert_eq!(latch.count, 0);
        tap(&mut latch, &[false, false, true], 12);
        assert_eq!(latch.count, 1);
        tap(&mut latch, &[false, true], 12);
        assert_eq!(latch.count, 2);
        tap(&mut latch, &[false; 13], 12);
        assert_eq!(latch.count, 0);
    }

    #[test]
    fn a_match_consumes_only_when_asked() {
        let mut latch = TapLatch::default();
        tap(&mut latch, &[true, false, true], 12);
        assert!(latch.matches(1, false));
        assert!(latch.matches(1, true));
        assert!(latch.matches(0, false));
    }

    #[test]
    fn the_window_scales_with_the_frame_scale_and_has_a_floor() {
        assert_eq!(AttackLatches::window(12, 256), 12);
        assert_eq!(AttackLatches::window(12, 512), 6);
        assert_eq!(AttackLatches::window(1, 256), 6);
    }
}
