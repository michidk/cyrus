//! Spatial native-call dispatch.
use super::super::{
    Argument, Attribute, CallKind, CallResult, GameHost, Native, Result, ScriptActorId, wrap,
};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_spatial_transform(
        &mut self,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        if function == Native::ResetRotation {
            anyhow::ensure!(
                arguments.len() == 3,
                "ResetRotation requires three arguments"
            );
            let mut axes = [false; 3];
            for (axis, argument) in arguments.iter().enumerate() {
                axes[axis] = argument.integer()? != 0;
            }
            self.registry
                .spatial
                .as_mut()
                .context("ResetRotation: spatial state not prepared")?
                .actor_mut(self.index)?
                .reset_rotation(axes)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // SetStartPosition(x, y, z) restores the flagged local-position axes from the saved start and
        // returns zero; unlike ResetPosition it leaves the position history, view origin and
        // transform alone.
        if function == Native::SetStartPosition {
            anyhow::ensure!(
                arguments.len() == 3,
                "SetStartPosition requires three arguments"
            );
            let mut axes = [false; 3];
            for (axis, argument) in arguments.iter().enumerate() {
                axes[axis] = argument.integer()? != 0;
            }
            if axes.contains(&true) {
                self.registry
                    .spatial
                    .as_mut()
                    .context("SetStartPosition: spatial state not prepared")?
                    .actor_mut(self.index)?
                    .restore_start_position(axes)?;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        // FixSlaveAngle(axis, angle) writes one local angle component of a slave from its master
        // group's angle and returns zero.
        if function == Native::FixSlaveAngle {
            let [axis, angle] = arguments else {
                anyhow::bail!("FixSlaveAngle requires two arguments")
            };
            let (axis, angle) = (axis.integer()?, angle.integer()?);
            let id = self.registry.id(self.index)?;
            self.registry.fix_slave_angle(id, axis, angle)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::AttachMe {
            let [group] = arguments else {
                anyhow::bail!("AttachMe requires one argument")
            };
            let group = group.integer()?;
            let attributes = &self.registry.actors[self.index].attributes;
            if group == 0
                || group == i32::from(attributes[Attribute::Slave])
                || group == i32::from(attributes[Attribute::Master])
            {
                return Ok(Some(CallResult::Complete(0)));
            }
            anyhow::ensure!(
                attributes[Attribute::Master] == 0,
                "AttachMe: master receiver {} requires separate ordinary and master family records",
                self.index
            );
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .context("AttachMe: spatial state not prepared")?;
            if attributes[Attribute::Slave] != 0
                && spatial.actor_mut(self.index)?.detach_from_family()?
            {
                self.registry.actors[self.index].attributes[Attribute::Slave] = 0;
                self.registry.map_updates.register(self.index)?;
            }
            let mut targets =
                self.registry
                    .actors
                    .iter()
                    .enumerate()
                    .filter_map(|(index, actor)| {
                        (i32::from(actor.attributes[Attribute::Master]) == group).then_some(index)
                    });
            if let Some(target) = targets.next() {
                anyhow::ensure!(
                    targets.next().is_none(),
                    "AttachMe: group {group} has multiple masters; family traversal order unavailable"
                );
                let no_rotation =
                    self.registry.actors[self.index].attributes[Attribute::NoSlaveRot] != 0;
                spatial.attach_ordinary(self.index, target, no_rotation)?;
                self.registry.actors[self.index].attributes[Attribute::Slave] = group as u8;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        // DetachMe: an unattached receiver is unchanged; an attached master only loses its slave
        // byte; an attached non-master leaves its family, keeps its effective pose as its local
        // pose, loses its slave byte and registers a map update. A receiver with no family record
        // is unchanged. Returns zero.
        if function == Native::DetachMe {
            anyhow::ensure!(arguments.is_empty(), "DetachMe requires no arguments");
            let attributes = &self.registry.actors[self.index].attributes;
            if attributes[Attribute::Slave] == 0 {
                return Ok(Some(CallResult::Complete(0)));
            }
            if attributes[Attribute::Master] != 0 {
                self.registry.actors[self.index].attributes[Attribute::Slave] = 0;
                return Ok(Some(CallResult::Complete(0)));
            }
            let detached = self
                .registry
                .spatial
                .as_mut()
                .context("DetachMe: spatial state not prepared")?
                .actor_mut(self.index)?
                .detach_from_family()?;
            if detached {
                self.registry.actors[self.index].attributes[Attribute::Slave] = 0;
                self.registry.map_updates.register(self.index)?;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        // SetPosition(x, y, z) and SetLocation(location) write the local position and its
        // history unchanged, move the selected player's shared reference triple (cyrus's
        // published camera position, **Inferred** as for ResetPosition) by the same displacement
        // and register the actor for the deferred map update; SetRotation(x, y, z) masks and
        // writes the angles and rebuilds the matrix without registering. All return zero.
        // SetObjectLocation(location, x, y, z): the selected location's coordinates are copied into
        // the local position on the axes whose flag is nonzero; the receiver is registered for the
        // deferred map update even when every flag is zero. Returns zero.
        if function == Native::SetObjectLocation {
            let [location, x, y, z] = arguments else {
                anyhow::bail!("SetObjectLocation requires four arguments")
            };
            let flags = [x.integer()?, y.integer()?, z.integer()?];
            self.registry
                .spatial
                .as_mut()
                .context("SetObjectLocation: spatial state not prepared")?
                .actor_mut(self.index)?
                .set_location_axes(location.integer()?, flags)?;
            self.registry.map_updates.register(self.index)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::SetPosition || function == Native::SetLocation {
            let (name, expected) = if function == Native::SetPosition {
                ("SetPosition", 3)
            } else {
                ("SetLocation", 1)
            };
            anyhow::ensure!(
                arguments.len() == expected,
                "{name} requires {expected} argument{}",
                if expected == 1 { "" } else { "s" }
            );
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .with_context(|| format!("{name}: spatial state not prepared"))?
                .actor_mut(self.index)?;
            let position = if function == Native::SetPosition {
                [
                    arguments[0].integer()?,
                    arguments[1].integer()?,
                    arguments[2].integer()?,
                ]
            } else {
                spatial.location_position(arguments[0].integer()?)?
            };
            let change = spatial.set_position(position);
            if self.registry.selected_player_index() == Some(self.index)
                && let Some(camera) = self.registry.camera.as_mut()
            {
                for (axis, delta) in change.into_iter().enumerate() {
                    camera.current.position[axis] =
                        camera.current.position[axis].wrapping_add(delta);
                }
            }
            self.registry.map_updates.register(self.index)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // PlayerSnap(angle, x_offset, z_offset): the selected player's local X and Z are copied
        // directly from the receiver's. The arguments only feed a temporary rotation whose result
        // the original discards; Y, heading and both history fields stay, nothing is registered
        // and no position-update or collision service runs. Returns zero.
        if function == Native::PlayerSnap {
            anyhow::ensure!(arguments.len() == 3, "PlayerSnap requires 3 arguments");
            for argument in arguments {
                argument.integer()?;
            }
            let player = self
                .registry
                .selected_player_index()
                .context("PlayerSnap: selected player unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .context("PlayerSnap: spatial state not prepared")?;
            let source = spatial.actor(self.index)?.local.position;
            let target = &mut spatial.actor_mut(player)?.local.position;
            target[0] = source[0];
            target[2] = source[2];
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::OffsetLocation {
            let [source, x, y, z] = arguments else {
                anyhow::bail!("OffsetLocation requires four arguments")
            };
            let source = source.actor().context("OffsetLocation source")?;
            let offset = [x.integer()?, y.integer()?, z.integer()?];
            let base = self
                .registry
                .query_position(source)
                .context("OffsetLocation source position")?;
            let receiver = self.registry.id(self.index)?;
            // Receiver resolution precedes transformation even though its result is discarded.
            self.registry
                .query_position(receiver)
                .context("OffsetLocation receiver position")?;
            let transformed = self
                .registry
                .spatial(source)?
                .query_pose()?
                .matrix
                .transform(offset)
                .context("OffsetLocation source matrix")?;
            self.registry
                .spatial
                .as_mut()
                .context("OffsetLocation: spatial state not prepared")?
                .actor_mut(self.index)?
                .local
                .position = std::array::from_fn(|axis| base[axis].wrapping_add(transformed[axis]));
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::SetRotation {
            anyhow::ensure!(arguments.len() == 3, "SetRotation requires 3 arguments");
            let angles = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            self.registry
                .spatial
                .as_mut()
                .context("SetRotation: spatial state not prepared")?
                .actor_mut(self.index)?
                .set_rotation(angles)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::ResetPosition {
            anyhow::ensure!(
                arguments.len() == 3,
                "ResetPosition requires three arguments"
            );
            let mut axes = [false; 3];
            for (axis, argument) in arguments.iter().enumerate() {
                axes[axis] = argument.integer()? != 0;
            }
            let change = self
                .registry
                .spatial
                .as_mut()
                .context("ResetPosition: spatial state not prepared")?
                .actor_mut(self.index)?
                .reset_position(axes)?;
            // The original moves its view-origin global, which cyrus keeps as the published
            // camera position, by the selected player's change (**Inferred** mapping).
            if self.registry.selected_player_index() == Some(self.index)
                && let Some(camera) = self.registry.camera.as_mut()
            {
                for (axis, delta) in change.into_iter().enumerate() {
                    camera.current.position[axis] =
                        camera.current.position[axis].wrapping_add(delta);
                }
            }
            if self.registry.spatial(id)?.parent.is_none() {
                self.registry.refresh_transform(id)?;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        // PointAt turns the receiver to face the shared native-command source actor: the negated shared query bearing of the truncated stored model coordinate
        // differences, written straight to the receiver's yaw without a matrix refresh.
        if function == Native::PointAt {
            anyhow::ensure!(arguments.is_empty(), "PointAt requires no arguments");
            let target = self.native_source_model_position("PointAt")?;
            let stored = |index: usize, role: &str| {
                self.registry.actors[index]
                    .model
                    .position
                    .with_context(|| format!("PointAt: {role} stored model position unavailable"))
            };
            let own = stored(self.index, "receiver")?;
            // Extended-precision differences truncate toward zero; an unrepresentable one converts
            // to the integer-indefinite value.
            let truncate = |axis: usize| {
                let difference = (f64::from(target[axis]) - f64::from(own[axis])).trunc();
                if (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&difference) {
                    difference as i32
                } else {
                    i32::MIN
                }
            };
            // The original negates its bearing helper's angle; `query_bearing` is that helper negated.
            let yaw = crate::script::spatial::query_bearing(truncate(0), truncate(2));
            self.registry
                .spatial
                .as_mut()
                .context("PointAt: spatial state not prepared")?
                .actor_mut(self.index)?
                .local
                .angles[1] = i32::from(yaw);
            return Ok(Some(CallResult::Complete(0)));
        }

        // MoveAboveMe places the receiver relative to the shared native-command source actor: the source
        // position plus the offset turned by the source's matrix, lowered by the receiver's
        // vertical extent and raised by the installed standing height. The selected player is
        // placed through the pending placement; any other receiver is written directly and
        // registered in the changed-actor list.
        if function == Native::MoveAboveMe {
            anyhow::ensure!(arguments.len() == 4, "MoveAboveMe requires four arguments");
            anyhow::ensure!(
                self.native_source_attribute("MoveAboveMe", Attribute::Slave)? == 0,
                "MoveAboveMe world {}: original diagnostic 137: slave source",
                self.registry.world
            );
            let offset = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            let angle = arguments[3].integer()?;
            let standing_height = self
                .inputs
                .player_collision
                .map(|source| source.scene.settings.standing_height)
                .or(self.registry.standing_height)
                .context("MoveAboveMe: standing height unavailable")?;
            let rotated = self
                .native_source_local("MoveAboveMe")?
                .matrix
                .transform(offset)?;
            let base = self.native_source_position("MoveAboveMe")?;
            let extent = self.registry.ordinary_collision_shape(id)?.positive_extent[1];
            let mut target: [i32; 3] =
                std::array::from_fn(|axis| base[axis].wrapping_add(rotated[axis]));
            target[1] = target[1]
                .wrapping_sub(extent.wrapping_shl(8))
                .wrapping_add(standing_height.wrapping_shl(8));
            if self.registry.selected_player_index() == Some(self.index) {
                self.registry.pending_player_placement = Some((target, wrap(angle) as u16));
            } else {
                let receiver = self
                    .registry
                    .spatial
                    .as_mut()
                    .context("MoveAboveMe: spatial state not prepared")?
                    .actor_mut(self.index)?;
                receiver.local.position = target;
                receiver.local.angles[1] = angle & 2047;
                self.registry.map_updates.register(self.index)?;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::MovePlayerAboveMe {
            anyhow::ensure!(
                arguments.len() == 4,
                "MovePlayerAboveMe requires four arguments"
            );
            let offset = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            let yaw = arguments[3].integer()?;
            let player = self
                .registry
                .selected_player()
                .context("MovePlayerAboveMe: selected player unavailable")?;
            let standing_height = self
                .registry
                .standing_height
                .context("MovePlayerAboveMe: standing height unavailable")?;
            // The offset turns with the receiver, the player stands on the result, and its
            // vertical extent and the standing height lower the stored Y.
            let rotated = self.registry.spatial(id)?.local.matrix.transform(offset)?;
            let base = self.registry.query_position(id)?;
            let extent = self
                .registry
                .ordinary_collision_shape(player)?
                .positive_extent[1];
            let mut position: [i32; 3] =
                std::array::from_fn(|axis| base[axis].wrapping_add(rotated[axis]));
            position[1] = position[1]
                .wrapping_sub(extent.wrapping_shl(8))
                .wrapping_sub(standing_height.wrapping_shl(8));
            self.registry.pending_player_placement = Some((position, wrap(yaw) as u16));
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_reload_rotation(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::ReloadRotation {
            anyhow::ensure!(arguments.is_empty(), "ReloadRotation requires no arguments");
            self.registry
                .spatial
                .as_mut()
                .context("ReloadRotation: spatial state not prepared")?
                .actor_mut(self.index)?
                .reset_rotation([false; 3])?;
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_family_membership(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // IsMaster and IsSlave test attr_master and attr_slave; TrueFunction ignores both arguments.
        if function == Native::IsMaster || function == Native::IsSlave {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let attribute = if function == Native::IsMaster {
                Attribute::Master
            } else {
                Attribute::Slave
            };
            return Ok(Some(CallResult::Complete(i32::from(
                self.registry.actors[self.index].attributes[attribute] != 0,
            ))));
        }

        // EnableSlaves, DisableSlaves, KillSlaves, HideSlaves, ShowSlaves and DetachSlaves search the
        // family tree for the receiver, but the
        // shared lookup never selects a node. For a finite acyclic family they change nothing,
        // leave the global slave scheduling alone and return zero. Cyclic families are outside
        // the contract.
        if [
            Native::EnableSlaves,
            Native::DisableSlaves,
            Native::KillSlaves,
            Native::HideSlaves,
            Native::ShowSlaves,
            Native::DetachSlaves,
        ]
        .contains(&function)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_distance_from_start(
        &mut self,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        // DistanceFromStart(axis): saved start minus the local position on axis 0..2 in map units;
        // any other axis returns zero before the coordinates are read.
        if function == Native::DistanceFromStart {
            let [axis] = arguments else {
                anyhow::bail!("DistanceFromStart requires one argument")
            };
            let value = match usize::try_from(axis.integer()?) {
                Ok(axis) if axis < 3 => self.registry.spatial(id)?.start_displacement(axis)?,
                _ => 0,
            };
            return Ok(Some(CallResult::Complete(value)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_spatial_region(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // IsWithinView(): the receiver's position (the effective one while it has a slave byte, its
        // local one otherwise) and the shared camera origin, each shifted to map units, go through
        // the shared rounded distance helper with a zero vertical difference; the result is one
        // when the distance is at most the installed view distance. The shared enable word is
        // treated as always set.
        if function == Native::IsWithinView {
            anyhow::ensure!(arguments.is_empty(), "IsWithinView requires no arguments");
            let threshold = self
                .registry
                .view_distance
                .context("IsWithinView: installed view distance unavailable")?;
            let id = self.registry.id(self.index)?;
            let spatial = self.registry.spatial(id)?;
            let position = if self.registry.actors[self.index].attributes[Attribute::Slave] != 0 {
                spatial
                    .effective
                    .context("IsWithinView: attached pose requires an explicit family refresh")?
                    .position
            } else {
                spatial.local.position
            };
            let camera = self
                .registry
                .camera
                .as_ref()
                .context("IsWithinView: camera state unavailable")?
                .current
                .position;
            let distance = cyrus_collision::distance::rounded_distance([
                (camera[0] >> 8).wrapping_sub(position[0] >> 8),
                0,
                (camera[2] >> 8).wrapping_sub(position[2] >> 8),
            ])?;
            return Ok(Some(CallResult::Complete(i32::from(distance <= threshold))));
        }

        // InCircle(x, z, radius): the receiver's query position (attached, linked or local) minus
        // (x, z) with 32-bit wrapping and a zero vertical difference goes through the shared rounded
        // distance helper; the result is one when it is at most the signed radius.
        if function == Native::InCircle {
            let [x, z, radius] = arguments else {
                anyhow::bail!("InCircle requires three arguments")
            };
            let (x, z, radius) = (x.integer()?, z.integer()?, radius.integer()?);
            let position = self.fighting_position(self.index)?;
            let distance = cyrus_collision::distance::rounded_distance([
                position[0].wrapping_sub(x),
                0,
                position[2].wrapping_sub(z),
            ])?;
            return Ok(Some(CallResult::Complete(i32::from(distance <= radius))));
        }

        if function == Native::InRectangle {
            let [x1, z1, x2, z2] = arguments else {
                anyhow::bail!("InRectangle requires four arguments")
            };
            let (x1, z1, x2, z2) = (x1.integer()?, z1.integer()?, x2.integer()?, z2.integer()?);
            let position = self.fighting_position(self.index)?;
            let inside = |value: i32, a: i32, b: i32| (a.min(b)..=a.max(b)).contains(&value);
            return Ok(Some(CallResult::Complete(i32::from(
                inside(position[0], x1, x2) && inside(position[2], z1, z2),
            ))));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_attachment_and_turn(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // Offset attaches the calling actor (the original's current script actor, which differs from
        // the receiver when the call is routed) to the receiver; UnOffset detaches the receiver.
        // PlayerOffset places the selected player relative to the receiver once.
        if kind == CallKind::Function && function == Native::Offset {
            let [x, y, z] = arguments else {
                anyhow::bail!("Offset requires three arguments")
            };
            let caller = self.registry.routed_caller.unwrap_or(self.index);
            self.registry.offset_actor(
                caller,
                self.index,
                [x.integer()?, y.integer()?, z.integer()?],
            )?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::UnOffset {
            anyhow::ensure!(arguments.is_empty(), "UnOffset requires no arguments");
            self.registry.unoffset_actor(self.index)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // TurnToAngle takes one ordinary turn step toward a heading and returns the signed
        // difference (zero when it arrived). An argument equal to the stored yaw returns first,
        // before any weapon query; the ordinary turn clamps the step to eight itself, so unlike
        // the FaceAngle task there is no zero-turn-rate completion.
        if kind == CallKind::Function && function == Native::TurnToAngle {
            let [angle] = arguments else {
                anyhow::bail!("TurnToAngle requires one argument")
            };
            let angle = angle.integer()?;
            let id = self.registry.id(self.index)?;
            if self.registry.spatial(id)?.local.angles[1] == angle {
                return Ok(Some(CallResult::Complete(0)));
            }
            let inputs = self.registry.point_facing_inputs(id, self.inputs)?;
            let sounds = self
                .sounds
                .as_mut()
                .context("TurnToAngle requires synchronous sounds")?;
            // Truncation keeps the heading modulo 2048, which is all the turn reads.
            let difference =
                self.registry
                    .turn_ordinary_angle(id, angle as u16, inputs, &mut **sounds)?;
            return Ok(Some(CallResult::Complete(difference)));
        }

        // TurnToPos(x, z) snaps onto a point within the facing threshold (returning zero) or turns
        // one ordinary step toward its bearing and returns the turn's value; the same owner as
        // the FacePos task, without that task's zero-turn-rate completion.
        if kind == CallKind::Function && function == Native::TurnToPos {
            let [x, z] = arguments else {
                anyhow::bail!("TurnToPos requires two arguments")
            };
            let point = [x.integer()?, 0, z.integer()?];
            let id = self.registry.id(self.index)?;
            let inputs = self.registry.point_facing_inputs(id, self.inputs)?;
            let sounds = self
                .sounds
                .as_mut()
                .context("TurnToPos requires synchronous sounds")?;
            let value = self
                .registry
                .turn_ordinary_point(id, point, inputs, &mut **sounds)?;
            return Ok(Some(CallResult::Complete(value)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_reload_position(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // ReloadPosition(): the local position is copied unchanged into the previous-position
        // history and zero is returned. The original also refreshes the model's single-precision
        // coordinates from the shifted position; cyrus positions models from the local position at
        // publication, so no separate store exists (**Inferred**).
        if function == Native::ReloadPosition {
            anyhow::ensure!(arguments.is_empty(), "ReloadPosition requires no arguments");
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .context("ReloadPosition: spatial state not prepared")?
                .actor_mut(self.index)?;
            spatial.previous_local_position = spatial.local.position;
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_spatial_distance(
        &mut self,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        // AtPos snaps the receiver onto the point when it is strictly within one and a half
        // movement-speed steps, registering it in the changed-actor list after the position stores.
        if function == Native::AtPos {
            let [x, y, z] = arguments else {
                anyhow::bail!("AtPos requires three arguments")
            };
            let point = [x.integer()?, y.integer()?, z.integer()?];
            return Ok(Some(CallResult::Complete(i32::from(
                self.registry.arrive_ordinary_point(id, point)?,
            ))));
        }

        if function == Native::DistanceFromLocation {
            let [location] = arguments else {
                anyhow::bail!("DistanceFromLocation requires one argument")
            };
            return Ok(Some(CallResult::Complete(
                self.registry
                    .spatial(id)?
                    .location_distance_from_local(location.integer()?)?,
            )));
        }

        if function == Native::DistanceToLocation {
            let [location] = arguments else {
                anyhow::bail!("DistanceToLocation requires one argument")
            };
            let mut inputs = self
                .inputs
                .player_query
                .context("player query inputs unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(self.index)?;
            inputs.linked_position = Some(spatial.linked_position);
            return Ok(Some(CallResult::Complete(
                spatial.location_distance(location.integer()?, inputs)?,
            )));
        }

        if function == Native::PlayerDistance
            || function == Native::AcuratePlayerDistance
            || function == Native::PlayerFacing
            || function == Native::PlayerLooking
            || function == Native::PlayerArc
            || function == Native::PlayerArcTan
        {
            let mut inputs = self
                .inputs
                .player_query
                .context("player query inputs unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(self.index)?;
            inputs.linked_position = Some(spatial.linked_position);
            return Ok(Some(CallResult::Complete(
                spatial
                    .query(function, arguments, inputs)?
                    .context("spatial query absent")?,
            )));
        }
        Ok(None)
    }
}
