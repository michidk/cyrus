//! Off-live replacement of terrain history and its projected mesh presentation.
use super::WaterPlayback;
use anyhow::{Context, Result, ensure};
use bevy::prelude::{Assets, Entity, Handle, Mesh, Mesh3d, Vec3, Visibility, World};
use cyrus_assets::environment::Environment;
use serde::{Deserialize, Serialize};

/// Opaque terrain history. Runtime entities and asset handles are supplied anew.
/// Capture this together with texture playback and the presentation clock after
/// deferred mesh visibility and admission writes have completed.
#[derive(Clone, Serialize, Deserialize)]
pub struct TerrainPlaybackSnapshot {
    version: u32,
    environment: [u8; 32],
    base: [u8; 32],
    tick: Option<u64>,
    window: Option<crate::terrain::Window>,
    overridden: bool,
    grid: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    dirty_normals: Vec<bool>,
    rgb: Vec<[f32; 3]>,
    meshes: Vec<MeshHistory>,
}

#[derive(Clone, Serialize, Deserialize)]
struct MeshHistory {
    source: [u8; 32],
    mapping: [u8; 32],
    view: Option<crate::terrain::View>,
    visible: bool,
    admitted: bool,
    water_attribute: bool,
}

/// One prepared mesh publication. The aggregate commits these together with the
/// resource and all other session replacements before render systems resume.
pub struct TerrainMeshReplacement {
    /// Fresh scene entity, explicitly bound to its saved ordinal.
    pub entity: Entity,
    /// Private animated mesh handle already allocated by off-live preparation.
    pub handle: Handle<Mesh>,
    /// Replacement geometry, without changes to live asset storage.
    pub mesh: Mesh,
    /// Exact saved visibility at the completed boundary.
    pub visibility: Visibility,
    /// Retained texture admission, independent of raster visibility.
    pub texture_admitted: bool,
}

/// Validated terrain replacement, with no writes to live resources or assets.
pub struct TerrainCandidate {
    playback: WaterPlayback,
    meshes: Vec<TerrainMeshReplacement>,
}

impl TerrainCandidate {
    /// Transfer both owners to the aggregate's coordinated publication boundary.
    pub fn into_parts(self) -> (WaterPlayback, Vec<TerrainMeshReplacement>) {
        (self.playback, self.meshes)
    }
}

fn digest(value: &impl Serialize) -> Result<[u8; 32]> {
    Ok(*blake3::hash(&serde_json::to_vec(value)?).as_bytes())
}

impl WaterPlayback {
    /// Saved mesh ordinals follow this order. The aggregate establishes matching
    /// fresh entities from scene identity, never by reusing saved entity IDs.
    pub fn terrain_entities(&self) -> Vec<Entity> {
        self.terrain.iter().map(|terrain| terrain.entity).collect()
    }

    /// Capture only a settled terrain update. The environment must be the one
    /// independently validated by the aggregate for this bound world.
    pub fn snapshot_playback(
        &self,
        environment: &Environment,
        world: &World,
    ) -> Result<TerrainPlaybackSnapshot> {
        let mut meshes = Vec::with_capacity(self.terrain.len());
        for terrain in &self.terrain {
            let entity = terrain.entity;
            ensure!(
                world
                    .get::<Mesh3d>(entity)
                    .is_some_and(|mesh| mesh.0 == terrain.animated),
                "terrain {entity}: animated mesh is not bound"
            );
            let visible = match world.get::<Visibility>(entity) {
                Some(Visibility::Inherited) => true,
                Some(Visibility::Hidden) => false,
                _ => anyhow::bail!("terrain {entity}: unsupported or missing visibility"),
            };
            meshes.push(MeshHistory {
                source: terrain.source.fingerprint()?,
                mapping: digest(&terrain.vertices)?,
                water_attribute: world
                    .get_resource::<Assets<Mesh>>()
                    .with_context(|| format!("terrain {entity}: mesh assets unavailable"))?
                    .get(&terrain.animated)
                    .with_context(|| format!("terrain {entity}: animated mesh unavailable"))?
                    .attribute(Mesh::ATTRIBUTE_UV_1)
                    .is_some(),
                view: terrain.view,
                visible,
                admitted: world
                    .get::<crate::terrain::TextureAdmission>(entity)
                    .is_some_and(|admission| admission.0),
            });
        }
        let saved = TerrainPlaybackSnapshot {
            version: 2,
            environment: digest(environment)?,
            base: digest(&self.base)?,
            tick: self.tick,
            window: self.window,
            overridden: self.overridden,
            grid: self.grid.clone(),
            normals: self.normals.clone(),
            dirty_normals: self.dirty_normals.clone(),
            rgb: self.rgb.clone(),
            meshes,
        };
        validate(&saved, self, environment)?;
        Ok(saved)
    }

