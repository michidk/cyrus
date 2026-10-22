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
    /// Moves the selected player to the placement `MovePlayerAboveMe` raised during the frame and
    /// cuts the camera there. The original consumes the request once per frame; its place among the
    /// frame's stages is **Inferred** to be late.
    fn apply_pending_player_placement(&mut self) -> Result<()> {
        let Some((position, yaw)) = self.pending_player_placement.take() else {
            return Ok(());
        };
        let player = self
            .selected_player()
            .context("MovePlayerAboveMe: selected player unavailable")?;
        let index = self.resolve(player)?;
        self.spatial
            .as_mut()
            .context("MovePlayerAboveMe: spatial state not prepared")?
            .actor_mut(index)?
            .relocate(position, yaw);
        self.refresh_transform(player)?;
        if let Some(camera) = self.camera.as_mut() {
            camera.mark_cut();
        }
        Ok(())
    }

    /// Evaluate once after this frame's actor/presentation work. Pending world
    /// transitions bypass timer updates; falling timeout precedes inhibited death,
    /// which precedes the win request. `ticks` is the shared nominal-tick increment.
    /// This does not clear requests or hit inhibition, nor restore a saved game.
    pub fn finish_world_loop(&mut self, ticks: u32) -> Result<WorldLoopOutcome> {
        if let Some(destination) = self.globals.request_mut().load_world {
            return Ok(WorldLoopOutcome::WorldTransition(destination));
        }
        self.apply_pending_player_placement()?;
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
        if matches!(result, Ok(WorldLoopOutcome::Continue)) {
            self.consume_player_swap_requests(ticks)
                .with_context(|| format!("world {} PushPlayer/PopPlayer", self.world))?;
        }
        result.with_context(|| format!("world {} end-of-frame exit", self.world))
    }

    /// `PushPlayer`, then `PopPlayer`, at the end of a world-loop iteration that does not leave
    /// the loop. Each clears its own flag whether or not it acts; push acts only while the swap
    /// state is clear and pop only while it is set. The exclusion of the inactive role follows
    /// from the retained swap state, which the world loop re-asserts every iteration.
    fn consume_player_swap_requests(&mut self, ticks: u32) -> Result<()> {
        let request = self.globals.request_mut();
        let push = std::mem::take(&mut request.push_player);
        let pop = std::mem::take(&mut request.pop_player);
        if push && !self.globals.player_swapped {
            self.exchange_players(true, ticks)?;
        }
        if pop && self.globals.player_swapped {
            self.exchange_players(false, ticks)?;
        }
        Ok(())
    }

    /// Select the other player role: it takes the selected role's position, heading and retained
    /// movement state, then the camera defaults follow the new selection. Returning to CYRUS
    /// also issues an ordinary request for animation group zero twice.
    fn exchange_players(&mut self, swapped: bool, ticks: u32) -> Result<()> {
        let from = self
            .selected_player_index()
            .context("player swap: selected player unavailable")?;
        let to = self
            .actors
            .iter()
            .enumerate()
            .position(|(index, actor)| {
                index != from
                    && matches!(
                        actor.source,
                        super::ScriptActorSource::Player
                            | super::ScriptActorSource::AlternatePlayer
                    )
            })
            .context("player swap: other player role unavailable")?;
        let placement = self
            .spatial
            .as_ref()
            .context("player swap: spatial state not prepared")?
            .actor(from)?
            .local;
        let movement = self.actors[from].player_movement.clone();
        let selected = self.select_player(swapped)?;
        anyhow::ensure!(
            selected.index == to,
            "player swap selected an unexpected role"
        );
        self.spatial
            .as_mut()
            .context("player swap: spatial state not prepared")?
            .actor_mut(to)?
            .relocate(placement.position, placement.angles[1]);
        self.actors[to].player_movement = movement;
        self.refresh_transform(selected)?;
        if !swapped && let Some(animation) = self.actors[to].animation.as_mut() {
            for _ in 0..2 {
                let frame = animation.playback.request(0, 0, ticks)?;
                animation.frame = frame.or(animation.frame);
            }
        }
        Ok(())
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
