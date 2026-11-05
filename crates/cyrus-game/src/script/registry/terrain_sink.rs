//! The terrain sink check that opens the ordinary ground-support probe.
//!
//! An airborne actor whose lower end is under the terrain by more than half of
//! `post_collide_height` returns to its retained stable position and starts to fall
//! (preservation player.md, "Terrain sink recovery").
use super::{ActorControlState, ScriptActorId, ScriptWorld, ground::OrdinaryGroundQuery};
use crate::script::{CollisionFlags, JumpMotion, LivingState};
use anyhow::{Context, Result, bail, ensure};
use cyrus_script::{AnimationGroup, Attribute};

/// Whether the terrain sample puts the actor under the surface: the sample is below the actor's
/// reference height (query Y in map units plus the positive Y extent) by more than half of
/// `post_collide_height`. Arithmetic wraps as the original's 32-bit values do.
pub(super) fn sinks_below_terrain(
    terrain_height: i32,
    query_y: i32,
    positive_y_extent: i32,
    post_collide_height: u32,
) -> bool {
    let reference = (query_y >> 8).wrapping_add(positive_y_extent);
    terrain_height < reference
        && reference.wrapping_sub(terrain_height).wrapping_abs()
            > ((post_collide_height as i32) >> 1)
}

impl ActorControlState {
    /// The check applies to an actor with falling, sliding or queued motion, or in one of the
    /// actor-bounce living states.
    fn admits_terrain_sink_check(&self) -> bool {
        self.falling_state != JumpMotion::NONE
            || self.sliding_state != 0
            || self.queued_motion != JumpMotion::NONE
            || self.living_state.is_actor_bounce()
    }

    /// The fall start: living state 15 is left alone; the bounce states and rope-jump queued
    /// motion become a standing queued jump with the living state and motion vector cleared;
    /// queued motion becomes falling (raised-arms bit kept) with the ordinary fall origin and timer; an existing fall
    /// restarts as a plain fall. Returns the animation group to request, if any.
    fn start_terrain_sink_fall(
        &mut self,
        local_y: i32,
        jump_height: u8,
        holding_weapon: bool,
        current_group: Option<AnimationGroup>,
    ) -> Option<AnimationGroup> {
        if self.living_state == LivingState::HANGING {
            return None;
        }
        if self.living_state.is_bounce() || self.queued_motion == JumpMotion::ROPE {
            self.queued_motion = JumpMotion::STANDING;
            self.living_state = LivingState::ORDINARY;
            self.motion_vector = [0; 3];
        }
        if self.queued_motion != JumpMotion::NONE {
            self.last_collide |= CollisionFlags::PREMETURE_JUMP_END;
            let raised = self.queued_motion.arms_raised();
            self.falling_state = JumpMotion::STANDING.with_arms_raised(raised);
            self.queued_motion = JumpMotion::NONE;
            let (origin, timer) = super::falling::ordinary_fall_origin(local_y, jump_height);
            self.fall_origin_y = origin;
            self.fall_timer = timer;
            return if raised {
                Some(AnimationGroup::FALL_UP_ARMS)
            } else if holding_weapon {
                (current_group != Some(AnimationGroup::FIGHT_FALL_ATTACK))
                    .then_some(AnimationGroup::FIGHT_FALL)
            } else {
                Some(AnimationGroup::FALL)
            };
        }
        if self.falling_state != JumpMotion::NONE {
            self.last_collide |= CollisionFlags::PREMETURE_FALL;
            let raised = self.falling_state.arms_raised();
            self.falling_state = JumpMotion::STANDING.with_arms_raised(raised);
        }
        None
    }
}

