//! Collision-facing model records follow preservation's current-model lifecycle.
//!
//! Position and geometry have distinct lifetimes; see the [engine contract].
//! Automatic model preparation applies surface 1–4 displacement using the matching
//! player scene's terrain and effective swim depth, and dispatches water effects
//! through the shared pool without drawing random values. Selected attr105-one
//! prepasses clear direction before null-source health; surface5/11 retains its
//! separate native-source route. Family refresh owns attached coordinates and bypasses
//! water positioning; linked model owner/offset positioning remains unavailable.
//!
//! [engine contract]: https://michidk.github.io/redguard-preservation/engine/SOUP.html#current-model-presentation
use super::super::Pose;
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_assets::animation::{Animations, ModelFrame};
use cyrus_collision::actor_models::ActorModelCollision;

/// Selected geometry and its stored transform, independent of later logical movement.
/// The captured source pose has eight fractional bits. Stored model position and
/// radius use original map units; presentation displacement never moves the actor.
pub struct ModelPresentation {
    /// Captured source pose and orientation; its Q8 position does not include model displacement.
    pub pose: Pose,
    /// Stored map-unit position shared by model geometry, bounds and rendering.
    pub position: [f32; 3],
    pub radius: f32,
    /// None selects the actor's retained static model; Some selects RAAN geometry.
    pub animation: Option<ModelFrame>,
}

