//! Demand-driven texture frames from preservation's texture-animation contract.
pub(super) mod admission;
mod checkpoint;
mod frames;
pub use checkpoint::{TexturePlaybackSnapshot, TextureSourcesSnapshot};

use super::{SceneMaterial, TerrainMaterial};
use crate::persistence::StagedRenderRoot;
use crate::{camera::ViewCamera, environment::EnvironmentRequest};
use anyhow::ensure;
use bevy::{
    camera::visibility::VisibilitySystems,
    gltf::{GltfExtras, GltfMaterialExtras},
    mesh::VertexAttributeValues,
    prelude::*,
};
use cyrus_assets::environment::Environment;
use cyrus_game::{clock::bios_tick, fidelity::Fidelity, script::ScriptBinding};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Description {
    version: u32,
    image_name: String,
    delay_ms: i16,
    textures: Vec<usize>,
}

#[derive(Deserialize)]
struct MaterialExtras {
    rgpre_texture_animation: Option<Description>,
}

#[derive(Component)]
struct Inspected;

#[derive(Component)]
struct Binding {
    key: String,
    frames: Vec<Handle<Image>>,
}

#[derive(Component)]
enum Admission {
    Model(admission::Model),
    Flat(admission::Flat),
}

#[derive(Default)]
struct Playback {
    frame: Option<usize>,
    deadline: u32,
}

impl Playback {
    fn request(&mut self, now: u32, count: usize, delay: u32) -> usize {
        if count == 1 {
            self.frame = Some(0);
        } else if self.frame.is_none() || now >= self.deadline {
            self.frame = Some(self.frame.map_or(0, |frame| (frame + 1) % count));
            self.deadline = now.wrapping_add(delay);
        }
        self.frame.unwrap_or(0)
    }
}

struct ImageState {
    count: usize,
    delay: u32,
    playback: Playback,
    held: bool,
}

impl ImageState {
    fn request(&mut self, now: u32) {
        if !self.held {
            self.playback.request(now, self.count, self.delay);
        }
    }
}

/// Loaded shared image timelines, with explicit inspection holds for matched captures.
/// Holds are absent during ordinary playback and are cleared on world replacement.
#[derive(Resource, Default)]
pub struct TextureAnimations {
    environment: Option<AssetId<Environment>>,
    states: HashMap<String, ImageState>,
    restored_binding: bool,
}

/// Observed zero-based image frame and inspection state, sorted by canonical image name.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TextureFrame {
    pub image: String,
    pub frame: Option<usize>,
    pub count: usize,
    pub held: bool,
}

impl TextureAnimations {
    /// Whether this owner is bound to the environment asset adopted at publication.
    pub fn is_bound_to(&self, environment: AssetId<Environment>) -> bool {
        self.environment == Some(environment)
    }

    /// Inspect loaded animated images. An unset frame has not yet received a draw request.
    pub fn snapshot(&self) -> Vec<TextureFrame> {
        let mut frames: Vec<_> = self
            .states
            .iter()
            .map(|(image, state)| TextureFrame {
                image: image.clone(),
                frame: state.playback.frame,
                count: state.count,
                held: state.held,
            })
            .collect();
        frames.sort_by(|a, b| a.image.cmp(&b.image));
        frames
    }

    /// Hold a measured frame for inspection, or resume automatic requests with `None`.
    /// Selection preserves the existing deadline, following preservation's explicit-frame rule.
    /// Unknown images and out-of-range frames fail without changing the timeline.
    pub fn hold(&mut self, image: &str, frame: Option<usize>) -> anyhow::Result<()> {
        let key = image.to_ascii_lowercase();
        let state = self
            .states
            .get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!("texture {image} is not loaded for animation"))?;
        if let Some(frame) = frame {
            ensure!(
                frame < state.count,
                "texture {image}: frame {frame} outside 0..{}",
                state.count
            );
            state.playback.frame = Some(frame);
        }
        state.held = frame.is_some();
        Ok(())
    }
}

