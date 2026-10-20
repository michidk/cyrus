//! Ordinary ground queries retain model ownership and selected-player fallback history.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_collision::{
    actor_shape::{ActorBounds, BodySphere},
    ground::{GroundProbe, SelectedSupport, SupportSource},
};

/// Caller-admitted ordinary support inputs in original map units.
/// The distance is the original response stage's probe distance, not a default.
#[derive(Clone, Copy)]
pub struct OrdinaryGroundQuery<'a> {
    pub scene: &'a crate::player::data::PlayerScene,
    pub animations: &'a cyrus_assets::animation::Animations,
    pub prepared: &'a super::model::OrdinaryCollisionQuery,
    pub distance: u32,
    pub sidedness: bool,
}

/// Initial ordinary collision shape, distinct from the currently rendered model.
/// Extents and sphere coordinates are original map units. Model-less initialization
/// has one body sphere and no separately prepared additional sphere.
pub struct OrdinaryCollisionShape {
    pub positive_extent: [i32; 3],
    pub negative_extent: [i32; 3],
    pub body: Vec<BodySphere>,
    pub additional: Option<BodySphere>,
}

#[derive(serde::Deserialize)]
struct PrimaryBounds {
    total_extent: [i32; 3],
    pos_extent: [i32; 3],
    neg_extent: [i32; 3],
    center_offset: [i32; 3],
}

impl ScriptWorld {
    /// Initialize the original ordinary loader shape once. Its definition path
    /// precedes placed-model fallback; changing presentation does not rebuild it.
    pub fn prepare_ordinary_collision_shape(
        &mut self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        scene: &crate::player::data::PlayerScene,
    ) -> Result<()> {
        self.require_model_identity(data)?;
        ensure!(
            scene.world == self.world,
            "world {} ordinary collision scene identity mismatch",
            self.world
        );
        let index = self.resolve(id)?;
        ensure!(
            self.actors[index].collision.is_none(),
            "world {} instance {index}: collision shape already prepared",
            self.world
        );
        let actor = &self.actors[index];
        ensure!(
            actor.ordinary_collision_role(),
            "world {} instance {index}: ordinary collision object path unavailable",
            self.world
        );
        let definition = data.actor(actor.name())?;
        let path = definition["model_load_path"]
            .as_str()
            .context("ordinary collision model path absent")?;
        ensure!(
            !matches!(actor.source, super::ScriptActorSource::Player) || path == "animation_table",
            "world {} selected player: animated collision definition required",
            self.world
        );
        // Preservation omits negative primary indices from its sparse metadata.
        let primary = match definition.get("mpsz_bounds_primary") {
            None => -1,
            Some(value) => value
                .as_i64()
                .context("ordinary collision primary bounds index invalid")?,
        };
        let authored = if path == "static" {
            data.metadata["mpob_objects"]
                .as_array()
                .and_then(|objects| objects.get(index))
                .and_then(|object| object["model_id"].as_i64())
                .context("ordinary collision MPOB model_id absent")?
                != -1
        } else {
            path == "animation_table" && primary != -1
        };
        let shape = if authored || path == "static" {
            let scale = definition["raex"]["unknown_12"]
                .as_i64()
                .context("ordinary collision RAEX radius scale absent")?;
            let scale = i32::from(
                i16::try_from(scale)
                    .context("ordinary collision RAEX radius scale outside signed word")?,
            );
            let bounds = if authored {
                let primary = usize::try_from(primary)
                    .context("ordinary collision primary bounds index negative")?;
                let value = data.metadata["mpsz_entries"]
                    .as_array()
                    .and_then(|entries| entries.get(primary))
                    .context("ordinary collision primary MPSZ record absent")?;
                let record: PrimaryBounds = serde_json::from_value(value.clone())
                    .context("ordinary collision invalid MPSZ record")?;
                ensure!(
                    record.center_offset == [0; 3],
                    "ordinary collision shifted MPSZ center unsupported"
                );
                for axis in 0..3 {
                    ensure!(
                        i64::from(record.pos_extent[axis]) + i64::from(record.neg_extent[axis])
                            == i64::from(record.total_extent[axis]),
                        "ordinary collision MPSZ axis {axis}: inconsistent total"
                    );
                }
                ActorBounds::new(record.pos_extent, record.neg_extent, scale)?
            } else {
                let system_scale = scene
                    .collision
                    .sphere_object_scale
                    .context("ordinary collision sphere_object_scale absent; re-export world")?;
                scene
                    .actor_collision
                    .as_ref()
                    .context("ordinary collision live geometry absent; re-export world")?
                    .initial_bounds(index, system_scale, scale)?
            };
            let positive_extent = bounds.positive_extent();
            let negative_extent = bounds.negative_extent();
            let spheres = bounds.prepare(
                scene.settings.post_collide_height,
                scene.settings.standing_height,
            )?;
            OrdinaryCollisionShape {
                positive_extent,
                negative_extent,
                body: spheres.body,
                additional: Some(spheres.additional),
            }
        } else {
            ensure!(
                matches!(path, "none" | "animation_table"),
                "ordinary collision model path {path} unsupported"
            );
            OrdinaryCollisionShape {
                positive_extent: [12; 3],
                negative_extent: [12; 3],
                body: vec![BodySphere {
                    center: [0; 3],
                    radius: 12,
                }],
                additional: None,
            }
        };
        self.actors[index].collision = Some(shape);
        Ok(())
    }

