//! The book menu as a real 3D scene: one mesh per model material group under a perspective camera.
//! Placements and camera follow preservation's engine/menu.md. The camera renders at the window's
//! resolution and extends the view around the authored 4:3 composition without letterboxing.
//!
//! Original engine coordinates have X right, Y down and Z forward. Bevy's have Y up and Z toward the
//! viewer, so every point and the camera position map `(x, y, z)` to `(x, -y, -z)`, a half turn
//! about X that keeps handedness and winding.
use super::StartupContent;
use super::book_motion::BookPose;
use super::menu::MenuState;
use super::menu_raster::Painted;
use super::view3d::{HEIGHT, LitMaterial, WIDTH};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    ecs::system::SystemParam,
    mesh::PrimitiveTopology,
    prelude::*,
};
use cyrus_assets::menu::{MenuAsset, MenuModel, PageTexture};
use cyrus_game::BookCamera;
use cyrus_world::heading::wrap;
use std::collections::{BTreeMap, HashMap};

/// Render layer of the book scene, apart from the world and the 2D startup content.
const BOOK_LAYER: usize = 5;

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
    target: Handle<Image>,
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
    // Unity modulation keeps the book unlit while filtering encoded channels.
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_COLOR,
        vec![[255.0, 255.0, 255.0, 1.0]; count],
    )
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

/// What the book scene draws into: commands, the asset stores and the entities it updates. The
/// meshes and materials are absent in hosts without a renderer, which draw nothing.
#[derive(SystemParam)]
pub(super) struct BookDrawing<'w, 's> {
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    meshes: Option<ResMut<'w, Assets<Mesh>>>,
    materials: Option<ResMut<'w, Assets<LitMaterial>>>,
    sprite: Query<'w, 's, (Entity, &'static mut Sprite), BookSprite>,
    visibility: Query<'w, 's, &'static mut Visibility>,
    cameras: Query<'w, 's, &'static mut Transform, With<BookCamera>>,
    projections: Query<'w, 's, (&'static Camera, &'static mut Projection), With<BookCamera>>,
    windows: Query<'w, 's, &'static Window>,
}

/// One mesh per model and texture or solid colour, spawned in its first pose.
fn spawn_groups(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<LitMaterial>,
    book: &MenuAsset,
    textures: &[Handle<Image>],
    images: &mut Assets<Image>,
) -> Vec<Group> {
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
    let mut textured: HashMap<usize, Handle<LitMaterial>> = HashMap::new();
    let mut groups = Vec::new();
    let layer = RenderLayers::layer(BOOK_LAYER);
    for ((model, texture, colour), faces) in grouped {
        let handle = match texture {
            Some(texture) => textured
                .entry(texture)
                .or_insert_with(|| materials.add(LitMaterial::book(textures[texture].clone())))
                .clone(),
            None => {
                let texture = images.add(super::view3d::raw_texture_image(1, 1, colour.to_vec()));
                materials.add(LitMaterial::book(texture))
            }
        };
        let group = Group {
            model,
            faces,
            mesh: Handle::default(),
        };
        let first = Shown {
            frame: 0,
            depth: 0.0,
        };
        let mesh = meshes.add(group_mesh(book, &group, first));
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
    groups
}

/// Builds the scene from the first painted surfaces: the render target the sprite shows, the page
/// textures, the models, the dialog layer and the camera. Gives the surfaces back when the book's
/// sprite does not exist yet.
fn create(
    drawing: &mut BookDrawing,
    menu: &mut MenuState,
    book: &MenuAsset,
    surfaces: Vec<PageTexture>,
) -> Result<BookScene, Vec<PageTexture>> {
    let BookDrawing {
        commands,
        images,
        meshes: Some(meshes),
        materials: Some(materials),
        sprite,
        ..
    } = drawing
    else {
        return Err(surfaces);
    };
    let Ok((sprite_entity, mut sprite)) = sprite.single_mut() else {
        return Err(surfaces);
    };
    let target = images.add(super::view3d::target_image([0, 0, 0, 255]));
    menu.image = target.clone();
    sprite.image = target.clone();
    let textures: Vec<Handle<Image>> = surfaces
        .into_iter()
        .map(|s| {
            images.add(super::view3d::raw_texture_image(
                s.width.into(),
                s.height.into(),
                s.rgba,
            ))
        })
        .collect();
    let groups = spawn_groups(commands, meshes, materials, book, &textures, images);
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
        super::view3d::view_camera(target.clone(), BOOK_LAYER, Color::BLACK),
    ));
    Ok(BookScene {
        target,
        textures,
        overlay,
        overlay_entity,
        groups,
        shown: [None; 5],
        camera: None,
        was_flying: false,
    })
}

