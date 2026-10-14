//! Collision-facing model records follow preservation's current-model lifecycle.
use super::super::Pose;
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_assets::animation::{Animations, ModelFrame};
use cyrus_collision::actor_models::ActorModelCollision;

/// Selected geometry and its stored transform, independent of later logical movement.
/// Position has eight fractional bits; radius is in original map units.
pub struct ModelPresentation {
    pub pose: Pose,
    pub radius: f32,
    /// None selects the actor's retained static model; Some selects RAAN geometry.
    pub animation: Option<ModelFrame>,
}

impl ModelPresentation {
    /// Stored instance transform for collision queries, with Q8 positions truncated to map units.
    /// The original row-major matrix is retained without renderer-axis conversion.
    pub fn collision_pose(&self) -> Result<cyrus_collision::collision_mesh::ModelPose> {
        use bevy::math::{Mat3, Vec3};
        let rows = self.pose.matrix.0.map(Vec3::from_array);
        cyrus_collision::collision_mesh::ModelPose::new(
            self.pose.position.map(|value| (value >> 8) as f32),
            Mat3::from_cols(rows[0], rows[1], rows[2]).transpose(),
        )
    }
}

/// Retained owners assigned by a selected support face. Presentation reads these
/// before the next ordinary ground stage clears them. IDs belong to this generation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelSupportOwners {
    pub actor: Option<ScriptActorId>,
    pub static_placement: Option<usize>,
}

pub(super) struct AdmissionState {
    filter: cyrus_collision::model_admission::ModelAdmission,
    secondary: Vec<ScriptActorId>,
    static_count: usize,
}

#[derive(Default)]
pub(super) struct ModelState {
    pub prepared: bool,
    pub support: ModelSupportOwners,
    pub static_radius: Option<f32>,
    pub animation_radius: Option<f32>,
    pub collision_flags: Option<u8>,
    pub current: Option<ModelPresentation>,
}

impl ModelState {
    pub fn show_static(&mut self, pose: Pose) {
        if let Some(radius) = self.static_radius {
            self.current = Some(ModelPresentation {
                pose,
                radius,
                animation: None,
            });
        }
    }
}

/// One admitted actor's ordered candidate lists and copied collision angles.
/// Prepare before its script wrapper, then reuse through late response. Query
/// origins and stored model poses remain owned by their respective stages.
pub struct OrdinaryCollisionQuery {
    actor: ScriptActorId,
    world: u32,
    content_id: String,
    pub(super) candidates: cyrus_collision::candidates::CollisionCandidates,
    pub(super) working_angles: [u16; 3],
}

/// Independently prepared late recovery candidates, bound to one actor and world.
/// This cannot be supplied to an ordinary support query.
pub struct StableRecoveryQuery {
    pub(super) prepared: OrdinaryCollisionQuery,
}

impl ScriptWorld {
    /// Prepare fresh stable-recovery admission from the current effective position
    /// and current model radius. Never reuses the ordinary update's retained list.
    pub fn prepare_stable_recovery_query(
        &mut self,
        id: ScriptActorId,
        scene: &crate::player::data::PlayerScene,
        data: &Animations,
        view: [i32; 2],
        actors: &[ScriptActorId],
    ) -> Result<StableRecoveryQuery> {
        Ok(StableRecoveryQuery {
            prepared: self.prepare_collision_query(id, scene, data, view, actors, true)?,
        })
    }

    /// Prepare collision admission once for an ordinary actor update. Later
    /// support/body stages require this actor-bound record and do not rerun
    /// spatial admission after movement or scripts. Captures working angles
    /// separately from the retained model's presentation matrix.
    pub fn prepare_ordinary_collision_query(
        &mut self,
        id: ScriptActorId,
        scene: &crate::player::data::PlayerScene,
        data: &Animations,
        view: [i32; 2],
        actors: &[ScriptActorId],
    ) -> Result<OrdinaryCollisionQuery> {
        self.prepare_collision_query(id, scene, data, view, actors, false)
    }

