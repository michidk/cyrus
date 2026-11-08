//! Queued motion admission precedes its request and vertical writer.
use crate::script::CombatFlags;
use crate::script::JumpMotion;
use crate::script::LivingState;
use crate::script::MotionDirection;
use crate::script::locomotion::StepDirection;
use crate::script::registry::ActionWords;
use cyrus_assets::surface::Surface;
use cyrus_soup::{AnimationGroup, Attribute};
use cyrus_world::heading::wrap;

/// Current receiver state and established edge-service outcome.
#[derive(Clone, Copy, Debug)]
pub struct OrdinaryQueuedInputs {
    pub queued: JumpMotion,
    pub torch_burning: u8,
    pub current_group: Option<AnimationGroup>,
    pub pending_group: Option<AnimationGroup>,
    pub has_group_49: bool,
    pub has_group_50: bool,
    pub holding_weapon: bool,
    pub edge_handled: bool,
}

/// Ordered handler decision. `handled` determines whether support response runs.
/// An advancing stage requests its group before running vertical arithmetic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrdinaryQueuedAction {
    pub clear_queued: bool,
    pub handled: bool,
    pub advance_vertical: bool,
    pub request_group: Option<AnimationGroup>,
}

/// Resolve the original low/high-byte state and pending-group arbitration.
/// Edge admission must be established by its owner, not inferred here.
pub fn ordinary_queued_action(input: OrdinaryQueuedInputs) -> OrdinaryQueuedAction {
    let mut action = OrdinaryQueuedAction::default();
    if input.queued == JumpMotion::NONE {
        return action;
    }
    let low = input.queued.kind();
    let high = input.queued.arms_raised();
    let torch = input.torch_burning != 0;
    if low == JumpMotion::BACKWARD {
        let available = if torch {
            input.has_group_50
        } else {
            input.has_group_49
        };
        if !available {
            action.clear_queued = true;
            return action;
        }
        if input.current_group
            != Some(if torch {
                AnimationGroup::JUMP_BACKWARD_TORCH
            } else {
                AnimationGroup::JUMP_BACKWARD
            })
        {
            action.handled = torch;
            return action;
        }
    }
    if input.pending_group.is_some_and(|group| {
        (AnimationGroup::JUMP_START..=AnimationGroup::JUMP_LEFT_START_TORCH).contains(&group)
            || group == AnimationGroup::FIGHT_JUMP_START
    }) {
        return action;
    }
    if input.edge_handled {
        action.clear_queued = true;
        action.handled = true;
        return action;
    }
    action.request_group = match low {
        JumpMotion::STANDING => Some(if torch {
            AnimationGroup::JUMP_TORCH
        } else if high {
            AnimationGroup::JUMP_UP_ARMS
        } else if input.holding_weapon {
            AnimationGroup::FIGHT_JUMP
        } else {
            AnimationGroup::JUMP
        }),
        JumpMotion::FORWARD => Some(if torch {
            AnimationGroup::JUMP_FORWARD_TORCH
        } else if high {
            AnimationGroup::JUMP_FORWARD_ARMS
        } else {
            AnimationGroup::JUMP_FORWARD
        }),
        JumpMotion::BACKWARD => Some(if torch {
            AnimationGroup::JUMP_BACKWARD_TORCH
        } else {
            AnimationGroup::JUMP_BACKWARD
        }),
        JumpMotion::RUNNING | JumpMotion::ROPE => Some(if torch {
            AnimationGroup::JUMP_RIGHT_TORCH
        } else if high {
            AnimationGroup::JUMP_RUN_ARMS
        } else {
            AnimationGroup::JUMP_RUN
        }),
        JumpMotion::RUNNING_LEFT_LEAD => Some(if torch {
            AnimationGroup::JUMP_LEFT_TORCH
        } else if high {
            AnimationGroup::JUMP_RUN_ARMS
        } else {
            AnimationGroup::JUMP_LEFT
        }),
        _ => None,
    };
    action.handled = true;
    action.advance_vertical = true;
    action
}

