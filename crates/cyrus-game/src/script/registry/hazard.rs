//! The nonplayer step hazard test follows the original's destination surface classification.
use super::{ScriptActorId, ScriptWorld, obstacle::OrdinaryObstacleQuery};
use anyhow::{Context, Result};

/// Receiver-relative destination check. Forward is WalkForward's admission;
/// the other directions follow preservation's fight-spacing admission rules.
#[derive(Clone, Copy, Debug)]
pub enum OrdinaryStepDirection {
    Forward,
    Left,
    Right,
    Backward,
}

/// Surface class of a contact normal's Y component (scale 256, negative up): 4 for floors,
/// 2 for steep slopes, 8 for ceilings and 1 otherwise.
pub(super) fn surface_class(normal_y: i32) -> u8 {
    if normal_y <= -161 {
        4
    } else if normal_y >= 161 {
        8
    } else if normal_y <= -42 {
        2
    } else {
        1
    }
}

/// One candidate surface: its contact height, normal and distance metric.
#[derive(Clone, Copy)]
struct Surface {
    y: i32,
    normal: [i32; 3],
    metric: i32,
}

const UNFOUND_Y: i32 = 9_999_999;
const UNFOUND_METRIC: i32 = 999_999;

impl ScriptWorld {
    /// Run WalkForward's body, hazard and support admission in order. Failed
    /// geometry marks the transient blocked-step byte; rope refusal and the
    /// lifecycle bypass preserve it. Positions are local Q8 coordinates.
    pub fn walk_forward_step_clear(
        &mut self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        self.ordinary_step_clear(
            id,
            OrdinaryStepDirection::Forward,
            query,
            environment,
            sounds,
        )
    }

    /// Check a receiver-relative destination without translating the receiver.
    /// Body, hazard and support queries short-circuit in that order. Refusal
    /// retains existing status bits and marks only the requested direction.
    pub fn ordinary_step_clear(
        &mut self,
        id: ScriptActorId,
        direction: OrdinaryStepDirection,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        let control = self.actors[index]
            .control
            .as_ref()
            .context("directional step control unavailable")?;
        if control.rope_attached {
            return Ok(false);
        }
        if control.death_selector == 7 && self.actors[index].attributes[1] & 1 == 0 {
            return Ok(true);
        }
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let spatial = self.spatial(id)?;
        let shape = self.ordinary_collision_shape(id)?;
        let attributes = &self.actors[index].attributes;
        let (axis, attribute, extent, reverse) = match direction {
            OrdinaryStepDirection::Forward => (2, 12, shape.positive_extent[2], false),
            OrdinaryStepDirection::Left => (0, 10, shape.negative_extent[0], true),
            OrdinaryStepDirection::Right => (0, 10, shape.positive_extent[0], false),
            OrdinaryStepDirection::Backward => (2, 13, shape.negative_extent[2], true),
        };
        let amount = i32::from(attributes[attribute])
            .wrapping_add(extent)
            .wrapping_mul(256);
        let mut offset = [0; 3];
        offset[axis] = if reverse {
            amount.wrapping_neg()
        } else {
            amount
        };
        let rotated = spatial.local.matrix.transform(offset)?;
        let mut destination = spatial.local.position;
        destination[0] = destination[0].wrapping_add(rotated[0]);
        destination[2] = destination[2].wrapping_add(rotated[2]);
        let obstacle = || OrdinaryObstacleQuery {
            scene: query.scene,
            animations: query.animations,
            prepared: query.prepared,
            vertical_offset: 0,
            respond: false,
        };
        let clear = self.ordinary_destination_body_clear(id, destination, obstacle())?
            && !self.step_hazard(id, environment.selected_player, destination, obstacle())?
            && self.admit_ordinary_destination(id, destination, query, environment, sounds)?;
        if !clear {
            let (byte, mask) = match direction {
                OrdinaryStepDirection::Forward => (0, 0x80),
                OrdinaryStepDirection::Left => (0, 0x20),
                OrdinaryStepDirection::Right => (0, 0x40),
                OrdinaryStepDirection::Backward => (1, 0x01),
            };
            self.actors[index]
                .control
                .as_mut()
                .context("directional step control unavailable")?
                .collision_response_auxiliary[byte] |= mask;
        }
        Ok(clear)
    }

    /// The nonplayer deep-water edge probe: whether the terrain point one step of 16384 from the
    /// receiver, along the motion direction (4 left, 5 right, 3 and 7 backward, otherwise forward)
    /// rotated by its matrix, is free of an uncovered steep rock edge. The caller applies it to
    /// deep-water receivers only; a world without terrain passes (preservation engine/SOUP.md,
    /// "Nonplayer deep-water and edge probe").
    pub fn edge_probe_clear(
        &self,
        id: ScriptActorId,
        direction: u8,
        scene: &crate::player::data::PlayerScene,
    ) -> Result<bool> {
        let Some(terrain) = &scene.terrain else {
            return Ok(true);
        };
        let offset = match direction {
            4 => [-0x4000, 0, 0],
            5 => [0x4000, 0, 0],
            3 | 7 => [0, 0, -0x4000],
            _ => [0, 0, 0x4000],
        };
        let spatial = self.spatial(id)?;
        let rotated = spatial.local.matrix.transform(offset)?;
        let position = spatial.local.position;
        let point = [
            i64::from(position[0].wrapping_add(rotated[0])),
            i64::from(position[1]),
            i64::from(position[2].wrapping_add(rotated[2])),
        ];
        let assignments = scene
            .collision
            .texture_surfaces
            .as_ref()
            .context("edge probe SURFACE.INI assignments unavailable; re-export world")?;
        terrain.edge_blocked(point, assignments)
    }

