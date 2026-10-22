//! Ordinary collision rollback restores separately owned position and receiver state.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_world::{
    animation::ActorAnimationCheckpoint,
    sound::{AnimationSoundState, ScriptSound},
};

/// Post-advancement receiver and movement flags for one ordinary actor update.
/// Position comes from the update-entry or stable record at the owning response.
pub struct OrdinaryRollbackSnapshot {
    actor: ScriptActorId,
    movement_flags: u8,
    pub(super) landing_recovery: bool,
    animation: Option<(ActorAnimationCheckpoint, Option<u32>)>,
}

/// Current geometry, fresh-admission view, and shared random owner for nonplayer recovery.
/// The candidate list is rebuilt from this view and actor order, independently of
/// the ordinary update's retained collision list.
pub struct OrdinaryStableRecoveryInputs<'a> {
    pub scene: &'a crate::player::data::PlayerScene,
    pub animations: &'a cyrus_assets::animation::Animations,
    pub view: [i32; 2],
    pub actors: &'a [ScriptActorId],
    pub random: &'a mut crate::random::EngineRandom,
}

impl ScriptWorld {
    /// Complete the admitted ordinary script wrapper's response-record maintenance.
    /// A nonzero record below age 200 requires caller-owned nominal ticks.
    /// The addition wraps as a byte; zero records reset age without reading timing.
    pub fn finish_ordinary_script_response(
        &mut self,
        id: ScriptActorId,
        nominal_ticks: Option<u32>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let control = self.actors[index].control.as_mut().with_context(|| {
            format!(
                "world {} instance {index}: ordinary script response control unavailable",
                self.world
            )
        })?;
        let marked = control.body_response_flags != 0
            || control.collision_transition_flags != 0
            || control.collision_response_auxiliary != [0; 2];
        if !marked {
            control.collision_response_age = 0;
        } else if control.collision_response_age < 200 {
            let ticks = nominal_ticks.with_context(|| {
                format!(
                    "world {} instance {index}: ordinary script response requires nominal_ticks",
                    self.world
                )
            })?;
            control.collision_response_age =
                control.collision_response_age.wrapping_add(ticks as u8);
        }
        control.body_response_flags = 0;
        control.collision_transition_flags = 0;
        control.collision_response_auxiliary = [0; 2];
        control.overlap_contact_identifier = 0;
        Ok(())
    }