pub(super) fn register(app: &mut App) {
    frames::register(app);
    app.init_resource::<TextureAnimations>()
        .add_systems(
            Update,
            bind.after(crate::flat::PrepareFlats)
                .before(super::apply_scene_fog),
        )
        .add_systems(
            PostUpdate,
            advance.after(VisibilitySystems::CheckVisibility).run_if(
                |logbook: Option<Res<cyrus_game::logbook::LogbookScreen>>| {
                    !logbook.is_some_and(|screen| screen.suspended())
                },
            ),
        );
}

// Binding joins retained frame handles, metadata, staging and the current world lifetime.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn bind(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    mut images: ResMut<TextureAnimations>,
    meshes: Res<Assets<Mesh>>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
    staged: Query<(), With<StagedRenderRoot>>,
    candidates: Query<
        (
            Entity,
            &MeshMaterial3d<StandardMaterial>,
            &GltfMaterialExtras,
            Option<&GltfExtras>,
            &Mesh3d,
            Option<&frames::Frames>,
            Option<&crate::flat::Flat>,
        ),
        Without<Inspected>,
    >,
) {
    let restored_binding = std::mem::take(&mut images.restored_binding);
    if images.environment != Some(request.0.id()) || (request.is_changed() && !restored_binding) {
        images.environment = Some(request.0.id());
        images.states.clear();
    }
    for (entity, material, extras, geometry, mesh, frames, flat) in &candidates {
        if std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|node| staged.contains(node))
        {
            continue;
        }
        let Some(mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        let terrain = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|node| names.get(node).is_ok_and(|name| name.as_str() == "Terrain"));
        let result = images.bind_mesh(mesh, extras, geometry, frames, flat, terrain);
        commands.entity(entity).insert(Inspected);
        match result {
            Ok(Some((binding, admission))) => {
                commands.entity(entity).insert(binding);
                if let Some(admission) = admission {
                    commands.entity(entity).insert(admission);
                }
            }
            Ok(None) => {}
            Err(error) => error!(
                "world {:?}, material {:?}: texture animation unavailable: {error:#}",
                request.0.path(),
                material.0.path()
            ),
        }
    }
}