/// Matches the window's physical pixels while retaining the original coordinates for dialogs.
fn fit_view(drawing: &mut BookDrawing, scene: &BookScene) {
    let Ok(window) = drawing.windows.single() else {
        return;
    };
    let Ok((camera, mut projection)) = drawing.projections.single_mut() else {
        return;
    };
    if !camera.is_active {
        return;
    }
    let size = window.physical_size().max(UVec2::ONE);
    if drawing
        .images
        .get(&scene.target)
        .is_some_and(|image| image.size() != size)
        && let Some(mut image) = drawing.images.get_mut(&scene.target)
    {
        image.resize(bevy::render::render_resource::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
    if let (Projection::Perspective(current), Projection::Perspective(fitted)) =
        (&mut *projection, super::view3d::fitted_projection(size))
        && (current.fov != fitted.fov || current.aspect_ratio != fitted.aspect_ratio)
    {
        *current = fitted;
    }
    let logical_size = Vec2::new(window.width(), window.height());
    let scale = super::viewport_scale(logical_size);
    if scale > 0.0
        && let Ok((_, mut sprite)) = drawing.sprite.single_mut()
    {
        sprite.custom_size = Some(logical_size / scale);
    }
}

/// Uploads what the last redraw painted: the page surfaces, or a dialog over the scene.
fn upload(drawing: &mut BookDrawing, scene: &BookScene, painted: Option<Painted>) {
    let BookDrawing {
        images, visibility, ..
    } = drawing;
    match painted {
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
}

/// Puts the models in the book's frames and, unless the flycam owns it, the camera on its pose.
fn follow_pose(
    drawing: &mut BookDrawing,
    scene: &mut BookScene,
    book: &MenuAsset,
    pose: &BookPose,
    flying: bool,
) {
    if let Some(meshes) = drawing.meshes.as_mut() {
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
    }
    // While the debug flycam is on the book never moves the camera; when it is turned off the camera
    // jumps to the book's current pose.
    let wanted = (pose.camera, pose.pitch);
    if !flying && (scene.camera != Some(wanted) || scene.was_flying) {
        for mut transform in &mut drawing.cameras {
            *transform = camera_transform(pose.camera, pose.pitch);
        }
        scene.camera = Some(wanted);
    }
    scene.was_flying = flying;
}

/// Uploads what the last redraw painted and keeps the models and camera on the book's pose.
pub(super) fn present(
    mut drawing: BookDrawing,
    mut menu: ResMut<MenuState>,
    definitions: Res<Assets<MenuAsset>>,
    scene: Option<ResMut<BookScene>>,
    flycam: Option<Res<cyrus_game::FlycamEngaged>>,
) {
    let Some(book) = definitions.get(&menu.asset) else {
        return;
    };
    if drawing.meshes.is_none() || drawing.materials.is_none() {
        // Headless hosts draw nothing; the menu state still advances.
        menu.painted = None;
        return;
    }
    let Some(mut scene) = scene else {
        let Some(Painted::Surfaces(surfaces)) = menu.painted.take() else {
            return;
        };
        match create(&mut drawing, &mut menu, book, surfaces) {
            // The resource appears at the next command flush; the pose syncs from the next frame.
            Ok(created) => drawing.commands.insert_resource(created),
            Err(surfaces) => menu.painted = Some(Painted::Surfaces(surfaces)),
        }
        return;
    };
    fit_view(&mut drawing, &scene);
    upload(&mut drawing, &scene, menu.painted.take());
    let Some(motion) = menu.motion.as_ref() else {
        return;
    };
    let flying = flycam.is_some_and(|flycam| flycam.0);
    follow_pose(&mut drawing, &mut scene, book, &motion.pose, flying);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_fills_resized_windows_and_leaves_paused_backdrops_untouched() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>().add_systems(
            Update,
            (
                super::super::fit_image,
                |mut drawing: BookDrawing, scene: Res<BookScene>| fit_view(&mut drawing, &scene),
            )
                .chain(),
        );
        let target = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(super::super::view3d::target_image([0, 0, 0, 255]));
        app.insert_resource(BookScene {
            target: target.clone(),
            textures: Vec::new(),
            overlay: Handle::default(),
            overlay_entity: Entity::PLACEHOLDER,
            groups: Vec::new(),
            shown: [None; 5],
            camera: None,
            was_flying: false,
        });
        let window = app.world_mut().spawn(Window::default()).id();
        let sprite = app
            .world_mut()
            .spawn((Sprite::from_image(target.clone()), StartupContent))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                BookCamera,
                Camera::default(),
                super::super::view3d::projection(),
                Transform::default(),
            ))
            .id();
        for size in [UVec2::new(1280, 720), UVec2::new(480, 800)] {
            app.world_mut()
                .get_mut::<Window>(window)
                .unwrap()
                .resolution
                .set_physical_resolution(size.x, size.y);
            app.update();
            let image = app
                .world()
                .resource::<Assets<Image>>()
                .get(&target)
                .unwrap();
            assert_eq!(image.size(), size);
            let displayed = app
                .world()
                .get::<Sprite>(sprite)
                .unwrap()
                .custom_size
                .unwrap()
                * app
                    .world()
                    .get::<Transform>(sprite)
                    .unwrap()
                    .scale
                    .truncate();
            assert!((displayed - size.as_vec2()).length() < 0.001);
        }
        app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = false;
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(800, 600);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&target)
                .unwrap()
                .size(),
            UVec2::new(480, 800),
        );
    }

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
