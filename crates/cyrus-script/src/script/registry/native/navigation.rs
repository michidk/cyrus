//! Navigation native-call dispatch.
use super::super::{
    Argument, Attribute, CallResult, GameHost, Native, Result, ScriptActorId, point_tasks,
};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_node_map_control(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if let Some(enabled) = match function {
            Native::EnableNodeMaps => Some(true),
            Native::DisableNodeMaps => Some(false),
            _ => None,
        } {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            self.registry.set_all_walk_maps_enabled(enabled)?;
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_wander(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if self.registry.actors[self.index]
            .runtime_identifier
            .is_none()
            && [
                Native::Wander,
                Native::MoveByAxis,
                Native::MoveAlongAxis,
                Native::WanderToLocation,
                Native::SMoveByAxis,
                Native::MoveObjectAxis,
                Native::MoveToLocation,
                Native::RotateByAxis,
                Native::SRotateByAxis,
                Native::RotateToAxis,
                Native::Move,
                Native::Rotate,
                Native::SMove,
                Native::SRotate,
                Native::SMoveAlongAxis,
                Native::MoveMarker,
                Native::MoveLocation,
                Native::MovePosition,
                Native::TestTask,
                Native::MoveNodeMarker,
                Native::MoveNodeLocation,
                Native::MoveNodePosition,
            ]
            .contains(&function)
        {
            anyhow::bail!("{function}: separate movement/relocation owner unavailable");
        }

        if function == Native::Wander {
            let [radius, tolerance] = arguments else {
                anyhow::bail!("Wander requires two arguments")
            };
            let radius = radius.integer()?;
            let tolerance = tolerance.integer()?;
            let home = self.registry.actors[self.index]
                .placement()
                .context("Wander home unavailable for standalone actor")?
                .map_position;
            let [x, y, z] = home.map(i32::try_from);
            let home = [x?, y?, z?];
            if self
                .walk_map_count
                .context("Wander navigation not prepared")?
                != 0
            {
                let id = ScriptActorId {
                    generation: self.generation,
                    index: self.index,
                };
                let random = self
                    .random
                    .as_deref_mut()
                    .context("Wander shared engine random stream unavailable")?;
                let sounds = self.sounds.as_mut().map(|sink| {
                    &mut **sink
                        as &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>
                });
                let result = self
                    .registry
                    .wander_through_maps(id, home, radius, tolerance, self.inputs, random, sounds)
                    .with_context(|| format!("Wander through walk-node maps (radius {radius})"))?;
                return Ok(Some(CallResult::Complete(result)));
            }
            let frame_scale = self
                .inputs
                .frame_scale
                .context("Wander requires frame_scale")?;
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .context("Wander spatial state not prepared")?
                .actor_mut(self.index)?;
            let random = self
                .random
                .as_deref_mut()
                .context("Wander shared engine random stream unavailable")?;
            let result = crate::script::navigation::wander_without_maps(
                home,
                radius,
                frame_scale,
                spatial,
                random,
            )?;
            self.registry.actors[self.index].attributes[Attribute::NodeInUse] = 1;
            return Ok(Some(CallResult::Complete(result)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_node_marker(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::AtNodeMarker {
            let [marker, distance] = arguments else {
                anyhow::bail!("AtNodeMarker requires two arguments")
            };
            let position = self
                .registry
                .spatial
                .as_ref()
                .context("AtNodeMarker spatial state not prepared")?
                .actor(self.index)?
                .local
                .position;
            let result = self
                .registry
                .markers
                .as_ref()
                .context("AtNodeMarker markers not prepared")?
                .contains(marker.integer()?, position, distance.integer()?)?;
            return Ok(Some(CallResult::Complete(i32::from(result))));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_node_position(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // AtNodeLocation compares a receiver location with the receiver's own position; the height
        // limit is half the actor's total Y extent (positive plus negative extent).
        // AtNodePosition(x, y, z, distance): the wrapping differences between the point and the
        // receiver's local position (the position AtNodeLocation reads) go through the shared
        // rounded distance helper unshifted, with no vertical adjustment; the signed result is at
        // most the full signed distance. An overflowing length converts to the integer-indefinite
        // value and is compared like any other signed result.
        if function == Native::AtNodePosition {
            let [x, y, z, distance] = arguments else {
                anyhow::bail!("AtNodePosition requires four arguments")
            };
            let point = [x.integer()?, y.integer()?, z.integer()?];
            let position = self
                .registry
                .spatial
                .as_ref()
                .context("AtNodePosition: spatial state not prepared")?
                .actor(self.index)?
                .local
                .position;
            let result = cyrus_collision::distance::rounded_distance_or_indefinite(
                std::array::from_fn(|axis| point[axis].wrapping_sub(position[axis])),
            );
            return Ok(Some(CallResult::Complete(i32::from(
                result <= distance.integer()?,
            ))));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_node_location(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::AtNodeLocation {
            let [location, distance] = arguments else {
                anyhow::bail!("AtNodeLocation requires two arguments")
            };
            let id = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            let shape = self.registry.ordinary_collision_shape(id)?;
            let total_height = shape.positive_extent[1].wrapping_add(shape.negative_extent[1]);
            let position = self
                .registry
                .spatial
                .as_ref()
                .context("AtNodeLocation spatial state not prepared")?
                .actor(self.index)?
                .local
                .position;
            let target = self.receiver_location("node location", location.integer()?)?;
            return Ok(Some(CallResult::Complete(i32::from(
                point_tasks::within_location(position, target, total_height, distance.integer()?)?,
            ))));
        }
        Ok(None)
    }
}
