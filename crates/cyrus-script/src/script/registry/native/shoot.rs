//! Shoot native-call dispatch.
use super::super::{Argument, CallKind, CallResult, GameHost, Native, Result, wrap};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_shoot(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // shoot(name, x, y, z, pitch, yaw, roll) creates an object of the named script at the
        // receiver's position plus the offset rotated by the receiver's orientation, with the
        // given orientation. shootPlayer(name, x, y, z) aims the object at the selected player:
        // yaw is the bearing of the raw difference, pitch the elevation of the difference
        // normalized to length 65536, roll zero. The receiver becomes the object's parent record.
        if kind == CallKind::Function
            && (function == Native::Shoot || function == Native::ShootPlayer)
        {
            let player_form = function == Native::ShootPlayer;
            anyhow::ensure!(
                arguments.len() == if player_form { 4 } else { 7 },
                "{function}: wrong argument count"
            );
            let Argument::String(name) = &arguments[0] else {
                anyhow::bail!("{function} requires a script name")
            };
            let id = self.registry.id(self.index)?;
            let local = self.registry.spatial(id)?.local;
            let offset = [
                arguments[1].integer()?,
                arguments[2].integer()?,
                arguments[3].integer()?,
            ];
            // The original builds this rotation by turning yaw by minus the heading and pitch and
            // roll by the plain angles; actor orientation negates all three, so this is the
            // actor-orientation builder with pitch and roll negated.
            let rotated = cyrus_world::transform::Matrix::orientation([
                wrap(local.angles[0].wrapping_neg()) as u16,
                local.wrapped_angles()[1],
                wrap(local.angles[2].wrapping_neg()) as u16,
            ])
            .transform(offset)?;
            let position =
                std::array::from_fn(|axis| rotated[axis].wrapping_add(local.position[axis]));
            let angles = if player_form {
                let player = self
                    .selected_player()
                    .with_context(|| format!("{function}: selected player unavailable"))?;
                let target = self
                    .registry
                    .spatial(self.registry.id(self.registry.resolve(player)?)?)?
                    .local
                    .position;
                let difference =
                    std::array::from_fn(|axis| target[axis].wrapping_sub(position[axis]));
                let unit = crate::script::spatial::normalized_direction(difference);
                let horizontal =
                    cyrus_collision::distance::rounded_distance([unit[0], 0, unit[2]])?;
                // The original's table bearing is the negation of the shared query bearing.
                let elevation = i32::from(wrap(
                    crate::script::spatial::query_bearing(horizontal, unit[1]).wrapping_neg(),
                ));
                [
                    wrap(elevation.wrapping_sub(0x200)) as u16,
                    crate::script::spatial::query_bearing(difference[0], difference[2]),
                    0,
                ]
            } else {
                [
                    wrap(arguments[4].integer()?) as u16,
                    wrap(arguments[5].integer()?) as u16,
                    wrap(arguments[6].integer()?) as u16,
                ]
            };
            self.registry.create_object(id, name, position, angles)?;
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
