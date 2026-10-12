//! Selection-owned scene, camera and media playback.
use crate::{
    loading::Blob,
    ui::{DataMode, Shading, Viewer},
};
use bevy::{
    audio::{AudioSinkPlayback, Volume},
    camera::{Viewport, primitives::Aabb},
    prelude::*,
};
use cyrus_app::bevy_egui::{PrimaryEguiContext, egui};
use cyrus_assets::Content;

#[derive(Component)]
pub struct ViewCamera;
#[derive(Component)]
pub struct PreviewRoot;
#[derive(Component)]
pub struct PreviewFrame(usize);
#[derive(Component)]
pub struct PreviewAudio;

pub fn setup(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        PrimaryEguiContext,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
    ));
    commands.spawn((
        Camera3d::default(),
        ViewCamera,
        AmbientLight {
            brightness: 1000.,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            near: 0.01,
            far: 1_000_000.,
            ..default()
        }),
        Transform::from_xyz(0., 2., 6.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 20000.,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.)),
    ));
}
pub fn sync(
    mut commands: Commands,
    mut viewer: ResMut<Viewer>,
    assets: Res<AssetServer>,
    mut roots: Query<(Entity, &WorldAssetRoot, &PreviewFrame, &mut Visibility), With<PreviewRoot>>,
    mut loaded_model: Local<Option<String>>,
    audio: Query<(Entity, &AudioPlayer<AudioSource>), With<PreviewAudio>>,
) {
    let selection_changed = viewer.scene_dirty;
    if viewer.scene_dirty {
        let frames = viewer.preview.as_ref().and_then(|p| match &p.content {
            Content::Model { frames, .. } => Some(frames),
            _ => None,
        });
        let key = frames.and_then(|frames| frames.first()).cloned();
        if key != *loaded_model {
            for (root, _, _, _) in &mut roots {
                commands.entity(root).despawn();
            }
            if let Some(frames) = frames {
                for (index, path) in frames.iter().enumerate() {
                    let handle =
                        assets.load(GltfAssetLabel::Scene(0).from_asset(format!("cache://{path}")));
                    commands.spawn((
                        PreviewRoot,
                        PreviewFrame(index),
                        WorldAssetRoot(handle),
                        if index == viewer.frame {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        },
                    ));
                }
                viewer.needs_frame = true;
            }
            *loaded_model = key;
        } else {
            for (_, _, frame, mut visibility) in &mut roots {
                *visibility = if frame.0 == viewer.frame {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
        viewer.scene_dirty = false;
    }
    viewer.render_loading = false;
    if let Some(Content::Model { frames, .. }) = viewer.preview.as_ref().map(|p| &p.content) {
        let total = frames.len();
        let done = roots
            .iter()
            .filter(|(_, root, _, _)| assets.is_loaded_with_dependencies(root.0.id()))
            .count();
        viewer.render_loading = done < total;
        if viewer.render_loading {
            viewer.load_status = format!("Loading model frames and textures ({done}/{total})");
            viewer.load_fraction = Some(done as f32 / total.max(1) as f32);
        }
    }
    if !selection_changed {
        for (_, root, _, _) in &roots {
            if let Some(bevy::asset::LoadState::Failed(error)) = assets.get_load_state(root.0.id())
            {
                viewer.error = Some(format!("Model load: {error}"));
                viewer.needs_frame = false;
                viewer.render_loading = false;
            }
        }
    }
    if !viewer.audio_dirty {
        for (_, player) in &audio {
            if !assets.is_loaded_with_dependencies(player.0.id()) {
                viewer.render_loading = true;
                viewer.load_fraction = None;
                viewer.load_status = "Loading audio…".into();
            }
            if let Some(bevy::asset::LoadState::Failed(error)) =
                assets.get_load_state(player.0.id())
            {
                viewer.error = Some(format!("Audio load: {error}"));
                viewer.playing = false;
                viewer.render_loading = false;
            }
        }
    }
    if viewer.audio_dirty {
        for (entity, _) in &audio {
            commands.entity(entity).despawn();
        }
        let source = viewer.preview.as_ref().and_then(|p| match &p.content {
            Content::Audio { wav, .. } => Some(wav.clone()),
            Content::Movie { wav, .. } => wav.clone(),
            _ => None,
        });
        if let Some(path) = source {
            commands.spawn((
                PreviewAudio,
                AudioPlayer::<AudioSource>(assets.load(format!("cache://{path}"))),
                PlaybackSettings {
                    paused: !viewer.playing,
                    volume: Volume::Linear(viewer.volume),
                    ..PlaybackSettings::ONCE
                },
            ));
        }
        viewer.audio_dirty = false;
    }
    if viewer.movie_blob.is_none()
        && viewer.movie.is_none()
        && viewer.error.is_none()
        && let Some(Content::Movie { smk, .. }) = viewer.preview.as_ref().map(|p| &p.content)
    {
        viewer.movie_blob = Some(assets.load(format!("cache://{smk}")));
    }
}

pub fn frame_camera(
    mut viewer: ResMut<Viewer>,
    meshes: Query<(&Aabb, &GlobalTransform), With<Mesh3d>>,
    mut cameras: Query<(&mut Transform, &mut Camera, &Projection), With<ViewCamera>>,
    windows: Query<&Window>,
    ready: Query<
        Entity,
        (
            With<PreviewRoot>,
            With<bevy::world_serialization::WorldInstance>,
        ),
    >,
) {
    if viewer.needs_frame && !viewer.render_loading && !ready.is_empty() && !meshes.is_empty() {
        let (mut min, mut max) = (Vec3::MAX, Vec3::MIN);
        for (bounds, t) in &meshes {
            // Transform all corners so rotated placements contribute correct bounds.
            for x in [-1., 1.] {
                for y in [-1., 1.] {
                    for z in [-1., 1.] {
                        let p = t.transform_point(
                            Vec3::from(bounds.center)
                                + Vec3::from(bounds.half_extents) * Vec3::new(x, y, z),
                        );
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
            }
        }
        viewer.center = (min + max) * 0.5;
        let aspect = (viewer.viewport.width() / viewer.viewport.height().max(1.)).max(0.01);
        if let Ok((_, _, Projection::Perspective(projection))) = cameras.single() {
            let half_vertical = projection.fov * 0.5;
            let half_horizontal = (half_vertical.tan() * aspect).atan();
            viewer.distance =
                (max - min).length().max(1.) * 0.55 / half_vertical.min(half_horizontal).sin();
        }
        viewer.needs_frame = false;
    }
    if let Ok((mut transform, mut camera, _)) = cameras.single_mut() {
        camera.is_active = viewer.data_mode == DataMode::Content
            && viewer
                .preview
                .as_ref()
                .is_some_and(|p| matches!(p.content, Content::Model { .. }));
        let rotation = viewer.rotation();
        transform.translation = viewer.center + rotation * Vec3::new(0., 0., viewer.distance);
        transform.look_at(viewer.center, Vec3::Y);
        if !viewer
            .preview
            .as_ref()
            .is_some_and(|p| matches!(p.content, Content::Model { .. }))
        {
            camera.viewport = None;
        } else if let Ok(window) = windows.single() {
            let rect = viewer.viewport;
            let scale = viewer.pixels_per_point;
            let size = window.physical_size();
            let origin = UVec2::new(
                (rect.min.x.max(0.) * scale) as u32,
                (rect.min.y.max(0.) * scale) as u32,
            )
            .min(size.saturating_sub(UVec2::ONE));
            let extent = UVec2::new(
                (rect.width().max(1.) * scale) as u32,
                (rect.height().max(1.) * scale) as u32,
            )
            .min(size - origin)
            .max(UVec2::ONE);
            camera.viewport = Some(Viewport {
                physical_position: origin,
                physical_size: extent,
                ..default()
            });
        }
    }
}

pub fn movie(
    mut viewer: ResMut<Viewer>,
    blobs: Res<Assets<Blob>>,
    assets: Res<AssetServer>,
    time: Res<Time>,
    mut sinks: Query<&mut AudioSink, With<PreviewAudio>>,
) {
    if let Some(handle) = viewer.movie_blob.as_ref() {
        if let Some(bytes) = blobs.get(handle) {
            match cyrus_assets::media::Movie::open(&bytes.0) {
                Ok(movie) => viewer.movie = Some(movie),
                Err(error) => viewer.error = Some(format!("Movie decode: {error:#}")),
            }
            viewer.movie_blob = None;
        } else if let Some(bevy::asset::LoadState::Failed(e)) = assets.get_load_state(handle.id()) {
            viewer.error = Some(format!("Movie load: {e}"));
            viewer.movie_blob = None;
        }
    }
    let duration = viewer.preview.as_ref().and_then(|p| match &p.content {
        Content::Audio { seconds, .. } | Content::Movie { seconds, .. } => Some(*seconds),
        _ => None,
    });
    let Some(duration) = duration else {
        return;
    };
    let has_audio = viewer.preview.as_ref().is_some_and(|p| match &p.content {
        Content::Audio { .. } => true,
        Content::Movie { wav, .. } => wav.is_some(),
        _ => false,
    });
    if viewer.seek_dirty {
        let mut seek_pending = has_audio && sinks.is_empty();
        for sink in &sinks {
            if sink.empty() {
                viewer.audio_dirty = true;
                seek_pending = true;
                continue;
            }
            if let Err(e) =
                sink.try_seek(std::time::Duration::from_secs_f64(viewer.position.max(0.)))
            {
                viewer.error = Some(format!("Seek: {e}"));
            }
        }
        let target = viewer.position;
        if let Some(movie) = &mut viewer.movie
            && target < movie.frame as f64 * movie.seconds_per_frame
            && let Err(e) = movie.seek(0)
        {
            viewer.error = Some(format!("Movie seek: {e}"));
        }
        viewer.seek_dirty = seek_pending;
    } else if viewer.playing && !viewer.decoding {
        if let Ok(sink) = sinks.single() {
            if sink.empty() {
                if viewer.movie.is_some() {
                    viewer.position += time.delta_secs_f64();
                } else {
                    viewer.position = duration;
                }
            } else {
                viewer.position = sink.position().as_secs_f64();
            }
        } else if !has_audio {
            viewer.position += time.delta_secs_f64();
        }
    }
    if viewer.position >= duration {
        viewer.position = if viewer.looping { 0. } else { duration };
        viewer.playing = viewer.looping;
        viewer.seek_dirty = viewer.looping;
        if viewer.looping {
            viewer.audio_dirty = true;
        }
    }
    let position = viewer.position;
    let mut waiting = false;
    if let Some(movie) = &mut viewer.movie {
        let target = ((position / movie.seconds_per_frame) as u32).min(movie.frames - 1);
        let mut error = None;
        for _ in 0..8 {
            if movie.frame >= target {
                break;
            }
            if let Err(e) = movie.advance() {
                error = Some(format!("Movie frame: {e}"));
                break;
            }
        }
        waiting = movie.frame < target;
        if let Some(error) = error {
            viewer.error = Some(error);
            viewer.playing = false;
        }
    }
    viewer.decoding = waiting;
    if viewer.movie_blob.is_some() || waiting {
        viewer.render_loading = true;
        viewer.load_fraction = None;
        viewer.load_status = if waiting {
            "Decoding movie frames…"
        } else {
            "Loading movie…"
        }
        .into();
    }
    for mut sink in &mut sinks {
        sink.set_volume(Volume::Linear(viewer.volume));
        if viewer.playing && !waiting {
            sink.play();
        } else {
            sink.pause();
        }
    }
}

pub fn texture(
    ctx: &egui::Context,
    viewer: &mut Viewer,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
) -> egui::TextureId {
    let image =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &pixels);
    if let Some(texture) = viewer.texture.as_mut() {
        texture.set(image, egui::TextureOptions::NEAREST);
        texture.id()
    } else {
        let texture = ctx.load_texture("asset-preview", image, egui::TextureOptions::NEAREST);
        let id = texture.id();
        viewer.texture = Some(texture);
        id
    }
}

#[derive(Component)]
pub struct Edges;
#[derive(Component)]
pub struct EdgesPrepared;
#[derive(Component)]
pub struct SurfaceStyle {
    original: StandardMaterial,
    mode: Shading,
}
fn shaded_material(original: &StandardMaterial, mode: Shading) -> StandardMaterial {
    let mut material = original.clone();
    match mode {
        Shading::Textured => {}
        Shading::Unlit => material.unlit = true,
        Shading::Solid | Shading::Wireframe => {
            material.unlit = mode == Shading::Wireframe;
            material.base_color_texture = None;
            material.normal_map_texture = None;
            material.metallic_roughness_texture = None;
            material.emissive_texture = None;
            material.emissive = LinearRgba::BLACK;
            material.base_color = if mode == Shading::Wireframe {
                Color::NONE
            } else {
                Color::srgb(0.65, 0.7, 0.75)
            };
            material.alpha_mode = AlphaMode::Mask(0.5);
        }
    }
    material
}
type UnpreparedMesh = (Without<Edges>, Without<EdgesPrepared>);
type SurfaceMaterials<'a> = (
    Entity,
    &'a mut MeshMaterial3d<StandardMaterial>,
    Option<&'a SurfaceStyle>,
);

/// Line geometry also works on WebGL, which has no polygon wireframe mode.
pub fn edges(
    mut commands: Commands,
    viewer: Res<Viewer>,
    sources: Query<(Entity, &Mesh3d), UnpreparedMesh>,
    mut visibility: Query<&mut Visibility, With<Edges>>,
    mut surfaces: Query<SurfaceMaterials<'_>, Without<Edges>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, mut handle, style) in &mut surfaces {
        if style.is_some_and(|style| style.mode == viewer.shading) {
            continue;
        }
        let original = if let Some(style) = style {
            style.original.clone()
        } else if let Some(material) = materials.get(&handle.0) {
            material.clone()
        } else {
            continue;
        };
        let material = shaded_material(&original, viewer.shading);
        if style.is_some() {
            if let Some(mut displayed) = materials.get_mut(&handle.0) {
                *displayed = material;
            }
        } else {
            handle.0 = materials.add(material);
        }
        commands.entity(entity).insert(SurfaceStyle {
            original,
            mode: viewer.shading,
        });
    }
    let show_edges = viewer.edges || viewer.shading == Shading::Wireframe;
    for mut visibility in &mut visibility {
        *visibility = if show_edges {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if !show_edges {
        return;
    }
    for (entity, handle) in &sources {
        let Some(mesh) = meshes.get(handle) else {
            continue;
        };
        if mesh.primitive_topology() != bevy::mesh::PrimitiveTopology::TriangleList {
            continue;
        }
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let indices: Vec<usize> = mesh
            .indices()
            .map_or_else(|| (0..positions.len()).collect(), |i| i.iter().collect());
        let mut lines = Vec::with_capacity(indices.len() * 2);
        for triangle in indices.as_chunks::<3>().0 {
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                if let (Some(a), Some(b)) = (positions.get(triangle[a]), positions.get(triangle[b]))
                {
                    lines.extend([*a, *b]);
                }
            }
        }
        let lines = Mesh::new(
            bevy::mesh::PrimitiveTopology::LineList,
            bevy::asset::RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, lines);
        let handle = meshes.add(lines);
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.9, 0.7),
            unlit: true,
            ..default()
        });
        commands.entity(entity).insert(EdgesPrepared).with_child((
            Edges,
            Mesh3d(handle),
            MeshMaterial3d(material),
        ));
    }
}

