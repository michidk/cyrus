//! End-of-frame exits use selected state after ordinary traversal, never mid-actor.
use super::ScriptWorld;
use anyhow::{Context, Result};
use serde::Serialize;

/// Result of preservation's world-loop exit arbitration. Dispatch is host-owned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum WorldLoopOutcome {
    Continue,
    WorldTransition([i32; 3]),
    Death,
    WonGame,
}

impl ScriptWorld {
    /// Evaluate once after this frame's actor/presentation work. Pending world
    /// transitions bypass timer updates; falling timeout precedes inhibited death,
    /// which precedes the win request. `ticks` is the shared nominal-tick increment.
    /// This does not clear requests or hit inhibition, nor restore a saved game.
    pub fn finish_world_loop(&mut self, ticks: u32) -> Result<WorldLoopOutcome> {
        if let Some(destination) = self.globals.request_mut().load_world {
            return Ok(WorldLoopOutcome::WorldTransition(destination));
        }
        let result = (|| {
            if let Some(index) = self.selected_player_index() {
                let actor = &self.actors[index];
                if let Some(error) = actor.failure() {
                    anyhow::bail!("selected instance {index}: {error}");
                }
                let falling = actor
                    .control
                    .as_ref()
                    .context("selected falling state unavailable")?
                    .falling_state
                    != 0;
                let health = self
                    .globals
                    .selected_health
                    .as_mut()
                    .context("selected health owner unavailable")?;
                let elapsed = health.advance_falling_elapsed(falling, ticks)?;
                if falling && (elapsed as i32) > self.updates.fall_death_ticks {
                    return Ok(WorldLoopOutcome::Death);
                }
                if health.hit_inhibited() {
                    let death = health.death().context("selected death state unavailable")?;
                    if actor.attributes[105] == 1 && death.mode() == 2 {
                        if (death.progress() as i32) > 12 {
                            return Ok(WorldLoopOutcome::Death);
                        }
                    } else {
                        let group = self
                            .globals
                            .player_death_override_group()
                            .context("selected death override unavailable")?;
                        let animation = &actor
                            .animation
                            .as_ref()
                            .context("selected death animation unavailable")?
                            .playback;
                        let (current, _, progress) = animation.frame_wait_state()?;
                        if current == group {
                            let cursor = animation
                                .current_command_cursor()?
                                .context("selected death animation cursor unavailable")?;
                            let complete = animation_complete(group, cursor, progress);
                            if complete {
                                return Ok(WorldLoopOutcome::Death);
                            }
                        }
                    }
                }
            }
            Ok(if self.globals.request_mut().won_game {
                WorldLoopOutcome::WonGame
            } else {
                WorldLoopOutcome::Continue
            })
        })();
        result.with_context(|| format!("world {} end-of-frame exit", self.world))
    }
}

fn animation_complete(group: i32, cursor: i32, progress: u8) -> bool {
    if matches!(group, 13 | 74) {
        cursor <= i32::from(progress)
    } else {
        (cursor as u16) != u16::from(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::animation_complete;
    #[test]
    fn completion_uses_signed_cursor_and_wrapping_frame_progress_not_displayed_pose() {
        for (group, cursor, progress, complete) in [
            (13, -1, 0, true),
            (13, 0, 0, true),
            (13, 1, 0, false),
            (74, 255, 255, true),
            (74, 256, 255, false),
            (122, 0, 0, false),
            (122, 1, 0, true),
            (122, -1, 255, true),
            (134, 255, 255, false),
            (134, 256, 255, true),
        ] {
            assert_eq!(animation_complete(group, cursor, progress), complete);
        }
    }
}
