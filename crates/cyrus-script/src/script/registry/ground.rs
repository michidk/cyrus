//! Ordinary ground queries retain model ownership and selected-player fallback history.
use super::{ScriptActorId, ScriptWorld};
use crate::script::JumpMotion;
use anyhow::{Context, Result, ensure};
use cyrus_collision::ground::SurfaceClass;
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

/// One bounds record: the extents, totals and center offset the actor's embedded record holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoundsRecord {
    pub positive_extent: [i32; 3],
    pub negative_extent: [i32; 3],
    pub total_extent: [i32; 3],
    pub center_offset: [i32; 3],
}

/// The three bounds an animated actor retains: its initial (primary) bounds and the secondary and
/// tertiary ones its RAHD record names. Every ordinary update copies one of them into the live
/// record by the current animation group's class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RetainedBounds {
    pub primary: BoundsRecord,
    pub secondary: Option<BoundsRecord>,
    pub tertiary: Option<BoundsRecord>,
}

impl RetainedBounds {
    /// The bounds the group class selects: class 2 the tertiary when present, class 1 the
    /// secondary when present, otherwise the primary.
    pub fn for_group_class(&self, class: u8) -> &BoundsRecord {
        match class {
            2 => self.tertiary.as_ref().unwrap_or(&self.primary),
            1 => self.secondary.as_ref().unwrap_or(&self.primary),
            _ => &self.primary,
        }
    }
}

/// Initial ordinary collision shape, distinct from the currently rendered model.
/// Extents and sphere coordinates are original map units. Model-less initialization
/// has one body sphere and no separately prepared additional sphere.
pub struct OrdinaryCollisionShape {
    pub positive_extent: [i32; 3],
    pub negative_extent: [i32; 3],
    /// The bounds record's totals (`size.size`): the sum of the two extents at creation, read back
    /// by the wall-avoidance probe's radius, and plain words a script may overwrite.
    pub total_extent: [i32; 3],
    /// The bounds record's center offset (`size.pos`): added unrotated to the residual body query's
    /// origin and to the selected player's fallback support sphere. Sphere construction ignores it.
    pub center_offset: [i32; 3],
    /// The retained primary, secondary and tertiary bounds the living-animation refresh copies from.
    pub retained: RetainedBounds,
    pub body: Vec<BodySphere>,
    pub additional: Option<BodySphere>,
}

#[derive(serde::Deserialize)]
struct MpszBounds {
    total_extent: [i32; 3],
    pos_extent: [i32; 3],
    neg_extent: [i32; 3],
    center_offset: [i32; 3],
}

fn mpsz_bounds(
    data: &cyrus_assets::animation::Animations,
    index: usize,
    scale: i32,
) -> Result<(ActorBounds, [i32; 3])> {
    let value = data.metadata["mpsz_entries"]
        .as_array()
        .and_then(|entries| entries.get(index))
        .with_context(|| format!("ordinary collision MPSZ record {index} absent"))?;
    let record: MpszBounds = serde_json::from_value(value.clone())
        .with_context(|| format!("ordinary collision invalid MPSZ record {index}"))?;
    for axis in 0..3 {
        ensure!(
            i64::from(record.pos_extent[axis]) + i64::from(record.neg_extent[axis])
                == i64::from(record.total_extent[axis]),
            "ordinary collision MPSZ record {index} axis {axis}: inconsistent total"
        );
    }
    Ok((
        ActorBounds::new(record.pos_extent, record.neg_extent, scale)?,
        record.center_offset,
    ))
}

impl RetainedBounds {
    /// Bounds with only the primary record, as an actor without secondary or tertiary shapes.
    pub fn only(primary: BoundsRecord) -> Self {
        Self {
            primary,
            secondary: None,
            tertiary: None,
        }
    }
}

