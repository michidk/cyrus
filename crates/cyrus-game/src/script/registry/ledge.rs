//! Selected-player ledge entry: the edge service, the ledge scan and its record classification.
//! The entry transition and the ledge living states remain unsupported, so a scan that yields a
//! usable record stops the update with a contextual error.
use super::{ScriptActorId, ScriptWorld, ground::OrdinaryGroundQuery};
use anyhow::{Context, Result};
use bevy::math::{Mat3, Vec3};
use cyrus_collision::{
    candidates::CollisionCandidates, collision_mesh::BodyProbe, collision_world::ResidualBodyQuery,
};

/// Forward offsets of the scan's two passes, in local Q8.
const LEDGE_SCAN_PASSES: [i32; 2] = [0, 6144];
/// Local Q8 forward reach of the scan segments beyond the pass offset.
const LEDGE_SCAN_REACH: i32 = 9728;
/// Local Q8 half-height of the vertical top segment.
const LEDGE_SCAN_HALF_HEIGHT: i32 = 40960;
/// A ledge table holds at most four records; a fifth is not guarded against.
const LEDGE_TABLE: usize = 4;
/// Local Q8 distance behind the wall at which the actor's spheres are fitted.
const LEDGE_FIT_OFFSET: i32 = 4096;
/// Group whose first frame's grip vertex is the hand position.
const LEDGE_HAND_GROUP: u64 = 7;

