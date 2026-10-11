//! Original static spatial ordering and model ground probes, independent of rendering.
use super::collision_mesh::CollisionMesh;
use super::ground::FallbackProbe;
use super::ground::GroundProbe;
use super::ground::GroundSupport;
use super::ground::ModelSupport;
use super::ground::SelectedSupport;
use super::ground::SupportSource;
use anyhow::Context;
use bevy::math::{Mat3, Vec3};
use cyrus_assets::collision::CollisionScene;
use cyrus_assets::collision::StaticPlacement;
use std::collections::BTreeMap;

struct Model {
    version: Option<[u8; 4]>,
    mesh: CollisionMesh,
    radius: f32,
    flags: u8,
    textures: Option<Vec<Option<[u16; 2]>>>,
}

/// Prepared world-only static collision registry. Dynamic actors are supplied separately.
pub struct StaticCollision {
    placements: Vec<StaticPlacement>,
    models: BTreeMap<String, Model>,
    cells: BTreeMap<(i32, i32), Vec<usize>>,
    unsupported: BTreeMap<String, String>,
    texture_surfaces: Option<BTreeMap<u16, u8>>,
}

/// Original candidate-list separation; indices identify authored static placements.
#[derive(Debug, PartialEq)]
pub struct StaticCandidates {
    pub faces: Vec<usize>,
    pub actors: Vec<usize>,
}

/// A body contact in world map units, before integer response conversion.
pub struct BodyContact {
    pub model: usize,
    pub face: usize,
    pub point: Vec3,
    pub normal: Vec3,
}

impl StaticCollision {
    /// Metadata committed by a successful support query; terrain clears the byte.
    /// Preserve the previous byte on a miss instead of calling this with no support.
    pub fn support_metadata(&self, source: SupportSource) -> anyhow::Result<u8> {
        match source {
            SupportSource::Terrain => Ok(0),
            SupportSource::Model { model, .. } => {
                let placement = self
                    .placements
                    .get(model)
                    .with_context(|| format!("support placement {model} outside world"))?;
                self.models
                    .get(&placement.model)
                    .map(|model| model.flags)
                    .with_context(|| {
                        format!(
                            "support placement {} model {} unavailable",
                            placement.id, placement.model
                        )
                    })
            }
        }
    }

    pub fn new(scene: &CollisionScene) -> anyhow::Result<Self> {
        anyhow::ensure!(
            scene
                .texture_surfaces
                .as_ref()
                .is_none_or(|table| table.values().all(|value| *value < 12)),
            "texture surface code outside 0..11"
        );
        let mut cells: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for (index, placement) in scene.placements.iter().enumerate().rev() {
            anyhow::ensure!(
                (0..=65535).contains(&placement.position[0])
                    && (0..=65535).contains(&placement.position[2]),
                "placement {}: static cell coordinates outside supported map",
                placement.id
            );
            cells
                .entry((placement.position[2] / 512, placement.position[0] / 512))
                .or_default()
                .push(index);
        }
        let models = scene
            .models
            .iter()
            .map(|(name, model)| {
                Ok((
                    name.clone(),
                    Model {
                        version: model.version,
                        mesh: CollisionMesh::new(model).with_context(|| format!("model {name}"))?,
                        radius: model.radius,
                        flags: model.flags,
                        textures: model.textures.clone(),
                    },
                ))
            })
            .collect::<anyhow::Result<_>>()?;
        Ok(Self {
            placements: scene.placements.clone(),
            models,
            cells,
            unsupported: scene.unsupported.clone(),
            texture_surfaces: scene.texture_surfaces.clone(),
        })
    }

    /// Texture identity for a successful contact; `None` means an authored solid color.
    /// Missing cache metadata is an error, distinct from an untextured face.
    pub fn face_texture(&self, placement: usize, face: usize) -> anyhow::Result<Option<[u16; 2]>> {
        let placement = self
            .placements
            .get(placement)
            .context("surface placement outside world")?;
        let model = self
            .models
            .get(&placement.model)
            .context("surface model unavailable")?;
        model
            .textures
            .as_ref()
            .with_context(|| {
                format!(
                    "placement {} model {}: missing face textures; re-export",
                    placement.id, placement.model
                )
            })?
            .get(face)
            .copied()
            .with_context(|| {
                format!(
                    "placement {} model {}: face {face} outside texture table",
                    placement.id, placement.model
                )
            })
    }

