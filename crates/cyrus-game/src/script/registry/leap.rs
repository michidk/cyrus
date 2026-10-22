//! The leap that a route link flagged `0x4000` starts, from preservation's SOUP.md phase twelve.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Start a nonplayer leap: queued motion 2 and the jump request (group 32, or 33 with a
    /// burning torch), with the fall origin at the current Y. Returns whether it started; a
    /// refused start leaves everything but the stable-position flag untouched. The selected
    /// player's cooldown and edge entry are not modeled, so it fails explicitly.
    pub(super) fn begin_point_leap(
        &mut self,
        id: ScriptActorId,
        selected_player: Option<ScriptActorId>,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            selected_player != Some(id),
            "world {} instance {index}: selected-player leap unsupported",
            self.world
        );
        let actor = &self.actors[index];
        let control = actor.control.as_ref().context("leap control unavailable")?;
        let group = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        let surface = actor.surface.with_context(|| {
            format!(
                "world {} instance {index}: leap stored surface unavailable",
                self.world
            )
        })?;
        if matches!(group, Some(114 | 116 | 130 | 131))
            || matches!(surface, 2 | 4)
            || control.rope_attached
            || control.queued_motion != 0
            || control.falling_state != 0
            || control.sliding_state != 0
            || !matches!(control.living_state, 0 | 15)
        {
            return Ok(false);
        }
        let torch = control.torch_burning != 0;
        let origin = self.spatial(id)?.local.position[1];
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("validated leap control");
        control.stable_position_valid = false;
        // A hanging actor takes the hang release instead, which only the selected player has.
        if control.living_state == 15 {
            return Ok(false);
        }
        control.fall_origin_y = origin;
        control.fall_timer = 0;
        control.sliding_distance = 0;
        control.queued_motion = 2;
        if self.updates.animation_system {
            self.request_ordinary_group(id, if torch { 33 } else { 32 }, false, ticks, sounds)?;
        }
        Ok(true)
    }
}
