//! Point navigation, facing and actor-control task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_point(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        Ok(match self {
            Self::GotoLocation => {
                let [location] = arguments else {
                    anyhow::bail!("GotoLocation requires one argument")
                };
                services
                    .point
                    .context("GotoLocation receiver owner unavailable")?
                    .goto_location(location.integer()?)?
                    == 0
            }
            Self::MoveMarker
            | Self::MoveLocation
            | Self::MovePosition
            | Self::MoveNodeMarker
            | Self::MoveNodeLocation
            | Self::MoveNodePosition => {
                let wrapper = if matches!(
                    self,
                    Self::MoveNodeMarker | Self::MoveNodeLocation | Self::MoveNodePosition
                ) {
                    PointWrapper::Node
                } else {
                    PointWrapper::Direct
                };
                let (target, tolerance) = match (&*self, arguments) {
                    (Self::MoveMarker | Self::MoveNodeMarker, [marker, tolerance]) => {
                        (MoveTarget::Marker(marker.integer()?), tolerance.integer()?)
                    }
                    (Self::MoveLocation | Self::MoveNodeLocation, [location, tolerance]) => (
                        MoveTarget::Location(location.integer()?),
                        tolerance.integer()?,
                    ),
                    (Self::MovePosition | Self::MoveNodePosition, [x, y, z, tolerance]) => (
                        MoveTarget::Position([x.integer()?, y.integer()?, z.integer()?]),
                        tolerance.integer()?,
                    ),
                    _ => anyhow::bail!(
                        "the marker and location move tasks take two arguments, the position tasks four"
                    ),
                };
                let host = services
                    .point
                    .context("Move point tasks require the receiver's point-walking owner")?;
                host.move_to(wrapper, target, tolerance)? == 0
            }
            Self::FollowPlayer => {
                anyhow::ensure!(arguments.len() == 1, "TestTask requires one argument");
                let host = services
                    .point
                    .context("TestTask requires the receiver's point-walking owner")?;
                let (own, target) = host.follow_positions()?;
                let distance =
                    crate::script::spatial::rounded_distance(std::array::from_fn(|axis| {
                        own[axis].wrapping_sub(target[axis])
                    }))?;
                // Equality does not request movement; the request's result is ignored.
                if distance > 131_072 {
                    host.move_to(PointWrapper::Node, MoveTarget::Position(target), 0)?;
                }
                // The handler reports one every poll, so the slot is never released.
                false
            }
            Self::PlayerOffset => {
                let [x, y, z] = arguments else {
                    anyhow::bail!("PlayerOffsettask requires three arguments")
                };
                let host = services
                    .point
                    .context("PlayerOffsettask requires the receiver's placement owner")?;
                host.player_offset([x.integer()?, y.integer()?, z.integer()?])?;
                false
            }
            Self::PlayerTilt | Self::PlayerTiltRevx => {
                let reversed = matches!(self, Self::PlayerTiltRevx);
                let name = if reversed {
                    "PlayerTiltRevx"
                } else {
                    "PlayerTilt"
                };
                let [angle, strength, divisor] = arguments else {
                    anyhow::bail!("{name} requires three arguments")
                };
                services
                    .point
                    .with_context(|| format!("{name} receiver owner unavailable"))?
                    .player_tilt(
                        angle.integer()?,
                        strength.integer()?,
                        divisor.integer()?,
                        reversed,
                    )?;
                false
            }
            Self::DragonBreath => {
                anyhow::ensure!(
                    arguments.is_empty(),
                    "DragonBreathTask requires no arguments"
                );
                let host = services
                    .point
                    .context("DragonBreathTask requires the receiver's update owner")?;
                !host.dragon_breath()?
            }
            Self::Projectile => {
                anyhow::ensure!(arguments.is_empty(), "projectileAI requires no arguments");
                let host = services
                    .point
                    .context("projectileAI requires the receiver's update owner")?;
                !host.projectile()?
            }
            Self::FaceAngle | Self::FacePos | Self::FaceObject => {
                let target = match (&*self, arguments) {
                    (Self::FaceAngle, [angle]) => FaceTarget::Angle(angle.integer()?),
                    (Self::FacePos, [x, z]) => FaceTarget::Point([x.integer()?, z.integer()?]),
                    (Self::FaceObject, [Argument::Actor(actor)]) => FaceTarget::Actor(*actor),
                    _ => anyhow::bail!(
                        "FaceAngle takes one argument, FacePos two and FaceObject one actor reference"
                    ),
                };
                let host = services
                    .point
                    .context("FaceAngle and FacePos require the receiver's facing owner")?;
                host.face(target)? == 0
            }
            Self::FacePlayer | Self::FacePlayerInertia => {
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
                let bearing = i32::from(crate::script::spatial::query_bearing(
                    player[0].wrapping_sub(input.position[0]),
                    player[2].wrapping_sub(input.position[2]),
                ));
                let heading = input.angles[1];
                // The query bearing is the negation of the routine's angle, which the routine
                // subtracts from the negated heading.
                let mut delta = wrap(bearing.wrapping_sub(heading));
                if delta == 0 {
                    return Ok(true);
                }
                let backwards = delta > 0x400;
                if backwards {
                    delta = (-delta - 0x400) & 0x3ff;
                }
                let step = if matches!(self, Self::FacePlayerInertia) {
                    ((delta << 8)
                        .checked_div(duration)
                        .context("FacePlayerInertia division by zero")?
                        .wrapping_sub(elapsed))
                        >> 8
                } else {
                    let remaining = elapsed.wrapping_sub(duration).wrapping_abs().max(1);
                    ((delta << 8) / remaining) >> 8
                };
                if step == 0 {
                    return Ok(true);
                }
                let heading = if backwards {
                    heading.wrapping_sub(step)
                } else {
                    heading.wrapping_add(step)
                };
                input.angles[1] = wrap(heading);
                elapsed >= duration.wrapping_sub(1)
            }
            Self::FaceLocation => {
                let [location, deadline] = arguments else {
                    anyhow::bail!("FaceLocation requires two arguments")
                };
                let (location, deadline) = (location.integer()?, deadline.integer()?);
                let input = spatial
                    .as_mut()
                    .context("FaceLocation requires receiver pose")?;
                let tick = input
                    .tick
                    .context("FaceLocation shared tick counter unavailable")?
                    as i32;
                let count = i32::try_from(input.locations.len())
                    .context("FaceLocation location count exceeds signed range")?;
                let target = if location < 0 || location > count {
                    input
                        .locations
                        .first()
                        .context("FaceLocation home unavailable")?
                } else {
                    input.locations.get(location as usize).context("FaceLocation selector equals location count: original out-of-range access unsupported")?
                };
                let bearing = i32::from(crate::script::spatial::query_bearing(
                    target[0].wrapping_sub(input.position[0]),
                    target[2].wrapping_sub(input.position[2]),
                ));
                let heading = input.angles[1];
                // The query bearing is the negation of the routine's angle, which the routine
                // subtracts from the negated heading.
                let mut delta = wrap(bearing.wrapping_sub(heading));
                if delta == 0 {
                    return Ok(true);
                }
                let backwards = delta > 0x400;
                if backwards {
                    delta = (-delta - 0x400) & 0x3ff;
                }
                // The magnitude wraps, so the minimum value stays negative; only zero becomes one.
                let divisor = match tick.wrapping_sub(deadline).wrapping_abs() {
                    0 => 1,
                    divisor => divisor,
                };
                let step = ((delta << 8) / divisor) >> 8;
                if step == 0 {
                    return Ok(true);
                }
                // The heading is stored without being masked back to 2048 units.
                input.angles[1] = if backwards {
                    heading.wrapping_sub(step)
                } else {
                    heading.wrapping_add(step)
                };
                !(deadline.wrapping_sub(1) > tick)
            }
            Self::WanderLocation { started, speed } => {
                let [location, duration] = arguments else {
                    anyhow::bail!("WanderToLocation requires two arguments")
                };
                let (location, duration) = (location.integer()?, duration.integer()?);
                let input = spatial
                    .as_mut()
                    .context("WanderToLocation requires receiver pose and frame_scale")?;
                let count = i32::try_from(input.locations.len())
                    .context("WanderToLocation location count exceeds signed range")?;
                let target = if location < 0 || location > count {
                    input
                        .locations
                        .first()
                        .context("WanderToLocation home unavailable")?
                } else {
                    input.locations.get(location as usize).context("WanderToLocation selector equals location count: original out-of-range access unsupported")?
                };
                let source = if input.slave {
                    input
                        .effective_position
                        .context("WanderToLocation attached receiver pose unavailable")?
                } else {
                    *input.position
                };
                let differences =
                    std::array::from_fn(|axis| target[axis].wrapping_sub(source[axis]));
                // Distance subtracts in the opposite order; wrapping makes this significant at MIN.
                let distance =
                    crate::script::spatial::rounded_distance(std::array::from_fn(|axis| {
                        source[axis].wrapping_sub(target[axis])
                    }))?;
                if !*started {
                    *speed = distance
                        .checked_div(duration)
                        .context("WanderToLocation division invalid")?;
                    *started = true;
                }
                let direction = crate::script::spatial::normalized_direction(differences);
                let step = speed.wrapping_mul(input.frame_scale as i32) >> 8;
                input.map_updates.register(input.actor_index)?;
                if distance > step {
                    for (axis, component) in direction.into_iter().enumerate() {
                        input.position[axis] = input.position[axis]
                            .wrapping_add(((component >> 8).wrapping_mul(step)) >> 8);
                    }
                    false
                } else {
                    *input.position = *target;
                    true
                }
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
