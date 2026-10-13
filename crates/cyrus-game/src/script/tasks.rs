//! Actor-owned native task tokens share waits and animation requests.
use anyhow::{Context, Result};
use cyrus_assets::script::CallKind;
use cyrus_script::{Argument, CallResult, Invocation};
use cyrus_world::animation::{ActorAnimation, AnimationSound};
use std::collections::BTreeMap;

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

#[derive(Clone, Copy)]
enum Task {
    Ambient { channel: Option<u8> },
    Movement { target: i32 },
    ObjectMovement { target: i32, traveled: i32 },
    ScaledMovement { origin: i32 },
    Rotation { target: i32, accumulator: i32 },
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
        ambient: Option<&AmbientCall<'_>>,
    ) -> Result<bool> {
        Ok(match self {
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

#[derive(Default)]
pub(super) struct NativeTasks {
    next: u64,
    tasks: BTreeMap<u64, Task>,
}
impl NativeTasks {
    pub fn call(
        &mut self,
        (function, kind): (&str, CallKind),
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        ambient: Option<&AmbientCall<'_>>,
    ) -> Result<Option<CallResult>> {
        let mut task = if function.eq_ignore_ascii_case("AmbientSound") {
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
        } else if function.eq_ignore_ascii_case("MoveObjectAxis") {
            Task::ObjectMovement {
                target: 0,
                traveled: 0,
            }
        } else if function.eq_ignore_ascii_case("MoveByAxis") {
            Task::Movement { target: 0 }
        } else if function.eq_ignore_ascii_case("sMoveByAxis") {
            Task::ScaledMovement { origin: 0 }
        } else if function.eq_ignore_ascii_case("Wait") {
            Task::Duration
        } else if function.eq_ignore_ascii_case("WaitOnTasks") {
            Task::Tasks
        } else if function.eq_ignore_ascii_case("WaitNonMulti") {
            Task::Blocking
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
        task.complete(arguments, invocation, animation, spatial, ambient)?;
        self.next = next;
        self.tasks.insert(next, task);
        Ok(Some(CallResult::Pending(next)))
    }
    pub fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        ambient: Option<&AmbientCall<'_>>,
    ) -> Result<bool> {
        let task = self
            .tasks
            .get_mut(&token)
            .with_context(|| format!("native task {token} absent"))?;
        let completed = task.complete(arguments, invocation, animation, spatial, ambient)?;
        if completed {
            self.tasks.remove(&token);
        }
        Ok(completed)
    }
    pub fn cancel(&mut self, token: u64) -> Result<()> {
        self.tasks
            .remove(&token)
            .with_context(|| format!("native task {token} absent"))?;
        Ok(())
    }
    pub fn animation_group(&self) -> Option<u16> {
        self.tasks.values().find_map(|task| match task {
            Task::Animation { group, .. } => Some(*group),
            _ => None,
        })
    }
}