    /// `MoveForward`'s forward step check. `None` when the check needs the collision query and
    /// none was supplied. Passes outright for a receiver whose lifecycle byte is 7 and whose
    /// attribute 1 has bit 0 clear, and for one whose attribute 1 has bit 1 clear (no hazard test);
    /// otherwise the destination, half of attribute byte 8 ahead, must not be a hazard.
    pub fn forward_step_clear(
        &self,
        id: ScriptActorId,
        selected_player: Option<ScriptActorId>,
        frame_scale: i32,
        query: Option<OrdinaryObstacleQuery<'_>>,
    ) -> Result<Option<bool>> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("forward step check control unavailable")?;
        if (control.death_selector == 7 && actor.attributes[1] & 1 == 0)
            || actor.attributes[1] & 2 == 0
        {
            return Ok(Some(true));
        }
        let Some(query) = query else {
            return Ok(None);
        };
        let distance = ((i32::from(actor.attributes[8]) << 8).wrapping_mul(frame_scale) >> 8) >> 1;
        let spatial = self.spatial(id)?;
        let destination = cyrus_world::transform::translate_forward(
            self.sight_position(index)?,
            spatial.local.angles[1],
            distance,
        )?;
        Ok(Some(!self.step_hazard(
            id,
            selected_player,
            destination,
            query,
        )?))
    }

    /// Whether stepping to `destination` (Q8 position) is hazardous: the destination surface is a
    /// steep slope, unless both the destination and the current surface are floors. Applies only
    /// to actors whose attribute 1 has bit 1 set, and not to the selected player while its
    /// support-history counter is nonzero. A surface missing at one of the two points
    /// counts as class one (**Inferred**: the original reads an uninitialized record there).
    pub fn step_hazard(
        &self,
        id: ScriptActorId,
        selected_player: Option<ScriptActorId>,
        destination: [i32; 3],
        query: OrdinaryObstacleQuery<'_>,
    ) -> Result<bool> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        if actor.attributes[1] & 2 == 0 {
            return Ok(false);
        }
        // The selected player skips the test while its support-history counter is nonzero.
        if selected_player == Some(id)
            && actor
                .player_movement
                .as_ref()
                .is_some_and(|movement| movement.history.ticks() != 0)
        {
            return Ok(false);
        }
        let extent = self.ordinary_collision_shape(id)?.positive_extent[1];
        let post = i32::try_from(query.scene.settings.post_collide_height)
            .context("step hazard post_collide_height exceeds signed range")?;
        let current = self.sight_position(index)?;
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let models = query
            .scene
            .actor_collision
            .as_ref()
            .context("step hazard live geometry unavailable; re-export world")?;
        let destination_centre = (destination[1] >> 8).wrapping_add(extent);
        let surface_at = |point: [i32; 3]| -> Result<Surface> {
            let centre = (point[1] >> 8).wrapping_add(extent);
            let (x, z) = (point[0] >> 8, point[2] >> 8);
            let contacts = query.scene.static_collision.runtime_segment_contacts(
                &query.prepared.candidates,
                models,
                &poses,
                [x, centre.wrapping_sub(post), z],
                [x, centre.wrapping_add(post), z],
                false,
                None,
            )?;
            let mut best = Surface {
                y: UNFOUND_Y,
                normal: [0; 3],
                metric: UNFOUND_METRIC,
            };
            for (source, contact) in contacts {
                let contact = contact.integer().with_context(|| {
                    format!(
                        "world {} instance {index} hazard contact {source:?}",
                        self.world
                    )
                })?;
                let class = surface_class(contact.normal[1]);
                let metric = (0x4000 - contact.rank as i32).abs();
                if matches!(class, 4 | 2) && metric < best.metric {
                    best = Surface {
                        y: contact.point[1],
                        normal: contact.normal,
                        metric,
                    };
                }
            }
            if let Some(ground) = query
                .scene
                .terrain
                .as_ref()
                .and_then(|terrain| terrain.sample(point.map(i64::from)))
            {
                {
                    let top = f64::from(centre.wrapping_add(post));
                    let reference = f64::from(destination_centre);
                    // The reference centre is the destination's for both points, as in the original.
                    if top >= ground.height
                        && i64::from(best.y.wrapping_sub(destination_centre)).abs()
                            > (ground.height - reference).round_ties_even().abs() as i64
                    {
                        best = Surface {
                            y: ground.height.round_ties_even() as i32,
                            normal: ground.normal,
                            metric: 0,
                        };
                    }
                }
            }
            Ok(best)
        };
        let at_destination = surface_at(destination)?;
        let at_current = if current[0] != destination[0] || current[2] != destination[2] {
            surface_at(current)?
        } else {
            at_destination
        };
        if at_destination.metric == UNFOUND_METRIC && at_current.metric == UNFOUND_METRIC {
            return Ok(false);
        }
        let class = |surface: Surface| {
            if surface.metric == UNFOUND_METRIC {
                1
            } else {
                surface_class(surface.normal[1])
            }
        };
        let (target, here) = (class(at_destination), class(at_current));
        Ok(!(target == 4 && here == 4) && target == 2)
    }
}

#[cfg(test)]
mod tests {
    use super::surface_class;

    #[test]
    fn surface_classes_follow_the_normal_y_bands() {
        assert_eq!(surface_class(-256), 4);
        assert_eq!(surface_class(-161), 4);
        assert_eq!(surface_class(-160), 2);
        assert_eq!(surface_class(-42), 2);
        assert_eq!(surface_class(-41), 1);
        assert_eq!(surface_class(0), 1);
        assert_eq!(surface_class(160), 1);
        assert_eq!(surface_class(161), 8);
    }
}
