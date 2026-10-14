//! Queued motion admission precedes its request and vertical writer.

/// Current receiver state and established edge-service outcome.
#[derive(Clone, Copy, Debug)]
pub struct OrdinaryQueuedInputs {
    pub queued: u8,
    pub torch_burning: u8,
    pub current_group: Option<u16>,
    pub pending_group: Option<u16>,
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
    pub request_group: Option<u16>,
}

/// Resolve the original low/high-byte state and pending-group arbitration.
/// Edge admission must be established by its owner, not inferred here.
pub fn ordinary_queued_action(input: OrdinaryQueuedInputs) -> OrdinaryQueuedAction {
    let mut action = OrdinaryQueuedAction::default();
    if input.queued == 0 {
        return action;
    }
    let low = input.queued & 127;
    let high = input.queued & 128 != 0;
    let torch = input.torch_burning != 0;
    if low == 3 {
        let available = if torch {
            input.has_group_50
        } else {
            input.has_group_49
        };
        if !available {
            action.clear_queued = true;
            return action;
        }
        if input.current_group != Some(if torch { 50 } else { 49 }) {
            action.handled = torch;
            return action;
        }
    }
    if input
        .pending_group
        .is_some_and(|group| (32..=37).contains(&group) || group == 111)
    {
        return action;
    }
    if input.edge_handled {
        action.clear_queued = true;
        action.handled = true;
        return action;
    }
    action.request_group = match low {
        1 => Some(if torch {
            38
        } else if high {
            40
        } else if input.holding_weapon {
            112
        } else {
            39
        }),
        2 => Some(if torch {
            41
        } else if high {
            43
        } else {
            42
        }),
        3 => Some(if torch { 50 } else { 49 }),
        8 | 24 => Some(if torch {
            45
        } else if high {
            48
        } else {
            44
        }),
        9 => Some(if torch {
            47
        } else if high {
            48
        } else {
            46
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
    /// from script continuations. Selected-player and state-15 services remain
    /// unsupported. Returns whether ordinary queued state was initialized.
    pub fn begin_ordinary_jump(
        &mut self,
        id: super::ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        requested_motion: u8,
        native_task_active: bool,
        environment: super::transition::ObstacleEnvironment,
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
            || control.queued_motion != 0
            || control.falling_state != 0
            || control.sliding_state != 0
            || !matches!(control.living_state, 0 | 15)
        {
            return Ok(false);
        }
        anyhow::ensure!(
            environment.selected_player != Some(id),
            "world {} instance {index} jump selected-player edge service unsupported",
            self.world
        );
        anyhow::ensure!(
            control.living_state != 15,
            "world {} instance {index} jump state-15 exit service unsupported",
            self.world
        );
        let group = actor
            .animation
            .as_ref()
            .context("ordinary jump animation receiver unavailable")?
            .playback
            .current_group()
            .context("ordinary jump current group unavailable")?;
        let force = control.jump_selector != 0;
        let (queued, request) = if force {
            (24, 85)
        } else {
            match group {
                18 => {
                    if requested_motion == 9 {
                        (9, 36)
                    } else {
                        (8, 34)
                    }
                }
                30 => {
                    if requested_motion == 9 {
                        (9, 37)
                    } else {
                        (8, 35)
                    }
                }
                _ => (1, if control.torch_burning == 0 { 32 } else { 33 }),
            }
        };
        let origin = self.spatial(id)?.local.position[1];
        let position = super::animation_sound_position(self.spatial.as_ref(), index);
        let actor = &mut self.actors[index];
        let control = actor.control.as_mut().expect("validated jump control");
        control.stable_position_valid = false;
        control.fall_origin_y = origin;
        control.fall_timer = 0;
        control.sliding_distance = 0;
        control.queued_motion = queued;
        if self.updates.animation_system {
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .expect("validated jump animation receiver");
            let mut emit = |sound| {
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
            .motion_direction = 2;
        Ok(true)
    }

    /// Run the ordinary queued handler against the actor's retained candidate list.
    /// Returns whether it handled the response, including clearance cancellation.
    /// Selected-player edge services remain unsupported; fine timing is supplied
    /// by its clock owner. Requests share actor playback and captured audio state.
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
        if control.queued_motion == 0 {
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
            actor.attributes[110],
            environment.item_system_ready,
            environment.shell_execution,
        );
        let action = ordinary_queued_action(OrdinaryQueuedInputs {
            queued: control.queued_motion,
            torch_burning: control.torch_burning,
            current_group: animation.playback.current_group(),
            pending_group: animation.playback.pending_group(),
            has_group_49: animation.playback.has_group(49),
            has_group_50: animation.playback.has_group(50),
            holding_weapon: holding,
            edge_handled: false,
        });
        if action.clear_queued {
            self.actors[id.index]
                .control
                .as_mut()
                .expect("validated queued control")
                .queued_motion = 0;
        }
        if !action.advance_vertical {
            return Ok(action.handled);
        }
        anyhow::ensure!(
            environment.selected_player != Some(id),
            "world {} instance {} queued selected-player edge service unsupported",
            self.world,
            id.index
        );
        if let Some(group) = action
            .request_group
            .filter(|_| self.updates.animation_system)
        {
            let position = super::animation_sound_position(self.spatial.as_ref(), id.index);
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
            != 0
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
                    .queued_motion = 0;
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