    pub fn ordinary_collision_shape(&self, id: ScriptActorId) -> Result<&OrdinaryCollisionShape> {
        self.actors[self.resolve(id)?]
            .collision
            .as_ref()
            .context("ordinary collision shape not prepared")
    }

    pub(super) fn initialize_collision_shapes(
        &mut self,
        data: &cyrus_assets::animation::Animations,
        scene: &crate::player::data::PlayerScene,
    ) -> Result<()> {
        for index in 0..self.actors.len() {
            if self.actors[index]
                .placement()
                .is_none_or(|placement| placement.object_type != 1)
            {
                continue;
            }
            let id = self.id(index)?;
            if let Err(error) = self.prepare_ordinary_collision_shape(id, data, scene) {
                self.actors[index].update_failure = Some(format!(
                    "world {} instance {index} actor {} collision shape preparation: {error:#}",
                    self.world,
                    self.actors[index].name()
                ));
            }
        }
        Ok(())
    }

    /// Execute the late recovery probe against its fresh candidate record. A
    /// model hit writes local position; terrain or absent animation accepts
    /// without assignment. Rejection leaves rollback to its owning stage.
    pub fn query_stable_recovery(
        &mut self,
        id: ScriptActorId,
        prepared: &super::model::StableRecoveryQuery,
        scene: &crate::player::data::PlayerScene,
        data: &cyrus_assets::animation::Animations,
    ) -> Result<bool> {
        let prepared = &prepared.prepared;
        self.require_collision_query(id, prepared, scene, data)?;
        let index = self.resolve(id)?;
        if self.actors[index].animation.is_none() {
            return Ok(true);
        }
        let spatial = self.spatial(id)?;
        let probe = OrdinaryStableProbe::new(
            spatial.query_pose()?.position,
            spatial.local.angles[1],
            self.ordinary_collision_shape(id)?.positive_extent[1],
            scene.settings.post_collide_height,
        )?;
        let poses = self.ordinary_collision_poses(prepared)?;
        let models = scene
            .actor_collision
            .as_ref()
            .context("stable recovery live geometry unavailable; re-export world")?;
        for candidate in &prepared.candidates.faces {
            let single = cyrus_collision::candidates::CollisionCandidates {
                faces: vec![*candidate],
                actors: Vec::new(),
            };
            let contacts = scene.static_collision.runtime_segment_contacts(
                &single,
                models,
                &poses,
                probe.start,
                probe.end,
                false,
                None,
            )?;
            for (_, contact) in contacts {
                let contact = contact.integer()?;
                let support = cyrus_collision::ground::GroundSupport {
                    height: f64::from(contact.point[1]),
                    normal: contact.normal,
                };
                if support.classification() == 4 {
                    self.spatial
                        .as_mut()
                        .context("stable recovery spatial state unavailable")?
                        .actor_mut(index)?
                        .local
                        .position = probe.model_position(contact.point[1]);
                    return Ok(true);
                }
            }
        }
        if let Some(terrain) = &scene.terrain {
            let height = terrain
                .sample(probe.start.map(|coordinate| i64::from(coordinate) * 256))
                .context("stable recovery terrain water or grid edge unsupported")?;
            return probe.accepts_terrain(height.height);
        }
        Ok(false)
    }