    fn prepare_collision_query(
        &mut self,
        id: ScriptActorId,
        scene: &crate::player::data::PlayerScene,
        data: &Animations,
        view: [i32; 2],
        actors: &[ScriptActorId],
        stable_recovery: bool,
    ) -> Result<OrdinaryCollisionQuery> {
        let index = self.resolve(id)?;
        ensure!(
            scene.world == self.world,
            "world {} ordinary collision scene identity mismatch",
            self.world
        );
        self.require_model_identity(data)?;
        ensure!(
            self.actors[index]
                .placement()
                .is_some_and(|placement| placement.object_type == 1),
            "world {} instance {index}: ordinary collision object path unavailable",
            self.world
        );
        if self.actors[index].collision.is_none() {
            self.prepare_ordinary_collision_shape(id, data, scene)?;
        }
        let spatial = self.spatial(id)?;
        let working_angles = spatial.angle_history;
        let model =
            self.model_presentation(id)?
                .map(|record| cyrus_collision::candidates::ModelBounds {
                    instance: cyrus_collision::candidates::ModelInstance::Runtime(index),
                    position: record.pose.position.map(|value| (value >> 8) as f32),
                    radius: record.radius,
                });
        let position = if stable_recovery && model.is_none() {
            spatial.local.position
        } else {
            spatial.query_pose()?.position
        };
        let query = cyrus_collision::candidates::CandidateQuery::new(position, model)?;
        let candidates = scene.static_collision.runtime_candidates(
            view,
            &query,
            &self.live_collision_inputs(actors)?,
            &[],
        )?;
        Ok(OrdinaryCollisionQuery {
            actor: id,
            world: self.world,
            content_id: self.content_id.clone(),
            candidates,
            working_angles,
        })
    }

    pub(super) fn require_collision_query(
        &self,
        id: ScriptActorId,
        query: &OrdinaryCollisionQuery,
        scene: &crate::player::data::PlayerScene,
        data: &Animations,
    ) -> Result<()> {
        self.resolve(id)?;
        ensure!(
            query.actor == id
                && query.world == self.world
                && scene.world == self.world
                && query.content_id == self.content_id,
            "world {} instance {}: collision query belongs to another actor or world",
            self.world,
            id.index
        );
        self.require_model_identity(data)
    }

    pub(super) fn ordinary_collision_poses(
        &self,
        query: &OrdinaryCollisionQuery,
    ) -> Result<std::collections::BTreeMap<usize, cyrus_collision::collision_mesh::ModelPose>> {
        let mut poses = std::collections::BTreeMap::new();
        for candidate in &query.candidates.faces {
            if let cyrus_collision::candidates::CandidateOwner::Actor(index) = candidate.owner {
                if poses.contains_key(&index) {
                    continue;
                }
                if let Some(record) = self.model_presentation(self.id(index)?)? {
                    poses.insert(index, record.collision_pose()?);
                }
            }
        }
        Ok(poses)
    }

    /// Fresh-world PVO secondary IDs, in preservation's load-time Z/X cell order.
    /// Persistent actors follow cell actors; only attr_non_moving actors are retained.
    /// The table is derived from initial placements, never from later logical movement.
    /// Dynamic allocation and save restoration require their own retained table.
    pub fn initial_pvo_secondary_actors(&self) -> Result<Vec<ScriptActorId>> {
        Ok(super::super::order::ActorLists::new(&self.actors)?
            .secondary_actors(&self.actors, self.generation))
    }

    /// Bind a fresh world's PVO table once. Moving actors and later attribute
    /// writes cannot rebuild or renumber the load-time secondary table.
    pub fn prepare_model_admission(
        &mut self,
        visibility: &cyrus_assets::collision::ModelVisibility,
        static_count: usize,
    ) -> Result<()> {
        ensure!(
            self.model_admission.is_none(),
            "world {} model admission already prepared",
            self.world
        );
        let secondary = self.initial_pvo_secondary_actors()?;
        let filter = cyrus_collision::model_admission::ModelAdmission::new(visibility.clone())?;
        self.model_admission = Some(AdmissionState {
            filter,
            secondary,
            static_count,
        });
        Ok(())
    }