/// Inspection timing is user-controlled and does not reproduce game script timing.
pub fn animate(mut viewer: ResMut<Viewer>, time: Res<Time>) {
    let count = match viewer.preview.as_ref().map(|p| &p.content) {
        Some(Content::Images { frames }) => frames.len(),
        Some(Content::Model { frames, .. }) => frames.len(),
        _ => return,
    };
    if !viewer.playing || count < 2 {
        return;
    }
    viewer.frame_elapsed += time.delta_secs_f64() * 1000.;
    let steps = (viewer.frame_elapsed / viewer.frame_ms) as usize;
    if steps == 0 {
        return;
    }
    viewer.frame_elapsed %= viewer.frame_ms;
    let next = viewer.frame.saturating_add(steps);
    viewer.frame = if viewer.looping {
        next % count
    } else {
        next.min(count - 1)
    };
    if !viewer.looping && next >= count - 1 {
        viewer.playing = false;
    }
    viewer.position = viewer.frame as f64 * viewer.frame_ms / 1000.;
    viewer.scene_dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edges_use_valid_lines_and_shading_restores_original_material() {
        let mut app = App::new();
        app.world_mut()
            .register_required_components::<Mesh3d, Visibility>();
        app.init_resource::<Viewer>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, edges);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(
            Mesh::new(
                bevy::mesh::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::default(),
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            ),
        );
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::srgb(0.2, 0.4, 0.6),
                ..default()
            });
        let surface = app
            .world_mut()
            .spawn((Mesh3d(mesh), MeshMaterial3d(material)))
            .id();
        app.world_mut().resource_mut::<Viewer>().shading = Shading::Wireframe;
        app.update();
        let world = app.world_mut();
        let (line, line_material, visibility) = world.query_filtered::<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &Visibility), With<Edges>>().single(world).unwrap();
        assert_eq!(*visibility, Visibility::Inherited);
        assert_eq!(
            world
                .resource::<Assets<Mesh>>()
                .get(line)
                .unwrap()
                .primitive_topology(),
            bevy::mesh::PrimitiveTopology::LineList
        );
        assert_eq!(
            world
                .resource::<Assets<Mesh>>()
                .get(line)
                .unwrap()
                .count_vertices(),
            6
        );
        assert_eq!(
            world
                .resource::<Assets<StandardMaterial>>()
                .get(&line_material.0)
                .unwrap()
                .depth_bias,
            0.
        );
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(surface)
            .unwrap();
        assert_eq!(
            world
                .resource::<Assets<StandardMaterial>>()
                .get(&handle.0)
                .unwrap()
                .base_color,
            Color::NONE
        );
        world.resource_mut::<Viewer>().shading = Shading::Textured;
        app.update();
        let world = app.world_mut();
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(surface)
            .unwrap();
        assert_eq!(
            world
                .resource::<Assets<StandardMaterial>>()
                .get(&handle.0)
                .unwrap()
                .base_color,
            Color::srgb(0.2, 0.4, 0.6)
        );
        let visibility = world
            .query_filtered::<&Visibility, With<Edges>>()
            .single(world)
            .unwrap();
        assert_eq!(*visibility, Visibility::Hidden);
    }

    #[test]
    fn flight_is_frame_rate_independent_and_shift_boosts_speed() {
        let mut once = Viewer::default();
        once.yaw = 0.;
        once.pitch = 0.;
        once.distance = 100.;
        let mut twice = Viewer::default();
        twice.yaw = 0.;
        twice.pitch = 0.;
        twice.distance = 100.;
        once.fly(Vec3::NEG_Z, 1., false);
        twice.fly(Vec3::NEG_Z, 0.5, false);
        twice.fly(Vec3::NEG_Z, 0.5, false);
        assert_eq!(once.center, Vec3::new(0., 0., -200.));
        assert_eq!(once.center, twice.center);
        twice.center = Vec3::ZERO;
        twice.fly(Vec3::NEG_Z, 1., true);
        assert_eq!(twice.center, Vec3::new(0., 0., -800.));
    }

    #[test]
    fn frame_playback_advances_stops_and_loops() {
        let mut app = App::new();
        let mut viewer = Viewer::default();
        viewer.set_preview(cyrus_assets::Preview {
            content: Content::Model {
                frames: vec!["a.glb".into(), "b.glb".into(), "c.glb".into()],
                palettes: vec![],
                data: None,
            },
            details: vec![],
            frame_interval_ms: None,
            metadata: None,
            related: vec![],
        });
        viewer.playing = true;
        viewer.frame_ms = 100.;
        app.insert_resource(viewer)
            .init_resource::<Time>()
            .add_systems(Update, animate);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(150));
        app.update();
        assert_eq!(app.world().resource::<Viewer>().frame, 1);
        app.update();
        assert_eq!(app.world().resource::<Viewer>().frame, 2);
        assert!(!app.world().resource::<Viewer>().playing);
        {
            let mut viewer = app.world_mut().resource_mut::<Viewer>();
            viewer.playing = true;
            viewer.looping = true;
            viewer.frame_elapsed = 0.;
        }
        app.update();
        assert_eq!(app.world().resource::<Viewer>().frame, 0);
        assert!(app.world().resource::<Viewer>().playing);
    }
}
