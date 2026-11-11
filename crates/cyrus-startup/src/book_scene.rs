//! The book menu as a real 3D scene: one mesh per model material group under a perspective camera.
//! Placements, projection and camera follow preservation's engine/menu.md; the camera renders into
//! a 640 by 480 image that the book sprite shows, so the existing letterboxing applies unchanged.
//!
//! Original engine coordinates have X right, Y down and Z forward. Bevy's have Y up and Z toward the
//! viewer, so every point and the camera position map `(x, y, z)` to `(x, -y, -z)`, a half turn
//! about X that keeps handedness and winding.
use super::StartupContent;
use super::menu::MenuState;
use super::menu_raster::Painted;
use super::view3d::{HEIGHT, WIDTH};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    mesh::PrimitiveTopology,
    prelude::*,
};
use cyrus_assets::menu::{MenuAsset, MenuModel};
use cyrus_game::BookCamera;
use cyrus_world::heading::wrap;
use std::collections::{BTreeMap, HashMap};

/// Render layer of the book scene, apart from the world and the 2D startup content.
const BOOK_LAYER: usize = 5;
/// Pixels whose interpolated alpha rounds to zero are not drawn (alpha 0 is the colour key).
const ALPHA_CUTOFF: f32 = 0.5 / 255.0;

/// One mesh of one model: the faces that share a texture, or a solid colour.
struct Group {
    model: usize,
    faces: Vec<usize>,
    mesh: Handle<Mesh>,
}

/// The pose a model's meshes were last built for.
#[derive(Clone, Copy, PartialEq)]
struct Shown {
    frame: usize,
    depth: f32,
}

/// The live 3D scene of the open book.
#[derive(Resource)]
pub(super) struct BookScene {
    textures: Vec<Handle<Image>>,
    overlay: Handle<Image>,
    overlay_entity: Entity,
    groups: Vec<Group>,
    shown: [Option<Shown>; 5],
    /// The camera pose last taken from the book's motion.
    camera: Option<([f32; 3], i32)>,
    was_flying: bool,
}

/// A book point in Bevy's frame. A forward turn ends one past the last authored frame, which shows
/// the last pose.
fn point(model: &MenuModel, frame: usize, depth: f32, vertex: usize) -> [f32; 3] {
    let p = model.frames[frame.min(model.frames.len() - 1)][vertex];
    [
        model.position[0] - p[0],
        -(model.position[1] + p[1]),
        -(model.position[2] + depth - p[2]),
    ]
}

/// Fan triangles of the group's faces, as `(face, corner index)` triples in draw order.
fn triangles(model: &MenuModel, faces: &[usize]) -> impl Iterator<Item = [(usize, usize); 3]> {
    faces.iter().flat_map(|&face| {
        (1..model.faces[face].corners.len().saturating_sub(1))
            .map(move |k| [(face, 0), (face, k), (face, k + 1)])
    })
}

fn positions(model: &MenuModel, faces: &[usize], shown: Shown) -> Vec<[f32; 3]> {
    triangles(model, faces)
        .flatten()
        .map(|(face, corner)| {
            point(
                model,
                shown.frame,
                shown.depth,
                model.faces[face].corners[corner].vertex,
            )
        })
        .collect()
}

fn group_mesh(book: &MenuAsset, group: &Group, shown: Shown) -> Mesh {
    let model = &book.models[group.model];
    let texture = model.faces[group.faces[0]]
        .texture
        .map(|index| &book.textures[index]);
    let uvs: Vec<[f32; 2]> = triangles(model, &group.faces)
        .flatten()
        .map(|(face, corner)| {
            let uv = model.faces[face].corners[corner].uv;
            // Corner UVs are in texels; the page surfaces are never empty.
            texture.map_or([0.0, 0.0], |t| {
                [uv[0] / f32::from(t.width), uv[1] / f32::from(t.height)]
            })
        })
        .collect();
    let count = uvs.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        positions(model, &group.faces, shown),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
}

