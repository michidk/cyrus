//! Axis rotation task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_rotation(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        spatial: &mut Option<SpatialCall<'_>>,
    ) -> Result<bool> {
        Ok(match self {
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
                let current = input.angles[axis];
                let elapsed = invocation
                    .elapsed
                    .context("RotateByAxis requires explicit elapsed input")?;
                let quotient = delta
                    .wrapping_shl(8)
                    .checked_div(duration)
                    .context("RotateByAxis fixed-point division overflow")?;
                if *target == 0 && elapsed == 0 {
                    *target = wrap(current.wrapping_add(delta)) << 8;
                    *accumulator = current << 8;
                }
                let mut step = quotient.wrapping_mul(input.frame_scale as i32) >> 8;
                if step == 0 && input.nominal_ticks != 0 {
                    step = input.nominal_ticks as i32;
                    if *target < 0 {
                        step = step.wrapping_neg();
                    }
                }
                let difference = wrap(current.wrapping_sub(*target >> 8).wrapping_add(1024)) - 1024;
                if difference.abs() <= (step >> 8).abs() {
                    input.angles[axis] = *target >> 8;
                    true
                } else {
                    *accumulator = accumulator.wrapping_add(step);
                    input.angles[axis] = wrap(*accumulator >> 8);
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
                let target = wrap(target.integer()?);
                let axis = match axis.integer()? {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                };
                let input = spatial
                    .as_mut()
                    .context("RotateToAxis requires spatial state")?;
                let current = input.angles[axis];
                let difference = wrap(current) - target;
                let remaining = if difference < -1024 {
                    -wrap(difference)
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
                    input.angles[axis] = target;
                    true
                } else {
                    *accumulator = accumulator.wrapping_add(step);
                    input.angles[axis] = wrap(*accumulator >> 8);
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
                    *origin = input.angles[axis as usize];
                }
                let difference = target.wrapping_sub(*origin);
                let movement = if difference < -1024 {
                    wrap(difference)
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
                        wrap(target)
                    } else {
                        wrap(origin.wrapping_add(offset))
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
                    *origin = input.angles[axis as usize];
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
                        wrap(origin.wrapping_add(if complete { delta } else { offset }));
                }
                complete
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