    /// Query primary support from static and stored live models, retaining the
    /// selected face owner and selected-player fallback/history. This performs
    /// neither motion response nor surface-response transitions. The admitted ordinary
    /// boundary owns clearing prior support. Follows preservation's ordinary
    /// primary support and supporting-model lifetime rules.
    pub fn query_ordinary_ground(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryGroundQuery<'_>,
    ) -> Result<Option<SelectedSupport>> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let positive_y_extent = self.ordinary_collision_shape(id)?.positive_extent[1];
        let position = self.spatial(id)?.query_pose()?.position;
        let models = query
            .scene
            .actor_collision
            .as_ref()
            .context("ordinary ground live geometry unavailable; re-export world")?;
        let probe = GroundProbe::new(position.map(i64::from), positive_y_extent, query.distance)?;
        let contacts = query.scene.static_collision.runtime_ground_contacts(
            &query.prepared.candidates,
            models,
            &poses,
            &probe,
            query.sidedness,
            (self.actors[id.index]
                .control
                .as_ref()
                .context("ordinary ground control unavailable")?
                .queued_motion
                == 24)
                .then_some(128),
        )?;
        let terrain = query
            .scene
            .terrain
            .as_ref()
            .and_then(|terrain| terrain.sample(position.map(i64::from)));
        let mut selected = probe.select(terrain, &contacts);
        if matches!(
            self.actors[id.index].source,
            super::ScriptActorSource::Player
        ) {
            let history = &self.actors[id.index]
                .player_movement
                .as_ref()
                .context("selected ground query requires initialized movement history")?
                .history;
            if selected.is_none() {
                let fallback = cyrus_collision::ground::FallbackProbe::new(
                    position.map(i64::from),
                    [0; 3],
                    positive_y_extent,
                    history.ticks() != 0,
                )?;
                selected = query.scene.static_collision.runtime_fallback_support(
                    &query.prepared.candidates,
                    models,
                    &poses,
                    &fallback,
                    query.sidedness,
                    (self.actors[id.index]
                        .control
                        .as_ref()
                        .expect("validated control")
                        .queued_motion
                        == 24)
                        .then_some(128),
                )?;
            }
            let metadata = selected
                .as_ref()
                .map(|support| match support.source() {
                    SupportSource::Actor { actor, .. } => self
                        .actors
                        .get(actor)
                        .and_then(|actor| actor.model.collision_flags)
                        .context("selected support live metadata unavailable"),
                    source => query.scene.static_collision.support_metadata(source),
                })
                .transpose()?;
            self.actors[id.index]
                .player_movement
                .as_mut()
                .expect("validated history")
                .history
                .finish_query(metadata, true);
            self.actors[id.index]
                .control
                .as_mut()
                .expect("validated control")
                .ground_kind = ground_kind(selected.as_ref());
        }
        if let Some(support) = &selected {
            let owner = match support.source() {
                SupportSource::Model { model, .. } => super::model::ModelSupportOwners {
                    static_placement: Some(model),
                    actor: None,
                },
                SupportSource::Actor { actor, .. } => super::model::ModelSupportOwners {
                    actor: Some(self.id(actor)?),
                    static_placement: None,
                },
                SupportSource::Terrain => Default::default(),
                SupportSource::Transient { .. } => {
                    anyhow::bail!("ordinary ground transient support owner unavailable")
                }
            };
            // A terrain result does not overwrite a prior face-query owner. The
            // ordinary boundary clears that owner before the first query.
            if !matches!(support.source(), SupportSource::Terrain) {
                self.set_model_support(id, owner)?;
            }
        }
        Ok(selected)
    }
}

/// Integer-map segment for the separately admitted late recovery query.
/// This is neither the ordinary ranked support probe nor queued clearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinaryStableProbe {
    pub start: [i32; 3],
    pub end: [i32; 3],
    positive_y_extent: i32,
}

impl OrdinaryStableProbe {
    /// Construct from effective query position and local heading. Forward
    /// translation occurs in Q8 before arithmetic conversion to map units.
    pub fn new(
        position: [i32; 3],
        heading: u16,
        positive_y_extent: i32,
        post_collide_height: u32,
    ) -> Result<Self> {
        let forward = cyrus_world::transform::translate_forward(position, heading, 6144)?;
        let mut start = forward.map(|coordinate| coordinate >> 8);
        let mut end = start;
        start[1] = start[1]
            .wrapping_add(positive_y_extent)
            .wrapping_sub(post_collide_height as i32);
        end[1] = end[1]
            .wrapping_add(positive_y_extent)
            .wrapping_add(post_collide_height as i32);
        Ok(Self {
            start,
            end,
            positive_y_extent,
        })
    }

    /// Original local-position assignment from the first class-four model hit.
    /// The owning query establishes classification and candidate order.
    pub fn model_position(&self, integer_height: i32) -> [i32; 3] {
        [
            self.start[0].wrapping_mul(256),
            integer_height
                .wrapping_sub(self.positive_y_extent)
                .wrapping_mul(256),
            self.start[2].wrapping_mul(256),
        ]
    }

    /// Terrain admission truncates toward zero and keeps inclusive bounds.
    /// Acceptance retains the current local position rather than applying height.
    pub fn accepts_terrain(&self, height: f64) -> Result<bool> {
        let height = height.trunc();
        ensure!(
            height.is_finite() && height >= f64::from(i32::MIN) && height <= f64::from(i32::MAX),
            "stable recovery terrain height outside integer map range"
        );
        Ok(self.start[1] <= height as i32 && height as i32 <= self.end[1])
    }
}

/// Kind byte of the original's ground-response record for a selected support: 0x40 for terrain.
pub(super) const TERRAIN_KIND: u8 = 0x40;

/// Kind byte of the ground-response record: zero without support, 0x40 for terrain and 0x20 for any
/// model or actor face.
fn ground_kind(support: Option<&SelectedSupport>) -> u8 {
    match support.map(SelectedSupport::source) {
        None => 0,
        Some(SupportSource::Terrain) => TERRAIN_KIND,
        Some(_) => 0x20,
    }
}

#[cfg(test)]
mod kind_tests {
    use super::*;

    #[test]
    fn the_response_kind_follows_the_support_source() {
        let support = |source| SelectedSupport::Fallback {
            source,
            normal: [0, 256, 0],
            actor_height: 0,
        };
        assert_eq!(ground_kind(None), 0);
        assert_eq!(ground_kind(Some(&support(SupportSource::Terrain))), 0x40);
        for source in [
            SupportSource::Model { model: 1, face: 2 },
            SupportSource::Actor { actor: 1, face: 2 },
        ] {
            assert_eq!(ground_kind(Some(&support(source))), 0x20);
        }
    }
}