    /// Save after ordinary animation advancement and before movement/collision.
    /// The audio owner captures its remembered slot in the same command order.
    pub fn capture_ordinary_rollback(
        &self,
        id: ScriptActorId,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<OrdinaryRollbackSnapshot> {
        let actor = &self.actors[self.resolve(id)?];
        let movement_flags = actor
            .control
            .as_ref()
            .context("ordinary rollback control unavailable")?
            .movement_flags;
        let animation = actor
            .animation
            .as_ref()
            .map(|animation| -> Result<_> {
                Ok((animation.playback.checkpoint()?, animation.frame))
            })
            .transpose()?;
        if animation.is_some() {
            sounds(
                id,
                ScriptSound::AnimationState(AnimationSoundState::Capture),
            )?;
        }
        Ok(OrdinaryRollbackSnapshot {
            actor: id,
            landing_recovery: actor.animation.as_ref().is_some_and(|animation| {
                matches!(
                    animation.playback.current_group(),
                    Some(67..=70 | 130 | 131)
                )
            }),
            movement_flags,
            animation,
        })
    }

    /// Apply the ordinary nonplayer stable response, including remembered-sound
    /// rollback, shared heading draw, and the age-gated bounds probe.
    /// Returns whether rejection changed the actor. Invalid stable records and
    /// accepted probes do not consume random state or restore the snapshot.
    pub fn recover_ordinary_stable(
        &mut self,
        id: ScriptActorId,
        input: OrdinaryStableRecoveryInputs<'_>,
        snapshot: &OrdinaryRollbackSnapshot,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        ensure!(
            snapshot.actor == id,
            "ordinary stable recovery snapshot belongs to another actor"
        );
        let control = self.actors[index]
            .control
            .as_ref()
            .context("ordinary stable recovery control unavailable")?;
        if !control.stable_position_valid {
            return Ok(false);
        }
        let position = control.stable_local_position;
        let age = control.collision_response_age;
        let prepared = self.prepare_stable_recovery_query(
            id,
            input.scene,
            input.animations,
            input.view,
            input.actors,
        )?;
        if self.query_stable_recovery(id, &prepared, input.scene, input.animations)? {
            return Ok(false);
        }
        // A rejected probe takes the selected player through a follow-up ground response that is
        // not modelled, and objects through a separate path.
        ensure!(
            self.selected_player_index() != Some(index)
                && self.actors[index]
                    .placement()
                    .is_some_and(|placement| placement.object_type == 1),
            "world {} instance {index}: stable recovery of the selected player or an object is unavailable",
            self.world
        );
        let turn = i32::from(self.actors[index].attributes[15]);
        ensure!(
            turn != 0,
            "world {} instance {index}: ordinary stable recovery attribute 15 must admit a positive random bound",
            self.world
        );
        self.restore_ordinary_rollback(id, snapshot, sounds)?;
        let actor = &mut self.actors[index];
        actor.attributes[6] = 0;
        let control = actor
            .control
            .as_mut()
            .expect("validated stable recovery control");
        control.falling_state = 0;
        control.sliding_state = 0;
        control.collision_transition_flags |= 1;
        let heading_delta = input.random.rnd(turn * 4)? - turn * 2;
        let spatial = self
            .spatial
            .as_mut()
            .context("ordinary stable recovery spatial state unavailable")?
            .actor_mut(index)?;
        spatial.local.position = position;
        spatial.local.angles[1] =
            ((i32::from(spatial.local.angles[1]) + heading_delta) & 2047) as u16;
        if age > 16 {
            self.resolve_ordinary_bounds_obstacle(
                id,
                super::obstacle::OrdinaryObstacleQuery {
                    scene: input.scene,
                    animations: input.animations,
                    prepared: &prepared.prepared,
                    vertical_offset: 0,
                    respond: true,
                },
            )?;
        }
        Ok(true)
    }

    pub(super) fn restore_ordinary_rollback(
        &mut self,
        id: ScriptActorId,
        snapshot: &OrdinaryRollbackSnapshot,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        ensure!(
            snapshot.actor == id,
            "world {} instance {index}: rollback snapshot belongs to another actor or generation",
            self.world
        );
        let actor = &mut self.actors[index];
        ensure!(
            actor.animation.is_some() == snapshot.animation.is_some(),
            "world {} instance {index}: rollback animation receiver changed",
            self.world
        );
        if let Some((playback, frame)) = &snapshot.animation {
            let animation = actor
                .animation
                .as_mut()
                .expect("validated rollback receiver");
            animation.playback.restore_checkpoint(playback)?;
            animation.frame = *frame;
            sounds(
                id,
                ScriptSound::AnimationState(AnimationSoundState::Restore),
            )?;
        }
        actor
            .control
            .as_mut()
            .context("ordinary rollback control unavailable")?
            .movement_flags = snapshot.movement_flags;
        Ok(())
    }

    /// Retain the ordinary end-of-update water-veto position when dry or falling.
    /// Water without falling preserves the earlier record, independently of
    /// update-entry and stable-position snapshots.
    pub fn retain_ordinary_water_position(&mut self, id: ScriptActorId) -> Result<bool> {
        let index = self.resolve(id)?;
        let falling = self.actors[index]
            .control
            .as_ref()
            .context("ordinary water position control unavailable")?
            .falling_state
            != 0;
        if matches!(self.actor_surface(id)?, 1..=4) && !falling {
            return Ok(false);
        }
        let position = self.spatial(id)?.local.position;
        self.spatial
            .as_mut()
            .context("ordinary water position spatial state unavailable")?
            .actor_mut(index)?
            .water_rollback_position = Some(position);
        Ok(true)
    }

    /// Apply the ordinary nonplayer water veto. Returns the original changed
    /// flag for the caller's remaining update stages. Selected players and actors
    /// with falling or queued motion return before reading surface state.
    pub fn rollback_ordinary_water(
        &mut self,
        id: ScriptActorId,
        selected_player: Option<ScriptActorId>,
        snapshot: &OrdinaryRollbackSnapshot,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        if Some(id) == selected_player {
            return Ok(false);
        }
        let control = self.actors[index]
            .control
            .as_ref()
            .context("ordinary water rollback control unavailable")?;
        if control.falling_state != 0 || control.queued_motion != 0 {
            return Ok(false);
        }
        if !matches!(self.actor_surface(id)?, 1..=4) {
            return Ok(false);
        }
        let position = self
            .spatial(id)?
            .water_rollback_position
            .context("ordinary water rollback retained position unavailable")?;
        self.restore_ordinary_rollback(id, snapshot, sounds)?;
        self.spatial
            .as_mut()
            .context("ordinary water rollback spatial state unavailable")?
            .actor_mut(index)?
            .local
            .position = position;
        let actor = &mut self.actors[index];
        actor.attributes[6] = 0;
        actor
            .control
            .as_mut()
            .expect("validated water rollback control")
            .collision_transition_flags |= 1;
        Ok(true)
    }
}
