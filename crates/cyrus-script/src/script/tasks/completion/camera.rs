//! Camera task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_camera(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        let is_cyrus = matches!(self, Self::LookCyrus { .. });
        Ok(match self {
            Self::ShowPlayer { counter } => {
                anyhow::ensure!(arguments.is_empty(), "showPlayer requires no arguments");
                let elapsed = invocation
                    .elapsed
                    .context("showPlayer requires elapsed input")?;
                let camera = services
                    .camera
                    .context("showPlayer camera session unavailable")?;
                let requested = camera.requested_slot.map_err(anyhow::Error::msg)?;
                !camera.state.show_player(requested, counter, elapsed)?
            }
            Self::ShowPlayerPan { counter, pan } => {
                let [duration] = arguments else {
                    anyhow::bail!("showPlayerPan requires one argument");
                };
                let elapsed = invocation
                    .elapsed
                    .context("showPlayerPan requires elapsed input")?;
                let camera = services
                    .camera
                    .context("showPlayerPan camera session unavailable")?;
                let requested_slot = camera.requested_slot.map_err(anyhow::Error::msg)?;
                let (bound_position, bound_angles) =
                    camera.bound_pose.map_err(anyhow::Error::msg)?;
                !camera.state.show_player_pan(
                    &crate::script::camera::PanInputs {
                        duration: duration.integer()?,
                        requested_slot,
                        bound_position,
                        bound_angles,
                    },
                    pan,
                    counter,
                    elapsed,
                )?
            }
            Self::ObjectPan {
                command,
                counter,
                pan,
            } => {
                let name = command.native();
                let [x, y, z, duration] = arguments else {
                    anyhow::bail!("{name} requires four arguments");
                };
                let elapsed = invocation
                    .elapsed
                    .context(format!("{name} requires elapsed input"))?;
                let camera = services
                    .camera
                    .context(format!("{name} camera session unavailable"))?;
                let target = if command.targets_selected_player() {
                    camera.selected_player
                } else {
                    camera.receiver
                }
                .map_err(anyhow::Error::msg)?;
                if command.targets_selected_player()
                    && target.holding_weapon.map_err(anyhow::Error::msg)?
                {
                    return Ok(true);
                }
                let (bound_position, bound_angles) = target.pose.map_err(anyhow::Error::msg)?;
                let inputs = crate::script::camera::ObjectPanInputs {
                    location: crate::script::camera::LocationInputs {
                        frame: command.frame(),
                        actor: target.actor,
                        camera_move: target.camera_move,
                        attributes: target.offsets,
                        arguments: [x.integer()?, y.integer()?, z.integer()?],
                        bound_position,
                        bound_angles,
                    },
                    pan_type: target.pan_type,
                    duration: duration.integer()?,
                };
                !if command.world() {
                    camera.state.show_object_world_pan(
                        &inputs,
                        target.selected_bound_position.map_err(anyhow::Error::msg)?,
                        pan,
                        counter,
                        elapsed,
                    )?
                } else {
                    camera
                        .state
                        .show_object_pan(&inputs, pan, counter, elapsed)?
                }
            }
            Self::ShowLocation { command, counter } => {
                let name = command.native();
                let [x, y, z] = arguments else {
                    anyhow::bail!("{name} requires three arguments");
                };
                let elapsed = invocation
                    .elapsed
                    .context(format!("{name} requires elapsed input"))?;
                let camera = services
                    .camera
                    .context(format!("{name} camera session unavailable"))?;
                let target = if command.targets_selected_player() {
                    camera.selected_player
                } else {
                    camera.receiver
                }
                .map_err(anyhow::Error::msg)?;
                if command.targets_selected_player()
                    && target.holding_weapon.map_err(anyhow::Error::msg)?
                {
                    return Ok(true);
                }
                let (bound_position, bound_angles) = target.pose.map_err(anyhow::Error::msg)?;
                let arguments = [x.integer()?, y.integer()?, z.integer()?];
                // Only the first invocation runs the pose update; later ones never reach it.
                let update = camera.update;
                let mut run = |state: &mut crate::script::camera::CameraState| match update {
                    Some(service) => service.update(state, bound_position, bound_angles),
                    None => anyhow::bail!("{name} camera update service unavailable"),
                };
                !if command.world() {
                    camera.state.show_world_location(
                        &crate::script::camera::WorldLocationInputs {
                            actor: target.actor,
                            camera_move: target.camera_move,
                            attributes: target.offsets,
                            position: arguments,
                            bound_position,
                            actor_position: bound_position,
                            bound_angles,
                        },
                        counter,
                        elapsed,
                        &mut run,
                    )?
                } else {
                    camera.state.show_location(
                        &crate::script::camera::LocationInputs {
                            frame: command.frame(),
                            actor: target.actor,
                            camera_move: target.camera_move,
                            attributes: target.offsets,
                            arguments,
                            bound_position,
                            bound_angles,
                        },
                        counter,
                        elapsed,
                        &mut run,
                    )?
                }
            }
            Self::Look { counter, look } | Self::LookCyrus { counter, look } => {
                let name = if is_cyrus {
                    Native::LookCyrus
                } else {
                    Native::LookObj
                };
                let [duration] = arguments else {
                    anyhow::bail!("{name} requires one argument");
                };
                let elapsed = invocation
                    .elapsed
                    .with_context(|| format!("{name} requires elapsed input"))?;
                let camera = services
                    .camera
                    .with_context(|| format!("{name} camera session unavailable"))?;
                let target = if is_cyrus {
                    camera.selected_player
                } else {
                    camera.receiver
                }
                .map_err(anyhow::Error::msg)?;
                let (bound_position, bound_angles) = target.pose.map_err(anyhow::Error::msg)?;
                !camera.state.look_at(
                    &crate::script::camera::LookInputs {
                        actor: target.actor,
                        camera_move: target.camera_move,
                        attributes: target.offsets,
                        bound_position,
                        bound_angles,
                        duration: duration.integer()?,
                    },
                    look,
                    counter,
                    elapsed,
                )?
            }
            Self::ObjectRotation => {
                anyhow::ensure!(arguments.len() == 4, "showObjRot requires four arguments");
                true
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