    pub fn model_support(&self, id: ScriptActorId) -> Result<ModelSupportOwners> {
        Ok(self.actors[self.resolve(id)?].model.support)
    }

    /// Store face-query ownership without recomputing it from current coordinates.
    /// None clears an owner; a stale actor identity is rejected before mutation.
    pub fn set_model_support(
        &mut self,
        id: ScriptActorId,
        support: ModelSupportOwners,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        if let Some(actor) = support.actor {
            self.resolve(actor)?;
        }
        if let Some(placement) = support.static_placement {
            let admission = self
                .model_admission
                .as_ref()
                .context("static support ownership requires model admission preparation")?;
            ensure!(
                placement < admission.static_count,
                "world {} support placement {placement} outside MPSO table",
                self.world
            );
        }
        self.actors[index].model.support = support;
        Ok(())
    }

    /// Refresh camera-selected models before ordinary simulation. Camera coordinates
    /// are Q8; PVO lookup truncates toward zero, while map membership uses its own rule.
    /// PVO-rejected ordinary actors retain records; rejected animation support clears them.
    pub fn refresh_automatic_models(&mut self, data: &Animations, camera: [i32; 3]) -> Result<()> {
        self.require_model_identity(data)?;
        let admission = self
            .model_admission
            .as_ref()
            .context("model admission preparation unavailable; re-export world")?;
        let selected = admission
            .filter
            .select(camera.map(|value| value / 256))
            .to_vec();
        let limit = admission.static_count + admission.secondary.len();
        ensure!(
            selected.iter().all(|&id| usize::from(id) < limit),
            "world {} PVO selected object outside prepared tables",
            self.world
        );
        let order = self
            .ordinary_actors([camera[0] >> 8, camera[2] >> 8])?
            .to_vec();
        for id in order {
            let actor = &self.actors[id.index];
            if actor
                .placement()
                .is_none_or(|placement| placement.object_type != 1)
            {
                continue;
            }
            if !selected.is_empty() && actor.attributes()[122] != 0 {
                continue;
            }
            let allowed = self.animation_support_admitted(id, &selected)?;
            if !allowed && data.actor(actor.name())?["model_load_path"] == "animation_table" {
                self.actors[id.index].model.current = None;
                self.actors[id.index].model.animation_radius = None;
                continue;
            }
            if let Err(error) = self.refresh_model_presentations(data, &[id]) {
                self.actors[id.index].update_failure = Some(format!(
                    "world {} instance {} actor {} model presentation: {error:#}",
                    self.world,
                    id.index,
                    self.actors[id.index].name()
                ));
            }
        }
        Ok(())
    }

    fn animation_support_admitted(&self, id: ScriptActorId, selected: &[u16]) -> Result<bool> {
        if selected.is_empty() {
            return Ok(true);
        }
        let admission = self
            .model_admission
            .as_ref()
            .context("model admission unavailable")?;
        let support = self.model_support(id)?;
        if let Some(placement) = support.static_placement {
            return Ok(selected
                .iter()
                .any(|&value| usize::from(value) == placement));
        }
        if let Some(actor) = support.actor
            && self.actors[self.resolve(actor)?].attributes()[122] != 0
        {
            return Ok(selected.iter().any(|&value| {
                usize::from(value)
                    .checked_sub(admission.static_count)
                    .and_then(|index| admission.secondary.get(index))
                    == Some(&actor)
            }));
        }
        Ok(true)
    }

