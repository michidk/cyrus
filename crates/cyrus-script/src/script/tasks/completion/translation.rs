//! Local-axis and timed spatial-transform task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_translation(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        spatial: &mut Option<SpatialCall<'_>>,
    ) -> Result<bool> {
        Ok(match self {
            Self::ScaledAxisMovement => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("sMoveAlongAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("sMoveAlongAxis requires explicit elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("sMoveAlongAxis requires spatial state and frame_scale")?;
                // The duration in frames at the current scale, then the step spread over it.
                let frames = duration
                    .wrapping_shl(8)
                    .checked_div(input.frame_scale as i32)
                    .filter(|frames| *frames != 0)
                    .context("sMoveAlongAxis frame division by zero")?;
                let step = delta
                    .wrapping_shl(8)
                    .checked_div(frames)
                    .context("sMoveAlongAxis division overflow")?
                    >> 8;
                let mut offset = [0; 3];
                offset[match axis {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                }] = step;
                let [pitch, yaw, roll] = *input.angles;
                let rotated = cyrus_world::transform::Matrix::euler(
                    [pitch, wrap(yaw.wrapping_neg()), roll].map(|angle| (angle & 2047) as u16),
                )
                .inverse_transform(offset)?;
                for (coordinate, amount) in input.position.iter_mut().zip(rotated) {
                    *coordinate = coordinate.wrapping_add(amount);
                }
                duration.wrapping_sub(1) <= elapsed
            }
            Self::Move | Self::Rotate | Self::ScaledMove | Self::ScaledRotate => {
                let [x, y, z, duration] = arguments else {
                    anyhow::bail!("Move, Rotate, sMove and sRotate require four arguments")
                };
                let operands = [x.integer()?, y.integer()?, z.integer()?];
                let duration = duration.integer()?;
                let elapsed = invocation
                    .elapsed
                    .context("Move, Rotate, sMove and sRotate require explicit elapsed input")?;
                let input = spatial.as_mut().context(
                    "Move, Rotate, sMove and sRotate require spatial state and nominal_ticks",
                )?;
                for (axis, operand) in operands.into_iter().enumerate() {
                    if operand == 0 {
                        continue;
                    }
                    if matches!(self, Self::Move | Self::ScaledMove) {
                        let step = if matches!(self, Self::Move) {
                            operand.wrapping_mul(input.nominal_ticks as i32)
                        } else {
                            operand
                                .wrapping_shl(8)
                                .checked_div(duration)
                                .context("sMove division by zero")?
                                >> 8
                        };
                        input.position[axis] = input.position[axis].wrapping_add(step);
                    } else {
                        // The magnitude is scaled by the elapsed input, then given its sign back.
                        let step = (operand.wrapping_abs().wrapping_shl(8))
                            .checked_div(duration)
                            .context("Rotate division by zero")?
                            .wrapping_mul(elapsed)
                            >> 8;
                        let step = if operand < 0 { -step } else { step };
                        // The original stores the sum unmasked; matrix builds mask it when they read it.
                        input.angles[axis] = input.angles[axis].wrapping_add(step);
                    }
                }
                duration.wrapping_sub(1) <= elapsed
            }
            Self::AxisMovement {
                target,
                accumulated,
            } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("MoveAlongAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                if delta == 0 || duration == 0 {
                    return Ok(true);
                }
                let elapsed = invocation
                    .elapsed
                    .context("MoveAlongAxis requires explicit elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("MoveAlongAxis requires spatial state and frame_scale")?;
                if elapsed == 0 {
                    *target = delta;
                    *accumulated = 0;
                }
                let step = (delta
                    .checked_div(duration)
                    .context("MoveAlongAxis division overflow")?
                    >> 8)
                    .wrapping_mul(input.frame_scale as i32);
                *accumulated = accumulated.wrapping_add(step);
                if (step < 0 && *accumulated <= *target) || (step > 0 && *accumulated >= *target) {
                    return Ok(true);
                }
                let mut offset = [0; 3];
                offset[match axis {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                }] = step;
                // An ordinary actor rotates by its angles, the sense opposite the stored matrix's.
                let rotated = cyrus_world::transform::Matrix::orientation(
                    input.angles.map(|angle| (angle & 2047) as u16),
                )
                .inverse_transform(offset)?;
                for (coordinate, amount) in input.position.iter_mut().zip(rotated) {
                    *coordinate = coordinate.wrapping_add(amount);
                }
                input.map_updates.register(input.actor_index)?;
                !(duration.wrapping_sub(1) > elapsed)
            }
            Self::Marker => {
                let [marker, duration] = arguments else {
                    anyhow::bail!("MoveToMarker requires two arguments")
                };
                let (marker, duration) = (marker.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("MoveToMarker requires elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("MoveToMarker requires receiver pose and frame_scale")?;
                // Steps left at the current frame scale, truncated toward zero.
                let remaining = duration
                    .wrapping_sub(1)
                    .wrapping_sub(elapsed)
                    .wrapping_shl(8)
                    .checked_div(input.frame_scale as i32)
                    .context("MoveToMarker division invalid")?;
                let target = input
                    .markers
                    .context("MoveToMarker markers not prepared")?
                    .position(marker)?;
                if !(duration.wrapping_sub(1) > elapsed) {
                    *input.position = target;
                    input.map_updates.register(input.actor_index)?;
                    return Ok(true);
                }
                let source = if input.slave {
                    input
                        .effective_position
                        .context("MoveToMarker attached receiver pose unavailable")?
                } else {
                    *input.position
                };
                let differences =
                    std::array::from_fn(|axis| target[axis].wrapping_sub(source[axis]));
                let direction = crate::script::spatial::normalized_direction(differences);
                let distance =
                    crate::script::spatial::rounded_distance(std::array::from_fn(|axis| {
                        source[axis].wrapping_sub(target[axis])
                    }))?;
                if remaining != 0 {
                    let step = distance
                        .checked_div(remaining)
                        .context("MoveToMarker division invalid")?;
                    for (axis, component) in direction.into_iter().enumerate() {
                        input.position[axis] = input.position[axis]
                            .wrapping_add(((component >> 10).wrapping_mul(step >> 2)) >> 4);
                    }
                    input.map_updates.register(input.actor_index)?;
                }
                false
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
                    input.map_updates.register(input.actor_index)?;
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
                let direction = crate::script::spatial::normalized_direction(differences);
                // Distance subtracts in the opposite order; wrapping makes this significant at MIN.
                let distance =
                    crate::script::spatial::rounded_distance(std::array::from_fn(|axis| {
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
                input.map_updates.register(input.actor_index)?;
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
                        (input.angles[1] & 2047) as u16,
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
                input.map_updates.register(input.actor_index)?;
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
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