/// An unlit material: the book's faces carry no light.
fn material(texture: Option<Handle<Image>>, colour: [u8; 4]) -> StandardMaterial {
    StandardMaterial {
        base_color: if texture.is_some() {
            Color::WHITE
        } else {
            Color::srgba_u8(colour[0], colour[1], colour[2], colour[3])
        },
        base_color_texture: texture,
        unlit: true,
        fog_enabled: false,
        alpha_mode: AlphaMode::Mask(ALPHA_CUTOFF),
        ..default()
    }
}

/// The camera transform for an engine camera position and pitch (2048 units per revolution).
fn camera_transform(position: [f32; 3], pitch: i32) -> Transform {
    #[allow(clippy::approx_constant)] // Decimal period from the original rotation table.
    let angle = (f64::from(wrap(pitch)) * 6.2831852 / 2048.0) as f32;
    Transform {
        translation: Vec3::new(position[0], -position[1], -position[2]),
        rotation: Quat::from_rotation_x(-angle),
        ..default()
    }
}

/// The sprite that shows the book, not a captured gameplay backdrop.
type BookSprite = (
    With<StartupContent>,
    Without<super::last_game::CapturedBackground>,
);

/// Uploads what the last redraw painted and keeps the models and camera on the book's pose.
// A Bevy system: its parameters are independent resources and queries.
#[allow(clippy::too_many_arguments)]
pub(super) fn present(
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    definitions: Res<Assets<MenuAsset>>,
    scene: Option<ResMut<BookScene>>,
    mut images: ResMut<Assets<Image>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut sprite: Query<(Entity, &mut Sprite), BookSprite>,
    mut visibility: Query<&mut Visibility>,
    flycam: Option<Res<cyrus_game::FlycamEngaged>>,
    mut cameras: Query<&mut Transform, With<BookCamera>>,
) {
    let Some(book) = definitions.get(&menu.asset) else {
        return;
    };
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else {
        // Headless hosts draw nothing; the menu state still advances.
        menu.painted = None;
        return;
    };
    let mut scene = scene;
    if scene.is_none() {
        let Some(Painted::Surfaces(surfaces)) = menu.painted.take() else {
            return;
        };
        let Ok((sprite_entity, mut sprite)) = sprite.single_mut() else {
            menu.painted = Some(Painted::Surfaces(surfaces));
            return;
        };
        let target = images.add(super::view3d::target_image([0, 0, 0, 255]));
        menu.image = target.clone();
        sprite.image = target.clone();

        let textures: Vec<Handle<Image>> = surfaces
            .into_iter()
            .map(|s| images.add(super::view3d::texture_image(s.width, s.height, s.rgba)))
            .collect();

        let mut grouped: BTreeMap<(usize, Option<usize>, [u8; 4]), Vec<usize>> = BTreeMap::new();
        for (index, model) in book.models.iter().enumerate() {
            for (face_index, face) in model.faces.iter().enumerate() {
                let colour = if face.texture.is_some() {
                    [0; 4]
                } else {
                    face.color
                };
                grouped
                    .entry((index, face.texture, colour))
                    .or_default()
                    .push(face_index);
            }
        }
        let mut textured: HashMap<usize, Handle<StandardMaterial>> = HashMap::new();
        let mut groups = Vec::new();
        let layer = RenderLayers::layer(BOOK_LAYER);
        for ((model, texture, colour), faces) in grouped {
            let handle = match texture {
                Some(texture) => textured
                    .entry(texture)
                    .or_insert_with(|| {
                        materials.add(material(Some(textures[texture].clone()), [0; 4]))
                    })
                    .clone(),
                None => materials.add(material(None, colour)),
            };
            let group = Group {
                model,
                faces,
                mesh: Handle::default(),
            };
            let pose = Shown {
                frame: 0,
                depth: 0.0,
            };
            let mesh = meshes.add(group_mesh(book, &group, pose));
            commands.spawn((
                StartupContent,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(handle),
                Transform::IDENTITY,
                NoFrustumCulling,
                layer.clone(),
            ));
            groups.push(Group { mesh, ..group });
        }

        let overlay = images.add(super::view3d::texture_image(
            WIDTH as u16,
            HEIGHT as u16,
            vec![0; (WIDTH * HEIGHT * 4) as usize],
        ));
        let overlay_entity = commands
            .spawn((
                Sprite::from_image(overlay.clone()),
                Transform::from_xyz(0.0, 0.0, 1.0),
                Visibility::Hidden,
                ChildOf(sprite_entity),
            ))
            .id();
        commands.spawn((
            StartupContent,
            BookCamera,
            super::view3d::view_camera(target, BOOK_LAYER, Color::BLACK),
        ));
        commands.insert_resource(BookScene {
            textures,
            overlay,
            overlay_entity,
            groups,
            shown: [None; 5],
            camera: None,
            was_flying: false,
        });
        // The resource appears at the next command flush; the pose syncs from the next frame.
        return;
    }
    let scene = scene.as_deref_mut().unwrap();
    match menu.painted.take() {
        Some(Painted::Surfaces(surfaces)) => {
            for (handle, surface) in scene.textures.iter().zip(surfaces) {
                if let Some(mut image) = images.get_mut(handle) {
                    image.data = Some(surface.rgba);
                }
            }
            if let Ok(mut visible) = visibility.get_mut(scene.overlay_entity) {
                *visible = Visibility::Hidden;
            }
        }
        Some(Painted::Dialog(dialog)) => {
            if let Some(mut image) = images.get_mut(&scene.overlay) {
                image.data = Some(dialog.into_raw());
            }
            if let Ok(mut visible) = visibility.get_mut(scene.overlay_entity) {
                *visible = Visibility::Inherited;
            }
        }
        None => {}
    }
    let Some(motion) = menu.motion.as_ref() else {
        return;
    };
    let pose = &motion.pose;
    for model in 0..5 {
        let wanted = Shown {
            frame: pose.frames[model],
            depth: pose.depth[model],
        };
        if scene.shown[model] == Some(wanted) {
            continue;
        }
        scene.shown[model] = Some(wanted);
        for group in scene.groups.iter().filter(|group| group.model == model) {
            if let Some(mut mesh) = meshes.get_mut(&group.mesh) {
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_POSITION,
                    positions(&book.models[model], &group.faces, wanted),
                );
            }
        }
    }
    // While the debug flycam is on the book never moves the camera; when it is turned off the camera
    // jumps to the book's current pose.
    let flying = flycam.is_some_and(|flycam| flycam.0);
    let wanted = (pose.camera, pose.pitch);
    if !flying && (scene.camera != Some(wanted) || scene.was_flying) {
        for mut transform in &mut cameras {
            *transform = camera_transform(pose.camera, pose.pitch);
        }
        scene.camera = Some(wanted);
    }
    scene.was_flying = flying;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_points_map_to_bevy_with_y_and_z_flipped() {
        let model = MenuModel {
            position: [10.0, 20.0, 30.0],
            frames: vec![vec![[1.0, 2.0, 3.0]]],
            faces: Vec::new(),
        };
        assert_eq!(point(&model, 0, 4.0, 0), [9.0, -22.0, -31.0]);
        // A frame one past the last authored one shows the last.
        assert_eq!(point(&model, 9, 4.0, 0), [9.0, -22.0, -31.0]);
    }

    #[test]
    fn pitch_follows_the_original_rotation_direction() {
        // A positive pitch of a quarter revolution turns a point straight ahead (engine +Z) toward the
        // top of the screen, where engine Y points down: view_y = -z.
        let t = camera_transform([0.0, 0.0, 0.0], 512);
        let view = t.rotation.inverse() * Vec3::new(0.0, 0.0, -1.0);
        assert!((view - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5, "{view}");
        let camera = camera_transform([1.0, 2.0, 3.0], 0);
        assert_eq!(camera.translation, Vec3::new(1.0, -2.0, -3.0));
    }
}