    pub(super) fn model_resources_prepared(&self) -> bool {
        self.actors
            .iter()
            .filter(|actor| {
                actor
                    .placement()
                    .is_some_and(|placement| placement.object_type == 1)
            })
            .all(|actor| actor.model.prepared)
    }
    /// Bind retained static resources without establishing presentation eligibility.
    /// The caller supplies geometry for this world; animation resources remain frame-selected.
    pub fn prepare_model_resources(
        &mut self,
        data: &Animations,
        statics: &ActorModelCollision,
    ) -> Result<()> {
        self.require_model_identity(data)?;
        let mut radii = Vec::new();
        for (index, actor) in self.actors.iter().enumerate() {
            if actor
                .placement()
                .is_none_or(|placement| placement.object_type != 1)
            {
                continue;
            }
            ensure!(
                !actor.model.prepared,
                "world {} instance {index}: model resources already prepared",
                self.world
            );
            let radius = match data.actor(actor.name())?["model_load_path"].as_str() {
                Some("static" | "none") => statics.loaded_radius(index)?,
                Some("animation_table") => None,
                path => anyhow::bail!(
                    "world {} instance {index}: model path {path:?} unsupported",
                    self.world
                ),
            };
            radii.push((index, radius, statics.collision_flags(index)?));
        }
        for (index, radius, flags) in radii {
            self.actors[index].model.static_radius = radius;
            self.actors[index].model.collision_flags = Some(flags);
            self.actors[index].model.prepared = true;
        }
        Ok(())
    }

    pub(super) fn require_model_identity(&self, data: &Animations) -> Result<()> {
        data.require_version()?;
        ensure!(
            data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} model presentation asset identity mismatch",
            self.world
        );
        Ok(())
    }

    /// Refresh exactly the caller-admitted presentation traversal, before ordinary updates.
    /// Omitted actors retain their prior record. Hidden/update-excluded admitted actors lose it.
    /// This does not derive PVO admission, ordinary eligibility, or drawing submission.
    pub fn refresh_model_presentations(
        &mut self,
        data: &Animations,
        admitted: &[ScriptActorId],
    ) -> Result<()> {
        self.require_model_identity(data)?;
        for &id in admitted {
            let index = self.resolve(id)?;
            self.actors[index].model.current = None;
            let actor = &self.actors[index];
            let control = actor.control.as_ref().with_context(|| {
                format!(
                    "world {} instance {index}: model control unavailable",
                    self.world
                )
            })?;
            if control.hidden || control.object_update_excluded {
                continue;
            }
            ensure!(
                actor.model.prepared,
                "world {} instance {index}: model resources not prepared",
                self.world
            );
            ensure!(
                actor.attributes()[1] & 4 == 0
                    || actor
                        .surface
                        .is_some_and(|surface| !(1..=5).contains(&surface) && surface != 11),
                "world {} instance {index}: water-dependent model position owner unavailable",
                self.world
            );
            let static_radius = actor.model.static_radius;
            let path = data.actor(actor.name())?["model_load_path"].as_str();
            let animation = match path {
                Some("static" | "none") => {
                    if static_radius.is_none() {
                        continue;
                    }
                    None
                }
                Some("animation_table") => {
                    self.actors[index].model.animation_radius = None;
                    let Some(frame) = self.animation_frame(id)? else {
                        continue;
                    };
                    Some(data.resolve_map_frame(index, frame)?)
                }
                path => anyhow::bail!(
                    "world {} instance {index}: model path {path:?} unsupported",
                    self.world
                ),
            };
            let radius = if let Some(frame) = &animation {
                data.models
                    .get(&frame.model)
                    .with_context(|| {
                        format!(
                            "world {} instance {index} model {} unavailable",
                            self.world, frame.model
                        )
                    })?
                    .radius
                    .context("animation header radius unavailable; re-export world")?
            } else {
                static_radius.context("retained static model unavailable")?
            };
            if self.spatial(id)?.parent.is_none() {
                self.refresh_transform(id)?;
            }
            let pose = self.spatial(id)?.query_pose()?;
            if animation.is_some() {
                self.actors[index].model.animation_radius = Some(radius);
            }
            self.actors[index].model.current = Some(ModelPresentation {
                pose,
                radius,
                animation,
            });
        }
        Ok(())
    }

    /// Current collision-facing model, retaining its presentation-time pose.
    pub fn model_presentation(&self, id: ScriptActorId) -> Result<Option<&ModelPresentation>> {
        let index = self.resolve(id)?;
        Ok(self.actors[index].model.current.as_ref())
    }

    /// Retained selected resource radius, including hidden actors without a current record.
    /// An absent animated frame clears it; later logical movement does not change it.
    pub fn model_radius(&self, id: ScriptActorId) -> Result<Option<f32>> {
        let index = self.resolve(id)?;
        let model = &self.actors[index].model;
        ensure!(
            model.prepared,
            "world {} instance {index}: model resources not prepared",
            self.world
        );
        Ok(model.static_radius.or(model.animation_radius))
    }

    /// Live ordinary-map inputs in caller-established traversal order. Current model
    /// records supply bounds; logical movement cannot silently refresh those bounds.
    /// An absent current model contributes no candidate and needs no collision metadata.
    /// Present standalone player and special-object collision ownership remains unsupported.
    pub fn live_collision_inputs(
        &self,
        order: &[ScriptActorId],
    ) -> Result<Vec<cyrus_collision::candidates::LiveActor>> {
        use cyrus_collision::candidates::{LiveActor, ModelBounds, ModelInstance};
        let mut inputs = Vec::new();
        for &id in order {
            let index = self.resolve(id)?;
            let actor = &self.actors[index];
            // Candidate admission rejects an absent current record before reading
            // model-path metadata or preparing any actor collision response.
            let Some(record) = actor.model.current.as_ref() else {
                continue;
            };
            ensure!(
                actor
                    .placement()
                    .is_some_and(|placement| placement.object_type == 1),
                "world {} instance {index}: live collision object path unavailable",
                self.world
            );
            let control = actor
                .control
                .as_ref()
                .context("live collision control unavailable")?;
            let flags = actor.model.collision_flags.with_context(|| {
                format!(
                    "world {} instance {index}: live collision metadata unavailable",
                    self.world
                )
            })?;
            inputs.push(LiveActor {
                actor: index,
                model: Some(ModelBounds {
                    instance: ModelInstance::Runtime(index),
                    position: record.pose.position.map(|value| (value >> 8) as f32),
                    radius: record.radius,
                }),
                // Designated-player exclusion does not select an ordinary MPOB actor.
                ordinary_excluded: false,
                animation_excluded: control.hidden,
                attribute_55: actor.attributes()[55],
                static_model_path: actor.model.static_radius.is_some(),
                collision_flags: flags,
            });
        }
        Ok(inputs)
    }
}