    /// Consume independently prepared terrain and explicitly bind saved ordinals
    /// to fresh entities. Geometry is rebuilt from retained history without a
    /// wave, clock or normal update. Failure leaves live assets untouched.
    pub fn from_snapshot(
        mut prepared: Self,
        environment: &Environment,
        snapshot: TerrainPlaybackSnapshot,
        bindings: &[Entity],
        meshes: &Assets<Mesh>,
    ) -> Result<TerrainCandidate> {
        validate(&snapshot, &prepared, environment)?;
        ensure!(
            bindings.len() == snapshot.meshes.len(),
            "terrain replacement binding census differs"
        );
        let mut ordered = Vec::with_capacity(bindings.len());
        for (&entity, saved) in bindings.iter().zip(&snapshot.meshes) {
            let index = prepared
                .terrain
                .iter()
                .position(|terrain| terrain.entity == entity)
                .with_context(|| {
                    format!("terrain {entity}: missing or duplicate replacement binding")
                })?;
            let mut terrain = prepared.terrain.remove(index);
            ensure!(
                terrain.source.fingerprint()? == saved.source
                    && digest(&terrain.vertices)? == saved.mapping,
                "terrain {entity}: prepared mesh source or grid mapping differs"
            );
            terrain.view = saved.view;
            ordered.push(terrain);
        }
        prepared.terrain = ordered;
        prepared.tick = snapshot.tick;
        prepared.window = snapshot.window;
        prepared.overridden = snapshot.overridden;
        prepared.grid = snapshot.grid;
        prepared.normals = snapshot.normals;
        prepared.dirty_normals = snapshot.dirty_normals;
        prepared.rgb = snapshot.rgb;
        let mut replacements = Vec::with_capacity(bindings.len());
        let mut scratch = crate::terrain::Preparation::default();
        let water = environment
            .water
            .as_ref()
            .filter(|water| water.parameters[0] != 0.0);
        for (terrain, saved) in prepared.terrain.iter_mut().zip(snapshot.meshes) {
            let original = meshes.get(&terrain.original).with_context(|| {
                format!("terrain {}: original mesh unavailable", terrain.entity)
            })?;
            let mesh = if let Some(view) = saved.view {
                let camera: Vec<_> = prepared
                    .grid
                    .iter()
                    .map(|&position| {
                        let p = view
                            .camera_from_local
                            .transform_point3(Vec3::from(position))
                            * 20.0;
                        [p.x * view.scale.x, p.y * view.scale.y, -p.z]
                    })
                    .collect();
                ensure!(
                    camera.iter().flatten().all(|v| v.is_finite()),
                    "terrain {}: projected grid overflows",
                    terrain.entity
                );
                let result = crate::terrain::update(
                    &mut terrain.source,
                    &terrain.vertices,
                    crate::terrain::Grid {
                        window: prepared
                            .window
                            .context("terrain replacement window missing")?,
                        local: &prepared.grid,
                        camera: &camera,
                        normals: &prepared.normals,
                        rgb: &prepared.rgb,
                        water: water.map(|water| water.animated_vertices.as_slice()),
                    },
                    view,
                    &mut scratch,
                );
                ensure!(
                    result.visible == saved.visible && result.texture_admitted == saved.admitted,
                    "terrain {}: saved visibility or admission differs from retained view",
                    terrain.entity
                );
                terrain.source.prepared.clone()
            } else {
                ensure!(
                    saved.visible && !saved.admitted,
                    "terrain {}: unprojected admission or visibility invalid",
                    terrain.entity
                );
                let mut mesh = original.clone();
                if saved.water_attribute {
                    mesh.insert_attribute(
                        Mesh::ATTRIBUTE_UV_1,
                        terrain
                            .vertices
                            .iter()
                            .map(|&vertex| {
                                [
                                    f32::from(u8::from(
                                        water.is_some_and(|water| water.animated_vertices[vertex]),
                                    )),
                                    0.0,
                                ]
                            })
                            .collect::<Vec<_>>(),
                    );
                }
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_POSITION,
                    terrain
                        .vertices
                        .iter()
                        .map(|&i| prepared.grid[i])
                        .collect::<Vec<_>>(),
                );
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_NORMAL,
                    terrain
                        .vertices
                        .iter()
                        .map(|&i| prepared.normals[i])
                        .collect::<Vec<_>>(),
                );
                if let Some(lighting) = &environment.terrain_lighting {
                    mesh.insert_attribute(
                        Mesh::ATTRIBUTE_COLOR,
                        terrain
                            .vertices
                            .iter()
                            .map(|&i| lighting.color(prepared.normals[i]))
                            .collect::<Vec<_>>(),
                    );
                }
                mesh
            };
            replacements.push(TerrainMeshReplacement {
                entity: terrain.entity,
                handle: terrain.animated.clone(),
                mesh,
                visibility: if saved.visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                texture_admitted: saved.admitted,
            });
        }
        Ok(TerrainCandidate {
            playback: prepared,
            meshes: replacements,
        })
    }
}

