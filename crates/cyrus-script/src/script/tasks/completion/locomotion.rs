//! Grounded directional locomotion task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_locomotion(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        Ok(match self {
            Self::Backward | Self::WalkBackward => {
                use crate::script::locomotion::{
                    BackwardAdmission, backward_admission, backward_translation, continues,
                    walk_backward_admitted,
                };
                let [duration] = arguments else {
                    anyhow::bail!("MoveBackward requires one argument")
                };
                let duration = duration.integer()?;
                let elapsed = invocation
                    .elapsed
                    .context("MoveBackward requires elapsed input")?;
                let motion = services
                    .locomotion
                    .context("MoveBackward locomotion state unavailable")?;
                let state = motion.state.map_err(anyhow::Error::msg)?;
                let walking =
                    matches!(self, Self::WalkBackward) && motion.admission.walk_backward_mode;
                let mut receiver = motion.motion;
                let (group, step) = if walking {
                    if !walk_backward_admitted(&state)
                        || !motion
                            .admission
                            .backward_step_clear
                            .context("WalkBackward step check input unavailable")?
                    {
                        return Ok(true);
                    }
                    let receiver = receiver
                        .as_mut()
                        .context("WalkBackward motion state unavailable")?;
                    *receiver.classification = MotionDirection::WALK_BACKWARD;
                    *receiver.direction = MotionDirection::WALK_BACKWARD;
                    if state.holding_weapon || state.combat_mode {
                        (
                            AnimationGroup::FIGHT_BACK,
                            state.walk_backward_combat_step_attribute,
                        )
                    } else {
                        (
                            if state.torch_burning {
                                AnimationGroup::WALK_BACKWARD_TORCH
                            } else {
                                AnimationGroup::WALK_BACKWARD
                            },
                            state.walk_backward_step_attribute,
                        )
                    }
                } else {
                    let mut probe_needed = false;
                    let admission = backward_admission(state, || {
                        probe_needed = true;
                        motion.admission.blocked_ahead_30.unwrap_or(false)
                    });
                    if admission != BackwardAdmission::Refused {
                        *receiver
                            .as_mut()
                            .context("MoveBackward motion state unavailable")?
                            .classification = MotionDirection::WALK_BACKWARD;
                    }
                    anyhow::ensure!(
                        !(probe_needed && motion.admission.blocked_ahead_30.is_none()),
                        "MoveBackward blocked-ahead probe input unavailable"
                    );
                    if admission == BackwardAdmission::Refused {
                        return Ok(true);
                    }
                    *receiver
                        .as_mut()
                        .context("MoveBackward motion state unavailable")?
                        .direction = MotionDirection::WALK_BACKWARD;
                    let BackwardAdmission::Proceed { group, step } = admission else {
                        return Ok(true);
                    };
                    (group, step)
                };
                let receiver = receiver.context("MoveBackward motion state unavailable")?;
                let input = animation.as_mut().context(
                    "MoveBackward requires prepared animation and explicit nominal ticks",
                )?;
                let spatial = spatial
                    .as_mut()
                    .context("MoveBackward requires receiver pose and frame_scale")?;
                if input.animation.has_group(group) {
                    let frame = match &mut input.sounds {
                        Some(sounds) => {
                            input
                                .animation
                                .request_with_sounds(group, 0, input.ticks, *sounds)?
                        }
                        None => input.animation.request(group, 0, input.ticks)?,
                    };
                    *input.frame = frame.or(*input.frame);
                    if motion.animation_system && input.animation.current_group() != Some(group) {
                        return Ok(!continues(duration, elapsed));
                    }
                    *motion.called |= 8;
                    // WalkBackward with a zero step length instead moves half of `attr_backward_speed`, unscaled.
                    let step_distance = if walking && step == 0 {
                        i32::from(state.step_attribute >> 1)
                    } else {
                        i32::from(step) << 8
                    };
                    let distance = crate::script::locomotion::initial_step_distance(
                        receiver.counters,
                        6,
                        state.step_adjustment,
                        step_distance,
                    );
                    if let Some((heading, distance)) = backward_translation(
                        (spatial.angles[1] & 2047) as u16,
                        distance,
                        spatial.frame_scale as i32,
                        input.animation.translation_suppressed(),
                    ) {
                        *spatial.position = cyrus_world::transform::translate_forward(
                            *spatial.position,
                            heading,
                            distance,
                        )?;
                    }
                }
                !continues(duration, elapsed)
            }
            Self::Forward | Self::WalkForward | Self::Lunge => {
                let walking = matches!(self, Self::WalkForward);
                let lunge = matches!(self, Self::Lunge);
                let name = if walking {
                    "WalkForward"
                } else if lunge {
                    "Lunge"
                } else {
                    "MoveForward"
                };
                use crate::script::locomotion::{
                    ForwardPlan, continues, forward_admitted, forward_plan, forward_translation,
                };
                let [duration] = arguments else {
                    anyhow::bail!("{name} requires one argument")
                };
                let duration = duration.integer()?;
                let elapsed = invocation
                    .elapsed
                    .with_context(|| format!("{name} requires elapsed input"))?;
                let motion = services
                    .locomotion
                    .with_context(|| format!("{name} locomotion state unavailable"))?;
                let state = motion.state.map_err(anyhow::Error::msg)?;
                // The lunge also needs the idle lifecycle state.
                if !motion
                    .admission
                    .forward_admitted
                    .unwrap_or_else(|| forward_admitted(&state))
                    || (lunge && state.living_state != LivingState::ORDINARY)
                {
                    return Ok(true);
                }
                let receiver = motion
                    .motion
                    .with_context(|| format!("{name} motion state unavailable"))?;
                let walking = walking && motion.admission.walk_forward_mode;
                if (walking || (*receiver.counters >> 4) & 3 < 3)
                    && !motion
                        .admission
                        .forward_step_clear
                        .with_context(|| format!("{name} forward step check input unavailable"))?
                {
                    return Ok(true);
                }
                let direction = if walking {
                    MotionDirection::WALK_FORWARD
                } else {
                    MotionDirection::FORWARD
                };
                *receiver.classification = direction;
                *receiver.direction = direction;
                let mut halved = false;
                let (group, step) =
                    if lunge {
                        (None, state.lunge_step_attribute)
                    } else if walking {
                        if state.holding_weapon || state.combat_mode {
                            (
                                Some(AnimationGroup::FIGHT_RUN),
                                state.walk_combat_step_attribute,
                            )
                        } else {
                            (
                                Some(if state.torch_burning {
                                    AnimationGroup::WALK_FORWARD_TORCH
                                } else {
                                    AnimationGroup::WALK
                                }),
                                state.walk_step_attribute,
                            )
                        }
                    } else {
                        match forward_plan(&state) {
                            ForwardPlan::Done => return Ok(true),
                            ForwardPlan::NeedsEdgeProbe => {
                                if !motion.admission.edge_probe_clear.with_context(|| {
                                    format!("{name} edge probe input unavailable")
                                })? {
                                    return Ok(true);
                                }
                                halved = true;
                                (
                                    Some(AnimationGroup::SWIM_FORWARD),
                                    state.forward_step_attribute,
                                )
                            }
                            ForwardPlan::Proceed { group, step } => (Some(group), step),
                        }
                    };
                let input = animation.as_mut().with_context(|| {
                    format!("{name} requires prepared animation and explicit nominal ticks")
                })?;
                let spatial = spatial
                    .as_mut()
                    .with_context(|| format!("{name} requires receiver pose and frame_scale"))?;
                let moves = match group {
                    None => true,
                    Some(group) if input.animation.has_group(group) => {
                        let frame = match &mut input.sounds {
                            Some(sounds) => input.animation.request_with_sounds(
                                group,
                                0,
                                input.ticks,
                                *sounds,
                            )?,
                            None => input.animation.request(group, 0, input.ticks)?,
                        };
                        *input.frame = frame.or(*input.frame);
                        if motion.animation_system && input.animation.current_group() != Some(group)
                        {
                            return Ok(!continues(duration, elapsed));
                        }
                        true
                    }
                    Some(_) => false,
                };
                if moves {
                    *motion.called |= 4;
                    let distance = crate::script::locomotion::initial_step_distance(
                        receiver.counters,
                        4,
                        state.step_adjustment,
                        if walking && step == 0 {
                            i32::from(state.forward_step_attribute / 2)
                        } else if halved {
                            (i32::from(step) << 8) >> 1
                        } else {
                            i32::from(step) << 8
                        },
                    );
                    if let Some((heading, distance)) = forward_translation(
                        (spatial.angles[1] & 2047) as u16,
                        distance,
                        spatial.frame_scale as i32,
                        input.animation.translation_suppressed(),
                    ) {
                        *spatial.position = cyrus_world::transform::translate_forward(
                            *spatial.position,
                            heading,
                            distance,
                        )?;
                    }
                }
                !continues(duration, elapsed)
            }
            Self::Strafe(strafe) => {
                use crate::script::locomotion::{
                    StrafeAdmission, continues, strafe_admission, strafe_motion,
                };
                let name = match strafe {
                    crate::script::locomotion::Strafe::Left => Native::MoveLeft,
                    crate::script::locomotion::Strafe::Right => Native::MoveRight,
                };
                let [duration] = arguments else {
                    anyhow::bail!("{name} requires one argument")
                };
                let duration = duration.integer()?;
                let elapsed = invocation
                    .elapsed
                    .with_context(|| format!("{name} requires elapsed input"))?;
                let motion = services
                    .locomotion
                    .with_context(|| format!("{name} locomotion state unavailable"))?;
                let state = motion.state.map_err(anyhow::Error::msg)?;
                let admission = strafe_admission(state, *strafe);
                if admission == StrafeAdmission::Refused {
                    return Ok(true);
                }
                let receiver = motion
                    .motion
                    .with_context(|| format!("{name} motion state unavailable"))?;
                *receiver.classification = strafe.motion_direction();
                *receiver.direction = strafe.motion_direction();
                let input = animation.as_mut().with_context(|| {
                    format!("{name} requires prepared animation and explicit nominal ticks")
                })?;
                let spatial = spatial
                    .as_mut()
                    .with_context(|| format!("{name} requires receiver pose and frame_scale"))?;
                let (group, step, turn) = match admission {
                    StrafeAdmission::Combat { group, step, turn } => (group, step, turn),
                    StrafeAdmission::OutsideCombat { .. }
                    | StrafeAdmission::DeepWater { .. }
                    | StrafeAdmission::Ledge { .. } => {
                        let group = match admission {
                            StrafeAdmission::OutsideCombat { group }
                            | StrafeAdmission::DeepWater { group }
                            | StrafeAdmission::Ledge { group } => Some(group),
                            _ => None,
                        };
                        if !motion.admission.lateral_step_clear.with_context(|| {
                            format!("{name} lateral step check input unavailable")
                        })? {
                            return Ok(true);
                        }
                        if matches!(admission, StrafeAdmission::DeepWater { .. })
                            && !motion
                                .admission
                                .edge_probe_clear
                                .with_context(|| format!("{name} edge probe input unavailable"))?
                        {
                            return Ok(true);
                        }
                        if let Some(group) = group.filter(|group| input.animation.has_group(*group))
                        {
                            let frame = match &mut input.sounds {
                                Some(sounds) => input.animation.request_with_sounds(
                                    group,
                                    0,
                                    input.ticks,
                                    *sounds,
                                )?,
                                None => input.animation.request(group, 0, input.ticks)?,
                            };
                            *input.frame = frame.or(*input.frame);
                        }
                        if motion.animation_system
                            && group
                                .is_some_and(|group| input.animation.current_group() != Some(group))
                        {
                            return Ok(!continues(duration, elapsed));
                        }
                        *motion.called |= strafe.called_flag();
                        let distance = crate::script::locomotion::initial_step_distance(
                            receiver.counters,
                            strafe.counter_shift(),
                            state.step_adjustment,
                            i32::from(state.lateral_step_attribute) << 8,
                        );
                        if let Some((heading, distance)) =
                            crate::script::locomotion::strafe_translation(
                                *strafe,
                                (spatial.angles[1] & 2047) as u16,
                                distance,
                                spatial.frame_scale as i32,
                                input.animation.translation_suppressed(),
                            )
                        {
                            *spatial.position = cyrus_world::transform::translate_forward(
                                *spatial.position,
                                heading,
                                distance,
                            )?;
                        }
                        return Ok(!continues(duration, elapsed));
                    }
                    StrafeAdmission::Refused => unreachable!("refusal returned above"),
                };
                let already_strafing = input.animation.current_group().is_some_and(|current| {
                    crate::script::locomotion::Strafe::COMBAT_GROUPS.contains(&current)
                });
                if already_strafing || !input.animation.has_group(group) {
                    return Ok(true);
                }
                let frame = match &mut input.sounds {
                    Some(sounds) => {
                        input
                            .animation
                            .request_with_sounds(group, 0, input.ticks, *sounds)?
                    }
                    None => input.animation.request(group, 0, input.ticks)?,
                };
                *input.frame = frame.or(*input.frame);
                *motion.called |= strafe.called_flag();
                let distance = crate::script::locomotion::initial_step_distance(
                    receiver.counters,
                    strafe.counter_shift(),
                    state.step_adjustment,
                    i32::from(step) << 8,
                );
                let (angle, movement) = strafe_motion(
                    *strafe,
                    (spatial.angles[1] & 2047) as u16,
                    turn,
                    distance,
                    spatial.frame_scale as i32,
                    input.animation.translation_suppressed(),
                );
                spatial.angles[1] = i32::from(angle);
                if let Some((heading, distance)) = movement {
                    *spatial.position = cyrus_world::transform::translate_forward(
                        *spatial.position,
                        heading,
                        distance,
                    )?;
                }
                !continues(duration, elapsed)
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