impl super::ScriptWorld {
    /// Begin an admitted ordinary jump through the original state writer.
    /// `native_task_active` comes from the motion-task owner; it is not inferred
    /// from script continuations. The original runs one routine for every actor: the selected
    /// player's edge services (`edge`, the ground query and clock inputs they read) run inside
    /// it, and for any other actor they do nothing, so a nonselected actor in living state 15
    /// clears its stable-position flag and leaves everything else unchanged. The selected player
    /// enters through `begin_player_jump` and fails here without `edge`.
    /// Returns whether ordinary queued state was initialized.
    #[allow(clippy::too_many_arguments)] // The selected player's ledge services take the full query.
    pub fn begin_ordinary_jump(
        &mut self,
        id: super::ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        requested_motion: u8,
        native_task_active: bool,
        environment: super::transition::ObstacleEnvironment,
        edge: Option<(super::ground::OrdinaryGroundQuery<'_>, super::LedgeTiming)>,
        sounds: &mut dyn FnMut(
            super::ScriptActorId,
            super::super::ScriptSound,
        ) -> anyhow::Result<()>,
    ) -> anyhow::Result<bool> {
        use anyhow::Context;
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary jump control unavailable")?;
        if native_task_active
            || control.queued_motion != JumpMotion::NONE
            || control.falling_state != JumpMotion::NONE
            || control.sliding_state != 0
            || !matches!(
                control.living_state,
                LivingState::ORDINARY | LivingState::HANGING
            )
        {
            return Ok(false);
        }
        if environment.selected_player == Some(id) {
            let (query, timing) = edge.with_context(|| {
                format!(
                    "world {} instance {index}: the selected player's jump entry needs its edge geometry and clock inputs",
                    self.world
                )
            })?;
            return self.begin_player_jump(
                id,
                query,
                environment,
                timing,
                requested_motion,
                sounds,
            );
        }
        // Nonselected actors skip both edge services; hang release does nothing for them.
        if control.living_state == LivingState::HANGING {
            self.actors[index]
                .control
                .as_mut()
                .expect("validated ordinary jump control")
                .stable_position_valid = false;
            return Ok(false);
        }
        self.initialize_ordinary_jump(id, requested_motion, false, environment, sounds)
    }

    /// The selected player's jump entry: the gates of the ordinary entry for the selected player
    /// (jump cooldown, rope, motion bytes, state 0 or 15, water), the state-15 hang release and
    /// the ledge edge entry, before the ordinary initialization. Returns whether it started a
    /// jump; a ledge catch or a release handles the key instead.
    #[allow(clippy::too_many_arguments)] // The ledge services take the full ground query.
    pub(super) fn begin_player_jump(
        &mut self,
        id: super::ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        timing: super::ledge_states::LedgeTiming,
        requested_motion: u8,
        sounds: &mut dyn FnMut(
            super::ScriptActorId,
            super::super::ScriptSound,
        ) -> anyhow::Result<()>,
    ) -> anyhow::Result<bool> {
        use anyhow::Context;
        self.require_model_identity(query.animations)?;
        let index = self.resolve(id)?;
        let surface = self.actor_surface(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("player jump control unavailable")?;
        if self.globals.conditional_ticks()? != 0
            || control.rope_attached
            || control.queued_motion != JumpMotion::NONE
            || control.falling_state != JumpMotion::NONE
            || control.sliding_state != 0
            || !matches!(
                control.living_state,
                LivingState::ORDINARY | LivingState::HANGING
            )
            || (actor.attributes[Attribute::PlayerType] == 1 && surface.is_water())
        {
            return Ok(false);
        }
        let state = control.living_state;
        if state == LivingState::HANGING {
            self.ledge_hang_release(id, query, environment, timing, sounds)?;
            return Ok(false);
        }
        if self.selected_edge_entry(id, query, environment, timing, sounds)? {
            return Ok(false);
        }
        self.initialize_ordinary_jump(id, requested_motion, false, environment, sounds)
    }

    pub(super) fn script_jump_admitted(
        &self,
        id: super::ScriptActorId,
        selected_player: Option<super::ScriptActorId>,
    ) -> anyhow::Result<bool> {
        use anyhow::Context;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor.control.as_ref().context("Jump control unavailable")?;
        let surface = self.actor_surface(id)?;
        let selected = selected_player == Some(id);
        let group = actor
            .animation
            .as_ref()
            .context("Jump animation receiver unavailable")?
            .playback
            .current_group()
            .context("Jump current group unavailable")?;
        if (selected && self.globals.conditional_ticks()? != 0)
            || surface.is_deep_water()
            || actor.tasks.is_active()
            || control.queued_motion != JumpMotion::NONE
            || control.falling_state != JumpMotion::NONE
            || control.sliding_state != 0
            || !matches!(
                control.living_state,
                LivingState::ORDINARY | LivingState::HANGING
            )
            || matches!(
                group,
                AnimationGroup::FIGHT_LAND
                    | AnimationGroup::FIGHT_LAND_ATTACK
                    | AnimationGroup::FIGHT_LAND_RIGHT
                    | AnimationGroup::FIGHT_LAND_LEFT
            )
            || (selected && actor.attributes[Attribute::PlayerType] == 1 && surface.is_water())
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// Zero-argument SOUP Jump, from preservation's player.md#script-jump.
    /// Admission refusal and handled edge services still complete the native immediately.
    pub(super) fn begin_script_jump(
        &mut self,
        id: super::ScriptActorId,
        geometry: Option<super::LocomotionGeometry<'_>>,
        environment: super::transition::ObstacleEnvironment,
        timing: super::ledge_states::LedgeTiming,
        sounds: &mut dyn FnMut(
            super::ScriptActorId,
            super::super::ScriptSound,
        ) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        if !self.script_jump_admitted(id, environment.selected_player)? {
            return Ok(());
        }
        let index = self.resolve(id)?;
        let control = self.actors[index]
            .control
            .as_ref()
            .expect("admitted Jump control");
        let selected = environment.selected_player == Some(id);
        let hanging = control.living_state == LivingState::HANGING;
        self.actors[index]
            .control
            .as_mut()
            .expect("validated Jump control")
            .stable_position_valid = false;
        if hanging && !selected {
            return Ok(());
        }
        if selected {
            let query = geometry
                .context("Jump selected-player edge geometry unavailable")?
                .ground;
            if hanging {
                self.ledge_hang_release(id, query, environment, timing, sounds)?;
                return Ok(());
            }
            if self.selected_edge_entry(id, query, environment, timing, sounds)? {
                return Ok(());
            }
        }
        self.initialize_ordinary_jump(id, 8, true, environment, sounds)?;
        Ok(())
    }

    /// The stage between the ordinary animation and script stages: queued (else falling) motion 8
    /// or 9 carries a jump forward, and motion 2 a forward-steered jump, at a rate set by
    /// `attr_jump_time` and the frame scale until the accumulated distance reaches its cap; deep water changes neither. Motion 3 uses the
    /// ordinary backward step and retained jump heading. It reads no actor kind.
    pub(super) fn run_pre_script_displacement(
        &mut self,
        id: super::ScriptActorId,
        frame_scale: u32,
        inputs: super::ScriptInputs<'_>,
        query: Option<super::obstacle::OrdinaryObstacleQuery<'_>>,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("pre-script displacement control unavailable")?;
        let motion = if control.queued_motion != JumpMotion::NONE {
            control.queued_motion
        } else {
            control.falling_state
        };
        if motion.kind() == JumpMotion::BACKWARD {
            return self.run_backward_jump_displacement(id, frame_scale, inputs, query);
        }
        if motion.kind() == JumpMotion::ROPE {
            return self.run_rope_jump_displacement(id, frame_scale);
        }
        // Per tick distance numerator and the cap of the accumulated distance, both in Q8.
        let (rate, cap): (i32, i32) = match motion.kind() {
            JumpMotion::RUNNING | JumpMotion::RUNNING_LEFT_LEAD => (0x10000, 0x18000),
            JumpMotion::FORWARD => (0xb400, 0x10000),
            _ => return Ok(()),
        };
        if control.rope_attached {
            return Ok(());
        }
        let divisor = i32::from(actor.attributes[Attribute::JumpTime]);
        anyhow::ensure!(
            divisor != 0,
            "world {} instance {index}: jump displacement needs attr_jump_time",
            self.world
        );
        let suppressed = actor
            .animation
            .as_ref()
            .is_some_and(|animation| animation.playback.translation_suppressed());
        let accumulated = control.sliding_distance;
        let distance = if accumulated < cap {
            let step = (rate / divisor).wrapping_mul(frame_scale as i32) >> 8;
            if cap < accumulated.wrapping_add(step) {
                cap.wrapping_sub(accumulated)
            } else {
                step
            }
        } else {
            0
        };
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("validated displacement control");
        control.motion_direction = MotionDirection::FORWARD;
        control.move_counter |= StepDirection::Forward.counter_mask();
        self.globals.locomotion_called |= StepDirection::Forward.called_flag();
        if suppressed {
            return Ok(());
        }
        control.sliding_distance = control.sliding_distance.wrapping_add(distance);
        if distance > 0 {
            let pose = self.spatial(id)?.local;
            let position = cyrus_world::transform::translate_forward(
                pose.position,
                pose.wrapped_angles()[1],
                distance,
            )?;
            self.spatial
                .as_mut()
                .context("pre-script displacement spatial state unavailable")?
                .actor_mut(index)?
                .local
                .position = position;
        }
        Ok(())
    }

    /// Preservation's pre-script motion-3 branch shares backward startup arithmetic.
    /// The exact queued byte 3 retains the jump heading even after the actor turns.
    fn run_backward_jump_displacement(
        &mut self,
        id: super::ScriptActorId,
        frame_scale: u32,
        inputs: super::ScriptInputs<'_>,
        query: Option<super::obstacle::OrdinaryObstacleQuery<'_>>,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        let index = self.resolve(id)?;
        let control = self.actors[index]
            .control
            .as_ref()
            .context("backward jump control unavailable")?;
        if control.rope_attached {
            return Ok(());
        }
        let combat_stance = control
            .combat
            .as_ref()
            .is_some_and(|combat| combat.flags.contains(CombatFlags::STANCE));
        let (_, holding_weapon) = self.spacing_weapon(id, inputs)?;
        let stance = holding_weapon || combat_stance;
        if stance
            && self.blocked_ahead(
                id,
                30,
                query.context("backward jump obstacle query unavailable")?,
            )?
        {
            return Ok(());
        }
        self.actors[index]
            .control
            .as_mut()
            .expect("validated control")
            .motion_direction = MotionDirection::BACKWARD;
        if self.actors[index]
            .surface
            .is_some_and(Surface::is_deep_water)
            && !self.edge_probe_clear(
                id,
                MotionDirection::BACKWARD,
                query.context("backward jump edge scene unavailable")?.scene,
            )?
        {
            return Ok(());
        }
        self.globals.locomotion_called |= StepDirection::Backward.called_flag();
        let actor = &mut self.actors[index];
        let animation = actor
            .animation
            .as_ref()
            .context("backward jump animation unavailable")?;
        let group = animation.playback.current_group();
        let suppressed = animation.playback.translation_suppressed();
        let length = if stance {
            i32::from(actor.attributes[Attribute::FightBackwardSpeed]) << 8
        } else if matches!(
            group,
            Some(AnimationGroup::SLIDE_BACKWARD | AnimationGroup::SLIDE_BACKWARD_TORCH)
        ) {
            i32::from(actor.attributes[Attribute::ForwardSpeed]) << 7
        } else {
            i32::from(actor.attributes[Attribute::BackwardSpeed]) << 8
        };
        let control = actor.control.as_mut().expect("validated control");
        let distance = super::super::locomotion::initial_step_distance(
            &mut control.move_counter,
            6,
            actor.attributes[Attribute::StepType],
            length,
        );
        let retained =
            (control.queued_motion == JumpMotion::BACKWARD).then_some(control.motion_vector[1]);
        let pose = self.spatial(id)?.local;
        let movement = super::super::locomotion::backward_translation(
            retained.map_or(pose.wrapped_angles()[1], |heading| heading as u16),
            distance,
            frame_scale as i32,
            suppressed,
        );
        if let Some((heading, distance)) = movement {
            let position =
                cyrus_world::transform::translate_forward(pose.position, heading, distance)?;
            self.spatial
                .as_mut()
                .context("backward jump spatial state unavailable")?
                .actor_mut(index)?
                .local
                .position = position;
        }
        Ok(())
    }

    /// Queued motion 24, the jump after a rope release: the speed word moves the actor
    /// `|speed| × frame scale >> 8` (Q8) forward at its heading, or backward (heading plus 1024)
    /// for a negative word, unless the current animation suppresses translation. It marks the
    /// motion classification (2 or 3) and the forward or backward movement bits, and does nothing
    /// while the actor is attached.
    fn run_rope_jump_displacement(
        &mut self,
        id: super::ScriptActorId,
        frame_scale: u32,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        let index = self.resolve(id)?;
        let suppressed = self.actors[index]
            .animation
            .as_ref()
            .is_some_and(|animation| animation.playback.translation_suppressed());
        let control = self.actors[index]
            .control
            .as_mut()
            .context("rope jump displacement control unavailable")?;
        if control.rope_attached {
            return Ok(());
        }
        let backward = control.motion_vector[2] < 0;
        control.motion_classification = if backward {
            MotionDirection::BACKWARD
        } else {
            MotionDirection::FORWARD
        };
        control.move_counter |= if backward {
            StepDirection::Backward
        } else {
            StepDirection::Forward
        }
        .counter_mask();
        let distance = control.motion_vector[2]
            .unsigned_abs()
            .wrapping_mul(frame_scale)
            >> 8;
        if suppressed || distance < 1 {
            return Ok(());
        }
        let pose = self.spatial(id)?.local;
        let heading = wrap(pose.angles[1].wrapping_add(if backward { 1024 } else { 0 })) as u16;
        let position = cyrus_world::transform::translate_forward(
            pose.position,
            heading,
            i32::try_from(distance).unwrap_or(i32::MAX),
        )?;
        self.spatial
            .as_mut()
            .context("rope jump displacement spatial state unavailable")?
            .actor_mut(index)?
            .local
            .position = position;
        Ok(())
    }

    /// The per-update jump window of the directional-state bytes, before ordinary movement. A
    /// pending byte is promoted after 36 ticks; outside the run and walk jump groups, or while
    /// queued motion runs, the low bits clear. A set low bit starts the jump with motion 8, and
    /// a set second bit with motion 9.
    pub(super) fn run_jump_window(
        &mut self,
        id: super::ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        fine_ticks: Option<u32>,
        frame_scale: Option<u32>,
        sounds: &mut dyn FnMut(
            super::ScriptActorId,
            super::super::ScriptSound,
        ) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        let index = self.resolve(id)?;
        let actor = &mut self.actors[index];
        let group = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        let task_active = actor.tasks.is_active();
        let Some(control) = actor.control.as_mut() else {
            return Ok(());
        };
        if let Some(motion) = control.advance_action_words(environment.ticks, group) {
            if environment.selected_player == Some(id) {
                let timing = super::ledge_states::LedgeTiming {
                    fine_ticks: fine_ticks
                        .context("jump window needs the step's fine-tick increment")?,
                    frame_scale,
                };
                self.begin_player_jump(id, query, environment, timing, motion, sounds)?;
            } else {
                self.begin_ordinary_jump(
                    id,
                    query.animations,
                    motion,
                    task_active,
                    environment,
                    None,
                    sounds,
                )?;
            }
        }
        Ok(())
    }

    /// Initialize an admitted jump: fall origin, queued state and its animation request.
    fn initialize_ordinary_jump(
        &mut self,
        id: super::ScriptActorId,
        requested_motion: u8,
        script_native: bool,
        environment: super::transition::ObstacleEnvironment,
        sounds: &mut dyn FnMut(
            super::ScriptActorId,
            super::super::ScriptSound,
        ) -> anyhow::Result<()>,
    ) -> anyhow::Result<bool> {
        use anyhow::Context;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary jump control unavailable")?;
        let group = actor
            .animation
            .as_ref()
            .context("ordinary jump animation receiver unavailable")?
            .playback
            .current_group()
            .context("ordinary jump current group unavailable")?;
        let force = control.jump_selector != 0;
        let selected = environment.selected_player == Some(id);
        let (queued, request) = if force {
            (JumpMotion::ROPE, AnimationGroup::ROPE_JUMP)
        } else {
            match group {
                AnimationGroup::RUN => {
                    if requested_motion == 9 {
                        (
                            JumpMotion::RUNNING_LEFT_LEAD,
                            AnimationGroup::JUMP_LEFT_START,
                        )
                    } else {
                        (JumpMotion::RUNNING, AnimationGroup::JUMP_RUN_START)
                    }
                }
                AnimationGroup::RUN_TORCH => {
                    if requested_motion == 9 {
                        (
                            JumpMotion::RUNNING_LEFT_LEAD,
                            AnimationGroup::JUMP_LEFT_START_TORCH,
                        )
                    } else {
                        (JumpMotion::RUNNING, AnimationGroup::JUMP_RIGHT_START_TORCH)
                    }
                }
                AnimationGroup::RUN_BACKWARD | AnimationGroup::WALK_BACKWARD_TORCH
                    if script_native || selected =>
                {
                    (
                        JumpMotion::BACKWARD,
                        if group == AnimationGroup::RUN_BACKWARD {
                            AnimationGroup::JUMP_BACKWARD
                        } else {
                            AnimationGroup::JUMP_BACKWARD_TORCH
                        },
                    )
                }
                AnimationGroup::FIGHT_RUN if script_native => {
                    (JumpMotion::RUNNING, AnimationGroup::JUMP_RUN_START)
                }
                _ if script_native => {
                    let holding = super::super::activation::weapon_predicates(
                        control.living_state,
                        Some(super::super::ActivationAnimation { group: Some(group) }),
                        control.combat_death_default.is_some(),
                        control.weapon_drawn,
                        actor.attributes[Attribute::HoldingWeapon],
                        environment.item_system_ready,
                        environment.shell_execution,
                    )
                    .0;
                    (
                        JumpMotion::STANDING,
                        if script_native
                            && (holding
                                || control.combat.as_ref().is_some_and(|combat| {
                                    combat.flags.contains(CombatFlags::STANCE)
                                }))
                        {
                            AnimationGroup::FIGHT_JUMP_START
                        } else if control.torch_burning == 0 {
                            AnimationGroup::JUMP_START
                        } else {
                            AnimationGroup::JUMP_START_TORCH
                        },
                    )
                }
                _ => (
                    JumpMotion::STANDING,
                    if control.torch_burning == 0 {
                        AnimationGroup::JUMP_START
                    } else {
                        AnimationGroup::JUMP_START_TORCH
                    },
                ),
            }
        };
        // The selected player's key jump from a backward run group takes the script row as well.
        let selected_backward = selected
            && matches!(
                group,
                AnimationGroup::RUN_BACKWARD | AnimationGroup::WALK_BACKWARD_TORCH
            );
        let heading = (script_native || selected_backward)
            .then(|| self.spatial(id).map(|pose| pose.local.angles[1]))
            .transpose()?;
        if !force && selected_backward {
            self.globals.set_conditional_ticks(6)?;
        }
        let origin = self.spatial(id)?.local.position[1];
        let position = super::animation_sound_position(self.spatial.as_ref(), index);
        let event_context = super::effects::ActorEventContext::capture(self, id);
        let event_camera = &mut self.camera;
        let event_effects = &mut self.effects;
        let actor = &mut self.actors[index];
        let control = actor.control.as_mut().expect("validated jump control");
        control.stable_position_valid = false;
        control.fall_origin_y = origin;
        control.fall_timer = 0;
        control.sliding_distance = 0;
        control.queued_motion = queued;
        if !force
            && ((script_native
                && matches!(
                    group,
                    AnimationGroup::RUN_BACKWARD
                        | AnimationGroup::WALK_BACKWARD_TORCH
                        | AnimationGroup::FIGHT_RUN
                ))
                || selected_backward)
        {
            control.motion_vector[1] = heading.expect("script Jump heading");
        }
        if self.updates.animation_system {
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .expect("validated jump animation receiver");
            let mut emit = |sound| {
                if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                    return event_context.dispatch(flags, event_camera.as_mut(), event_effects);
                }
                sounds(
                    id,
                    super::super::ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    },
                )
            };
            let result = if force {
                animation.playback.force_request_with_sounds(
                    request,
                    0,
                    environment.ticks,
                    &mut emit,
                )
            } else {
                animation
                    .playback
                    .request_with_sounds(request, 0, environment.ticks, &mut emit)
            };
            match result {
                Ok(frame) => animation.frame = frame.or(animation.frame),
                Err(error) => {
                    let message = format!(
                        "world {} instance {index} jump request: {error:#}",
                        self.world
                    );
                    actor.animation_failure = Some(message.clone());
                    anyhow::bail!(message);
                }
            }
        }
        actor
            .control
            .as_mut()
            .expect("validated jump control")
            .motion_classification = MotionDirection::FORWARD;
        actor
            .control
            .as_mut()
            .expect("validated jump control")
            .motion_direction = MotionDirection::FORWARD;
        Ok(true)
    }

    /// Run the ordinary queued handler against the actor's retained candidate list.
    /// Returns whether it handled the response, including clearance cancellation.
    /// Selected-player edge handling precedes queued vertical continuation.
    /// Fine timing is supplied by its clock owner. Requests share actor playback
    /// and captured audio state.
    pub fn resolve_ordinary_queued_motion(
        &mut self,
        id: super::ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        fine_ticks: u32,
        additional_ticks: u32,
        sounds: &mut dyn FnMut(
            super::ScriptActorId,
            super::super::ScriptSound,
        ) -> anyhow::Result<()>,
    ) -> anyhow::Result<bool> {
        use anyhow::Context;
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let actor = &self.actors[id.index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary queued handler control unavailable")?;
        if control.queued_motion == JumpMotion::NONE {
            return Ok(false);
        }
        let animation = actor
            .animation
            .as_ref()
            .context("ordinary queued handler animation receiver unavailable")?;
        let (holding, _, _) = super::super::activation::weapon_predicates(
            control.living_state,
            Some(super::super::ActivationAnimation {
                group: animation.playback.current_group(),
            }),
            control.combat_death_default.is_some(),
            control.weapon_drawn,
            actor.attributes[Attribute::HoldingWeapon],
            environment.item_system_ready,
            environment.shell_execution,
        );
        let action = ordinary_queued_action(OrdinaryQueuedInputs {
            queued: control.queued_motion,
            torch_burning: control.torch_burning,
            current_group: animation.playback.current_group(),
            pending_group: animation.playback.pending_group(),
            has_group_49: animation.playback.has_group(AnimationGroup::JUMP_BACKWARD),
            has_group_50: animation
                .playback
                .has_group(AnimationGroup::JUMP_BACKWARD_TORCH),
            holding_weapon: holding,
            edge_handled: false,
        });
        if action.clear_queued {
            self.actors[id.index]
                .control
                .as_mut()
                .expect("validated queued control")
                .queued_motion = JumpMotion::NONE;
        }
        if !action.advance_vertical {
            return Ok(action.handled);
        }
        // A successful selected-player edge service handles the update and clears queued motion.
        if environment.selected_player == Some(id) {
            let timing = super::ledge_states::LedgeTiming {
                fine_ticks,
                frame_scale: None,
            };
            if self.selected_edge_service(id, query, environment, timing, sounds)? {
                let control = self.actors[id.index]
                    .control
                    .as_mut()
                    .expect("validated queued control");
                control.queued_motion = JumpMotion::NONE;
                return Ok(true);
            }
        }
        if let Some(group) = action
            .request_group
            .filter(|_| self.updates.animation_system)
        {
            let position = super::animation_sound_position(self.spatial.as_ref(), id.index);
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let actor = &mut self.actors[id.index];
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .expect("validated queued animation receiver");
            let result =
                animation
                    .playback
                    .request_with_sounds(group, 0, environment.ticks, &mut |sound| {
                        if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                            return event_context.dispatch(
                                flags,
                                event_camera.as_mut(),
                                event_effects,
                            );
                        }
                        sounds(
                            id,
                            super::super::ScriptSound::Animation {
                                sound,
                                surface,
                                position,
                            },
                        )
                    });
            match result {
                Ok(frame) => animation.frame = frame.or(animation.frame),
                Err(error) => {
                    let message = format!(
                        "world {} instance {} queued request: {error:#}",
                        self.world, id.index
                    );
                    actor.animation_failure = Some(message.clone());
                    anyhow::bail!(message);
                }
            }
        }
        let actor = &self.actors[id.index];
        let timer = actor
            .control
            .as_ref()
            .expect("validated queued control")
            .fall_timer;
        let suppressed = actor
            .animation
            .as_ref()
            .is_some_and(|animation| animation.playback.translation_suppressed());
        let vertical = super::vertical::OrdinaryVerticalInputs::for_queued(
            timer,
            fine_ticks,
            additional_ticks,
            query.scene.settings.post_collide_height,
            suppressed,
        )
        .with_context(|| format!("world {} instance {} queued factors", self.world, id.index))?;
        self.apply_ordinary_queued_vertical(
            id,
            vertical,
            query.animations,
            environment.ticks,
            sounds,
        )?;
        if self.actors[id.index]
            .control
            .as_ref()
            .expect("validated queued control")
            .queued_motion
            != JumpMotion::NONE
        {
            let clearance = self.query_ordinary_queued_clearance(id, query)?;
            let threshold = self.ordinary_collision_shape(id)?.positive_extent[1]
                .wrapping_sub(4)
                .wrapping_mul(256);
            if clearance < threshold {
                let previous = self.spatial(id)?.previous_local_position;
                self.actors[id.index]
                    .control
                    .as_mut()
                    .expect("validated queued control")
                    .queued_motion = JumpMotion::NONE;
                self.spatial
                    .as_mut()
                    .context("ordinary queued rollback spatial unavailable")?
                    .actor_mut(id.index)?
                    .local
                    .position = previous;
            }
        }
        Ok(true)
    }
}

impl super::ActorControlState {
    fn advance_action_words(&mut self, ticks: u32, group: Option<AnimationGroup>) -> Option<u8> {
        self.jump_window = self.jump_window.wrapping_add(ticks as u8);
        if self.jump_window > 36 {
            self.actions.accepted |= self.actions.pending;
            self.actions.pending = 0;
            self.jump_window = 0;
        }
        if self.queued_motion != JumpMotion::NONE
            || !matches!(group, Some(AnimationGroup::RUN | AnimationGroup::RUN_TORCH))
        {
            self.actions
                .iter_mut()
                .for_each(|word| *word &= !ActionWords::JUMPS);
        }
        let motion = if self.actions.accepted & ActionWords::JUMP_RIGHT != 0 {
            8
        } else if self.actions.accepted & ActionWords::JUMP_LEFT != 0 {
            9
        } else {
            return None;
        };
        self.actions
            .iter_mut()
            .for_each(|word| *word &= !ActionWords::JUMPS);
        Some(motion)
    }
}

#[cfg(test)]
mod action_word_tests {
    use super::super::ActorControlState;
    use cyrus_soup::AnimationGroup;