    /// Resolve contact material through the installed SURFACE.INI assignment table.
    /// Version-4/5 solid-color faces resolve to surface zero.
    pub fn face_surface(&self, placement: usize, face: usize) -> anyhow::Result<u8> {
        let texture = self.face_texture(placement, face)?;
        let Some([set, image]) = texture else {
            let model = &self.models[&self.placements[placement].model];
            anyhow::ensure!(
                matches!(model.version, Some(version) if version == *b"v4.0" || version == *b"v5.0"),
                "placement {placement} face {face}: solid-color model version unsupported"
            );
            return Ok(0);
        };
        anyhow::ensure!(
            set < 512 && image < 128,
            "placement {placement} face {face}: invalid texture {set},{image}"
        );
        let table = self
            .texture_surfaces
            .as_ref()
            .context("missing SURFACE.INI assignments; re-export world")?;
        Ok(table.get(&((set << 7) | image)).copied().unwrap_or(0))
    }

    /// View-centered static window, then actor-centered distance selection in map units.
    /// A missing actor model uses the original radius 200 fallback.
    pub fn candidates(
        &self,
        view: [i32; 2],
        actor: [i64; 3],
        actor_radius: Option<f32>,
    ) -> anyhow::Result<StaticCandidates> {
        anyhow::ensure!(
            view.iter().all(|v| (0..=65535).contains(v)),
            "static view outside map"
        );
        let radius = match actor_radius {
            Some(radius) => {
                anyhow::ensure!(
                    radius.is_finite() && (0.0..i32::MAX as f32 / 2.0).contains(&radius),
                    "invalid actor model radius"
                );
                (2.0 * radius).trunc()
            }
            None => 200.0,
        };
        let origin = Vec3::from_array(actor.map(|v| (v >> 8) as f32));
        let first = view.map(|v| {
            (if v < 4608 {
                0
            } else if v + 4608 > 65535 {
                56319
            } else {
                v - 4608
            }) / 512
        });
        let mut result = StaticCandidates {
            faces: Vec::new(),
            actors: Vec::new(),
        };
        for z in 0..18 {
            for x in 0..18 {
                if (x == 0 || x == 17) && (z == 0 || z == 17) {
                    continue;
                }
                let Some(indices) = self.cells.get(&(first[1] + z, first[0] + x)) else {
                    continue;
                };
                for &index in indices {
                    let placement = &self.placements[index];
                    let model = self.models.get(&placement.model).with_context(|| {
                        format!(
                            "placement {} model {}: {}",
                            placement.id,
                            placement.model,
                            self.unsupported
                                .get(&placement.model)
                                .map_or("missing collision model", String::as_str)
                        )
                    })?;
                    if model.flags & 2 != 0 {
                        continue;
                    }
                    let displacement =
                        origin - Vec3::from_array(placement.position.map(|v| v as f32));
                    let distance_squared = displacement.as_dvec3().length_squared() as f32;
                    let combined = radius + model.radius;
                    if f64::from(distance_squared) > f64::from(combined).powi(2) {
                        continue;
                    }
                    if model.flags & 1 != 0 {
                        if result.actors.len() < 64 {
                            result.actors.push(index);
                        }
                    } else {
                        if result.faces.len() == 64 {
                            return Ok(result);
                        }
                        result.faces.push(index);
                    }
                }
            }
        }
        Ok(result)
    }