impl ScriptWorld {
    /// The original's per-update bounds refresh: an actor with a combat and an animation record
    /// copies the retained bounds its current group class selects into its live bounds record, so a
    /// script-written extent lasts only until the next update. Spheres are not rebuilt.
    pub fn refresh_living_bounds(&mut self, id: ScriptActorId) -> Result<()> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        if actor
            .control
            .as_ref()
            .is_none_or(|control| control.combat.is_none())
        {
            return Ok(());
        }
        let Some(group) = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group())
        else {
            return Ok(());
        };
        let Some(shape) = actor.collision.as_ref() else {
            return Ok(());
        };
        let record = *shape
            .retained
            .for_group_class(cyrus_world::animation::group_class(group));
        let shape = self.actors[index]
            .collision
            .as_mut()
            .expect("checked collision shape");
        shape.positive_extent = record.positive_extent;
        shape.negative_extent = record.negative_extent;
        shape.total_extent = record.total_extent;
        shape.center_offset = record.center_offset;
        Ok(())
    }

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
        self.map_updates.learn_capacity(
            scene
                .settings
                .max_remap_objects
                .and_then(|capacity| usize::try_from(capacity).ok()),
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
            !matches!(
                actor.source,
                super::ScriptActorSource::Player | super::ScriptActorSource::AlternatePlayer
            ) || path == "animation_table",
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
            let (bounds, center_offset) = if authored {
                let primary = usize::try_from(primary)
                    .context("ordinary collision primary bounds index negative")?;
                mpsz_bounds(data, primary, scale)?
            } else {
                let system_scale = scene
                    .collision
                    .sphere_object_scale
                    .context("ordinary collision sphere_object_scale absent; re-export world")?;
                (
                    scene
                        .actor_collision
                        .as_ref()
                        .context("ordinary collision live geometry absent; re-export world")?
                        .initial_bounds(index, system_scale, scale)?,
                    [0; 3],
                )
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
                total_extent: std::array::from_fn(|axis| {
                    positive_extent[axis].wrapping_add(negative_extent[axis])
                }),
                center_offset,
                retained: RetainedBounds::only(BoundsRecord {
                    positive_extent,
                    negative_extent,
                    total_extent: std::array::from_fn(|axis| {
                        positive_extent[axis].wrapping_add(negative_extent[axis])
                    }),
                    center_offset,
                }),
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
                total_extent: [24; 3],
                center_offset: [0; 3],
                retained: RetainedBounds::only(BoundsRecord {
                    positive_extent: [12; 3],
                    negative_extent: [12; 3],
                    total_extent: [24; 3],
                    center_offset: [0; 3],
                }),
                body: vec![BodySphere {
                    center: [0; 3],
                    radius: 12,
                }],
                additional: None,
            }
        };
        let mut shape = shape;
        if path == "animation_table" {
            // The animated loader retains the bounds the second and third RAHD indices name.
            for (key, slot) in [
                ("mpsz_bounds_0", &mut shape.retained.secondary),
                ("mpsz_bounds_1", &mut shape.retained.tertiary),
            ] {
                // An index of -1 (or an absent key) leaves the shape absent.
                let Some(record) = definition
                    .get(key)
                    .and_then(|value| value.as_i64())
                    .filter(|record| *record >= 0)
                else {
                    continue;
                };
                let scale = definition["raex"]["unknown_12"]
                    .as_i64()
                    .context("ordinary collision RAEX radius scale absent")?;
                let scale = i32::from(
                    i16::try_from(scale)
                        .context("ordinary collision RAEX radius scale outside signed word")?,
                );
                let record = usize::try_from(record)
                    .with_context(|| format!("ordinary collision {key} index negative"))?;
                let (bounds, center_offset) = mpsz_bounds(data, record, scale)?;
                let (positive_extent, negative_extent) =
                    (bounds.positive_extent(), bounds.negative_extent());
                *slot = Some(BoundsRecord {
                    positive_extent,
                    negative_extent,
                    total_extent: std::array::from_fn(|axis| {
                        positive_extent[axis].wrapping_add(negative_extent[axis])
                    }),
                    center_offset,
                });
            }
        }
        self.actors[index].collision = Some(shape);
        Ok(())
    }

    /// Contact body spheres in map units, selected by the retained living group.
    /// Preservation's player contact-body contract selects tertiary MPSZ only for
    /// reaction/death class 2 with combat state. Ordinary support remains a separate owner.
    pub(super) fn contact_body_spheres<'a>(
        &'a self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        post_collide_height: u32,
    ) -> Result<std::borrow::Cow<'a, [BodySphere]>> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let primary = &self.ordinary_collision_shape(id)?.body;
        let reaction = actor
            .control
            .as_ref()
            .is_some_and(|control| control.combat.is_some())
            && actor
                .animation
                .as_ref()
                .and_then(|animation| animation.playback.current_group())
                .is_some_and(|group| cyrus_world::animation::group_class(group) == 2);
        if !reaction {
            return Ok(std::borrow::Cow::Borrowed(primary));
        }
        let definition = data.actor(actor.name())?;
        let alternate = definition
            .get("mpsz_bounds_1")
            .map(|value| {
                value
                    .as_i64()
                    .context("contact tertiary MPSZ index invalid")
            })
            .transpose()?;
        let alternate = match alternate {
            None | Some(-1) => return Ok(std::borrow::Cow::Borrowed(primary)),
            Some(index) => usize::try_from(index).with_context(|| {
                format!(
                    "world {} instance {} actor {} contact mpsz_bounds_1 {index} invalid",
                    self.world,
                    id.index,
                    actor.name()
                )
            })?,
        };
        let scale = definition["raex"]["unknown_12"]
            .as_i64()
            .context("contact RAEX radius scale absent")?;
        let scale = i32::from(
            i16::try_from(scale).context("contact RAEX radius scale outside signed word")?,
        );
        let body = mpsz_bounds(data, alternate, scale)
            .and_then(|(bounds, _)| bounds.body_spheres(post_collide_height))
            .with_context(|| {
                format!(
                    "world {} instance {index} actor {} contact mpsz_bounds_1 {alternate}",
                    self.world,
                    actor.name()
                )
            })?;
        Ok(std::borrow::Cow::Owned(body))
    }

    pub fn ordinary_collision_shape(&self, id: ScriptActorId) -> Result<&OrdinaryCollisionShape> {
        let index = self.resolve(id)?;
        self.actors[index].collision.as_ref().with_context(|| {
            format!(
                "world {} instance {index} actor {}: ordinary collision shape not prepared",
                self.world,
                self.actors[index].name()
            )
        })
    }

    pub(super) fn initialize_collision_shapes(
        &mut self,
        data: &cyrus_assets::animation::Animations,
        scene: &crate::player::data::PlayerScene,
    ) -> Result<()> {
        self.standing_height = Some(scene.settings.standing_height);
        for index in 0..self.actors.len() {
            // The selected player is prepared with the rest: other actors' collision responses
            // read its shape before its own update has run.
            if !self.actors[index].ordinary_collision_role() {
                continue;
            }
            let id = self.id(index)?;
            if self.actors[index].collision.is_none()
                && let Err(error) = self.prepare_ordinary_collision_shape(id, data, scene)
            {
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
            self.query_position(id)?,
            spatial.local.wrapped_angles()[1],
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
                if support.classification() == SurfaceClass::Floor {
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
            // A water cell or an outside-grid probe has no terrain support. The
            // original recovery branch rejects it and lets its caller restore
            // the retained stable position; it is not malformed scene data.
            if let Some(height) =
                terrain.sample(probe.start.map(|coordinate| i64::from(coordinate) * 256))
            {
                return probe.accepts_terrain(height.height);
            }
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
        let position = self.query_position(id)?;
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
                == JumpMotion::ROPE)
                .then_some(128),
        )?;
        let terrain = query
            .scene
            .terrain
            .as_ref()
            .and_then(|terrain| terrain.sample(position.map(i64::from)));
        let mut selected = probe.select(terrain, &contacts);
        if self.selected_player_index() == Some(id.index) {
            let history = &self.actors[id.index]
                .player_movement
                .as_ref()
                .context("selected ground query requires initialized movement history")?
                .history;
            if selected.is_none() {
                let fallback = cyrus_collision::ground::FallbackProbe::new(
                    position.map(i64::from),
                    self.ordinary_collision_shape(id)?.center_offset,
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
                        == JumpMotion::ROPE)
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

/// Kind byte of the selected player's ground-response record (preservation SOUP.md
/// "PlayerOnScape"). The ground update clears it and sets the kind of the chosen support.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct GroundKind(u8);

impl GroundKind {
    pub const NONE: Self = Self(0);
    /// A model or actor face.
    pub const FACE: Self = Self(0x20);
    pub const TERRAIN: Self = Self(0x40);
}

fn ground_kind(support: Option<&SelectedSupport>) -> GroundKind {
    match support.map(SelectedSupport::source) {
        None => GroundKind::NONE,
        Some(SupportSource::Terrain) => GroundKind::TERRAIN,
        Some(_) => GroundKind::FACE,
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
        assert_eq!(ground_kind(None), GroundKind::NONE);
        assert_eq!(
            ground_kind(Some(&support(SupportSource::Terrain))),
            GroundKind::TERRAIN
        );
        for source in [
            SupportSource::Model { model: 1, face: 2 },
            SupportSource::Actor { actor: 1, face: 2 },
        ] {
            assert_eq!(ground_kind(Some(&support(source))), GroundKind::FACE);
        }
    }
}