fn validate(
    saved: &TerrainPlaybackSnapshot,
    prepared: &WaterPlayback,
    environment: &Environment,
) -> Result<()> {
    ensure!(
        saved.version == 2,
        "terrain snapshot version {} unsupported",
        saved.version
    );
    ensure!(
        saved.environment == digest(environment)?,
        "terrain environment differs"
    );
    ensure!(
        saved.base == digest(&prepared.base)?,
        "terrain base grid differs"
    );
    ensure!(
        saved.meshes.len() == prepared.terrain.len(),
        "terrain prepared mesh census differs"
    );
    let water = environment
        .water
        .as_ref()
        .filter(|water| water.parameters[0] != 0.0);
    let side = environment
        .terrain_lighting
        .as_ref()
        .map(|lighting| lighting.side)
        .or_else(|| water.map(|water| water.side));
    let Some(side) = side else {
        ensure!(
            saved.meshes.is_empty()
                && prepared.base.is_empty()
                && saved.grid.is_empty()
                && saved.normals.is_empty()
                && saved.rgb.is_empty()
                && saved.dirty_normals.is_empty()
                && saved.tick.is_none()
                && saved.window.is_none(),
            "terrain history present in an environment without terrain"
        );
        return Ok(());
    };
    let count = side
        .checked_mul(side)
        .context("terrain grid size overflow")?;
    ensure!(
        !saved.meshes.is_empty() && saved.tick.is_some() && prepared.source.is_some(),
        "terrain update has not settled"
    );
    saved
        .window
        .context("terrain snapshot window missing")?
        .validate(side)?;
    ensure!(
        prepared.base.len() == count
            && saved.grid.len() == count
            && saved.normals.len() == count
            && saved.rgb.len() == count,
        "terrain history grid lengths differ"
    );
    ensure!(
        saved.dirty_normals.len() == water.map_or(0, |_| count),
        "terrain dirty-normal census differs"
    );
    if let Some(water) = water {
        ensure!(
            saved.dirty_normals == super::dirty_normals(water, saved.window.unwrap()),
            "terrain dirty-normal admission differs"
        );
    }
    ensure!(
        saved.grid.iter().flatten().all(|v| v.is_finite())
            && saved
                .normals
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= 1.01)
            && saved
                .rgb
                .iter()
                .flatten()
                .all(|v| v.is_finite() && (0.0..=255.0).contains(v)),
        "terrain history contains invalid geometry or lighting"
    );
    for (i, (position, base)) in saved.grid.iter().zip(&prepared.base).enumerate() {
        ensure!(
            position[0] == base[0] && position[2] == base[2],
            "terrain grid vertex {i}: horizontal coordinates differ"
        );
        let amplitude = water
            .filter(|water| water.animated_vertices[i] && saved.window.unwrap().contains(i))
            .map_or(0.0, |water| water.parameters[0].abs() * 1.5 / 20.0);
        ensure!(
            position[1] >= base[1] - amplitude && position[1] <= base[1] + amplitude,
            "terrain grid vertex {i}: height outside authored wave range"
        );
        let expected = environment
            .terrain_lighting
            .as_ref()
            .map_or([255.0; 3], |lighting| lighting.rgb(saved.normals[i]));
        ensure!(
            saved.rgb[i] == expected,
            "terrain grid vertex {i}: retained lighting differs from normal"
        );
    }
    for terrain in &prepared.terrain {
        ensure!(
            terrain.vertices.iter().all(|&i| i < count),
            "terrain {}: prepared grid mapping out of range",
            terrain.entity
        );
    }
    for saved in &saved.meshes {
        if let Some(view) = saved.view {
            ensure!(
                view.camera_from_local.is_finite()
                    && view.camera_from_local.determinant().is_finite()
                    && view.camera_from_local.determinant() != 0.0
                    && view.scale.is_finite()
                    && view.scale.x != 0.0
                    && view.scale.y != 0.0
                    && view.viewport.iter().all(|v| v.is_finite() && *v > 0.0)
                    && view.clipping[0].is_finite()
                    && view.clipping[0] > 0.0
                    && view.clipping[1].is_finite()
                    && view.clipping[1] > view.clipping[0],
                "terrain saved projection invalid"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{CELL_SIZE, EnvironmentRequest, Fidelity, Water, animate_water};
    use super::*;
    use anyhow::Result;
    use bevy::prelude::*;
    use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology};
    use std::time::Duration;

    fn fixture(projected: bool) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.world_mut().spawn_empty();
        let mut camera = Camera::default();
        if projected {
            camera.computed.clip_from_view = bevy::camera::CameraProjection::get_clip_from_view(
                &PerspectiveProjection::default(),
            );
            camera.computed.target_info = Some(bevy::camera::RenderTargetInfo {
                physical_size: UVec2::new(640, 480),
                scale_factor: 1.0,
            });
        }
        let camera = app
            .world_mut()
            .spawn((
                camera,
                crate::camera::ViewCamera,
                GlobalTransform::from(
                    Transform::from_xyz(-CELL_SIZE, 25.0, 25.0)
                        .looking_at(Vec3::new(-CELL_SIZE, 0.0, -CELL_SIZE), Vec3::Y),
                ),
            ))
            .id();
        let mut environments = Assets::default();
        let env = environments.add(Environment {
            texture_scale: [1, 1],
            object_lights: Vec::new(),
            prelighting: Default::default(),
            model_lighting: Default::default(),
            sky: None,
            background: [0; 3],
            clipping: [7.0, 3800.0],
            fog_table: [[0; 4]; 16],
            terrain_lighting: Some(cyrus_assets::environment::TerrainLighting {
                side: 4,
                ambient: [64.0; 3],
                directional: [128.0; 3],
                direction: [0.0, -1.0, 0.0],
            }),
            water: Some(Water {
                side: 4,
                parameters: [24.0, 512.0, 0.0],
                animated_vertices: (0..16).map(|i| i == 10).collect(),
            }),
        });
        app.insert_resource(environments)
            .insert_resource(EnvironmentRequest(env))
            .init_resource::<Fidelity>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<WaterPlayback>()
            .init_resource::<Time<Real>>()
            .add_systems(Update, animate_water);
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            (0..16)
                .map(|i| {
                    [
                        -((i % 4) as f32) * CELL_SIZE,
                        0.0,
                        -((i / 4) as f32) * CELL_SIZE,
                    ]
                })
                .collect::<Vec<_>>(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 16]);
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            (0..16)
                .map(|i| [(i % 4) as f32 / 3.0, (i / 4) as f32 / 3.0])
                .collect::<Vec<_>>(),
        );
        mesh.insert_indices(bevy::mesh::Indices::U32(
            (0..3)
                .flat_map(|row| {
                    (0..3).flat_map(move |x| {
                        let i = row * 4 + x;
                        [i, i + 5, i + 1, i + 5, i, i + 4]
                    })
                })
                .collect(),
        ));
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let entity = app
            .world_mut()
            .spawn((
                Name::new("Terrain"),
                Mesh3d(mesh),
                GlobalTransform::IDENTITY,
                Visibility::Inherited,
            ))
            .id();
        app.update();
        (app, camera, entity)
    }

    fn fresh_fixture(projected: bool) -> (App, Entity, Entity) {
        let (mut app, camera, old_entity) = fixture(projected);
        let original = app.world().resource::<WaterPlayback>().terrain[0]
            .original
            .clone();
        app.world_mut().despawn(old_entity);
        let entity = app
            .world_mut()
            .spawn((
                Name::new("Terrain"),
                Mesh3d(original),
                GlobalTransform::IDENTITY,
                Visibility::Inherited,
            ))
            .id();
        assert_ne!(entity, old_entity);
        app.update();
        (app, camera, entity)
    }

    fn environment(app: &App) -> &Environment {
        app.world()
            .resource::<Assets<Environment>>()
            .get(&app.world().resource::<EnvironmentRequest>().0)
            .unwrap()
    }

    fn snapshot(app: &App) -> TerrainPlaybackSnapshot {
        app.world()
            .resource::<WaterPlayback>()
            .snapshot_playback(environment(app), app.world())
            .unwrap()
    }

    fn mesh_state(app: &App) -> Vec<String> {
        let playback = app.world().resource::<WaterPlayback>();
        playback
            .terrain
            .iter()
            .filter(|terrain| {
                app.world().get::<Visibility>(terrain.entity) != Some(&Visibility::Hidden)
            })
            .map(|terrain| {
                let mesh = app
                    .world()
                    .resource::<Assets<Mesh>>()
                    .get(&terrain.animated)
                    .unwrap();
                [
                    Mesh::ATTRIBUTE_POSITION,
                    Mesh::ATTRIBUTE_NORMAL,
                    Mesh::ATTRIBUTE_COLOR,
                    Mesh::ATTRIBUTE_UV_0,
                    Mesh::ATTRIBUTE_UV_1,
                    crate::terrain::PROJECTED,
                    crate::terrain::RED,
                    crate::terrain::GREEN,
                    crate::terrain::BLUE,
                ]
                .iter()
                .chain(crate::terrain::TEXTURE.iter())
                .map(|attribute| format!("{:?}", mesh.attribute(attribute.id)))
                .collect::<Vec<_>>()
                .join("\n")
            })
            .collect()
    }

    fn publish(app: &mut App, candidate: TerrainCandidate) {
        let (playback, replacements) = candidate.into_parts();
        for replacement in replacements {
            *app.world_mut()
                .resource_mut::<Assets<Mesh>>()
                .get_mut(&replacement.handle)
                .unwrap() = replacement.mesh;
            app.world_mut().entity_mut(replacement.entity).insert((
                replacement.visibility,
                crate::terrain::TextureAdmission(replacement.texture_admitted),
            ));
        }
        app.insert_resource(playback);
    }

    #[test]
    fn ordinary_terrain_binding_excludes_staged_scene_descendants() {
        let (mut app, _, _) = fixture(false);
        let live_mesh = app.world().resource::<WaterPlayback>().terrain[0]
            .original
            .clone();
        let root = app
            .world_mut()
            .spawn((
                crate::persistence::StagedRenderRoot,
                Visibility::Hidden,
                Name::new("Terrain"),
            ))
            .id();
        let staged = app
            .world_mut()
            .spawn((ChildOf(root), Mesh3d(live_mesh.clone())))
            .id();
        app.world_mut().resource_mut::<WaterPlayback>().invalidate();
        app.update();
        assert!(
            !app.world()
                .resource::<WaterPlayback>()
                .terrain_entities()
                .contains(&staged)
        );
        assert_eq!(app.world().get::<Mesh3d>(staged).unwrap().0, live_mesh);
    }

    #[test]
    fn terrain_continues_after_rebinding_with_rebased_time_and_camera_changes() -> Result<()> {
        // Mod-supplied heights live in the mod, so only the original wave continues here.
        {
            for projected in [false, true] {
                let (mut live, camera, _) = fixture(projected);
                for _ in 0..3 {
                    live.world_mut()
                        .resource_mut::<Time<Real>>()
                        .advance_by(Duration::from_millis(67));
                    live.update();
                }
                let saved = snapshot(&live);
                {
                    assert!(!saved.dirty_normals[15]);
                    assert_eq!(saved.normals[15], [0.0, 1.0, 0.0]);
                    assert_ne!(
                        saved.normals[15],
                        crate::lighting::grid_normals(&saved.grid, 4)[15],
                        "phase-only normal rebuilding must change this retained diagonal"
                    );
                }
                let clock = crate::presentation_clock::PresentationClock::snapshot(
                    None,
                    live.world().resource(),
                );
                let (mut fresh, fresh_camera, entity) = fresh_fixture(projected);
                let prepared = fresh
                    .world_mut()
                    .remove_resource::<WaterPlayback>()
                    .unwrap();
                let candidate = WaterPlayback::from_snapshot(
                    prepared,
                    environment(&fresh),
                    serde_json::from_value(serde_json::to_value(&saved)?)?,
                    &[entity],
                    fresh.world().resource(),
                )?;
                let clock = crate::presentation_clock::PresentationClock::from_snapshot(
                    clock,
                    fresh.world().resource(),
                )?;
                fresh.insert_resource(clock);
                publish(&mut fresh, candidate);
                assert_eq!(mesh_state(&fresh), mesh_state(&live));
                assert_eq!(
                    serde_json::to_value(snapshot(&fresh))?,
                    serde_json::to_value(&saved)?
                );
                for step in 0..4 {
                    for (app, camera) in [(&mut live, camera), (&mut fresh, fresh_camera)] {
                        app.world_mut()
                            .resource_mut::<Time<Real>>()
                            .advance_by(Duration::from_millis(67));
                        if step == 1 {
                            *app.world_mut().get_mut::<GlobalTransform>(camera).unwrap() =
                                GlobalTransform::from(
                                    Transform::from_xyz(-CELL_SIZE, 25.0, 25.0)
                                        .looking_at(Vec3::new(-CELL_SIZE, 50.0, 50.0), Vec3::Y),
                                );
                        }
                        if step == 2 {
                            *app.world_mut().get_mut::<GlobalTransform>(camera).unwrap() =
                                GlobalTransform::from(
                                    Transform::from_xyz(-2.0 * CELL_SIZE, 25.0, 25.0).looking_at(
                                        Vec3::new(-CELL_SIZE, 0.0, -CELL_SIZE),
                                        Vec3::Y,
                                    ),
                                );
                        }
                        if step == 3 {
                            app.world_mut().despawn(camera);
                        }
                        app.update();
                    }
                    assert_eq!(mesh_state(&fresh), mesh_state(&live));
                    assert_eq!(
                        serde_json::to_value(snapshot(&fresh))?,
                        serde_json::to_value(snapshot(&live))?
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn malformed_terrain_candidates_leave_live_state_and_meshes_untouched() -> Result<()> {
        let (live, _, _) = fixture(true);
        let saved = serde_json::to_value(snapshot(&live))?;
        let live_meshes = mesh_state(&live);
        let mut invalid = Vec::new();
        for (field, value) in [
            ("version", serde_json::json!(1)),
            ("grid", serde_json::json!([])),
            ("dirty_normals", serde_json::json!([])),
            ("window", serde_json::Value::Null),
            ("meshes", serde_json::json!([])),
        ] {
            let mut case = saved.clone();
            case[field] = value;
            invalid.push(case);
        }
        let mut case = saved.clone();
        case["window"]["side"] = serde_json::json!(0);
        invalid.push(case);
        let mut case = saved.clone();
        case["normals"][0][0] = serde_json::json!(2.0);
        invalid.push(case);
        let mut case = saved.clone();
        case["meshes"][0]["mapping"] = serde_json::json!(vec![0; 32]);
        invalid.push(case);
        for (path, value) in [
            (vec!["window", "center", "0"], serde_json::json!(i32::MIN)),
            (vec!["meshes", "0", "visible"], serde_json::json!(false)),
            (
                vec!["meshes", "0", "view", "clipping", "0"],
                serde_json::json!(0.0),
            ),
            (vec!["grid", "0", "0"], serde_json::json!(123.0)),
            (vec!["grid", "0", "1"], serde_json::json!(500.0)),
            (vec!["rgb", "0", "0"], serde_json::json!(0.0)),
        ] {
            let mut case = saved.clone();
            let pointer = format!("/{}", path.join("/"));
            *case.pointer_mut(&pointer).unwrap() = value;
            invalid.push(case);
        }
        for case in invalid {
            let (mut fresh, _, entity) = fixture(true);
            let prepared = fresh
                .world_mut()
                .remove_resource::<WaterPlayback>()
                .unwrap();
            assert!(
                WaterPlayback::from_snapshot(
                    prepared,
                    environment(&fresh),
                    serde_json::from_value(case)?,
                    &[entity],
                    fresh.world().resource()
                )
                .is_err()
            );
            assert_eq!(serde_json::to_value(snapshot(&live))?, saved);
            assert_eq!(mesh_state(&live), live_meshes);
        }
        Ok(())
    }
    #[test]
    fn terrain_free_environment_and_foreign_bindings_are_explicit() -> Result<()> {
        let (live, _, _) = fixture(true);
        let saved = snapshot(&live);
        let (mut fresh, _, _) = fixture(true);
        let prepared = fresh
            .world_mut()
            .remove_resource::<WaterPlayback>()
            .unwrap();
        assert!(
            WaterPlayback::from_snapshot(
                prepared,
                environment(&fresh),
                saved.clone(),
                &[Entity::PLACEHOLDER],
                fresh.world().resource()
            )
            .is_err()
        );
        let (mut fresh, _, entity) = fixture(true);
        let prepared = fresh
            .world_mut()
            .remove_resource::<WaterPlayback>()
            .unwrap();
        let mut foreign: Environment =
            serde_json::from_value(serde_json::to_value(environment(&fresh))?)?;
        foreign.water.as_mut().unwrap().parameters[0] += 1.0;
        assert!(
            WaterPlayback::from_snapshot(
                prepared,
                &foreign,
                saved,
                &[entity],
                fresh.world().resource()
            )
            .is_err()
        );
        foreign.water = None;
        foreign.terrain_lighting = None;
        let empty = WaterPlayback::default().snapshot_playback(&foreign, &World::new())?;
        let (playback, meshes) = WaterPlayback::from_snapshot(
            WaterPlayback::default(),
            &foreign,
            empty,
            &[],
            &Assets::default(),
        )?
        .into_parts();
        assert!(playback.terrain_entities().is_empty());
        assert!(meshes.is_empty());
        Ok(())
    }
}