// Admission needs the current world camera, authored corners, material and hierarchy.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn advance(
    time: Res<Time<Real>>,
    clock: Option<Res<crate::presentation_clock::PresentationClock>>,
    distance: Res<super::RenderDistance>,
    fidelity: Res<Fidelity>,
    mut images: ResMut<TextureAnimations>,
    assets: Res<Assets<Image>>,
    meshes: Res<Assets<Mesh>>,
    mut materials: ResMut<Assets<SceneMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    camera: Query<(&GlobalTransform, &Projection), With<ViewCamera>>,
    bindings: Query<(
        &Binding,
        &Admission,
        &Mesh3d,
        &GlobalTransform,
        &InheritedVisibility,
        &MeshMaterial3d<SceneMaterial>,
        Entity,
    )>,
    terrain: Query<(
        Entity,
        &Binding,
        &crate::terrain::TextureAdmission,
        &MeshMaterial3d<TerrainMaterial>,
    )>,
    hierarchy: (Query<&ChildOf>, Query<(), With<StagedRenderRoot>>),
    scripted: Query<(), With<ScriptBinding>>,
    animated: Query<(), With<crate::animation::BoundFrameMesh>>,
    mut clip: Local<admission::Clip>,
) {
    let (parents, staged) = hierarchy;
    let is_staged = |entity| {
        std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|node| staged.contains(node))
    };
    let (Ok((camera, projection)), Some(clipping)) =
        (camera.single(), distance.effective(&fidelity))
    else {
        return;
    };
    let projection = projection.get_clip_from_view();
    let scale = Vec2::new(projection.x_axis.x, projection.y_axis.y);
    let now = bios_tick(crate::presentation_clock::PresentationClock::elapsed(
        clock.as_deref(),
        &time,
    ));
    for (entity, binding, admission, _) in &terrain {
        if !is_staged(entity)
            && admission.0
            && binding
                .frames
                .iter()
                .all(|frame| assets.contains(frame.id()))
            && let Some(state) = images.states.get_mut(&binding.key)
        {
            state.request(now);
        }
    }
    for (binding, admission, mesh, transform, visibility, material, entity) in &bindings {
        if is_staged(entity)
            || !visibility.get()
            || binding
                .frames
                .iter()
                .any(|frame| assets.get(frame).is_none())
        {
            continue;
        }
        if std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|node| scripted.contains(node))
        {
            continue;
        }
        let Some(material) = materials.get(&material.0) else {
            continue;
        };
        let attributes = material.extension.lighting.w;
        if matches!(admission, Admission::Model(_))
            && (attributes < 0.0 || attributes as u8 & 4 != 0)
        {
            continue;
        }
        let Some(mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let normals = if animated.contains(entity) {
            match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
                Some(VertexAttributeValues::Float32x3(normals)) => Some(normals.as_slice()),
                _ => continue,
            }
        } else {
            None
        };
        let admitted = match admission {
            Admission::Model(model) => model.admits(
                positions, normals, transform, camera, scale, clipping, &mut clip,
            ),
            Admission::Flat(flat) => {
                super::compositing::selected_cell(transform.translation(), camera.translation())
                    .is_some()
                    && flat.admits(positions, transform, camera, scale, clipping, &mut clip)
            }
        };
        if admitted && let Some(state) = images.states.get_mut(&binding.key) {
            state.request(now);
        }
    }
    for (binding, _, _, _, _, material, entity) in &bindings {
        if is_staged(entity) {
            continue;
        }
        let Some(frame) = images
            .states
            .get(&binding.key)
            .and_then(|state| state.playback.frame)
        else {
            continue;
        };
        let texture = &binding.frames[frame];
        if materials
            .get(&material.0)
            .is_some_and(|material| material.base.base_color_texture.as_ref() != Some(texture))
            && let Some(mut material) = materials.get_mut(&material.0)
        {
            material.base.base_color_texture = Some(texture.clone());
        }
    }
    for (entity, binding, _, material) in &terrain {
        if is_staged(entity) {
            continue;
        }
        let Some(frame) = images
            .states
            .get(&binding.key)
            .and_then(|state| state.playback.frame)
        else {
            continue;
        };
        let texture = &binding.frames[frame];
        if terrain_materials
            .get(&material.0)
            .is_some_and(|material| material.base.base_color_texture.as_ref() != Some(texture))
            && let Some(mut material) = terrain_materials.get_mut(&material.0)
        {
            material.base.base_color_texture = Some(texture.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_the_environment_discards_inspection_holds() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<TextureAnimations>()
            .insert_resource(EnvironmentRequest(Handle::default()))
            .add_systems(Update, bind);
        app.update();
        app.world_mut()
            .resource_mut::<TextureAnimations>()
            .states
            .insert(
                "d52001".into(),
                ImageState {
                    count: 5,
                    delay: 1,
                    playback: Playback::default(),
                    held: false,
                },
            );
        app.world_mut()
            .resource_mut::<TextureAnimations>()
            .hold("d52001", Some(4))
            .unwrap();
        app.update();
        assert!(app.world().resource::<TextureAnimations>().snapshot()[0].held);
        app.insert_resource(EnvironmentRequest(Handle::default()));
        app.update();
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .snapshot()
                .is_empty()
        );
    }

    #[test]
    fn inspection_hold_validates_and_preserves_shared_deadline() {
        let mut images = TextureAnimations::default();
        images.states.insert(
            "d52001".into(),
            ImageState {
                count: 5,
                delay: 2,
                playback: Playback {
                    frame: Some(1),
                    deadline: 20,
                },
                held: false,
            },
        );
        assert!(images.hold("missing", Some(0)).is_err());
        assert!(images.hold("d52001", Some(5)).is_err());
        assert_eq!(images.snapshot()[0].frame, Some(1));
        images.hold("D52001", Some(4)).unwrap();
        images.states.get_mut("d52001").unwrap().request(100);
        assert_eq!(
            images.snapshot(),
            vec![TextureFrame {
                image: "d52001".into(),
                frame: Some(4),
                count: 5,
                held: true
            }]
        );
        assert_eq!(images.states["d52001"].playback.deadline, 20);
        images.hold("d52001", None).unwrap();
        images.states.get_mut("d52001").unwrap().request(100);
        assert_eq!(images.snapshot()[0].frame, Some(0));
        assert!(!images.snapshot()[0].held);
        assert_eq!(images.states["d52001"].playback.deadline, 102);
    }

    #[test]
    fn admitted_requests_share_a_deadline_and_do_not_catch_up() {
        let mut playback = Playback::default();
        assert_eq!(playback.request(100, 4, 2), 0);
        assert_eq!(playback.request(100, 4, 2), 0);
        assert_eq!(playback.request(101, 4, 2), 0);
        assert_eq!(playback.request(102, 4, 2), 1);
        assert_eq!(playback.request(1000, 4, 2), 2);
        assert_eq!(playback.request(1001, 4, 2), 2);
        assert_eq!(playback.request(1002, 4, 2), 3);
        assert_eq!(playback.request(1004, 4, 2), 0);
    }
    #[test]
    fn renderer_retains_hidden_frames_and_resumes_once_for_shared_materials() {
        use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology};
        use std::time::Duration;
        let mut app = App::new();
        app.insert_resource(Time::<Real>::default())
            .insert_resource(super::super::RenderDistance {
                original: Some([1.0, 100.0]),
                override_far: None,
            })
            .insert_resource(Fidelity::default())
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SceneMaterial>>()
            .init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<TextureAnimations>()
            .add_systems(Update, advance);
        let frames: Vec<_> = (0..4)
            .map(|_| {
                app.world_mut()
                    .resource_mut::<Assets<Image>>()
                    .add(Image::default())
            })
            .collect();
        app.world_mut()
            .resource_mut::<TextureAnimations>()
            .states
            .insert(
                "a00000".into(),
                ImageState {
                    count: 4,
                    delay: 2,
                    playback: Playback::default(),
                    held: false,
                },
            );
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD,
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [-0.1, -0.1, 0.0],
                [0.1, -0.1, 0.0],
                [0.1, 0.1, 0.0],
                [-0.1, 0.1, 0.0],
            ],
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]],
        );
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let material = SceneMaterial {
            base: StandardMaterial {
                base_color_texture: Some(frames[0].clone()),
                ..default()
            },
            extension: super::super::ModelExtension {
                solid_color: Vec4::ZERO,
                chroma_key: Vec4::ZERO,
                texture_layout: UVec4::ZERO,
                texture_scale: Vec4::ZERO,
                compositing: Vec4::new(-1.0, 0.0, 0.0, 0.0),
                fog: super::super::FogExtension {
                    color: Vec4::ZERO,
                    clipping: Vec4::ZERO,
                    table: [Vec4::ZERO; 16],
                    water: Vec4::ZERO,
                    ambient: Vec4::ZERO,
                    directional: Vec4::ZERO,
                    direction: Vec4::ZERO,
                },
                lights: Handle::default(),
                placement: Mat4::IDENTITY,
                lighting: Vec4::ZERO,
            },
        };
        let terrain_frames: Vec<_> = (0..4)
            .map(|_| {
                app.world_mut()
                    .resource_mut::<Assets<Image>>()
                    .add(Image::default())
            })
            .collect();
        let terrain_material = app
            .world_mut()
            .resource_mut::<Assets<TerrainMaterial>>()
            .add(TerrainMaterial {
                base: StandardMaterial {
                    base_color_texture: Some(terrain_frames[0].clone()),
                    ..default()
                },
                extension: super::super::TerrainExtension {
                    fog: material.extension.fog.clone(),
                },
            });
        let terrain_node = app
            .world_mut()
            .spawn((
                Binding {
                    key: "a00000".into(),
                    frames: terrain_frames.clone(),
                },
                crate::terrain::TextureAdmission(false),
                MeshMaterial3d(terrain_material.clone()),
            ))
            .id();
        let materials: Vec<_> = (0..2)
            .map(|_| {
                app.world_mut()
                    .resource_mut::<Assets<SceneMaterial>>()
                    .add(material.clone())
            })
            .collect();
        let nodes: Vec<_> = materials.iter().enumerate().map(|(index, material)| {
            let admission = admission::Model::parse(r#"{"rgpre_admission":{"version":1,"radius_fixed":256,"polygons":[{"vertices":[0,1,2],"normal":[0,0,1]}]}}"#).unwrap().unwrap();
            let admission = if index == 0 { Admission::Model(admission) } else {
                Admission::Flat(admission::Flat::from_mesh(app.world().resource::<Assets<Mesh>>().get(&mesh).unwrap(), 2.0).unwrap())
            };
            app.world_mut().spawn((Binding { key: "a00000".into(), frames: frames.clone() }, admission, Mesh3d(mesh.clone()), GlobalTransform::from_translation(Vec3::new(-1000.0,0.0,-1001.0)), InheritedVisibility::VISIBLE, MeshMaterial3d(material.clone()))).id()
        }).collect();
        app.world_mut().spawn((
            ViewCamera,
            GlobalTransform::from_translation(Vec3::new(-1000.0, 0.0, -1000.0)),
            Projection::Perspective(PerspectiveProjection::default()),
        ));
        let check = |app: &App, frame: usize| {
            assert_eq!(
                app.world()
                    .resource::<Assets<TerrainMaterial>>()
                    .get(&terrain_material)
                    .unwrap()
                    .base
                    .base_color_texture,
                Some(terrain_frames[frame].clone())
            );
            for material in &materials {
                assert_eq!(
                    app.world()
                        .resource::<Assets<SceneMaterial>>()
                        .get(material)
                        .unwrap()
                        .base
                        .base_color_texture,
                    Some(frames[frame].clone())
                );
            }
        };
        for entity in nodes.iter().copied().chain([terrain_node]) {
            app.world_mut().entity_mut(entity).insert(StagedRenderRoot);
        }
        app.world_mut()
            .get_mut::<crate::terrain::TextureAdmission>(terrain_node)
            .unwrap()
            .0 = true;
        app.update();
        assert_eq!(
            app.world().resource::<TextureAnimations>().snapshot()[0].frame,
            None
        );
        for entity in nodes.iter().copied().chain([terrain_node]) {
            app.world_mut()
                .entity_mut(entity)
                .remove::<StagedRenderRoot>();
        }
        app.world_mut()
            .get_mut::<crate::terrain::TextureAdmission>(terrain_node)
            .unwrap()
            .0 = false;
        app.update();
        check(&app, 0);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_secs(1));
        app.update();
        check(&app, 1);
        for &node in &nodes {
            app.world_mut()
                .entity_mut(node)
                .insert(InheritedVisibility::HIDDEN);
        }
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_secs(10));
        app.update();
        check(&app, 1);
        app.world_mut()
            .entity_mut(nodes[0])
            .insert(InheritedVisibility::VISIBLE);
        app.update();
        check(&app, 2);
        for &node in &nodes {
            app.world_mut()
                .entity_mut(node)
                .insert(InheritedVisibility::HIDDEN);
        }
        app.world_mut()
            .entity_mut(terrain_node)
            .insert(crate::terrain::TextureAdmission(true));
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_secs(1));
        app.update();
        check(&app, 3);
        app.update();
        check(&app, 3);
        app.world_mut()
            .entity_mut(terrain_node)
            .insert(crate::terrain::TextureAdmission(false));
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_secs(10));
        app.update();
        check(&app, 3);
        let camera = app
            .world_mut()
            .query_filtered::<Entity, With<ViewCamera>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .entity_mut(camera)
            .insert(GlobalTransform::IDENTITY);
        app.world_mut().entity_mut(nodes[1]).insert((
            InheritedVisibility::VISIBLE,
            GlobalTransform::from_translation(Vec3::new(0.0, 0.0, -1.0)),
        ));
        app.update();
        check(&app, 3);
        app.world_mut()
            .entity_mut(camera)
            .insert(GlobalTransform::from_translation(Vec3::new(
                -1000.0, 0.0, -1000.0,
            )));
        app.world_mut()
            .entity_mut(nodes[1])
            .insert(GlobalTransform::from_translation(Vec3::new(
                -1000.0, 0.0, -1001.0,
            )));
        app.update();
        check(&app, 0);
        app.update();
        check(&app, 0);
        app.world_mut()
            .resource_mut::<TextureAnimations>()
            .hold("a00000", Some(2))
            .unwrap();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_secs(10));
        app.update();
        check(&app, 2);
        app.update();
        check(&app, 2);
        app.world_mut()
            .resource_mut::<TextureAnimations>()
            .hold("a00000", None)
            .unwrap();
        app.update();
        check(&app, 3);
    }
}

