//! Selected-player ledge entry: the edge service and the ledge scan. Ledge records found by the
//! scan, their classification and the ledge living states remain unsupported.
use super::{ScriptActorId, ScriptWorld, ground::OrdinaryGroundQuery};
use anyhow::{Context, Result};

/// Forward offsets of the scan's two passes, in local Q8.
const LEDGE_SCAN_PASSES: [i32; 2] = [0, 6144];
/// Local Q8 forward reach of the scan segments beyond the pass offset.
const LEDGE_SCAN_REACH: i32 = 9728;
/// Local Q8 half-height of the vertical top segment.
const LEDGE_SCAN_HALF_HEIGHT: i32 = 40960;

impl ScriptWorld {
    /// Run the selected player's edge service for an animated actor without support. Returns
    /// true when the caller's fall or queued handling must end. Other actors return false.
    pub(super) fn selected_edge_service(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("edge service control unavailable")?;
        if environment.selected_player != Some(id)
            || actor.attributes[105] == 1
            || control.torch_burning != 0
        {
            return Ok(false);
        }
        let group = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        if self.updates.animation_system
            && !environment.shell_execution
            && (matches!(control.living_state, 33 | 34) || matches!(group, Some(149 | 150)))
        {
            return Ok(false);
        }
        let surface = actor.surface.with_context(|| {
            format!(
                "world {} instance {index}: edge service surface unavailable",
                self.world
            )
        })?;
        if matches!(surface, 2 | 4) {
            anyhow::ensure!(
                control.living_state != 15,
                "world {} instance {index}: ledge hang release unsupported",
                self.world
            );
            return Ok(false);
        }
        match control.living_state {
            10..=19 => return Ok(true),
            0 => {}
            _ => return Ok(false),
        }
        // Entry sets the entering state and clears the ledge-support reference before the scan;
        // without a record it sets the living state back to zero.
        anyhow::ensure!(
            !self.ledge_scan_finds_record(id, query, environment)?,
            "world {} instance {index}: ledge record classification and transition unsupported",
            self.world
        );
        self.actors[index].model.ledge_support = None;
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("validated edge service control");
        if control.action_transition_counter != 0 {
            control.queued_motion &= !0x80;
            control.falling_state &= !0x80;
            self.actors[index].sync_falling_motion();
        }
        Ok(false)
    }

    /// Whether the ledge scan appends a record. A nonzero action-transition counter or a held
    /// weapon finds none. Polygon sidedness is disabled for the scan's queries.
    fn ledge_scan_finds_record(
        &self,
        id: ScriptActorId,
        query: OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
    ) -> Result<bool> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ledge scan control unavailable")?;
        let (holding, _, _) = super::super::activation::weapon_predicates(
            control.living_state,
            Some(super::super::ActivationAnimation {
                group: actor
                    .animation
                    .as_ref()
                    .and_then(|animation| animation.playback.current_group()),
            }),
            control.combat_death_default.is_some(),
            control.weapon_drawn,
            actor.attributes[110],
            environment.item_system_ready,
            environment.shell_execution,
        );
        if control.action_transition_counter != 0 || holding {
            return Ok(false);
        }
        let pose = self.spatial(id)?.query_pose()?;
        let heading = pose.angles[1];
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let models = query
            .scene
            .actor_collision
            .as_ref()
            .context("ledge scan live geometry unavailable; re-export world")?;
        let collision = &query.scene.static_collision;
        // Local Q8 points turn with the actor, join its position and become integer map units.
        let place = |local: [i32; 3]| -> Result<[i32; 3]> {
            let rotated = pose.matrix.transform(local)?;
            Ok(std::array::from_fn(|axis| {
                rotated[axis].wrapping_add(pose.position[axis]) >> 8
            }))
        };
        for offset in LEDGE_SCAN_PASSES {
            let forward = LEDGE_SCAN_REACH + offset;
            let top_start = place([0, LEDGE_SCAN_HALF_HEIGHT, forward])?;
            let top_end = place([0, -LEDGE_SCAN_HALF_HEIGHT, forward])?;
            let wall_start = place([0, 0, forward])?;
            let wall_end = place([0, 0, offset])?;
            for candidate in &query.prepared.candidates.faces {
                // The candidate's top is its class 4 contact with the strictly greatest rank.
                let mut top: Option<(u32, i32)> = None;
                for contact in collision.model_segment_contacts(
                    candidate.model,
                    Some(models),
                    &poses,
                    top_start,
                    top_end,
                    false,
                )? {
                    let contact = contact.integer()?;
                    if super::hazard::surface_class(contact.normal[1]) == 4
                        && contact.rank > top.map_or(0, |(rank, _)| rank)
                    {
                        top = Some((contact.rank, contact.point[1]));
                    }
                }
                let Some((_, top_y)) = top else {
                    continue;
                };
                // A facing wall just below the top makes a record.
                let wall_y = top_y.wrapping_add(16);
                let walls = collision.runtime_segment_contacts(
                    &query.prepared.candidates,
                    models,
                    &poses,
                    [wall_start[0], wall_y, wall_start[2]],
                    [wall_end[0], wall_y, wall_end[2]],
                    false,
                    None,
                )?;
                for (_, contact) in walls {
                    let contact = contact.integer()?;
                    if super::hazard::surface_class(contact.normal[1]) == 1
                        && contact.rank > 0
                        && facing(heading, contact.normal)
                    {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}

/// Whether a wall normal faces the actor heading: its displacement heading plus 1024 lies
/// within 256 of the heading, differences wrapped to 2048.
fn facing(heading: u16, normal: [i32; 3]) -> bool {
    let wall =
        crate::player::motion::displacement_heading(i64::from(normal[0]), i64::from(normal[2]))
            .wrapping_add(1024)
            & 2047;
    let difference = (i32::from(wall) - i32::from(heading)) & 2047;
    difference.min(2048 - difference) <= 256
}

#[cfg(test)]
mod tests {
    use super::facing;

    #[test]
    fn a_wall_faces_the_actor_within_256_of_its_heading() {
        let normal = [0, 0, -256];
        let wall = (crate::player::motion::displacement_heading(0, -256) + 1024) & 2047;
        assert!(facing(wall, normal));
        assert!(facing((wall + 256) & 2047, normal));
        assert!(facing(wall.wrapping_sub(256) & 2047, normal));
        assert!(!facing((wall + 257) & 2047, normal));
        assert!(!facing((wall + 1024) & 2047, normal));
    }
}