/// Bind the prepared world's portable geometry to logical actors once per generation.
/// This preparation neither creates current records nor admits collision updates.
pub(crate) fn prepare_resources(
    prepared: bevy::prelude::Res<cyrus_world::lifecycle::PreparedWorld>,
    mut actors: bevy::prelude::ResMut<super::ScriptActors>,
    animations: bevy::prelude::Res<bevy::prelude::Assets<Animations>>,
    request: Option<bevy::prelude::Res<crate::player::data::PlayerSceneRequest>>,
    scenes: Option<bevy::prelude::Res<bevy::prelude::Assets<crate::player::data::PlayerScene>>>,
) {
    if actors.error.is_some() {
        return;
    }
    let Some(world) = actors.current.as_mut() else {
        return;
    };
    if world.model_resources_prepared() {
        return;
    }
    let (Some(request), Some(scenes)) = (request, scenes) else {
        return;
    };
    if request.0.path() != Some(&prepared.player_path) {
        return;
    }
    let (Some(scene), Some(data)) = (scenes.get(&request.0), animations.get(&prepared.animations))
    else {
        return;
    };
    let result = (|| -> Result<()> {
        ensure!(
            scene.world == world.world,
            "world {} model geometry belongs to world {}",
            world.world,
            scene.world
        );
        let models = scene
            .actor_collision
            .as_ref()
            .context("live collision geometry unavailable; re-export world")?;
        let visibility = scene
            .collision
            .model_visibility
            .as_ref()
            .context("model visibility preparation unavailable; re-export world")?;
        world.prepare_model_resources(data, models)?;
        world.initialize_collision_shapes(data, scene)?;
        world.prepare_model_admission(visibility, scene.collision.placements.len())
    })();
    if let Err(error) = result {
        actors.error = Some(format!("automatic model resource preparation: {error:#}"));
    }
}