impl ModelPresentation {
    /// Stored instance transform for collision queries, including presentation displacement.
    /// The original row-major matrix is retained without renderer-axis conversion.
    pub fn collision_pose(&self) -> Result<cyrus_collision::collision_mesh::ModelPose> {
        use bevy::math::{Mat3, Vec3};
        let rows = self.pose.matrix.0.map(Vec3::from_array);
        cyrus_collision::collision_mesh::ModelPose::new(
            self.position,
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

pub(super) struct ModelState {
    pub prepared: bool,
    pub support: ModelSupportOwners,
    /// Separate ledge support, retained across ordinary ground-owner clearing.
    pub ledge_support: Option<ScriptActorId>,
    pub static_radius: Option<f32>,
    pub animation_radius: Option<f32>,
    pub collision_flags: Option<u8>,
    /// Stored map-unit coordinates survive loss of the current geometry record.
    /// None denotes an older snapshot that did not preserve this owner.
    pub position: Option<[f32; 3]>,
    pub current: Option<ModelPresentation>,
    pub weapon: Option<super::super::attachment::EquippedWeapon>,
}

impl Default for ModelState {
    fn default() -> Self {
        Self {
            prepared: false,
            support: ModelSupportOwners::default(),
            ledge_support: None,
            static_radius: None,
            animation_radius: None,
            collision_flags: None,
            position: Some([0.0; 3]),
            current: None,
            weapon: None,
        }
    }
}

impl ModelState {
    pub fn show_static(&mut self, pose: Pose) {
        let position = pose.position.map(|value| (value >> 8) as f32);
        self.position = Some(position);
        if let Some(radius) = self.static_radius {
            self.current = Some(ModelPresentation {
                position,
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
            self.actors[index].ordinary_collision_role(),
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
                    position: record.position,
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

    /// Run this frame's family/group stage, then refresh camera-selected models before simulation.
    /// Call once per admitted world-loop iteration; family refresh also publishes hidden actors.
    /// Camera coordinates are Q8; PVO lookup truncates toward zero, while map membership
    /// uses its own rule.
    /// PVO-rejected ordinary actors retain records; rejected animation support clears them.
    /// `scene` must belong to this world when terrain or swim-depth inputs are needed.
    /// Missing cache settings, previous model coordinates or effect services fail explicitly.
    pub fn refresh_automatic_models(
        &mut self,
        data: &Animations,
        camera: [i32; 3],
        scene: Option<&crate::player::data::PlayerScene>,
        inputs: super::ScriptInputs<'_>,
        mut random: Option<&mut crate::random::EngineRandom>,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
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
        self.refresh_families()?;
        self.globals.group_sync.update(&self.actors);
        let order = self
            .ordinary_actors([camera[0] >> 8, camera[2] >> 8])?
            .to_vec();
        for id in order {
            let actor = &self.actors[id.index];
            if !matches!(
                actor.source,
                super::ScriptActorSource::Player | super::ScriptActorSource::AlternatePlayer
            ) && actor
                .placement()
                .is_none_or(|placement| placement.object_type != 1)
            {
                continue;
            }
            if !selected.is_empty() && actor.attributes()[122] != 0 {
                continue;
            }
            let allowed = self.animation_support_admitted(id, &selected)?;
            let rejected_animation =
                !allowed && data.actor(actor.name())?["model_load_path"] == "animation_table";
            let refreshed = (|| -> Result<()> {
                self.refresh_model_presentations_with_health(
                    data,
                    &[id],
                    Some(inputs),
                    scene,
                    random.as_deref_mut(),
                    Some(sounds),
                    !rejected_animation,
                )?;
                if rejected_animation {
                    self.actors[id.index].model.current = None;
                    self.actors[id.index].model.animation_radius = None;
                    return Ok(());
                }
                let actor = &self.actors[id.index];
                if actor.attributes[23] == 0
                    || actor
                        .model
                        .current
                        .as_ref()
                        .is_none_or(|model| model.animation.is_none())
                {
                    return Ok(());
                }
                let selected_player = self
                    .selected_player_index()
                    .context("weapon model selected player unavailable")?;
                if id.index == selected_player && actor.attributes[105] == 1 {
                    return Ok(());
                }
                let hud = inputs.hud.context("weapon model inventory unavailable")?;
                let weapon = super::WeaponInputs {
                    selected_player: self.id(selected_player)?,
                    item_system_ready: hud.inventory_ready()?,
                    shell_execution: inputs
                        .shell_execution
                        .context("weapon model shell context unavailable")?,
                };
                self.update_actor_weapon_transition(
                    id,
                    weapon,
                    hud,
                    inputs
                        .nominal_ticks
                        .context("weapon model nominal ticks unavailable")?,
                    sounds,
                )?;
                self.refresh_actor_weapon_attachment(id, data, weapon)
            })();
            if let Err(error) = refreshed {
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

    /// Prepare a standalone player role's animated model owner without a
    /// fictitious MPOB placement. Fresh animated players retain the allocator-zero
    /// collision metadata described in preservation's animation rules.
    /// Caller admission still owns presentation timing.
    pub fn prepare_player_model_resources(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
    ) -> Result<()> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        ensure!(
            matches!(
                self.actors[index].source,
                super::ScriptActorSource::Player | super::ScriptActorSource::AlternatePlayer
            ),
            "world {} instance {index}: standalone player model required",
            self.world
        );
        ensure!(
            !self.actors[index].model.prepared,
            "world {} instance {index}: model resources already prepared",
            self.world
        );
        ensure!(
            matches!(
                data.actor(self.actors[index].name())?["model_load_path"].as_str(),
                Some("animation_table" | "none")
            ),
            "world {} standalone player: model path unsupported",
            self.world
        );
        self.actors[index].model.collision_flags = Some(0);
        self.actors[index].model.prepared = true;
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
        self.refresh_model_presentations_with_health(data, admitted, None, None, None, None, true)
    }

    #[allow(clippy::too_many_arguments)] // Model publication shares caller-owned scene, health, random and sound services.
    fn refresh_model_presentations_with_health(
        &mut self,
        data: &Animations,
        admitted: &[ScriptActorId],
        inputs: Option<super::ScriptInputs<'_>>,
        scene: Option<&crate::player::data::PlayerScene>,
        mut random: Option<&mut crate::random::EngineRandom>,
        mut sounds: Option<&mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>>,
        publish_geometry: bool,
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
            let static_radius = actor.model.static_radius;
            let path = data.actor(actor.name())?["model_load_path"].as_str();
            let animation = match path {
                Some("static" | "none") => None,
                Some("animation_table") => {
                    self.actors[index].model.animation_radius = None;
                    let Some(frame) = self.animation_frame(id)? else {
                        continue;
                    };
                    if !publish_geometry {
                        None
                    } else {
                        Some(if self.actors[index].placement().is_none() {
                            data.resolve_frame(self.actors[index].name(), frame)?
                        } else {
                            data.resolve_map_frame(index, frame)?
                        })
                    }
                }
                path => anyhow::bail!(
                    "world {} instance {index}: model path {path:?} unsupported",
                    self.world
                ),
            };
            let radius = if let Some(frame) = &animation {
                Some(
                    data.models
                        .get(&frame.model)
                        .with_context(|| {
                            format!(
                                "world {} instance {index} model {} unavailable",
                                self.world, frame.model
                            )
                        })?
                        .radius
                        .context("animation header radius unavailable; re-export world")?,
                )
            } else {
                static_radius.filter(|_| publish_geometry)
            };
            let actor = &self.actors[index];
            let family_position = actor.attributes()[26] != 0;
            if family_position {
                ensure!(
                    self.spatial(id)?.parent.is_some() && self.spatial(id)?.effective.is_some(),
                    "world {} instance {index}: family model position owner unavailable",
                    self.world
                );
            }
            // A linked actor takes its owner-relative position and skips water processing.
            let linked_position = if family_position {
                None
            } else {
                self.linked_model_position(index)?
            };
            let water_surface =
                if family_position || linked_position.is_some() || actor.attributes()[1] & 4 == 0 {
                    None
                } else {
                    let surface = actor.surface.context("model surface unavailable")?;
                    matches!(surface, 1..=5 | 11).then_some(surface)
                };
            if water_surface.is_some() {
                ensure!(
                    inputs.is_some(),
                    "world {} instance {index}: water-dependent model position owner unavailable",
                    self.world
                );
                ensure!(
                    actor.attributes()[26] == 0
                        && self.spatial(id)?.parent.is_none()
                        && self.spatial(id)?.linked_position.is_none(),
                    "world {} instance {index}: attached water model position unavailable",
                    self.world
                );
            }
            let selected = self.selected_player_index() == Some(index);
            if selected
                && self.actors[index].attributes()[105] == 1
                && let Some(1..=4) = water_surface
            {
                self.actors[index]
                    .control
                    .as_mut()
                    .expect("validated control")
                    .directional_state = [0; 2];
                if matches!(water_surface, Some(2 | 4)) {
                    self.apply_selected_death_health(
                        id,
                        -255,
                        None,
                        inputs.expect("validated model health services"),
                        random.as_deref_mut(),
                        sounds
                            .as_deref_mut()
                            .context("model health sound executor unavailable")?,
                    )?;
                }
            }
            if self.spatial(id)?.parent.is_none() {
                self.refresh_transform(id)?;
            }
            let pose = self.spatial(id)?.query_pose()?;
            let previous_position = self.actors[index].model.position;
            if animation.is_some() {
                self.actors[index].model.animation_radius = radius;
            }
            let position = if family_position {
                previous_position.context("family model coordinates unavailable")?
            } else if let Some(position) = linked_position {
                position
            } else {
                pose.position.map(|value| (value >> 8) as f32)
            };
            self.actors[index].model.position = Some(position);
            self.actors[index].model.current = radius.map(|radius| ModelPresentation {
                position,
                pose,
                radius,
                animation,
            });
            if let Some(surface @ 1..=4) = water_surface {
                self.position_water_model(id, surface, pose, previous_position, scene)?;
                continue;
            }
            if let Some(surface) = water_surface {
                let progress = if selected {
                    self.globals
                        .selected_health
                        .as_ref()
                        .and_then(|state| state.death())
                        .context("model selected death progress unavailable")?
                        .progress()
                } else {
                    0
                };
                // Publication precedes mode selection and fatal health; actor coordinates stay Q8.
                let y = (pose.position[1] >> 8)
                    .wrapping_add(64)
                    .wrapping_add(progress as i32) as f32;
                self.actors[index]
                    .model
                    .position
                    .as_mut()
                    .expect("published position")[1] = y;
                if let Some(current) = self.actors[index].model.current.as_mut() {
                    current.position[1] = y;
                }
                if selected && surface == 5 {
                    self.set_model_death_mode_one()?;
                }
                let control = self.actors[index]
                    .control
                    .as_ref()
                    .expect("validated control");
                if control.queued_motion != 0
                    || control.directional_state.iter().any(|state| state & 3 != 0)
                {
                    continue;
                }
                let inputs = inputs.expect("validated model health services");
                let sounds = sounds
                    .as_deref_mut()
                    .context("model health sound executor unavailable")?;
                if selected {
                    self.apply_selected_health_command(
                        id,
                        -255,
                        1,
                        inputs,
                        random.as_deref_mut(),
                        sounds,
                    )?;
                } else {
                    let combat_mode = self.actors[index]
                        .control
                        .as_ref()
                        .and_then(|control| control.combat.as_ref())
                        .is_some_and(|combat| combat.flags & 4 != 0);
                    self.change_ordinary_health(
                        id,
                        -255,
                        data,
                        super::transition::ObstacleEnvironment {
                            auto_grab: false,
                            selected_player: self.selected_player(),
                            item_system_ready: inputs
                                .hud
                                .context("model health inventory unavailable")?
                                .inventory_ready()?,
                            shell_execution: inputs
                                .shell_execution
                                .context("model health shell context unavailable")?,
                            ticks: inputs
                                .nominal_ticks
                                .context("model health nominal ticks unavailable")?,
                        },
                        Some(combat_mode),
                        sounds,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn position_water_model(
        &mut self,
        id: ScriptActorId,
        surface: u8,
        pose: Pose,
        previous: Option<[f32; 3]>,
        scene: Option<&crate::player::data::PlayerScene>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let selected = self.selected_player_index() == Some(index);
        let health = if selected {
            Some(
                self.globals
                    .selected_health
                    .as_ref()
                    .context("water model selected health unavailable")?,
            )
        } else {
            None
        };
        let inhibited = health.is_some_and(|health| health.hit_inhibited());
        let y = if surface == 1 || inhibited {
            let progress = if surface != 1 {
                health
                    .and_then(|health| health.death())
                    .context("water model death progress unavailable")?
                    .progress()
            } else {
                0
            };
            (pose.position[1] >> 8)
                .wrapping_add(64)
                .wrapping_add(progress as i32) as f32
        } else {
            let mut coverage =
                if surface == 2 {
                    256
                } else {
                    let scene = scene.context("water model terrain scene unavailable")?;
                    ensure!(
                        scene.world == self.world,
                        "world {} water model scene belongs to world {}",
                        self.world,
                        scene.world
                    );
                    let position = self.actors[index]
                        .model
                        .position
                        .expect("published position");
                    let sample = position.map(|value| i64::from(value as i32) * 256);
                    scene
                        .terrain
                        .as_ref()
                        .context("water model terrain unavailable")?
                        .material(
                            sample,
                            scene.collision.texture_surfaces.as_ref().context(
                                "water model surface assignments unavailable; re-export",
                            )?,
                        )?
                        .context("water model sample outside terrain")?
                        .coverage
                };
            if coverage != 0 && coverage != 256 {
                coverage = (coverage * 2).min(256);
            }
            let mut y = (pose.position[1] >> 8).wrapping_add(i32::from(coverage >> 2)) as f32;
            if matches!(
                self.actors[index].bound_animation_group().flatten(),
                Some(141..=143)
            ) {
                let scene = scene.context("water model swim-depth scene unavailable")?;
                ensure!(
                    scene.world == self.world,
                    "world {} water model scene belongs to world {}",
                    self.world,
                    scene.world
                );
                y = (f64::from(y)
                    + f64::from(
                        scene
                            .settings
                            .swim_depth
                            .context("water model swim_depth unavailable; re-export")?,
                    )) as f32;
            }
            y
        };
        self.actors[index]
            .model
            .position
            .as_mut()
            .expect("published position")[1] = y;
        if let Some(current) = self.actors[index].model.current.as_mut() {
            current.position[1] = y;
        }
        if surface != 1 && !inhibited {
            let old_y = previous
                .context("water model prior coordinates unavailable; recapture session")?[1];
            let difference = f64::from(float_to_model_integer(f64::from(old_y))) - f64::from(y);
            let difference = float_to_model_integer(difference).wrapping_abs();
            if difference > 16 {
                let camera = self
                    .camera
                    .as_ref()
                    .context("water model published camera unavailable")?;
                self.spatial(camera.camera_actor)
                    .context("water model camera pose not initialized")?;
                let mut point = super::effects::death_effect_point(
                    self.spatial(id)?.local.position,
                    camera.current.position,
                );
                let height = self.ordinary_collision_shape(id)?.positive_extent[1];
                point[1] = point[1]
                    .wrapping_add(height.wrapping_mul(256))
                    .wrapping_add(1536);
                self.spawn_environment_effect(0x2500, point)?;
            }
        }
        Ok(())
    }

    /// Resolve an authored source vertex with the actor's current local matrix and
    /// integer position. Uses original model-point conversion, independent of the
    /// retained collision presentation pose and renderer geometry. Animated sources
    /// perform the original Q8 integer conversion before matrix input storage;
    /// static sources store their original Q8 vertex directly as f32.
    pub fn actor_model_point(
        &self,
        id: ScriptActorId,
        data: &Animations,
        statics: Option<&ActorModelCollision>,
        vertex: i32,
    ) -> Result<[i32; 3]> {
        let source = self.actor_model_source_vertex(id, data, statics, vertex)?;
        let local = self.spatial(id)?.local;
        local
            .matrix
            .model_point(source, local.position)
            .with_context(|| {
                format!(
                    "world {} instance {} actor {} model vertex {vertex}",
                    self.world,
                    id.index,
                    self.actors[id.index].name()
                )
            })
    }

    /// Original Q8 source vertex before the actor matrix and position are applied.
    /// Attachment publication shares this conversion with model-point queries.
    pub(super) fn actor_model_source_vertex(
        &self,
        id: ScriptActorId,
        data: &Animations,
        statics: Option<&ActorModelCollision>,
        vertex: i32,
    ) -> Result<[f32; 3]> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let context = || {
            format!(
                "world {} instance {index} actor {} model vertex {vertex}",
                self.world,
                actor.name()
            )
        };
        let source = match data.actor(actor.name())?["model_load_path"].as_str() {
            Some("animation_table") if self.updates.dummy_model_manager => [0.0; 3],
            Some("animation_table") => self
                .actor_animation_source_vertex(id, data, vertex)?
                .map(|value| value as f32),
            Some("static") => {
                let vertex_index = usize::try_from(vertex).with_context(context)?;
                statics
                    .context("static model geometry unavailable")
                    .with_context(context)?
                    .vertex(index, vertex_index)
                    .with_context(context)?
                    .map(|value| value * 256.0)
            }
            path => anyhow::bail!("{}: model path {path:?} unsupported", context()),
        };
        Ok(source)
    }

    fn actor_animation_source_vertex(
        &self,
        id: ScriptActorId,
        data: &Animations,
        vertex: i32,
    ) -> Result<[i32; 3]> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let context = || {
            format!(
                "world {} instance {index} actor {} model vertex {vertex}",
                self.world,
                actor.name()
            )
        };
        let vertex_index = usize::try_from(vertex).with_context(context)?;
        let reference = self
            .animation_frame(id)?
            .with_context(|| format!("{}: animation frame unavailable", context()))?;
        let frame = if actor.placement().is_some() {
            data.resolve_map_frame(index, reference)
        } else {
            data.resolve_frame(actor.name(), reference)
        }
        .with_context(context)?;
        let position = data
            .models
            .get(&frame.model)
            .and_then(|model| model.positions.get(frame.frame))
            .and_then(|positions| positions.get(vertex_index))
            .copied()
            .with_context(|| {
                format!(
                    "{}: model {} frame {} source vertex unavailable",
                    context(),
                    frame.model,
                    frame.frame
                )
            })?;
        cyrus_world::transform::animation_vertex_q8(position).with_context(context)
    }

    /// Last attachment published in an admitted model pass, independent of later movement.
    pub fn equipped_weapon(
        &self,
        id: ScriptActorId,
    ) -> Result<Option<&super::super::attachment::EquippedWeapon>> {
        Ok(self.actors[self.resolve(id)?].model.weapon.as_ref())
    }

    /// Publish the equipped model and combat endpoints after its synchronous transition.
    /// Explicit calls require established item-system and shell admission.
    pub fn refresh_actor_weapon_attachment(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        inputs: super::WeaponInputs,
    ) -> Result<()> {
        self.require_model_identity(data)?;
        self.resolve(inputs.selected_player)?;
        if !inputs.item_system_ready || inputs.shell_execution {
            return Ok(());
        }
        let actor = &self.actors[self.resolve(id)?];
        let control = actor
            .control
            .as_ref()
            .context("weapon attachment control unavailable")?;
        if control.combat.is_none() && actor.attributes[110] == 0 {
            return Ok(());
        }
        ensure!(
            actor.attributes[110] == 0,
            "intrinsic weapon attachment unsupported"
        );
        let item = u16::from(actor.attributes[23]);
        let definition = self
            .weapons
            .get(&item)
            .with_context(|| format!("item {item}: prepared weapon geometry unavailable"))?;
        let group = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        let hand_model =
            control.living_state != 20 && (control.living_state == 21 || control.weapon_drawn);
        let drawn = hand_model || (control.living_state != 20 && group == Some(129));
        let model = if hand_model {
            &definition.hand_model
        } else {
            &definition.hilt_model
        };
        ensure!(
            data.models.contains_key(model),
            "item {item}: attachment model {model} unavailable"
        );
        let fields = if drawn {
            ["grip0", "grip1"]
        } else {
            ["scabbard0", "scabbard1"]
        };
        let metadata = data.actor(actor.name())?;
        let mut vertices = [[0; 3]; 2];
        for (vertex, field) in vertices.iter_mut().zip(fields) {
            let index = metadata["raex"][field]
                .as_i64()
                .and_then(|value| i16::try_from(value).ok())
                .with_context(|| format!("actor {} RAEX {field} unavailable", actor.name()))?;
            *vertex = if self.updates.dummy_model_manager {
                [0; 3]
            } else {
                self.actor_animation_source_vertex(id, data, i32::from(index))?
            };
        }
        let pose = self.spatial(id)?.local;
        let geometry = super::super::attachment::prepare_weapon_attachment(
            vertices,
            [0; 3],
            pose.matrix,
            pose.position.map(|value| value as f32 / 256.0),
            pose.angles[2],
            drawn,
        )?;
        let endpoints = geometry.combat_endpoints(definition.length)?;
        let weapon = super::super::attachment::EquippedWeapon {
            item,
            model: model.clone(),
            geometry,
            endpoints,
        };
        self.actors[id.index].model.weapon = Some(weapon);
        Ok(())
    }

    /// Current collision-facing model, retaining its presentation-time pose.
    pub fn model_presentation(&self, id: ScriptActorId) -> Result<Option<&ModelPresentation>> {
        let index = self.resolve(id)?;
        Ok(self.actors[index].model.current.as_ref())
    }

    /// Stored map-unit model coordinates, independent of current geometry and logical movement.
    /// Fresh actors begin at zero. Older snapshots without retained geometry may be unknown.
    pub fn model_position(&self, id: ScriptActorId) -> Result<Option<[f32; 3]>> {
        Ok(self.actors[self.resolve(id)?].model.position)
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
    /// The selected standalone player participates; the inactive alternate role is excluded.
    pub fn live_collision_inputs(
        &self,
        order: &[ScriptActorId],
    ) -> Result<Vec<cyrus_collision::candidates::LiveActor>> {
        use cyrus_collision::candidates::{LiveActor, ModelBounds, ModelInstance};
        let mut inputs = Vec::new();
        for &id in order {
            let index = self.resolve(id)?;
            let actor = &self.actors[index];
            // The inactive player role fails the original ordinary exclusion query.
            if matches!(actor.source, super::ScriptActorSource::AlternatePlayer) {
                continue;
            }
            // Candidate admission rejects an absent current record before reading
            // model-path metadata or preparing any actor collision response.
            let Some(record) = actor.model.current.as_ref() else {
                continue;
            };
            ensure!(
                actor.ordinary_collision_role(),
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
                    position: record.position,
                    radius: record.radius,
                }),
                // Player-role exclusion was applied before reading the current model.
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

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ModelSnapshot {
    prepared: bool,
    support_actor: Option<(u64, usize)>,
    ledge_support: Option<(u64, usize)>,
    support_static: Option<usize>,
    static_radius: Option<u32>,
    animation_radius: Option<u32>,
    collision_flags: Option<u8>,
    #[serde(default)]
    position: Option<[u32; 3]>,
    current: Option<ModelPresentationSnapshot>,
    weapon: Option<super::super::attachment::EquippedWeaponSnapshot>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ModelPresentationSnapshot {
    pose: super::super::spatial::SavedPose,
    #[serde(default)]
    position: Option<[u32; 3]>,
    radius: u32,
    animation: Option<(String, usize)>,
}
impl ModelState {
    pub(super) fn snapshot(&self) -> ModelSnapshot {
        ModelSnapshot {
            prepared: self.prepared,
            weapon: self.weapon.as_ref().map(|weapon| weapon.snapshot()),
            support_actor: self.support.actor.map(|id| (id.generation, id.index)),
            ledge_support: self.ledge_support.map(|id| (id.generation, id.index)),
            support_static: self.support.static_placement,
            static_radius: self.static_radius.map(f32::to_bits),
            animation_radius: self.animation_radius.map(f32::to_bits),
            collision_flags: self.collision_flags,
            position: self.position.map(|position| position.map(f32::to_bits)),
            current: self
                .current
                .as_ref()
                .map(|current| ModelPresentationSnapshot {
                    pose: current.pose.into(),
                    position: Some(current.position.map(f32::to_bits)),
                    radius: current.radius.to_bits(),
                    animation: current
                        .animation
                        .as_ref()
                        .map(|frame| (frame.model.clone(), frame.frame)),
                }),
        }
    }

    /// Restore retained presentation independently of the actor's newer logical pose.
    pub(super) fn from_snapshot(
        saved: ModelSnapshot,
        data: &Animations,
        static_count: Option<usize>,
        rebind: &impl Fn(u64, usize) -> Result<ScriptActorId>,
    ) -> Result<Self> {
        let radius = |bits| -> Result<f32> {
            let value = f32::from_bits(bits);
            ensure!(
                value.is_finite() && value >= 0.0,
                "saved model radius invalid"
            );
            Ok(value)
        };
        if let Some(index) = saved.support_static {
            ensure!(
                static_count.is_some_and(|count| index < count),
                "saved support placement {index}: outside admitted static table"
            );
        }
        let support = ModelSupportOwners {
            actor: saved
                .support_actor
                .map(|(generation, index)| rebind(generation, index))
                .transpose()?,
            static_placement: saved.support_static,
        };
        let current = saved
            .current
            .map(|saved| -> Result<_> {
                let animation = saved
                    .animation
                    .map(|(model, frame)| -> Result<_> {
                        let installed = data
                            .models
                            .get(&model)
                            .with_context(|| format!("saved model {model}: geometry absent"))?;
                        ensure!(
                            frame < installed.positions.len(),
                            "saved model {model} frame {frame}: outside geometry"
                        );
                        Ok(ModelFrame { model, frame })
                    })
                    .transpose()?;
                let pose = saved.pose.restore()?;
                // Earlier snapshots admitted only undisplaced model positions.
                let position = saved.position.map_or_else(
                    || pose.position.map(|value| (value >> 8) as f32),
                    |bits| bits.map(f32::from_bits),
                );
                ensure!(
                    position.iter().all(|value| value.is_finite()),
                    "saved model position invalid"
                );
                Ok(ModelPresentation {
                    pose,
                    position,
                    radius: radius(saved.radius)?,
                    animation,
                })
            })
            .transpose()?;
        let position = saved
            .position
            .map(|bits| bits.map(f32::from_bits))
            .or_else(|| current.as_ref().map(|record| record.position));
        ensure!(
            position.is_none_or(|position| position.iter().all(|value| value.is_finite())),
            "saved retained model position invalid"
        );
        ensure!(
            current
                .as_ref()
                .is_none_or(|record| Some(record.position) == position),
            "saved current and retained model position disagree"
        );
        let weapon = saved.weapon.map(|saved| saved.restore()).transpose()?;
        if let Some(weapon) = &weapon {
            ensure!(
                data.models.contains_key(&weapon.model),
                "saved weapon model {} unavailable",
                weapon.model
            );
        }
        Ok(Self {
            weapon,
            prepared: saved.prepared,
            ledge_support: saved
                .ledge_support
                .map(|(generation, index)| rebind(generation, index))
                .transpose()?,
            support,
            static_radius: saved.static_radius.map(radius).transpose()?,
            animation_radius: saved.animation_radius.map(radius).transpose()?,
            collision_flags: saved.collision_flags,
            position,
            current,
        })
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ModelAdmissionSnapshot {
    secondary: Vec<(u64, usize)>,
    static_count: usize,
}
impl AdmissionState {
    pub(super) fn snapshot(&self) -> ModelAdmissionSnapshot {
        ModelAdmissionSnapshot {
            secondary: self
                .secondary
                .iter()
                .map(|id| (id.generation, id.index))
                .collect(),
            static_count: self.static_count,
        }
    }

    /// Exact visibility/content matching belongs to the enclosing world candidate.
    pub(super) fn from_snapshot(
        saved: ModelAdmissionSnapshot,
        visibility: &cyrus_assets::collision::ModelVisibility,
        static_count: usize,
        rebind: &impl Fn(u64, usize) -> Result<ScriptActorId>,
    ) -> Result<Self> {
        ensure!(
            saved.static_count == static_count,
            "saved static model census differs"
        );
        let mut seen = std::collections::BTreeSet::new();
        let secondary = saved
            .secondary
            .into_iter()
            .map(|(generation, index)| {
                ensure!(
                    seen.insert((generation, index)),
                    "duplicate saved secondary actor {index}"
                );
                rebind(generation, index)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            filter: cyrus_collision::model_admission::ModelAdmission::new(visibility.clone())?,
            secondary,
            static_count,
        })
    }
}

// Original float-to-integer conversion returns signed minimum outside its range.
fn float_to_model_integer(value: f64) -> i32 {
    if (f64::from(i32::MIN)..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}