    /// Forward obstacle probe against face-model candidates, in their original order.
    /// Spheres and rotation are actor-local; position has eight fractional bits.
    /// Dynamic/transient face models must be queried separately by the caller.
    pub fn blocks_body(
        &self,
        candidates: &StaticCandidates,
        position: [i64; 3],
        rotation: Mat3,
        spheres: &[(Vec3, f32)],
        sidedness: bool,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(rotation.is_finite(), "body probe: nonfinite rotation");
        anyhow::ensure!(
            spheres
                .iter()
                .all(|(center, radius)| center.is_finite() && radius.is_finite() && *radius >= 0.0),
            "body probe: invalid sphere"
        );
        let origin = Vec3::from_array(position.map(|v| (v >> 8) as f32));
        for &index in &candidates.faces {
            let placement = self
                .placements
                .get(index)
                .context("body candidate outside static placements")?;
            let model = self
                .models
                .get(&placement.model)
                .context("body candidate model unavailable")?;
            let admitted = admitted_spheres(origin, rotation, spheres, placement, model.radius);
            if model
                .mesh
                .intersects_spheres(&admitted, sidedness)
                .with_context(|| {
                    format!(
                        "placement {} model {} body probe",
                        placement.id, placement.model
                    )
                })?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Collect static face contacts in candidate, sphere, subobject and face order.
    /// Dynamic/transient face models and response eligibility remain caller-owned.
    pub fn body_contacts(
        &self,
        candidates: &StaticCandidates,
        position: [i64; 3],
        rotation: Mat3,
        spheres: &[(Vec3, f32)],
        sidedness: bool,
    ) -> anyhow::Result<Vec<BodyContact>> {
        anyhow::ensure!(rotation.is_finite(), "body probe: nonfinite rotation");
        anyhow::ensure!(
            spheres
                .iter()
                .all(|(center, radius)| center.is_finite() && radius.is_finite() && *radius >= 0.0),
            "body probe: invalid sphere"
        );
        let origin = Vec3::from_array(position.map(|v| (v >> 8) as f32));
        let mut contacts = Vec::new();
        for &index in &candidates.faces {
            let placement = self
                .placements
                .get(index)
                .context("body candidate outside static placements")?;
            let model = self
                .models
                .get(&placement.model)
                .context("body candidate model unavailable")?;
            let translation = Vec3::from_array(placement.position.map(|v| v as f32));
            let placement_rotation =
                Mat3::from_cols_array(&placement.rotation.map(|v| v as f32 / 268_435_456.0));
            let admitted = admitted_spheres(origin, rotation, spheres, placement, model.radius);
            for contact in model
                .mesh
                .sphere_contacts(&admitted, sidedness)
                .with_context(|| {
                    format!(
                        "placement {} model {} body contacts",
                        placement.id, placement.model
                    )
                })?
            {
                contacts.push(BodyContact {
                    model: index,
                    face: contact.face,
                    point: placement_rotation * contact.projection + translation,
                    normal: placement_rotation * contact.normal,
                });
            }
        }
        Ok(contacts)
    }

    /// Select slope/ground geometry for a forward probe after body clearance.
    /// `check_slope` reflects the caller's original collision attributes and actor state.
    /// The caller must run support-state updates and `SelectedSupport::allows_forward` afterward.
    pub fn forward_support(
        &self,
        candidates: &StaticCandidates,
        probe: &GroundProbe,
        terrain: Option<GroundSupport>,
        check_slope: bool,
        fallback: Option<&FallbackProbe>,
        sidedness: bool,
    ) -> anyhow::Result<Option<SelectedSupport>> {
        let contacts = self.ground_contacts(candidates, probe, sidedness)?;
        if check_slope {
            let surface = probe
                .slope_surface(terrain, &contacts)
                .context("forward probe: missing proposed slope surface; fallback unsupported")?;
            if surface.classification() == 2 {
                return Ok(None);
            }
        }
        if let Some(support) = probe.select(terrain, &contacts) {
            return Ok(Some(support));
        }
        let fallback = fallback.context("forward probe: missing selected-player fallback state")?;
        self.fallback_support(candidates, fallback, sidedness)
    }

    /// Secondary support for the designated player after no primary contact was selected.
    /// The caller must apply actor-state candidate exclusions first. The first non-side
    /// contact wins, and the actor's runtime Y is retained even for an underside contact.
    pub fn fallback_support(
        &self,
        candidates: &StaticCandidates,
        probe: &FallbackProbe,
        sidedness: bool,
    ) -> anyhow::Result<Option<SelectedSupport>> {
        let sphere = [(
            Vec3::from_array(probe.center.map(|v| v as f32)),
            probe.radius,
        )];
        // Query one model at a time: later models cannot fail or replace an accepted contact.
        for &model in &candidates.faces {
            let single = StaticCandidates {
                faces: vec![model],
                actors: Vec::new(),
            };
            for contact in
                self.body_contacts(&single, [0; 3], Mat3::IDENTITY, &sphere, sidedness)?
            {
                let support = SelectedSupport::Fallback {
                    model: contact.model,
                    face: contact.face,
                    normal: contact
                        .normal
                        .to_array()
                        .map(|v| (v * 256.0).round_ties_even() as i32),
                    actor_height: probe.actor_height,
                };
                if (GroundSupport {
                    height: 0.0,
                    normal: contact
                        .normal
                        .to_array()
                        .map(|v| (v * 256.0).round_ties_even() as i32),
                })
                .classification()
                    != 1
                {
                    return Ok(Some(support));
                }
            }
        }
        Ok(None)
    }

    /// Query admitted static support and commit the designated player's shared history.
    /// Slope rollback, surface-state updates and dynamic candidates remain caller-owned.
    /// Build the fallback probe using the history at query entry, before this update.
    pub fn selected_player_support(
        &self,
        candidates: &StaticCandidates,
        probe: &GroundProbe,
        terrain: Option<GroundSupport>,
        fallback: &FallbackProbe,
        sidedness: bool,
        history: &mut super::ground::SupportHistory,
    ) -> anyhow::Result<Option<SelectedSupport>> {
        let selected =
            self.forward_support(candidates, probe, terrain, false, Some(fallback), sidedness)?;
        let metadata = selected
            .as_ref()
            .map(|support| self.support_metadata(support.source()))
            .transpose()?;
        history.finish_query(metadata, true);
        Ok(selected)
    }

    /// Ground contacts in face-model order; the original support query ignores the actor list.
    pub fn ground_contacts(
        &self,
        candidates: &StaticCandidates,
        probe: &GroundProbe,
        sidedness: bool,
    ) -> anyhow::Result<Vec<ModelSupport>> {
        let (start, end) = probe.endpoints();
        let start = Vec3::from_array(start.map(|v| v as f32));
        let end = Vec3::from_array(end.map(|v| v as f32));
        let mut contacts = Vec::new();
        for &index in &candidates.faces {
            let placement = self
                .placements
                .get(index)
                .context("ground candidate outside static placements")?;
            let model = self
                .models
                .get(&placement.model)
                .context("ground candidate model unavailable")?;
            let translation = Vec3::from_array(placement.position.map(|v| v as f32));
            let rotation =
                Mat3::from_cols_array(&placement.rotation.map(|v| v as f32 / 268_435_456.0));
            let inverse = rotation.transpose();
            for contact in model.mesh.segment_contacts(
                inverse * (start - translation),
                inverse * (end - translation),
                sidedness,
            )? {
                let point = rotation * contact.contact.point + translation;
                let normal = rotation * contact.contact.normal;
                contacts.push(ModelSupport {
                    model: index,
                    face: contact.face,
                    rank: contact.contact.rank,
                    support: GroundSupport {
                        height: f64::from(point.y.round_ties_even()),
                        normal: normal
                            .to_array()
                            .map(|v| (v * 256.0).round_ties_even() as i32),
                    },
                });
            }
        }
        Ok(contacts)
    }
}

fn admitted_spheres(
    origin: Vec3,
    rotation: Mat3,
    spheres: &[(Vec3, f32)],
    placement: &StaticPlacement,
    model_radius: f32,
) -> Vec<(Vec3, f32)> {
    let translation = Vec3::from_array(placement.position.map(|v| v as f32));
    let local_model = rotation.transpose() * (translation - origin);
    let inverse =
        Mat3::from_cols_array(&placement.rotation.map(|v| v as f32 / 268_435_456.0)).transpose();
    spheres
        .iter()
        .filter_map(|&(center, radius)| {
            let delta = center - local_model;
            let limit = f64::from(model_radius * model_radius) + f64::from(radius * radius);
            (delta.as_dvec3().length_squared() <= limit)
                .then(|| (inverse * (rotation * center + origin - translation), radius))
        })
        .collect()
}