impl ScriptWorld {
    /// Run the selected player's edge service for an animated actor without support. Returns
    /// true when the caller's fall or queued handling must end. Other actors return false.
    pub(super) fn selected_edge_service(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        fine_ticks: u32,
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
        let result = self.ledge_scan(id, query, environment, fine_ticks)?;
        anyhow::ensure!(
            result == 0,
            "world {} instance {index}: ledge entry transition unsupported (scan result {result})",
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

    /// The ledge scan result: 1 when a class 1 record exists, else 2 for class 2, else 3 for class
    /// 3, else 0. A nonzero action-transition counter or a held weapon finds none. Polygon
    /// sidedness is disabled for the scan's queries and fit tests. Entry runs in living state 19.
    fn ledge_scan(
        &self,
        id: ScriptActorId,
        query: OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        fine_ticks: u32,
    ) -> Result<u8> {
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
            return Ok(0);
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
        let shape = self.ordinary_collision_shape(id)?;
        let settings = &query.scene.settings;
        let post = i32::try_from(settings.post_collide_height)
            .context("ledge scan post_collide_height exceeds signed range")?;
        let standing = settings.standing_height;
        let y = pose.position[1] >> 8;
        let airborne = control.falling_state != 0 || control.queued_motion != 0;
        let state = 19;
        // The wrapping step product uses the first vertical factor.
        let step = (post.wrapping_mul(512).wrapping_sub(256)).min(
            i32::from(actor.attributes[32])
                .wrapping_mul(super::vertical::ordinary_vertical_factor(0)?)
                .wrapping_mul(i32::try_from(fine_ticks)?.wrapping_add(2)),
        );
        let mut stance = Stance {
            y,
            negative: shape.negative_extent[1],
            positive: shape.positive_extent[1],
            reach: (step >> 9).max(14),
            hand_y: None,
            post,
            airborne: airborne || (14..=17).contains(&state),
            hanging: state == 15,
        };
        // Local Q8 points turn with the actor, join its position and become integer map units.
        let place = |local: [i32; 3]| -> Result<[i32; 3]> {
            let rotated = pose.matrix.transform(local)?;
            Ok(std::array::from_fn(|axis| {
                rotated[axis].wrapping_add(pose.position[axis]) >> 8
            }))
        };
        let rows = cyrus_world::transform::Matrix::collision_orientation(pose.angles)
            .0
            .map(Vec3::from_array);
        let rotation = Mat3::from_cols(rows[0], rows[1], rows[2]).transpose();
        let body: Vec<(Vec3, f32)> = shape
            .body
            .iter()
            .map(|sphere| {
                (
                    Vec3::from_array(sphere.center.map(|value| value as f32)),
                    sphere.radius as f32,
                )
            })
            .collect();
        let head = [(
            Vec3::new(0.0, (stance.positive - (post >> 1)) as f32, 0.0),
            (post >> 1) as f32,
        )];
        // A body contact fails the body test and ends the loop; a class 8 contact of the head
        // sphere fails the head test.
        let fits = |top: i32, point: [i32; 2], record_heading: u16| -> Result<(bool, bool)> {
            let [dx, dz] = heading_offset(LEDGE_FIT_OFFSET, record_heading);
            let position = [
                i64::from(point[0]) * 256 + i64::from(dx),
                i64::from(top.wrapping_sub(stance.positive).wrapping_sub(standing)) * 256,
                i64::from(point[1]) * 256 + i64::from(dz),
            ];
            let (mut body_fits, mut head_fits) = (true, true);
            for candidate in &query.prepared.candidates.faces {
                let single = CollisionCandidates {
                    faces: vec![*candidate],
                    actors: Vec::new(),
                };
                let contacts = |spheres: &[(Vec3, f32)]| {
                    collision.runtime_residual_body_contacts(
                        &single,
                        models,
                        &poses,
                        ResidualBodyQuery {
                            probe: BodyProbe {
                                position,
                                rotation,
                                spheres,
                                sidedness: false,
                            },
                            excluded_candidate_type: None,
                        },
                    )
                };
                if !contacts(&body)?.is_empty() {
                    body_fits = false;
                    break;
                }
                if contacts(&head)?
                    .iter()
                    .any(|contact| contact.classification == 8)
                {
                    head_fits = false;
                }
            }
            Ok((body_fits, head_fits))
        };
        let mut records: Vec<LedgeRecord> = Vec::new();
        for offset in LEDGE_SCAN_PASSES {
            let forward = LEDGE_SCAN_REACH + offset;
            let top_start = place([0, LEDGE_SCAN_HALF_HEIGHT, forward])?;
            let top_end = place([0, -LEDGE_SCAN_HALF_HEIGHT, forward])?;
            let wall_start = place([0, 0, forward])?;
            let wall_end = place([0, 0, offset])?;
            let mut undersides = Vec::new();
            // The facing class 1 contact with the greatest rank at one height: point and normal.
            let wall_at = |wall_y: i32| -> Result<Option<([i32; 3], [i32; 3])>> {
                let walls = collision.runtime_segment_contacts(
                    &query.prepared.candidates,
                    models,
                    &poses,
                    [wall_start[0], wall_y, wall_start[2]],
                    [wall_end[0], wall_y, wall_end[2]],
                    false,
                    None,
                )?;
                let mut best: Option<(u32, [i32; 3], [i32; 3])> = None;
                for (_, contact) in walls {
                    let contact = contact.integer()?;
                    if super::hazard::surface_class(contact.normal[1]) == 1
                        && facing(heading, contact.normal)
                        && contact.rank > best.map_or(0, |(rank, ..)| rank)
                    {
                        best = Some((contact.rank, contact.point, contact.normal));
                    }
                }
                Ok(best.map(|(_, point, normal)| (point, normal)))
            };
            let mut tops = Vec::new();
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
                    match super::hazard::surface_class(contact.normal[1]) {
                        4 if contact.rank > top.map_or(0, |(rank, _)| rank) => {
                            top = Some((contact.rank, contact.point[1]));
                        }
                        8 if candidate.candidate_type & 0x08 == 0 => {
                            undersides.push(contact.point[1]);
                        }
                        _ => {}
                    }
                }
                if let Some((_, top_y)) = top {
                    tops.push(top_y);
                }
            }
            for top in tops {
                if records.len() >= LEDGE_TABLE {
                    break;
                }
                // A facing wall just below the top makes a record.
                let Some((point, normal)) = wall_at(top.wrapping_add(16))? else {
                    continue;
                };
                records.push(LedgeRecord {
                    top,
                    point: [point[0], point[2]],
                    heading: wall_heading(normal),
                    class: 0,
                    clear: 0,
                });
            }
            if stance.airborne && stance.hand_y.is_none() && !records.is_empty() {
                stance.hand_y = Some(match self.ledge_hand_vertex(id, query.animations)? {
                    Some(grip) => y.wrapping_add(grip[1] >> 8),
                    None => y.wrapping_sub(1000),
                });
            }
            for record in &mut records {
                record.class = ledge_class(&stance, record.top, &undersides);
                if record.class != 0 {
                    let (body_fits, head_fits) = fits(record.top, record.point, record.heading)?;
                    (record.class, record.clear) = settle(record.class, body_fits, head_fits);
                }
            }
            // The result is the lowest nonzero class of any record.
            if let Some(result) = records
                .iter()
                .map(|record| record.class)
                .filter(|&c| c != 0)
                .min()
            {
                return Ok(result);
            }
        }
        Ok(0)
    }
}

