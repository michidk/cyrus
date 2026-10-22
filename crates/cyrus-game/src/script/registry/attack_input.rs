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

/// The two latches the armed attack selection reads, plus the pending jump attack flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AttackLatches {
    action: TapLatch,
    forward: TapLatch,
    jump_attack_pending: bool,
}

impl AttackLatches {
    /// The tap window: `tap_time` frames scaled by the frame scale, at least six iterations.
    fn window(tap_time: i32, frame_scale: i32) -> i32 {
        (tap_time.wrapping_shl(8) / frame_scale.max(1)).max(6)
    }

    pub(super) fn update(&mut self, held: &PlayerInput, tap_time: i32, frame_scale: i32) {
        let window = Self::window(tap_time, frame_scale);
        self.action.update(held.action, window);
        self.forward.update(held.forward, window);
    }
}

/// The attack move the key state selects on the ground, before availability tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroundMove {
    /// A double-tapped forward key: the lunge.
    Lunge,
    Move(i32),
}

impl ScriptWorld {
    /// Advance the tap latches once per controller iteration with the held keys.
    pub(super) fn update_attack_latches(
        &mut self,
        id: ScriptActorId,
        held: &PlayerInput,
        tap_time: i32,
        frame_scale: i32,
    ) {
        if let Some(movement) = self.actors[id.index].player_movement.as_mut() {
            movement.latches.update(held, tap_time, frame_scale);
        }
    }

    /// The armed player's action key after movement dispatch (jump key not held): pick a combo move
    /// from the latches and start it. A held jump attack and the lunge stay unsupported, and the
    /// original's in-reach test before a start is not modelled.
    pub(super) fn control_player_attack(
        &mut self,
        id: ScriptActorId,
        held: &PlayerInput,
        inputs: ScriptInputs<'_>,
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
            // A jump attack starts only once the player is falling.
            latches.jump_attack_pending = true;
            if falling {
                bail!("selected-player jump attack unsupported");
            }
            if !airborne {
                latches.jump_attack_pending = false;
            }
            None
        } else if walking_jump {
            latches.action.matches(latches.action.count, true);
            None
        } else if latches.forward.matches(1, true) {
            Some(GroundMove::Lunge)
        } else if held.forward {
            Some(GroundMove::Move(3))
        } else if latches.action.matches(2, true) {
            Some(GroundMove::Move(2))
        } else if latches.action.matches(1, false) {
            Some(GroundMove::Move(1))
        } else if latches.action.matches(0, false) {
            Some(GroundMove::Move(0))
        } else {
            latches.action.matches(latches.action.count, true);
            None
        };
        self.actors[id.index]
            .player_movement
            .as_mut()
            .expect("validated latches")
            .latches = latches;
        match chosen {
            None => Ok(()),
            Some(GroundMove::Lunge) => bail!("selected-player lunge attack unsupported"),
            Some(GroundMove::Move(mov)) => self.start_player_attack(id, mov, inputs, sounds),
        }
    }

    /// Start combo move `mov` when its authored attack slot exists and the pending word allows it:
    /// moves 0 and 3 need no pending move, 1 follows 0 and 2 follows 1.
    fn start_player_attack(
        &mut self,
        id: ScriptActorId,
        mov: i32,
        inputs: ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let combat = self.actors[id.index]
            .control
            .as_mut()
            .and_then(|control| control.combat.as_mut())
            .context("player attack combat unavailable")?;
        let slot = usize::try_from(mov).context("negative attack move")?;
        if combat.attack_indices.get(slot).copied().flatten().is_none() {
            return Ok(());
        }
        let pending = combat.retained_action_indices[0];
        let allowed = match mov {
            0 | 3 => pending == -1,
            _ => pending == mov - 1,
        };
        if !allowed {
            return Ok(());
        }
        combat.action = mov;
        combat.retained_action_indices[0] = mov;
        // The current motion lookup indexes the attack slots by the action, so the move is the slot.
        combat.retained_action_indices[1] = mov;
        self.execute_combat_attack(id, 0, inputs, sounds)?;
        Ok(())
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
