//! Demand-driven texture frames from preservation's texture-animation contract.
mod admission;
mod frames;

use super::SceneMaterial;
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
use serde::Deserialize;
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
    admission: admission::Model,
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
}

#[derive(Resource, Default)]
struct Images {
    environment: Option<AssetId<Environment>>,
    states: HashMap<String, ImageState>,
}

pub(super) fn register(app: &mut App) {
    frames::register(app);
    app.init_resource::<Images>()
        .add_systems(Update, bind.before(super::apply_scene_fog))
        .add_systems(
            PostUpdate,
            advance.after(VisibilitySystems::CheckVisibility),
        );
}

// Binding joins retained frame handles, metadata and the current world lifetime.
#[allow(clippy::type_complexity)]
fn bind(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    mut images: ResMut<Images>,
    meshes: Res<Assets<Mesh>>,
    candidates: Query<
        (
            Entity,
            &MeshMaterial3d<StandardMaterial>,
            &GltfMaterialExtras,
            Option<&GltfExtras>,
            &Mesh3d,
            Option<&frames::Frames>,
        ),
        Without<Inspected>,
    >,
) {
    if images.environment != Some(request.0.id()) || request.is_changed() {
        images.environment = Some(request.0.id());
        images.states.clear();
    }
    for (entity, material, extras, geometry, mesh, frames) in &candidates {
        let Some(mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        let result = (|| -> anyhow::Result<Option<Binding>> {
            let Some(description) =
                serde_json::from_str::<MaterialExtras>(&extras.value)?.rgpre_texture_animation
            else {
                return Ok(None);
            };
            // Terrain admission and scripted presentation use separate ownership inputs.
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
            if let Some(existing) = images.states.get(&key) {
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
            images
                .states
                .entry(key.clone())
                .or_insert_with(|| ImageState {
                    count,
                    delay,
                    playback: Playback::default(),
                });
            Ok(Some(Binding {
                key,
                frames,
                admission,
            }))
        })();
        commands.entity(entity).insert(Inspected);
        match result {
            Ok(Some(binding)) => {
                commands.entity(entity).insert(binding);
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
    distance: Res<super::RenderDistance>,
    fidelity: Res<Fidelity>,
    mut images: ResMut<Images>,
    assets: Res<Assets<Image>>,
    meshes: Res<Assets<Mesh>>,
    mut materials: ResMut<Assets<SceneMaterial>>,
    camera: Query<(&GlobalTransform, &Projection), With<ViewCamera>>,
    bindings: Query<(
        &Binding,
        &Mesh3d,
        &GlobalTransform,
        &InheritedVisibility,
        &MeshMaterial3d<SceneMaterial>,
        Entity,
    )>,
    parents: Query<&ChildOf>,
    scripted: Query<(), With<ScriptBinding>>,
    animated: Query<(), With<crate::animation::BoundFrameMesh>>,
    mut clip: Local<admission::Clip>,
) {
    let (Ok((camera, projection)), Some(clipping)) =
        (camera.single(), distance.effective(&fidelity))
    else {
        return;
    };
    let projection = projection.get_clip_from_view();
    let scale = Vec2::new(projection.x_axis.x, projection.y_axis.y);
    let now = bios_tick(time.elapsed());
    for (binding, mesh, transform, visibility, material, entity) in &bindings {
        if !visibility.get()
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
        if attributes < 0.0 || attributes as u8 & 4 != 0 {
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
        if binding.admission.admits(
            positions, normals, transform, camera, scale, clipping, &mut clip,
        ) && let Some(state) = images.states.get_mut(&binding.key)
        {
            state.playback.request(now, state.count, state.delay);
        }
    }
    for (binding, _, _, _, material, _) in &bindings {
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
            .init_resource::<Images>()
            .add_systems(Update, advance);
        let frames: Vec<_> = (0..4)
            .map(|_| {
                app.world_mut()
                    .resource_mut::<Assets<Image>>()
                    .add(Image::default())
            })
            .collect();
        app.world_mut().resource_mut::<Images>().states.insert(
            "a00000".into(),
            ImageState {
                count: 4,
                delay: 2,
                playback: Playback::default(),
            },
        );
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD,
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[-0.1, -0.1, 0.0], [0.1, -0.1, 0.0], [0.0, 0.1, 0.0]],
        );
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let material = SceneMaterial {
            base: StandardMaterial {
                base_color_texture: Some(frames[0].clone()),
                ..default()
            },
            extension: super::super::ModelExtension {
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
        let materials: Vec<_> = (0..2)
            .map(|_| {
                app.world_mut()
                    .resource_mut::<Assets<SceneMaterial>>()
                    .add(material.clone())
            })
            .collect();
        let nodes: Vec<_> = materials.iter().map(|material| {
            let admission = admission::Model::parse(r#"{"rgpre_admission":{"version":1,"radius_fixed":256,"polygons":[{"vertices":[0,1,2],"normal":[0,0,1]}]}}"#).unwrap().unwrap();
            app.world_mut().spawn((Binding { key: "a00000".into(), frames: frames.clone(), admission }, Mesh3d(mesh.clone()), GlobalTransform::from_translation(Vec3::new(0.0,0.0,-1.0)), InheritedVisibility::VISIBLE, MeshMaterial3d(material.clone()))).id()
        }).collect();
        app.world_mut().spawn((
            ViewCamera,
            GlobalTransform::IDENTITY,
            Projection::Perspective(PerspectiveProjection::default()),
        ));
        let check = |app: &App, frame: usize| {
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
        app.update();
        check(&app, 2);
    }
}
