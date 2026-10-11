//! Selection-owned scene, camera and media playback.
use crate::{loading::Blob, ui::Viewer};
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
        Projection::Perspective(PerspectiveProjection {
            near: 0.01,
            far: 1_000_000.,
            ..default()
        }),
        Transform::from_xyz(0., 2., 6.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.)),
    ));
}
pub fn sync(
    mut commands: Commands,
    mut viewer: ResMut<Viewer>,
    assets: Res<AssetServer>,
    roots: Query<(Entity, &WorldAssetRoot), With<PreviewRoot>>,
    audio: Query<(Entity, &AudioPlayer<AudioSource>), With<PreviewAudio>>,
) {
    let selection_changed = viewer.scene_dirty;
    if viewer.scene_dirty {
        for (root, _) in &roots {
            commands.entity(root).despawn();
        }
        if let Some(Content::Model { frames, .. }) = viewer.preview.as_ref().map(|p| &p.content)
            && let Some(path) = frames.get(viewer.frame)
        {
            let handle =
                assets.load(GltfAssetLabel::Scene(0).from_asset(format!("cache://{path}")));
            commands.spawn((PreviewRoot, WorldAssetRoot(handle)));
            viewer.needs_frame = true;
        }
        viewer.scene_dirty = false;
    }
    if !selection_changed {
        for (_, root) in &roots {
            if let Some(bevy::asset::LoadState::Failed(error)) = assets.get_load_state(root.0.id())
            {
                viewer.error = Some(format!("Model load: {error}"));
                viewer.needs_frame = false;
            }
        }
    }
    if !viewer.audio_dirty {
        for (_, player) in &audio {
            if let Some(bevy::asset::LoadState::Failed(error)) =
                assets.get_load_state(player.0.id())
            {
                viewer.error = Some(format!("Audio load: {error}"));
                viewer.playing = false;
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
    if viewer.needs_frame && !ready.is_empty() && !meshes.is_empty() {
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
        let rotation = Quat::from_euler(EulerRot::YXZ, viewer.yaw, viewer.pitch, 0.);
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
    if let Some(texture) = viewer.textures.first_mut() {
        texture.set(image, egui::TextureOptions::NEAREST);
        texture.id()
    } else {
        let texture = ctx.load_texture("asset-preview", image, egui::TextureOptions::NEAREST);
        let id = texture.id();
        viewer.textures.push(texture);
        id
    }
}

#[derive(Component)]
pub struct Edges;
#[derive(Component)]
pub struct EdgesPrepared;
type UnpreparedMesh = (Without<Edges>, Without<EdgesPrepared>);

/// Line geometry also works on WebGL, which has no polygon wireframe mode.
pub fn edges(
    mut commands: Commands,
    viewer: Res<Viewer>,
    sources: Query<(Entity, &Mesh3d), UnpreparedMesh>,
    mut visibility: Query<&mut Visibility, With<Edges>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for mut visibility in &mut visibility {
        *visibility = if viewer.wireframe {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if !viewer.wireframe {
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
            depth_bias: -1.,
            ..default()
        });
        commands.entity(entity).insert(EdgesPrepared).with_child((
            Edges,
            Mesh3d(handle),
            MeshMaterial3d(material),
        ));
    }
}
