//! Actor-owned native task tokens share waits and animation requests.
use anyhow::{Context, Result};
use cyrus_script::program::CallKind;
use cyrus_script::{Argument, CallResult, Invocation};
use cyrus_world::animation::{ActorAnimation, AnimationSound};
use std::collections::BTreeMap;
mod persistence;
pub use persistence::{NativeTaskBindings, NativeTasksSnapshot};

/// One explicit invocation's animation and synchronous sound destination.
pub(super) struct AnimationCall<'a, 'sound> {
    pub animation: &'a mut ActorAnimation,
    pub ticks: u32,
    pub frame: &'a mut Option<u32>,
    pub sounds: Option<&'sound mut dyn FnMut(AnimationSound) -> Result<()>>,
}

/// Current receiver pose and shared timing inputs for spatial native tasks.
pub(super) struct SpatialCall<'a> {
    pub position: &'a mut [i32; 3],
    pub angles: &'a mut [u16; 3],
    pub frame_scale: u32,
    pub nominal_ticks: u32,
    pub matrix: cyrus_world::transform::Matrix,
    pub slave: bool,
    pub locations: &'a [[i32; 3]],
    pub effective_position: Option<[i32; 3]>,
    /// Selected player's local position, when the caller supplies the player query.
    pub player_position: Option<[i32; 3]>,
    pub actor_index: usize,
    pub map_updates: &'a mut Vec<usize>,
}

/// Explicit playback service and receiver state for one native ambient invocation.
pub(super) struct AmbientCall<'a> {
    pub driver: Option<&'a dyn cyrus_world::sound::AmbientSoundDriver>,
    pub reset: Option<bool>,
    pub position: Option<[i32; 3]>,
    pub excluded: Option<bool>,
    pub generation: u64,
    pub actor_index: usize,
}

/// Shared services sampled for each native task invocation.
#[derive(Default)]
pub(super) struct NativeServices<'a> {
    pub ambient: Option<&'a AmbientCall<'a>>,
    pub frame_wait: Option<FrameWaitCall>,
    pub dialogue: Option<&'a mut super::DialogueState>,
    pub player_dialogue: Option<PlayerDialogueCall<'a>>,
    pub locomotion: Option<LocomotionCall<'a>>,
}

/// Receiver state sampled for locomotion tasks. Fallible reads surface only when used.
pub(super) struct LocomotionCall<'a> {
    pub state: Result<super::locomotion::BackwardState, String>,
    pub admission: LocomotionAdmission,
    pub animation_system: bool,
    pub motion: Option<MotionCall<'a>>,
    pub called: &'a mut u8,
}

/// Admission sampled before collision writes; animation and movement state is read afterwards.
#[derive(Default)]
pub(super) struct LocomotionAdmission {
    pub blocked_ahead_30: Option<bool>,
    pub forward_step_clear: Option<bool>,
    pub walk_forward_mode: bool,
    pub forward_admitted: Option<bool>,
}

pub(super) struct MotionCall<'a> {
    pub direction: &'a mut u8,
    pub counters: &'a mut u8,
}

/// Read-only animation state, sampled independently of playback tick availability.
pub(super) struct FrameWaitCall {
    pub receiver: Result<(i32, i32, u8), String>,
    pub player: Result<Option<(i32, i32, u8)>, String>,
}

pub(super) struct PlayerDialogueCall<'a> {
    pub source: Option<&'a dyn super::DialogueSource>,
    pub player: Option<super::ScriptActorId>,
    pub reset: Option<bool>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