impl TextureAnimations {
    fn bind_mesh(
        &mut self,
        mesh: &Mesh,
        extras: &GltfMaterialExtras,
        geometry: Option<&GltfExtras>,
        frames: Option<&frames::Frames>,
        flat: Option<&crate::flat::Flat>,
        terrain: bool,
    ) -> anyhow::Result<Option<(Binding, Option<Admission>)>> {
        let Some(description) =
            serde_json::from_str::<MaterialExtras>(&extras.value)?.rgpre_texture_animation
        else {
            return Ok(None);
        };
        let admission = if terrain {
            None
        } else if let Some(flat) = flat {
            Some(Admission::Flat(admission::Flat::from_mesh(
                mesh,
                flat.radius,
            )?))
        } else {
            let Some(geometry) = geometry else {
                return Ok(None);
            };
            let Some(admission) = admission::Model::parse(&geometry.value)? else {
                return Ok(None);
            };
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                anyhow::bail!("animated texture mesh lacks three-component positions")
            };
            admission.validate(positions)?;
            Some(Admission::Model(admission))
        };
        ensure!(
            description.version == 1,
            "texture animation version {} unsupported",
            description.version
        );
        ensure!(
            !description.textures.is_empty(),
            "texture {} has no frames",
            description.image_name
        );
        ensure!(
            description.delay_ms >= 0,
            "texture {} has unsupported negative delay {}",
            description.image_name,
            description.delay_ms
        );
        ensure!(
            description.image_name.len() == 6
                && description
                    .image_name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric()),
            "texture name {:?} is not a shipped canonical image key",
            description.image_name
        );
        let key = description.image_name.to_ascii_lowercase();
        let delay = (f64::from(description.delay_ms) * 18.2 / 1000.0)
            .round_ties_even()
            .max(1.0) as u32;
        let count = description.textures.len();
        if let Some(existing) = self.states.get(&key) {
            ensure!(
                existing.count == count && existing.delay == delay,
                "texture {key} has conflicting shared frame metadata"
            );
        }
        let frames = frames.ok_or_else(|| {
            anyhow::anyhow!("animated texture frames were not retained by the GLTF loader")
        })?;
        ensure!(
            frames.0.len() == count,
            "texture frame count disagrees with GLTF metadata"
        );
        let frames = frames.0.clone();
        self.states
            .entry(key.clone())
            .or_insert_with(|| ImageState {
                count,
                delay,
                playback: Playback::default(),
                held: false,
            });
        Ok(Some((Binding { key, frames }, admission)))
    }

    /// Bind only an off-live scene forest, without advancing or replacing live timelines.
    /// Callers must keep these roots hidden until coordinated session publication.
    /// Every mesh validates before binding components are installed.
    pub fn prepare(
        world: &mut World,
        environment: AssetId<Environment>,
        roots: &[Entity],
    ) -> anyhow::Result<Self> {
        let mut prepared = Self {
            environment: Some(environment),
            ..default()
        };
        let mut pending: Vec<_> = roots.iter().map(|&entity| (entity, false)).collect();
        let mut visited = std::collections::HashSet::new();
        let mut bindings = Vec::new();
        for &root in roots {
            ensure!(
                world.get::<Visibility>(root) == Some(&Visibility::Hidden)
                    && world.get::<StagedRenderRoot>(root).is_some(),
                "texture preparation root {root:?} is not hidden"
            );
        }
        while let Some((entity, parent_terrain)) = pending.pop() {
            ensure!(
                visited.insert(entity),
                "texture preparation hierarchy repeats entity {entity:?}"
            );
            ensure!(
                world.get_entity(entity).is_ok(),
                "texture preparation entity {entity:?} absent"
            );
            let terrain = parent_terrain
                || world
                    .get::<Name>(entity)
                    .is_some_and(|name| name.as_str() == "Terrain");
            if let Some(children) = world.get::<Children>(entity) {
                pending.extend(children.iter().map(|child| (child, terrain)));
            }
            let (Some(mesh), Some(extras)) = (
                world.get::<Mesh3d>(entity),
                world.get::<GltfMaterialExtras>(entity),
            ) else {
                continue;
            };
            let mesh = world
                .get_resource::<Assets<Mesh>>()
                .and_then(|assets| assets.get(&mesh.0))
                .ok_or_else(|| anyhow::anyhow!("texture preparation mesh {entity:?} absent"))?;
            let binding = prepared
                .bind_mesh(
                    mesh,
                    extras,
                    world.get::<GltfExtras>(entity),
                    world.get::<frames::Frames>(entity),
                    world.get::<crate::flat::Flat>(entity),
                    terrain,
                )
                .map_err(|error| {
                    anyhow::anyhow!("texture preparation entity {entity:?}: {error:#}")
                })?;
            bindings.push((entity, binding));
        }
        for (entity, binding) in bindings {
            let mut entity = world.entity_mut(entity);
            entity.insert(Inspected);
            if let Some((binding, admission)) = binding {
                entity.insert(binding);
                if let Some(admission) = admission {
                    entity.insert(admission);
                }
            }
        }
        Ok(prepared)
    }
}