impl ScriptWorld {
    /// Run the terrain sink check for one ordinary actor. Returns whether it fired. A sample off
    /// the height grid reads unrelated memory in the original; here it does not fire.
    pub(super) fn recover_terrain_sink(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        if self
            .camera
            .as_ref()
            .is_some_and(|camera| camera.transition != 0)
        {
            return Ok(false);
        }
        let Some(terrain) = query.scene.terrain.as_ref() else {
            return Ok(false);
        };
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("terrain sink control unavailable")?;
        if !control.admits_terrain_sink_check() {
            return Ok(false);
        }
        let position = self.spatial(id)?.query_pose()?.position;
        let Some(height) = terrain.height(position.map(i64::from)) else {
            return Ok(false);
        };
        let height = height.trunc();
        ensure!(
            height.is_finite() && height >= f64::from(i32::MIN) && height <= f64::from(i32::MAX),
            "terrain sink height outside integer map range"
        );
        let extent = self.ordinary_collision_shape(id)?.positive_extent[1];
        if !sinks_below_terrain(
            height as i32,
            position[1],
            extent,
            query.scene.settings.post_collide_height,
        ) {
            return Ok(false);
        }
        let actor = &self.actors[id.index];
        let control = actor.control.as_ref().expect("admitted sink control");
        let stable = control.stable_local_position;
        let current_group = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        let (holding, _, _) = super::super::activation::weapon_predicates(
            control.living_state,
            actor
                .animation
                .as_ref()
                .map(|_| super::super::ActivationAnimation {
                    group: current_group,
                }),
            control.combat_death_default.is_some(),
            control.weapon_drawn,
            actor.attributes[Attribute::HoldingWeapon],
            environment.item_system_ready,
            environment.shell_execution,
        );
        let jump_height = actor.attributes[Attribute::JumpHeight];
        let static_model = actor.animation.is_none();
        self.spatial
            .as_mut()
            .context("terrain sink spatial state unavailable")?
            .actor_mut(id.index)?
            .local
            .position = stable;
        let actor = &mut self.actors[id.index];
        let control = actor.control.as_mut().expect("admitted sink control");
        control.move_counter = 0;
        let group = control.start_terrain_sink_fall(stable[1], jump_height, holding, current_group);
        actor.sync_falling_motion();
        if let Some(group) = group
            && self.updates.animation_system
        {
            if static_model {
                bail!(
                    "world {} instance {}: terrain sink fall start for a static-model actor is unsupported",
                    self.world,
                    id.index
                );
            }
            // The original's fall start forces all three groups.
            self.request_ordinary_group(id, group, true, environment.ticks, sounds)?;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sink_test_needs_the_sample_below_by_more_than_half_the_margin() {
        // Reference 100 + 20; margin 50 so the threshold is 25.
        let y = 100 << 8;
        assert!(sinks_below_terrain(94, y, 20, 50));
        assert!(!sinks_below_terrain(95, y, 20, 50));
        assert!(!sinks_below_terrain(120, y, 20, 50));
        assert!(!sinks_below_terrain(130, y, 20, 50));
        assert!(!sinks_below_terrain(94, y, 20, 100));
        assert!(sinks_below_terrain(69, y, 20, 100));
    }

    #[test]
    fn the_sink_test_uses_the_arithmetic_shift_of_the_query_height() {
        assert!(sinks_below_terrain(-1, -1, 50, 50));
        // -1 >> 8 is -1, so the reference is 49.
        assert!(!sinks_below_terrain(24, -1, 50, 50));
        assert!(sinks_below_terrain(23, -1, 50, 50));
    }

    fn control(living: LivingState, queued: u8, falling: u8) -> ActorControlState {
        ActorControlState {
            living_state: living,
            queued_motion: JumpMotion::from_byte(queued),
            falling_state: JumpMotion::from_byte(falling),
            ..Default::default()
        }
    }

    #[test]
    fn the_check_admits_airborne_states_only() {
        assert!(!control(LivingState::ORDINARY, 0, 0).admits_terrain_sink_check());
        assert!(control(LivingState::ORDINARY, 2, 0).admits_terrain_sink_check());
        assert!(control(LivingState::ORDINARY, 0, 1).admits_terrain_sink_check());
        assert!(control(LivingState::LANDED_ON_ACTOR, 0, 0).admits_terrain_sink_check());
        assert!(control(LivingState::FALLING_FROM_ACTOR, 0, 0).admits_terrain_sink_check());
        assert!(!control(LivingState::BOUNCE_ENTRY, 0, 0).admits_terrain_sink_check());
        let mut sliding = control(LivingState::ORDINARY, 0, 0);
        sliding.sliding_state = 1;
        assert!(sliding.admits_terrain_sink_check());
    }

    #[test]
    fn hanging_is_left_unchanged() {
        let mut state = control(LivingState::HANGING, 2, 1);
        assert_eq!(state.start_terrain_sink_fall(0, 10, false, None), None);
        assert_eq!(state.queued_motion, JumpMotion::FORWARD);
        assert_eq!(state.falling_state, JumpMotion::STANDING);
        assert_eq!(state.last_collide, CollisionFlags::default());
    }

    #[test]
    fn queued_motion_becomes_falling_with_origin_timer_mark_and_group() {
        let mut state = control(LivingState::ORDINARY, 0x82, 0);
        let group = state.start_terrain_sink_fall(1000, 3, false, None);
        assert_eq!(group, Some(AnimationGroup::FALL_UP_ARMS));
        assert_eq!(state.falling_state.byte(), 0x81);
        assert_eq!(state.queued_motion, JumpMotion::NONE);
        assert_eq!(state.fall_origin_y, 1000 + 3 * 256);
        assert_eq!(state.fall_timer, 30);
        assert_eq!(state.last_collide, CollisionFlags::PREMETURE_JUMP_END);

        let mut state = control(LivingState::ORDINARY, 8, 0);
        assert_eq!(
            state.start_terrain_sink_fall(0, 0, false, None),
            Some(AnimationGroup::FALL)
        );
        assert_eq!(state.falling_state, JumpMotion::STANDING);

        let mut state = control(LivingState::ORDINARY, 8, 0);
        assert_eq!(
            state.start_terrain_sink_fall(0, 0, true, None),
            Some(AnimationGroup::FIGHT_FALL)
        );
        let mut state = control(LivingState::ORDINARY, 8, 0);
        assert_eq!(
            state.start_terrain_sink_fall(0, 0, true, Some(AnimationGroup::FIGHT_FALL_ATTACK)),
            None
        );
        assert_eq!(state.falling_state, JumpMotion::STANDING);
    }

    #[test]
    fn bounce_states_and_rope_motion_first_become_a_standing_queued_jump() {
        for living in [
            LivingState::LANDED_ON_ACTOR,
            LivingState::BOUNCE_ENTRY,
            LivingState::BOUNCE_FALLING,
        ] {
            let mut state = control(living, 0, 0);
            state.motion_vector = [5, 6, 7];
            let group = state.start_terrain_sink_fall(0, 0, false, None);
            assert_eq!(state.motion_vector, [0; 3]);
            assert_eq!(state.living_state, LivingState::ORDINARY);
            assert_eq!(state.falling_state, JumpMotion::STANDING);
            assert_eq!(state.last_collide, CollisionFlags::PREMETURE_JUMP_END);
            assert_eq!(group, Some(AnimationGroup::FALL));
        }
        let mut state = control(LivingState::ORDINARY, 24, 0);
        state.start_terrain_sink_fall(0, 0, false, None);
        assert_eq!(state.falling_state, JumpMotion::STANDING);
        assert_eq!(state.last_collide, CollisionFlags::PREMETURE_JUMP_END);
        // The raised-arms variant of the rope jump is not the rope code and stays queued-as-is.
        let mut state = control(LivingState::ORDINARY, 0x98, 0);
        state.start_terrain_sink_fall(0, 0, false, None);
        assert_eq!(state.falling_state.byte(), 0x81);
    }

    #[test]
    fn an_existing_fall_restarts_as_a_plain_fall_without_a_group() {
        let mut state = control(LivingState::ORDINARY, 0, 0x89);
        state.fall_origin_y = 77;
        state.fall_timer = 5;
        assert_eq!(state.start_terrain_sink_fall(0, 9, false, None), None);
        assert_eq!(state.falling_state.byte(), 0x81);
        assert_eq!(state.last_collide, CollisionFlags::PREMETURE_FALL);
        assert_eq!((state.fall_origin_y, state.fall_timer), (77, 5));
    }
}