enum Task {
    Dialogue,
    AnimationFrame { player: bool },
    PlayerDialogue { started: bool },
    Ambient { channel: Option<u8> },
    Movement { target: i32 },
    ObjectMovement { target: i32, traveled: i32 },
    ScaledMovement { origin: i32 },
    Rotation { target: i32, accumulator: i32 },
    AbsoluteRotation { movement: i32, accumulator: i32 },
    ScaledRotation { origin: i32 },
    ScaledRelativeRotation { origin: i32 },
    FacePlayer,
    Backward,
    Strafe(super::locomotion::Strafe),
    Forward,
    WalkForward,
    Location,
    Duration,
    Tasks,
    Blocking,
    Animation { group: u16, started: bool },
}
impl Task {
    fn complete(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        let ambient = services.ambient;
        Ok(match self {
            Self::PlayerDialogue { started } => {
                let [label] = arguments else {
                    anyhow::bail!("RTXp requires one argument");
                };
                let input = services
                    .player_dialogue
                    .context("RTXp service inputs unavailable")?;
                if input.reset.context("RTXp sample-reset input unavailable")? {
                    return Ok(true);
                }
                invocation.elapsed.context("RTXp requires elapsed input")?;
                let dialogue = services
                    .dialogue
                    .context("RTXp dialogue records unavailable")?;
                if !*started {
                    let player = input.player.context("RTXp selected player unavailable")?;
                    let source = input.source.context("RTXp source service unavailable")?;
                    let std::task::Poll::Ready(line) = source.player_line(
                        label.integer()?.to_le_bytes(),
                        player,
                        dialogue.settings().max_text_width,
                    )?
                    else {
                        return Ok(false);
                    };
                    *started = true;
                    if let Some(line) = line {
                        anyhow::ensure!(
                            line.sample.is_none_or(|sample| sample < 16),
                            "RTXp source returned invalid sample slot"
                        );
                        dialogue.begin_player_line(player, line.text, line.sample, line.lifetime);
                    }
                }
                !dialogue.line_active(Some(0))
            }
            Self::AnimationFrame { player } => {
                let [group, threshold] = arguments else {
                    anyhow::bail!("animation frame wait requires two arguments");
                };
                let (group, threshold) = (group.integer()?, threshold.integer()?);
                let input = services
                    .frame_wait
                    .context("animation frame wait inputs unavailable")?;
                let state = if *player {
                    input.player.map_err(anyhow::Error::msg)?
                } else {
                    Some(input.receiver.map_err(anyhow::Error::msg)?)
                };
                match state {
                    None => true,
                    Some((current, pending, progress)) => {
                        if current == group {
                            i32::from(progress) >= threshold
                        } else {
                            pending != group
                        }
                    }
                }
            }
            Self::Dialogue => {
                anyhow::ensure!(arguments.is_empty(), "WaitOnDialog requires no arguments");
                !services
                    .dialogue
                    .context("WaitOnDialog records unavailable")?
                    .waiting()
            }
            Self::Ambient { channel } => {
                let [effect, volume, rate] = arguments else {
                    anyhow::bail!("AmbientSound requires three arguments")
                };
                let elapsed = invocation
                    .elapsed
                    .context("AmbientSound requires elapsed input")?;
                let input = ambient.context("AmbientSound playback service unavailable")?;
                let reset = input
                    .reset
                    .context("AmbientSound sample-reset input unavailable")?;
                let driver = input
                    .driver
                    .context("AmbientSound playback service unavailable")?;
                let position = input
                    .position
                    .context("AmbientSound receiver pose unavailable")?;
                let excluded = input
                    .excluded
                    .context("AmbientSound receiver exclusion state unavailable")?;
                if elapsed == 0 || reset {
                    *channel = None;
                }
                let updated = driver
                    .update(cyrus_world::sound::AmbientSoundRequest {
                        generation: input.generation,
                        actor_index: input.actor_index,
                        channel: *channel,
                        effect: effect.integer()?,
                        volume: volume.integer()?,
                        rate_offset_hz: rate.integer()?,
                        position,
                        object_update_excluded: excluded,
                    })
                    .map_err(anyhow::Error::msg)?;
                anyhow::ensure!(
                    updated.channel.is_none_or(|slot| slot < 16),
                    "AmbientSound playback returned an invalid sample slot"
                );
                *channel = updated.channel;
                !updated.active
            }
            Self::FacePlayer => {
                let [duration] = arguments else {
                    anyhow::bail!("FacePlayer requires one argument")
                };
                let duration = duration.integer()?;
                let elapsed = invocation
                    .elapsed
                    .context("FacePlayer requires elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("FacePlayer requires receiver pose")?;
                let player = input
                    .player_position
                    .context("FacePlayer selected-player position unavailable")?;
                let bearing = i32::from(super::spatial::query_bearing(
                    player[0].wrapping_sub(input.position[0]),
                    player[2].wrapping_sub(input.position[2]),
                ));
                let heading = i32::from(input.angles[1]);
                let mut delta = heading.wrapping_neg().wrapping_sub(bearing) & 0x7ff;
                if delta == 0 {
                    return Ok(true);
                }
                let remaining = elapsed.wrapping_sub(duration).wrapping_abs().max(1);
                let backwards = delta > 0x400;
                if backwards {
                    delta = (-delta - 0x400) & 0x3ff;
                }
                let step = ((delta << 8) / remaining) >> 8;
                if step == 0 {
                    return Ok(true);
                }
                let heading = if backwards {
                    heading - step
                } else {
                    heading + step
                };
                input.angles[1] = (heading & 0x7ff) as u16;
                elapsed >= duration.wrapping_sub(1)
            }
            Self::Backward => {
                use super::locomotion::{
                    BACKWARD_MOTION_DIRECTION, BackwardAdmission, backward_admission,
                    backward_translation, continues,
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
                let mut probe_needed = false;
                let admission = backward_admission(state, || {
                    probe_needed = true;
                    motion.admission.blocked_ahead_30.unwrap_or(false)
                });
                anyhow::ensure!(
                    !(probe_needed && motion.admission.blocked_ahead_30.is_none()),
                    "MoveBackward blocked-ahead probe input unavailable"
                );
                if admission == BackwardAdmission::Refused {
                    return Ok(true);
                }
                let receiver = motion
                    .motion
                    .context("MoveBackward motion state unavailable")?;
                *receiver.direction = BACKWARD_MOTION_DIRECTION;
                let BackwardAdmission::Proceed { group, step } = admission else {
                    return Ok(true);
                };
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
                    let distance = super::locomotion::initial_step_distance(
                        receiver.counters,
                        6,
                        state.step_adjustment,
                        i32::from(step) << 8,
                    );
                    if let Some((heading, distance)) = backward_translation(
                        spatial.angles[1],
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
            Self::Forward | Self::WalkForward => {
                let walking = matches!(self, Self::WalkForward);
                let name = if walking {
                    "WalkForward"
                } else {
                    "MoveForward"
                };
                use super::locomotion::{
                    FORWARD_MOTION_DIRECTION, ForwardPlan, continues, forward_admitted,
                    forward_plan, forward_translation,
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
                if !motion
                    .admission
                    .forward_admitted
                    .unwrap_or_else(|| forward_admitted(&state))
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
                *receiver.direction = if walking { 6 } else { FORWARD_MOTION_DIRECTION };
                let (group, step) = if walking {
                    if state.holding_weapon || state.combat_mode {
                        (31, state.walk_combat_step_attribute)
                    } else {
                        (
                            if state.torch_burning { 21 } else { 20 },
                            state.walk_step_attribute,
                        )
                    }
                } else {
                    match forward_plan(&state) {
                        ForwardPlan::Done => return Ok(true),
                        ForwardPlan::NeedsEdgeProbe => anyhow::bail!(
                            "MoveForward in deep water unsupported: edge probe input unavailable"
                        ),
                        ForwardPlan::Proceed { group, step } => (group, step),
                    }
                };
                let input = animation.as_mut().with_context(|| {
                    format!("{name} requires prepared animation and explicit nominal ticks")
                })?;
                let spatial = spatial
                    .as_mut()
                    .with_context(|| format!("{name} requires receiver pose and frame_scale"))?;
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
                    *motion.called |= 4;
                    let distance = super::locomotion::initial_step_distance(
                        receiver.counters,
                        4,
                        state.step_adjustment,
                        if walking && step == 0 {
                            i32::from(state.forward_step_attribute / 2)
                        } else {
                            i32::from(step) << 8
                        },
                    );
                    if let Some((heading, distance)) = forward_translation(
                        spatial.angles[1],
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
                use super::locomotion::{
                    StrafeAdmission, continues, strafe_admission, strafe_motion,
                };
                let name = match strafe {
                    super::locomotion::Strafe::Left => "MoveLeft",
                    super::locomotion::Strafe::Right => "MoveRight",
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
                *motion
                    .motion
                    .with_context(|| format!("{name} motion state unavailable"))?
                    .direction = strafe.motion_direction();
                let StrafeAdmission::Combat { group, step, turn } = admission else {
                    anyhow::bail!(
                        "{name} outside combat stance unsupported: lateral step check, edge probe and hanging-edge test"
                    );
                };
                let input = animation.as_mut().with_context(|| {
                    format!("{name} requires prepared animation and explicit nominal ticks")
                })?;
                let spatial = spatial
                    .as_mut()
                    .with_context(|| format!("{name} requires receiver pose and frame_scale"))?;
                let already_strafing = input.animation.current_group().is_some_and(|current| {
                    super::locomotion::Strafe::COMBAT_GROUPS.contains(&current)
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
                let (angle, movement) = strafe_motion(
                    *strafe,
                    spatial.angles[1],
                    turn,
                    step,
                    spatial.frame_scale as i32,
                    input.animation.translation_suppressed(),
                );
                spatial.angles[1] = angle;
                if let Some((heading, distance)) = movement {
                    *spatial.position = cyrus_world::transform::translate_forward(
                        *spatial.position,
                        heading,
                        distance,
                    )?;
                }
                !continues(duration, elapsed)
            }
            Self::Location => {
                let [location, duration] = arguments else {
                    anyhow::bail!("MoveToLocation requires two arguments")
                };
                let (location, duration) = (location.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("MoveToLocation requires elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("MoveToLocation requires receiver pose and frame_scale")?;
                let count = i32::try_from(input.locations.len())
                    .context("MoveToLocation location count exceeds signed range")?;
                let target = if location < 0 || location > count {
                    input
                        .locations
                        .first()
                        .context("MoveToLocation home unavailable")?
                } else {
                    input.locations.get(location as usize).context("MoveToLocation selector equals location count: original out-of-range access unsupported")?
                };
                if elapsed > duration {
                    *input.position = *target;
                    if !input.map_updates.contains(&input.actor_index) {
                        input.map_updates.push(input.actor_index);
                    }
                    return Ok(true);
                }
                let source = if input.slave {
                    input
                        .effective_position
                        .context("MoveToLocation attached receiver pose unavailable")?
                } else {
                    *input.position
                };
                let differences =
                    std::array::from_fn(|axis| target[axis].wrapping_sub(source[axis]));
                let direction = super::spatial::normalized_direction(differences);
                // Distance subtracts in the opposite order; wrapping makes this significant at MIN.
                let distance = super::spatial::rounded_distance(std::array::from_fn(|axis| {
                    source[axis].wrapping_sub(target[axis])
                }))?;
                let divisor = duration.wrapping_add(1).wrapping_sub(elapsed);
                let movement = distance
                    .checked_div(divisor)
                    .context("MoveToLocation division invalid")?
                    .wrapping_mul(input.frame_scale as i32)
                    >> 8;
                for (axis, component) in direction.into_iter().enumerate() {
                    input.position[axis] = input.position[axis]
                        .wrapping_add(((component >> 8).wrapping_mul(movement)) >> 8);
                }
                if !input.map_updates.contains(&input.actor_index) {
                    input.map_updates.push(input.actor_index);
                }
                false
            }
            Self::ObjectMovement { target, traveled } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("MoveObjectAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                if delta == 0 || duration == 0 {
                    return Ok(true);
                }
                let elapsed = invocation
                    .elapsed
                    .context("MoveObjectAxis requires explicit elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("MoveObjectAxis requires receiver pose and frame_scale")?;
                if elapsed == 0 {
                    *target = delta;
                    *traveled = 0;
                }
                let step = (delta
                    .checked_div(duration)
                    .context("MoveObjectAxis division overflow")?
                    >> 8)
                    .wrapping_mul(input.frame_scale as i32);
                *traveled = traveled.wrapping_add(step);
                if (step < 0 && *traveled <= *target) || (step > 0 && *traveled >= *target) {
                    return Ok(true);
                }
                if axis == 2 && !input.slave && input.angles[0] == 0 && input.angles[2] == 0 {
                    *input.position = cyrus_world::transform::translate_forward(
                        *input.position,
                        input.angles[1],
                        step,
                    )?;
                } else {
                    let mut displacement = [0; 3];
                    if (0..=2).contains(&axis) {
                        displacement[axis as usize] = step;
                    }
                    let transformed = input.matrix.transform(displacement)?;
                    *input.position =
                        std::array::from_fn(|i| input.position[i].wrapping_add(transformed[i]));
                }
                if !input.map_updates.contains(&input.actor_index) {
                    input.map_updates.push(input.actor_index);
                }
                elapsed >= duration.wrapping_sub(1)
            }
            Self::Movement { target } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("MoveByAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                if delta == 0 || duration == 0 {
                    return Ok(true);
                }
                let input = spatial
                    .as_mut()
                    .context("MoveByAxis requires spatial state and frame_scale")?;
                let elapsed = invocation
                    .elapsed
                    .context("MoveByAxis requires explicit elapsed input")?;
                let step = (delta
                    .checked_div(duration)
                    .context("MoveByAxis division overflow")?
                    >> 8)
                    .wrapping_mul(input.frame_scale as i32);
                let coordinate = &mut input.position[match axis {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                }];
                if *target == 0 {
                    *target = coordinate.wrapping_add(delta);
                }
                let next = coordinate.wrapping_add(step);
                if (step < 0 && next <= *target)
                    || (step > 0 && next >= *target)
                    || elapsed >= duration.wrapping_sub(1)
                {
                    *coordinate = *target;
                    true
                } else {
                    *coordinate = next;
                    false
                }
            }
            Self::ScaledMovement { origin } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("sMoveByAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("sMoveByAxis requires explicit elapsed input")?;
                let distance = delta
                    .wrapping_shl(8)
                    .checked_div(duration)
                    .context("sMoveByAxis division by zero or overflow")?
                    .wrapping_mul(elapsed)
                    >> 8;
                let complete = elapsed >= duration.wrapping_sub(1);
                if (0..=2).contains(&axis) {
                    let input = spatial
                        .as_mut()
                        .context("sMoveByAxis requires spatial state")?;
                    let coordinate = &mut input.position[axis as usize];
                    if *origin == 0 && elapsed == 0 {
                        *origin = *coordinate;
                    }
                    *coordinate = origin.wrapping_add(if complete { delta } else { distance });
                }
                complete
            }
            Self::Rotation {
                target,
                accumulator,
            } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("RotateByAxis requires three arguments")
                };
                let (delta, duration) = (delta.integer()?, duration.integer()?);
                if delta == 0 || duration == 0 {
                    return Ok(true);
                }
                let input = spatial.as_mut().context(
                    "RotateByAxis requires spatial state, frame_scale and nominal_ticks",
                )?;
                let axis = match axis.integer()? {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                };
                let current = i32::from(input.angles[axis]);
                let elapsed = invocation
                    .elapsed
                    .context("RotateByAxis requires explicit elapsed input")?;
                let quotient = delta
                    .wrapping_shl(8)
                    .checked_div(duration)
                    .context("RotateByAxis fixed-point division overflow")?;
                if *target == 0 && elapsed == 0 {
                    *target = (current.wrapping_add(delta) & 2047) << 8;
                    *accumulator = current << 8;
                }
                let mut step = quotient.wrapping_mul(input.frame_scale as i32) >> 8;
                if step == 0 && input.nominal_ticks != 0 {
                    step = input.nominal_ticks as i32;
                    if *target < 0 {
                        step = step.wrapping_neg();
                    }
                }
                let difference = ((current - (*target >> 8) + 1024) & 2047) - 1024;
                if difference.abs() <= (step >> 8).abs() {
                    input.angles[axis] = (*target >> 8) as u16;
                    true
                } else {
                    *accumulator = accumulator.wrapping_add(step);
                    input.angles[axis] = ((*accumulator >> 8) & 2047) as u16;
                    false
                }
            }
            Self::AbsoluteRotation {
                movement,
                accumulator,
            } => {
                let [axis, target, duration] = arguments else {
                    anyhow::bail!("RotateToAxis requires three arguments")
                };
                let duration = duration.integer()?;
                if duration == 0 {
                    return Ok(true);
                }
                let target = target.integer()? & 2047;
                let axis = match axis.integer()? {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                };
                let input = spatial
                    .as_mut()
                    .context("RotateToAxis requires spatial state")?;
                let current = i32::from(input.angles[axis]);
                let difference = (current & 2047) - target;
                let remaining = if difference < -1024 {
                    -(difference & 2047)
                } else if difference > 1024 {
                    2048 - difference
                } else {
                    -difference
                };
                let elapsed = invocation
                    .elapsed
                    .context("RotateToAxis requires elapsed input")?;
                if *movement == 0 && elapsed == 0 {
                    *movement = remaining << 8;
                    *accumulator = current << 8;
                }
                let mut step = movement
                    .checked_div(duration)
                    .context("RotateToAxis fixed-point division overflow")?
                    .wrapping_mul(input.frame_scale as i32)
                    >> 8;
                if step == 0 && input.nominal_ticks != 0 {
                    step = (input.nominal_ticks as i32).wrapping_shl(8);
                    if *movement < 0 {
                        step = step.wrapping_neg();
                    }
                }
                if remaining.abs() <= (step >> 8).abs() {
                    input.angles[axis] = target as u16;
                    true
                } else {
                    *accumulator = accumulator.wrapping_add(step);
                    input.angles[axis] = ((*accumulator >> 8) & 2047) as u16;
                    false
                }
            }
            Self::ScaledRotation { origin } => {
                let [axis, target, duration] = arguments else {
                    anyhow::bail!("sRotateToAxis requires three arguments")
                };
                let (axis, target, duration) =
                    (axis.integer()?, target.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("sRotateToAxis requires elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("sRotateToAxis requires spatial state")?;
                if elapsed == 0 && (0..=2).contains(&axis) {
                    *origin = i32::from(input.angles[axis as usize]);
                }
                let difference = target.wrapping_sub(*origin);
                let movement = if difference < -1024 {
                    difference & 2047
                } else if difference > 1024 {
                    -(difference & 1023)
                } else {
                    difference
                };
                let end = duration.wrapping_sub(1);
                let offset = movement
                    .wrapping_shl(8)
                    .checked_div(end)
                    .context("sRotateToAxis duration causes division by zero or overflow")?
                    .wrapping_mul(elapsed)
                    >> 8;
                let complete = elapsed >= end;
                if (0..=2).contains(&axis) {
                    input.angles[axis as usize] = if complete {
                        (target & 2047) as u16
                    } else {
                        (origin.wrapping_add(offset) & 2047) as u16
                    };
                }
                complete
            }
            Self::ScaledRelativeRotation { origin } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("sRotateByAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("sRotateByAxis requires elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("sRotateByAxis requires spatial state")?;
                if elapsed == 0 && (0..=2).contains(&axis) {
                    *origin = i32::from(input.angles[axis as usize]);
                }
                let magnitude = delta
                    .wrapping_abs()
                    .wrapping_shl(8)
                    .checked_div(duration)
                    .context("sRotateByAxis duration causes division by zero or overflow")?
                    .wrapping_mul(elapsed)
                    >> 8;
                let offset = if delta < 0 {
                    magnitude.wrapping_neg()
                } else {
                    magnitude
                };
                let complete = elapsed >= duration.wrapping_sub(1);
                if (0..=2).contains(&axis) {
                    input.angles[axis as usize] =
                        (origin.wrapping_add(if complete { delta } else { offset }) & 2047) as u16;
                }
                complete
            }
            Self::Duration => {
                let [duration] = arguments else {
                    anyhow::bail!("Wait requires one argument")
                };
                invocation
                    .elapsed
                    .context("Wait requires explicit elapsed input")?
                    >= duration.integer()?.wrapping_sub(1)
            }
            Self::Tasks | Self::Blocking => {
                anyhow::ensure!(
                    arguments.is_empty(),
                    "task-count wait requires no arguments"
                );
                match self {
                    Self::Tasks => invocation.unpaused_tasks <= 1,
                    _ => invocation.unpaused_blocking_tasks <= 1,
                }
            }
            Self::Animation { group, started } => {
                let [requested, count] = arguments else {
                    anyhow::bail!("PushAnimation requires two arguments")
                };
                let requested = u16::try_from(requested.integer()?)
                    .context("PushAnimation group outside supported range")?;
                anyhow::ensure!(
                    requested <= i16::MAX as u16,
                    "PushAnimation signed group unsupported"
                );
                let count = count.integer()? as u8;
                let input = animation.as_mut().context(
                    "PushAnimation requires prepared animation and explicit nominal ticks",
                )?;
                *group = requested;
                if *started {
                    input.animation.current_group() != Some(requested)
                } else {
                    if input.animation.current_group() == Some(requested) {
                        input.animation.set_loop_count(count)?;
                        *started = true;
                    } else {
                        let frame = match &mut input.sounds {
                            Some(sounds) => input.animation.request_with_sounds(
                                requested,
                                0,
                                input.ticks,
                                *sounds,
                            )?,
                            None => input.animation.request(requested, 0, input.ticks)?,
                        };
                        *input.frame = frame.or(*input.frame);
                    }
                    false
                }
            }
        })
    }
}

/// Actor-owned native continuations with replacement-only persistence APIs.
/// Execution and polling remain internal to the game host.
#[derive(Default, Debug)]
pub struct NativeTasks {
    next: u64,
    tasks: BTreeMap<u64, Task>,
}
/// Locomotion operation whose geometry must be sampled for the actual receiver.
#[derive(Clone, Copy)]
pub(super) enum LocomotionOperation {
    Backward,
    Forward,
    WalkForward,
}
impl LocomotionOperation {
    pub(super) fn named(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("MoveBackward") {
            Some(Self::Backward)
        } else if name.eq_ignore_ascii_case("MoveForward") {
            Some(Self::Forward)
        } else if name.eq_ignore_ascii_case("WalkForward") {
            Some(Self::WalkForward)
        } else {
            None
        }
    }
}
impl NativeTasks {
    pub(super) fn locomotion_operation(&self, token: u64) -> Option<LocomotionOperation> {
        match self.tasks.get(&token)? {
            Task::Backward => Some(LocomotionOperation::Backward),
            Task::Forward => Some(LocomotionOperation::Forward),
            Task::WalkForward => Some(LocomotionOperation::WalkForward),
            _ => None,
        }
    }
    pub(super) fn call(
        &mut self,
        (function, kind): (&str, CallKind),
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<Option<CallResult>> {
        let mut task = if function.eq_ignore_ascii_case("WaitOnDialog") {
            anyhow::ensure!(
                kind == CallKind::Task,
                "WaitOnDialog requires task encoding"
            );
            Task::Dialogue
        } else if function.eq_ignore_ascii_case("RTXp") {
            anyhow::ensure!(kind == CallKind::Task, "RTXp requires task encoding");
            Task::PlayerDialogue { started: false }
        } else if function.eq_ignore_ascii_case("AmbientSound") {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "AmbientSound requires multitask encoding"
            );
            Task::Ambient { channel: None }
        } else if function.eq_ignore_ascii_case("RotateByAxis") {
            Task::Rotation {
                target: 0,
                accumulator: 0,
            }
        } else if function.eq_ignore_ascii_case("RotateToAxis") {
            Task::AbsoluteRotation {
                movement: 0,
                accumulator: 0,
            }
        } else if function.eq_ignore_ascii_case("sRotateToAxis") {
            Task::ScaledRotation { origin: 0 }
        } else if function.eq_ignore_ascii_case("sRotateByAxis") {
            Task::ScaledRelativeRotation { origin: 0 }
        } else if function.eq_ignore_ascii_case("FacePlayer") {
            Task::FacePlayer
        } else if function.eq_ignore_ascii_case("MoveBackward") {
            Task::Backward
        } else if function.eq_ignore_ascii_case("MoveForward") {
            Task::Forward
        } else if function.eq_ignore_ascii_case("WalkForward") {
            Task::WalkForward
        } else if function.eq_ignore_ascii_case("MoveLeft") {
            Task::Strafe(super::locomotion::Strafe::Left)
        } else if function.eq_ignore_ascii_case("MoveRight") {
            Task::Strafe(super::locomotion::Strafe::Right)
        } else if function.eq_ignore_ascii_case("MoveObjectAxis") {
            Task::ObjectMovement {
                target: 0,
                traveled: 0,
            }
        } else if function.eq_ignore_ascii_case("MoveByAxis") {
            Task::Movement { target: 0 }
        } else if function.eq_ignore_ascii_case("sMoveByAxis") {
            Task::ScaledMovement { origin: 0 }
        } else if function.eq_ignore_ascii_case("MoveToLocation") {
            Task::Location
        } else if function.eq_ignore_ascii_case("Wait") {
            Task::Duration
        } else if function.eq_ignore_ascii_case("WaitOnTasks") {
            Task::Tasks
        } else if function.eq_ignore_ascii_case("WaitNonMulti") {
            Task::Blocking
        } else if function.eq_ignore_ascii_case("WaitAnimFrame")
            || function.eq_ignore_ascii_case("WaitPlayerAnimFrame")
        {
            anyhow::ensure!(kind == CallKind::Task, "{function} requires task encoding");
            Task::AnimationFrame {
                player: function.eq_ignore_ascii_case("WaitPlayerAnimFrame"),
            }
        } else if function.eq_ignore_ascii_case("PushAnimation") {
            Task::Animation {
                group: 0,
                started: false,
            }
        } else {
            return Ok(None);
        };
        anyhow::ensure!(
            kind != CallKind::Function,
            "function-encoded {function} timing unsupported"
        );
        let next = self
            .next
            .checked_add(1)
            .context("native task identity exhausted")?;
        // Initial completion still allocates a slot and yields until a later task pass.
        task.complete(arguments, invocation, animation, spatial, services)?;
        self.next = next;
        self.tasks.insert(next, task);
        Ok(Some(CallResult::Pending(next)))
    }
    pub(super) fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        let task = self
            .tasks
            .get_mut(&token)
            .with_context(|| format!("native task {token} absent"))?;
        let completed = task.complete(arguments, invocation, animation, spatial, services)?;
        if completed {
            self.tasks.remove(&token);
        }
        Ok(completed)
    }
    pub(super) fn cancel(&mut self, token: u64) -> Result<()> {
        self.tasks
            .remove(&token)
            .with_context(|| format!("native task {token} absent"))?;
        Ok(())
    }
    pub(super) fn animation_group(&self) -> Option<u16> {
        self.tasks.values().find_map(|task| match task {
            Task::Animation { group, .. } => Some(*group),
            _ => None,
        })
    }
}

#[cfg(test)]
mod frame_wait_tests {
    use super::*;

    #[test]
    fn frame_wait_poll_uses_full_arguments_and_current_before_pending() {
        for (current, pending, progress, group, threshold, expected) in [
            (7, -1, 0, 7, 1, false),
            (7, -1, 1, 7, 1, true),
            (7, -1, 255, 7, 256, false),
            (7, -1, 0, 7, i32::MIN, true),
            (7, -1, 255, 7, i32::MAX, false),
            (8, 7, 255, 7, 0, false),
            (8, 9, 0, 7, 255, true),
            (7, 7, 1, 7, 1, true),
            (-32768, -1, 1, -32768, 1, true),
            (-32768, -1, 0, 32768, 1, true),
            (-1, -1, 0, 65535, 1, true),
        ] {
            for player in [false, true] {
                let mut tasks = NativeTasks::default();
                let function = if player {
                    "WaitPlayerAnimFrame"
                } else {
                    "WaitAnimFrame"
                };
                let inputs = || NativeServices {
                    frame_wait: Some(FrameWaitCall {
                        receiver: if player {
                            Err("caller must not be read".into())
                        } else {
                            Ok((current, pending, progress))
                        },
                        player: if player {
                            Ok(Some((current, pending, progress)))
                        } else {
                            Err("player must not be read".into())
                        },
                    }),
                    ..Default::default()
                };
                let invocation = Invocation {
                    elapsed: None,
                    unpaused_tasks: 0,
                    unpaused_blocking_tasks: 0,
                };
                let args = [Argument::Integer(group), Argument::Integer(threshold)];
                let Some(CallResult::Pending(token)) = tasks
                    .call(
                        (function, CallKind::Task),
                        &args,
                        invocation,
                        &mut None,
                        &mut None,
                        inputs(),
                    )
                    .unwrap()
                else {
                    panic!("task must allocate continuation")
                };
                assert_eq!(
                    tasks
                        .poll(token, &args, invocation, &mut None, &mut None, inputs())
                        .unwrap(),
                    expected,
                    "{function}: current={current}, pending={pending}, progress={progress}, group={group}, threshold={threshold}"
                );
            }
        }
    }
}