#[cfg(test)]
mod preparation_tests {
    use super::*;

    #[test]
    fn preparation_rejects_whole_forest_before_installing_any_binding() {
        let mut world = World::new();
        world.insert_resource(Assets::<Mesh>::default());
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::MAIN_WORLD,
        ));
        let root = world
            .spawn((StagedRenderRoot, Visibility::Hidden, Name::new("Terrain")))
            .id();
        let metadata = |version| GltfMaterialExtras {
            value: serde_json::json!({
                "rgpre_texture_animation": {"version": version, "image_name": "A00000",
                    "delay_ms": 100, "textures": [0]}
            })
            .to_string(),
        };
        let valid = world
            .spawn((
                ChildOf(root),
                Mesh3d(mesh.clone()),
                metadata(1),
                frames::Frames(vec![Handle::default()]),
            ))
            .id();
        let invalid = world
            .spawn((
                ChildOf(root),
                Mesh3d(mesh),
                metadata(2),
                frames::Frames(vec![Handle::default()]),
            ))
            .id();
        let environment = Handle::<Environment>::default().id();
        assert!(TextureAnimations::prepare(&mut world, environment, &[root]).is_err());
        assert!(world.get::<Binding>(valid).is_none());
        assert!(world.get::<Inspected>(valid).is_none());
        world.entity_mut(invalid).insert(metadata(1));
        let prepared = TextureAnimations::prepare(&mut world, environment, &[root]).unwrap();
        assert_eq!(
            prepared.snapshot(),
            vec![TextureFrame {
                image: "a00000".into(),
                frame: None,
                count: 1,
                held: false
            }]
        );
        assert!(world.get::<Binding>(valid).is_some());
        assert!(world.get::<Binding>(invalid).is_some());
        world.insert_resource(Assets::<Image>::default());
        let image = world.resource_mut::<Assets<Image>>().add(Image::default());
        for entity in [valid, invalid] {
            world.get_mut::<Binding>(entity).unwrap().frames = vec![image.clone()];
        }
        prepared.snapshot_sources(&world, &[root]).unwrap();
        world.entity_mut(valid).remove::<Binding>();
        assert!(
            prepared
                .snapshot_sources(&world, &[root])
                .err()
                .unwrap()
                .to_string()
                .contains("authored animation is not bound")
        );
        assert!(!world.contains_resource::<TextureAnimations>());
    }
}