    #[test]
    fn action_promotion_uses_wrapping_byte_and_strict_threshold() {
        let mut control = ActorControlState {
            actions: [2147483907, 1073741824].into(),
            jump_window: 35,
            ..Default::default()
        };
        assert_eq!(
            control.advance_action_words(1, Some(AnimationGroup::RUN)),
            None
        );
        assert_eq!(<[u32; 2]>::from(control.actions), [2147483907, 1073741824]);
        assert_eq!(
            control.advance_action_words(1, Some(AnimationGroup::RUN)),
            Some(8)
        );
        assert_eq!(<[u32; 2]>::from(control.actions), [0, 3221225728]);
        assert_eq!(control.jump_window, 0);
        control.jump_window = 255;
        control.actions = [0x102, 0].into();
        assert_eq!(
            control.advance_action_words(1, Some(AnimationGroup::RUN_TORCH)),
            None
        );
        assert_eq!(control.jump_window, 0);
        assert_eq!(<[u32; 2]>::from(control.actions), [0x102, 0]);
        assert_eq!(
            control.advance_action_words(37, Some(AnimationGroup::RUN_TORCH)),
            Some(9)
        );
        assert_eq!(<[u32; 2]>::from(control.actions), [0, 0x100]);
    }

    #[test]
    fn queued_or_other_group_clears_only_low_bits_after_promotion() {
        for (queued, group) in [
            (crate::script::JumpMotion::STANDING, AnimationGroup::RUN),
            (
                crate::script::JumpMotion::NONE,
                AnimationGroup::RUN_BACKWARD,
            ),
        ] {
            let mut control = ActorControlState {
                actions: [0x123, 0x202].into(),
                queued_motion: queued,
                jump_window: 36,
                ..Default::default()
            };
            assert_eq!(control.advance_action_words(1, Some(group)), None);
            assert_eq!(<[u32; 2]>::from(control.actions), [0, 0x320]);
        }
    }
}
