//! Receiver and selected-player alignment task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_alignment(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        spatial: &mut Option<SpatialCall<'_>>,
        mut services: NativeServices<'_>,
    ) -> Result<bool> {
        Ok(match self {
            Self::Bounce(state) => {
                use crate::script::bounce::{BounceArguments, step};
                let [power, speed, steer] = arguments else {
                    anyhow::bail!("BounceObject takes three arguments");
                };
                let port = services
                    .bounce
                    .context("BounceObject receiver state unavailable")?;
                step(
                    &mut *port,
                    state,
                    BounceArguments {
                        power: power.integer()?,
                        speed: speed.integer()?,
                        steer: steer.integer()?,
                    },
                )?
            }
            Self::PickupLineUp { .. } | Self::ParallelLineUp => {
                use crate::script::lineup::{PickupState, pickup_mover, plain_mover};
                let [angle, x_offset, z_offset] = arguments else {
                    anyhow::bail!(
                        "PlayerPickupLineUp and PlayerParallelLineUp take three arguments"
                    );
                };
                let elapsed = invocation
                    .elapsed
                    .context("lineup requires elapsed input")?;
                let port = services
                    .lineup
                    .context("lineup selected-player state unavailable")?;
                let (_, reference) = port.reference(None)?;
                let angle = wrap(i32::from(reference.heading) + angle.integer()?);
                let target = [
                    reference.position[0].wrapping_add(x_offset.integer()?),
                    reference.position[1],
                    reference.position[2].wrapping_add(z_offset.integer()?),
                ];
                let result = match self {
                    Self::PickupLineUp { fallback } => {
                        let mut state = PickupState {
                            fallback: *fallback,
                        };
                        let result = pickup_mover(&mut *port, &mut state, elapsed, angle, target);
                        *fallback = state.fallback;
                        result
                    }
                    _ => plain_mover(&mut *port, angle, target),
                };
                if let Some(error) = port.failure() {
                    return Err(error);
                }
                result == 0
            }
            Self::LineUp {
                initialized,
                approach,
                fallback,
                reference,
            } => {
                use crate::script::lineup::{AlignState, approach_mover, close_mover};
                let [angle, x_offset, z_offset] = arguments else {
                    anyhow::bail!("PlayerLineUp takes three arguments");
                };
                let port = services
                    .lineup
                    .context("lineup selected-player state unavailable")?;
                if !*initialized {
                    // A strictly greater distance selects approach mode; equality closes in.
                    *approach = port.receiver_distance()? > port.lineup_distance()?;
                    *initialized = true;
                }
                let (index, aligned) = port.reference(if *approach { None } else { *reference })?;
                reference.get_or_insert(index);
                let mut state = AlignState {
                    fallback: *fallback,
                };
                let offsets = [x_offset.integer()?, z_offset.integer()?];
                let angle = angle.integer()?;
                let result = if *approach {
                    approach_mover(&mut *port, &mut state, angle, offsets, aligned)
                } else {
                    close_mover(&mut *port, &mut state, angle, offsets, aligned)
                };
                *fallback = state.fallback;
                if let Some(error) = port.failure() {
                    return Err(error);
                }
                let result = result?;
                port.set_lineup_result(result);
                result == 0
            }
            Self::ObjectPickupLineUp { fallback } => {
                use crate::script::lineup::{PickupState, pickup_mover};
                let [angle, x_offset, z_offset] = arguments else {
                    anyhow::bail!("ObjectPickupLineUp takes three arguments");
                };
                let elapsed = invocation
                    .elapsed
                    .context("lineup requires elapsed input")?;
                let port = services
                    .lineup
                    .context("ObjectPickupLineUp receiver alignment state unavailable")?;
                let reference = port.source_reference("ObjectPickupLineUp")?;
                let angle = wrap(i32::from(reference.heading) + angle.integer()?);
                let target = [
                    reference.position[0].wrapping_add(x_offset.integer()?),
                    reference.position[1],
                    reference.position[2].wrapping_add(z_offset.integer()?),
                ];
                let mut state = PickupState {
                    fallback: *fallback,
                };
                let result = pickup_mover(&mut *port, &mut state, elapsed, angle, target);
                *fallback = state.fallback;
                if let Some(error) = port.failure() {
                    return Err(error);
                }
                result == 0
            }
            Self::ReceiverLineUp {
                fallback,
                reference,
            } => {
                use crate::script::lineup::{AlignState, close_mover};
                let [angle, x_offset, z_offset] = arguments else {
                    anyhow::bail!("LineUp takes three arguments");
                };
                let port = services
                    .lineup
                    .context("LineUp receiver alignment state unavailable")?;
                let retained = match *reference {
                    Some(index) => index,
                    None => port.source_index("LineUp")?,
                };
                let (index, aligned) = port.reference(Some(retained))?;
                *reference = Some(index);
                let mut state = AlignState {
                    fallback: *fallback,
                };
                let result = close_mover(
                    &mut *port,
                    &mut state,
                    angle.integer()?,
                    [x_offset.integer()?, z_offset.integer()?],
                    aligned,
                );
                *fallback = state.fallback;
                if let Some(error) = port.failure() {
                    return Err(error);
                }
                result? == 0
            }
            Self::ObjectParallelLineUp => {
                let [angle, x_offset, z_offset] = arguments else {
                    anyhow::bail!("ObjectParallelLineUp takes three arguments");
                };
                let port = services
                    .lineup
                    .context("ObjectParallelLineUp receiver alignment state unavailable")?;
                let reference = port.source_reference("ObjectParallelLineUp")?;
                let angle = wrap(i32::from(reference.heading) + angle.integer()?);
                let target = [
                    reference.position[0].wrapping_add(x_offset.integer()?),
                    reference.position[1],
                    reference.position[2].wrapping_add(z_offset.integer()?),
                ];
                let result = crate::script::lineup::plain_mover(&mut *port, angle, target);
                if let Some(error) = port.failure() {
                    return Err(error);
                }
                result == 0
            }
            Self::ObjectLineUp => {
                let [angle, x_offset, z_offset] = arguments else {
                    anyhow::bail!("ObjectLineUp takes three arguments");
                };
                let port = services
                    .lineup
                    .context("ObjectLineUp receiver alignment state unavailable")?;
                let reference = port.source_reference("ObjectLineUp")?;
                let result = crate::script::lineup::object_mover(
                    &mut *port,
                    angle.integer()?,
                    [x_offset.integer()?, z_offset.integer()?],
                    reference,
                );
                if let Some(error) = port.failure() {
                    return Err(error);
                }
                result? == 0
            }
            Self::PlayerFaceTalk => {
                let [distance] = arguments else {
                    anyhow::bail!("PlayerFaceTalk takes one argument");
                };
                let port = services
                    .lineup
                    .context("PlayerFaceTalk selected-player state unavailable")?;
                let (target, heading) = port.talk_target(distance.integer()?)?;
                let result = crate::script::lineup::face_talk(&mut *port, target, heading);
                if let Some(error) = port.failure() {
                    return Err(error);
                }
                result? == 0
            }
            Self::PlayerFace => {
                use crate::script::alignment::{self, Facing};
                anyhow::ensure!(arguments.is_empty(), "PlayerFaceObj takes no arguments");
                let call = services
                    .player_face
                    .context("PlayerFaceObj selected-player state unavailable")?;
                let player = call.state.map_err(anyhow::Error::msg)?;
                let outcome = call.outcome;
                // A collision response left unresolved for over eight updates abandons the turn.
                if alignment::response_expired(player.response_present, player.response_age) {
                    outcome.return_state = Some(6);
                    return Ok(true);
                }
                let input = spatial
                    .as_mut()
                    .context("PlayerFaceObj requires receiver pose and frame_scale")?;
                let facing = alignment::face(alignment::FacingInput {
                    position: player.position,
                    heading: player.angles[1],
                    target: *input.position,
                    speed: player.speed,
                    frame_scale: input.frame_scale,
                    rate_attribute: player.rate_attribute,
                    holding_weapon: player.holding_weapon,
                    torch: player.torch,
                })?;
                let mut angles = player.angles;
                angles[2] = alignment::level_roll(angles[2], input.nominal_ticks);
                match facing {
                    Facing::Arrived => {
                        outcome.position =
                            Some([input.position[0], player.position[1], input.position[2]]);
                    }
                    Facing::Turning { heading, group } => {
                        angles[1] = heading;
                        if let Some(group) = group {
                            let animation = services
                                .player_animation
                                .as_mut()
                                .context("PlayerFaceObj selected-player animation unavailable")?;
                            let frame = match &mut animation.sounds {
                                Some(sounds) => animation.animation.request_with_sounds(
                                    group,
                                    0,
                                    animation.ticks,
                                    *sounds,
                                )?,
                                None => animation.animation.request(group, 0, animation.ticks)?,
                            };
                            *animation.frame = frame.or(*animation.frame);
                        }
                    }
                }
                outcome.angles = (angles != player.angles).then_some(angles);
                outcome.aligning = facing.ongoing();
                !facing.ongoing()
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