/// Actor body measures that decide a record's class, in integer map units. Y increases downward.
struct Stance {
    y: i32,
    negative: i32,
    positive: i32,
    reach: i32,
    /// Looked up on the first record, since only an airborne class needs it.
    hand_y: Option<i32>,
    post: i32,
    airborne: bool,
    hanging: bool,
}

/// One scan record: the wall point's X/Z, the top's Y and the facing heading of the wall.
struct LedgeRecord {
    top: i32,
    point: [i32; 2],
    heading: u16,
    class: u8,
    /// Record byte 2: whether a class 3 record leaves room to climb.
    clear: u8,
}

/// Class of a record by its top's height against the actor, before the fit test: 1 low, 2 high,
/// 3 reachable from the hand while airborne or hanging, 0 none. An underside within eight units
/// of the top rejects it.
fn ledge_class(stance: &Stance, top: i32, undersides: &[i32]) -> u8 {
    let (y, quarter) = (stance.y, stance.negative >> 2);
    let class = if stance.airborne {
        if stance.hanging
            || stance
                .hand_y
                .is_some_and(|hand| hand.wrapping_sub(top).abs() < stance.reach)
        {
            3
        } else {
            0
        }
    } else if y - stance.negative - stance.reach <= top && top < y - quarter {
        2
    } else if y - quarter <= top && top < y + stance.positive - stance.post {
        1
    } else {
        0
    };
    if undersides.iter().any(|&under| (top - under).abs() < 8) {
        0
    } else {
        class
    }
}

/// Class and record byte 2 after the fit test: a body that does not fit rejects classes 1 and 2,
/// and leaves class 3 without room to climb; a blocked head also leaves it without room.
fn settle(class: u8, body_fits: bool, head_fits: bool) -> (u8, u8) {
    match class {
        3 => (3, u8::from(head_fits && body_fits)),
        _ if !body_fits => (0, 0),
        _ => (class, 0),
    }
}

/// X/Z offset of a Q8 distance at a heading, nearest rounding through the shared sine table.
fn heading_offset(distance: i32, heading: u16) -> [i32; 2] {
    let index = usize::from(heading.wrapping_neg() & 2047);
    let offset = |sample: f32| (f64::from(distance) * f64::from(sample)).round_ties_even() as i32;
    [
        offset(cyrus_world::transform::rotation_sample(index)),
        offset(cyrus_world::transform::rotation_sample(index + 512)),
    ]
}

/// A wall normal's facing heading: its displacement heading plus 1024.
fn wall_heading(normal: [i32; 3]) -> u16 {
    crate::player::motion::displacement_heading(i64::from(normal[0]), i64::from(normal[2]))
        .wrapping_add(1024)
        & 2047
}

