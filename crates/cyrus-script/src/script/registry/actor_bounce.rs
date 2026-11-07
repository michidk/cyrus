//! The ground probe's bounce handler for an actor that landed on a bouncing actor.
//!
//! Live actor state behind [`BouncePort`] and the support attributes the shared state machine
//! reads.
use super::{ScriptActorId, ScriptWorld};
use crate::script::bounce::{
    ActorBounceSupport, BouncePort, actor_bounce_step, actor_bounce_waiting,
};
use crate::script::{JumpMotion, LivingState};
use anyhow::{Context, Result};
use cyrus_assets::surface::Surface;
use cyrus_soup::{AnimationGroup, Attribute};

type Sounds<'s> = &'s mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>;

struct BouncingActor<'w, 's> {
    world: &'w mut ScriptWorld,
    id: ScriptActorId,
    frame_scale: u32,
    ticks: u32,
    selected: bool,
    holding: bool,
    sounds: Sounds<'s>,
}

impl BouncingActor<'_, '_> {
    fn control(&self) -> &super::ActorControlState {
        self.world.actors[self.id.index]
            .control
            .as_ref()
            .expect("validated actor bounce control")
    }

    fn control_mut(&mut self) -> &mut super::ActorControlState {
        self.world.actors[self.id.index]
            .control
            .as_mut()
            .expect("validated actor bounce control")
    }

    fn local(&self) -> ([i32; 3], [u16; 3]) {
        let local = self
            .world
            .spatial(self.id)
            .expect("validated actor bounce pose")
            .local;
        (local.position, local.wrapped_angles())
    }
}

impl BouncePort for BouncingActor<'_, '_> {
    fn item_system_ready(&self) -> Result<bool> {
        // Only `BounceObject`'s first call reads this; the actor bounce has no such gate.
        Ok(true)
    }
    fn frame_scale(&self) -> u32 {
        self.frame_scale
    }
    fn living_state(&self) -> LivingState {
        self.control().living_state
    }
    fn set_living_state(&mut self, state: LivingState) {
        self.control_mut().living_state = state;
    }
    fn surface(&self) -> Surface {
        self.world.actors[self.id.index].surface.unwrap_or_default()
    }
    fn holding_weapon(&mut self) -> Result<bool> {
        Ok(self.holding)
    }
    fn combat_mode(&self) -> bool {
        self.control()
            .combat
            .as_ref()
            .is_some_and(|combat| combat.flags.contains(crate::script::CombatFlags::STANCE))
    }
    fn torch_burning(&self) -> bool {
        self.control().torch_burning != 0
    }
    fn jump_height(&self) -> u8 {
        self.world.actors[self.id.index].attributes[Attribute::JumpHeight]
    }
    fn clear_motion(&mut self) {
        let control = self.control_mut();
        control.falling_state = JumpMotion::NONE;
        control.queued_motion = JumpMotion::NONE;
        control.sliding_state = 0;
        control.motion_vector[0] = 0;
        control.motion_vector[1] = 0;
        self.world.actors[self.id.index].sync_falling_motion();
    }
    fn falling_state(&self) -> JumpMotion {
        self.control().falling_state
    }
    fn begin_fall(&mut self) {
        let (position, _) = self.local();
        let (origin, timer) = super::falling::ordinary_fall_origin(position[1], self.jump_height());
        let control = self.control_mut();
        control.fall_origin_y = origin;
        control.fall_timer = timer;
        control.falling_state = JumpMotion::STANDING;
        self.world.actors[self.id.index].sync_falling_motion();
    }
    fn lift(&self) -> i32 {
        self.control().fall_origin_y
    }
    fn set_lift(&mut self, lift: i32) {
        self.control_mut().fall_origin_y = lift;
    }
    fn position(&self) -> [i32; 3] {
        self.local().0
    }
    fn set_position(&mut self, position: [i32; 3]) -> Result<()> {
        let index = self.id.index;
        self.world
            .spatial
            .as_mut()
            .context("actor bounce spatial state not prepared")?
            .actor_mut(index)?
            .local
            .position = position;
        self.world.map_updates.register(index)?;
        Ok(())
    }
    fn heading(&self) -> u16 {
        self.local().1[1]
    }
    fn set_heading(&mut self, heading: u16) {
        let index = self.id.index;
        if let Some(spatial) = self
            .world
            .spatial
            .as_mut()
            .and_then(|spatial| spatial.actor_mut(index).ok())
            && spatial.local.angles[1] != i32::from(heading)
        {
            spatial.local.angles[1] = i32::from(heading);
            // A failed transform refresh surfaces on the next pose read of the same actor.
            let _ = spatial.refresh_transform();
        }
    }
    fn is_selected_player(&self) -> bool {
        self.selected
    }
    fn turn_delta(&self, scale: u32) -> i64 {
        self.world.globals.turn.delta(scale)
    }
    fn current_group(&self) -> Option<AnimationGroup> {
        self.world.actors[self.id.index]
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group())
    }
    fn request_group(&mut self, group: AnimationGroup, force: bool) -> Result<()> {
        self.world
            .request_ordinary_group(self.id, group, force, self.ticks, &mut *self.sounds)
    }
}

impl ScriptWorld {
    /// Run one update of an actor in the actor-bounce states whose support the handler's prefix
    /// found valid. Returns whether the update is handled, which ends the ground probe. The
    /// state 27 wait reads the support's animation: its current group, exit marker (the original's
    /// `anim.stop_flag`), command cursor and command count, which is the frame count the
    /// original reads from the first word of the group's table.
    pub(super) fn run_actor_bounce(
        &mut self,
        id: ScriptActorId,
        frame_scale: Option<u32>,
        ticks: u32,
        environment: super::transition::ObstacleEnvironment,
        sounds: Sounds<'_>,
    ) -> Result<bool> {
        let frame_scale = frame_scale.context("actor bounce update requires frame_scale")?;
        let support_id = self.actors[id.index]
            .model
            .support
            .actor
            .context("actor bounce support unavailable")?;
        let support = &self.actors[self.resolve(support_id)?];
        let playback = &support
            .animation
            .as_ref()
            .context("actor bounce support animation unavailable")?
            .playback;
        let waiting = actor_bounce_waiting(
            playback.current_group(),
            playback.exit_reached(),
            playback.current_command_cursor()?,
            playback.current_command_count()?,
        );
        let support = ActorBounceSupport {
            power: i32::from(support.attributes[Attribute::BouncePower]),
            speed: i32::from(support.attributes[Attribute::BounceMove]),
            steer: support.attributes[Attribute::BounceTurn] != 0,
            waiting,
        };
        let actor = &self.actors[id.index];
        let control = actor
            .control
            .as_ref()
            .context("actor bounce control unavailable")?;
        let (holding, _, _) = super::super::activation::weapon_predicates(
            control.living_state,
            actor
                .animation
                .as_ref()
                .map(|animation| super::super::ActivationAnimation {
                    group: animation.playback.current_group(),
                }),
            control.combat_death_default.is_some(),
            control.weapon_drawn,
            actor.attributes[Attribute::HoldingWeapon],
            environment.item_system_ready,
            environment.shell_execution,
        );
        let mut port = BouncingActor {
            world: self,
            id,
            frame_scale,
            ticks,
            selected: environment.selected_player == Some(id),
            holding,
            sounds,
        };
        actor_bounce_step(&mut port, support, |port| {
            port.world.actors[port.id.index].model.support.actor = None;
        })
    }
}
