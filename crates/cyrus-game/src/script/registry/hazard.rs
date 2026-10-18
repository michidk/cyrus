//! The nonplayer step hazard test follows the original's destination surface classification.
use super::{ScriptActorId, ScriptWorld, obstacle::OrdinaryObstacleQuery};
use anyhow::{Context, Result, ensure};

/// Surface class of a contact normal's Y component (scale 256, negative up): 4 for floors,
/// 2 for steep slopes, 8 for ceilings and 1 otherwise.
fn surface_class(normal_y: i32) -> u8 {
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
    /// to actors whose attribute 1 has bit 1 set. A surface missing at one of the two points
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
        ensure!(
            selected_player != Some(id),
            "world {} instance {index}: selected-player step hazard gate unavailable",
            self.world
        );
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