/// Hand vertex lookup of the actor's group 7, shared with the later ledge states.
impl ScriptWorld {
    /// Grip vertex 0 of the first `ShowFrame` of the actor's animation group 7, in Q8. None when
    /// the group or its frame is absent.
    fn ledge_hand_vertex(
        &self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
    ) -> Result<Option<[i32; 3]>> {
        let index = self.resolve(id)?;
        let metadata = data.actor(self.actors[index].name())?;
        let Some(reference) = metadata["animation_groups"]
            .as_array()
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group["group_index"].as_u64() == Some(LEDGE_HAND_GROUP))
            })
            .and_then(|group| group["commands"].as_array())
            .and_then(|commands| commands.iter().find(|command| command["opcode"] == 0))
            .and_then(|command| command["frame_reference"].as_u64())
        else {
            return Ok(None);
        };
        let grip = metadata["raex"]["grip0"]
            .as_i64()
            .and_then(|value| i16::try_from(value).ok())
            .with_context(|| {
                format!(
                    "world {} instance {index}: ledge hand grip unavailable",
                    self.world
                )
            })?;
        self.actor_frame_source_vertex(id, data, u32::try_from(reference)?, i32::from(grip))
            .map(Some)
    }
}

/// Whether a wall normal faces the actor heading: its facing heading lies within 256 of the
/// heading, differences wrapped to 2048.
fn facing(heading: u16, normal: [i32; 3]) -> bool {
    let difference = (i32::from(wall_heading(normal)) - i32::from(heading)) & 2047;
    difference.min(2048 - difference) <= 256
}

#[cfg(test)]
mod tests {
    use super::{Stance, facing, heading_offset, ledge_class, settle};

    fn stance(airborne: bool) -> Stance {
        Stance {
            y: 1000,
            negative: 64,
            positive: 40,
            reach: 14,
            hand_y: Some(900),
            post: 30,
            airborne,
            hanging: false,
        }
    }

    #[test]
    fn grounded_tops_split_by_height_into_classes_two_and_one() {
        let grounded = stance(false);
        // Class 2 spans Y - neg - reach up to Y - neg/4 (exclusive); class 1 continues to the feet.
        assert_eq!(ledge_class(&grounded, 922, &[]), 2);
        assert_eq!(ledge_class(&grounded, 921, &[]), 0);
        assert_eq!(ledge_class(&grounded, 983, &[]), 2);
        assert_eq!(ledge_class(&grounded, 984, &[]), 1);
        assert_eq!(ledge_class(&grounded, 1009, &[]), 1);
        assert_eq!(ledge_class(&grounded, 1010, &[]), 0);
    }

    #[test]
    fn airborne_tops_need_the_hand_within_reach_and_an_underside_rejects() {
        let airborne = stance(true);
        assert_eq!(ledge_class(&airborne, 887, &[]), 3);
        assert_eq!(ledge_class(&airborne, 886, &[]), 0);
        assert_eq!(ledge_class(&airborne, 887, &[894]), 0);
        assert_eq!(ledge_class(&airborne, 887, &[895]), 3);
    }

    #[test]
    fn the_fit_test_rejects_low_and_high_ledges_and_flags_class_three_clearance() {
        assert_eq!(settle(1, false, true), (0, 0));
        assert_eq!(settle(2, true, false), (2, 0));
        assert_eq!(settle(3, true, true), (3, 1));
        assert_eq!(settle(3, true, false), (3, 0));
        assert_eq!(settle(3, false, true), (3, 0));
    }

    #[test]
    fn a_heading_offset_follows_the_sine_table() {
        assert_eq!(heading_offset(4096, 0), [0, 4096]);
        assert_eq!(heading_offset(4096, 1024), [0, -4096]);
        assert_eq!(heading_offset(4096, 512)[0].abs(), 4096);
    }

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
