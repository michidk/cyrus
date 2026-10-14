//! Separate model draw geometry preserves authored pose/admission topology while clipping lit RGB.
//!
//! Model color setup: <https://github.com/michidk/redguard-preservation/commit/8e504c1>.
//! Model projection: <https://github.com/michidk/redguard-preservation/commit/c91863c>.
//! Model clipping: <https://github.com/michidk/redguard-preservation/commit/dd8c5f8>.
use super::{ModelExtension, SceneMaterial, texture::admission::Model};
use crate::{
    camera::ViewCamera,
    terrain::{ClipVertex, Preparation},
};
use anyhow::ensure;
use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers, VisibilitySystems},
    gltf::GltfExtras,
    mesh::{MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    render::render_resource::VertexFormat,
};
use std::collections::HashSet;

pub(super) const PROJECTED: MeshVertexAttribute =
    MeshVertexAttribute::new("Model_Projected", 83010, VertexFormat::Float32x4);

#[derive(Component)]
struct Source {
    draw: Entity,
    polygons: Model,
    previous: Option<(Mat4, Mat4, Vec2, AssetId<Mesh>, ModelExtension, Vec2)>,
}

#[derive(Component)]
struct Draw;

#[derive(Component)]
struct Inspected;

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, bind.after(super::apply_scene_fog))
        .add_systems(
            PostUpdate,
            prepare
                .after(VisibilitySystems::CheckVisibility)
                .after(crate::animation::apply_poses),
        );
}

// Binding joins source geometry, metadata and its existing material/layer ownership.
#[allow(clippy::type_complexity)]
fn bind(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Assets<SceneMaterial>>,
    sources: Query<
        (
            Entity,
            &Mesh3d,
            &MeshMaterial3d<SceneMaterial>,
            &GltfExtras,
            Option<&RenderLayers>,
        ),
        (
            Without<Inspected>,
            Without<Draw>,
            Without<crate::flat::Flat>,
        ),
    >,
) {
    for (entity, mesh, material, extras, layers) in &sources {
        let Some(material_asset) = materials.get(&material.0) else {
            continue;
        };
        if material_asset.extension.lighting.w < 0.0
            || material_asset.extension.lighting.w as u8 & 4 != 0
        {
            continue;
        }
        let Some(mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        let parsed = (|| -> anyhow::Result<Model> {
            let model = Model::parse(&extras.value)?
                .ok_or_else(|| anyhow::anyhow!("missing authored polygons"))?;
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                anyhow::bail!("missing model positions");
            };
            model.validate(positions)?;
            Ok(model)
        })();
        commands.entity(entity).insert(Inspected);
        let polygons = match parsed {
            Ok(model) => model,
            Err(error) => {
                error!("model {entity:?}: camera clipping unavailable: {error}");
                continue;
            }
        };
        let draw_mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, default()));
        let draw = commands
            .spawn((
                Draw,
                Mesh3d(draw_mesh),
                MeshMaterial3d(material.0.clone()),
                Transform::IDENTITY,
                Visibility::Inherited,
                NoFrustumCulling,
                layers.cloned().unwrap_or_default(),
                ChildOf(entity),
            ))
            .id();
        commands.entity(entity).insert((
            RenderLayers::none(),
            Source {
                draw,
                polygons,
                previous: None,
            },
        ));
    }
}

// Preparation consumes independent camera, mesh, material and image change streams.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn prepare(
    mut sources: Query<
        (
            &Mesh3d,
            &GlobalTransform,
            &MeshMaterial3d<SceneMaterial>,
            &mut Source,
            Option<Ref<crate::animation::BoundFrameMesh>>,
        ),
        Without<Draw>,
    >,
    mut draws: Query<(&Mesh3d, &mut MeshMaterial3d<SceneMaterial>), With<Draw>>,
    cameras: Query<(&GlobalTransform, &Projection, &Camera), With<ViewCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Assets<SceneMaterial>>,
    images: Res<Assets<Image>>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut image_events: MessageReader<AssetEvent<Image>>,
    mut clip: Local<Preparation>,
) {
    let changed_meshes: HashSet<_> = mesh_events
        .read()
        .filter_map(|event| match event {
            AssetEvent::Modified { id } | AssetEvent::Added { id } => Some(*id),
            _ => None,
        })
        .collect();
    let changed_images: HashSet<_> = image_events
        .read()
        .filter_map(|event| match event {
            AssetEvent::Modified { id } | AssetEvent::Added { id } => Some(*id),
            _ => None,
        })
        .collect();
    let Ok((camera, projection, viewport_camera)) = cameras.single() else {
        return;
    };
    let Some(viewport) = viewport_camera.physical_viewport_size() else {
        return;
    };
    if viewport.min_element() == 0 {
        return;
    }
    let viewport = viewport.as_vec2();
    let camera = camera.to_matrix();
    let projection = projection.get_clip_from_view();
    let scale = Vec2::new(projection.x_axis.x, projection.y_axis.y);
    for (mesh, transform, material, mut source, animated) in &mut sources {
        let Some(material_asset) = materials.get(&material.0) else {
            continue;
        };
        let extension = &material_asset.extension;
        let key = (
            camera,
            transform.to_matrix(),
            scale,
            mesh.0.id(),
            extension.clone(),
            viewport,
        );
        let Ok((draw, mut draw_material)) = draws.get_mut(source.draw) else {
            continue;
        };
        if draw_material.0 != material.0 {
            draw_material.0 = material.0.clone();
        }
        if source.previous.as_ref() == Some(&key)
            && !animated.as_ref().is_some_and(|pose| pose.is_changed())
            && !changed_meshes.contains(&mesh.0.id())
            && !changed_images.contains(&extension.lights.id())
        {
            continue;
        }
        let Some(original) = meshes.get(&mesh.0) else {
            continue;
        };
        let Some(lights) = images.get(&extension.lights) else {
            continue;
        };
        let result = draw_mesh(
            original,
            &source.polygons,
            key.1,
            camera,
            scale,
            viewport.to_array(),
            extension,
            lights,
            animated.is_some(),
            &mut clip,
        );
        match result {
            Ok(prepared) => {
                if let Some(mut target) = meshes.get_mut(&draw.0) {
                    *target = prepared;
                }
                source.previous = Some(key);
            }
            Err(error) => {
                error!("model {:?}: camera clipping failed: {error}", mesh.0.id());
                source.previous = Some(key);
            }
        }
    }
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    (a.x * b.x + a.y * b.y) + a.z * b.z
}

/// Runtime light records use the same ordered list and quantized distance as the shader.
fn light_colors(
    mesh: &Mesh,
    transform: Mat4,
    material: &ModelExtension,
    lights: &Image,
) -> anyhow::Result<Vec<[f32; 4]>> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        anyhow::bail!("missing positions")
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        anyhow::bail!("missing normals")
    };
    let Some(VertexAttributeValues::Float32x2(inputs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        anyhow::bail!("missing lighting inputs")
    };
    ensure!(
        normals.len() == positions.len() && inputs.len() == positions.len(),
        "lighting input count mismatch"
    );
    let retained = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(colors)) => {
            ensure!(
                colors.len() == positions.len(),
                "retained color count mismatch"
            );
            Some(colors)
        }
        _ => None,
    };
    let axes = Vec3::new(-1.0, -1.0, 1.0);
    let (origin, basis) = if material.lighting.y != 0.0 {
        (
            material.placement.w_axis.truncate(),
            Mat3::from_mat4(material.placement),
        )
    } else {
        (
            transform.w_axis.truncate() * axes * 20.0 + Vec3::new(0.0, 0.0, 65_535.996),
            Mat3::from_cols(
                transform.x_axis.truncate() * axes * axes.x,
                transform.y_axis.truncate() * axes * axes.y,
                transform.z_axis.truncate() * axes * axes.z,
            ),
        )
    };
    let mut selected = Vec::new();
    if let Some(data) = &lights.data {
        for record in data.as_chunks::<32>().0.iter().skip(256) {
            let values: [f32; 8] = std::array::from_fn(|i| {
                f32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap())
            });
            let relative = Vec3::new(values[0], values[1], values[2]) - origin;
            if dot(relative, relative) - material.lighting.x * material.lighting.x
                <= values[3] * values[3]
            {
                selected.push((
                    basis.transpose() * relative,
                    Vec3::new(values[4], values[5], values[6]),
                    values[7],
                ));
            }
        }
    }
    Ok(positions
        .iter()
        .zip(normals)
        .zip(inputs)
        .enumerate()
        .map(|(i, ((position, normal), input))| {
            let position = Vec3::from(*position);
            let normal = Vec3::from(*normal);
            let mut color = retained.map_or_else(
                || {
                    let weight = (-dot(
                        material.fog.direction.truncate(),
                        transform.transform_vector3(normal),
                    ))
                    .max(0.0);
                    material.fog.ambient.truncate()
                        + Vec3::splat(input[0])
                        + material.fog.directional.truncate() * weight
                },
                |colors| Vec3::from_slice(&colors[i]) * 255.0,
            );
            for &(light, strength, numerator) in &selected {
                let delta = position * axes * 20.0 - light;
                let squared = dot(delta, delta);
                if squared == 0.0 {
                    continue;
                }
                let (distance, inverse) = crate::lighting::quantized_distance(squared);
                let normal_dot = dot(delta * inverse, normal * axes);
                if normal_dot >= 0.0 {
                    continue;
                }
                let attenuation = numerator / ((2.0 * distance + squared) + distance);
                if attenuation >= 0.0 {
                    color += strength * (-normal_dot * attenuation);
                }
            }
            let tint = if retained.is_some() { 1.0 } else { input[1] };
            let rgb = color.clamp(Vec3::ZERO, Vec3::splat(255.0)) * tint;
            [
                rgb.x,
                rgb.y,
                rgb.z,
                retained.map_or(0.0, |colors| colors[i][3]),
            ]
        })
        .collect())
}

// Each input is a separate documented stage: topology, pose, view, lighting and clipping.
#[allow(clippy::too_many_arguments)]
fn draw_mesh(
    original: &Mesh,
    model: &Model,
    transform: Mat4,
    camera: Mat4,
    scale: Vec2,
    viewport: [f32; 2],
    material: &ModelExtension,
    lights: &Image,
    animated: bool,
    clip: &mut Preparation,
) -> anyhow::Result<Mesh> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        original.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        anyhow::bail!("missing positions")
    };
    let Some(VertexAttributeValues::Float32x3(normals)) =
        original.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        anyhow::bail!("missing normals")
    };
    let Some(VertexAttributeValues::Float32x2(uv)) = original.attribute(Mesh::ATTRIBUTE_UV_0)
    else {
        anyhow::bail!("missing texture coordinates")
    };
    model.validate(positions)?;
    ensure!(
        normals.len() == positions.len() && uv.len() == positions.len(),
        "vertex attribute count mismatch"
    );
    let camera_from_local = camera.inverse() * transform;
    let center = camera_from_local.transform_point3(Vec3::ZERO) * 20.0;
    if super::texture::admission::classify(
        [center.x * scale.x, -center.y * scale.y, -center.z],
        model.radius_fixed as f32 / 256.0,
        scale,
        [material.fog.clipping.x, material.fog.clipping.y],
    )
    .is_none()
    {
        return Ok(prepared_mesh(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Default::default(),
        ));
    }
    let colors = light_colors(original, transform, material, lights)?;
    let local_camera = transform
        .inverse()
        .transform_point3(camera.w_axis.truncate());
    let mut output_positions = Vec::new();
    let mut output_normals = Vec::new();
    let mut output_uv = Vec::new();
    let mut output_colors = Vec::new();
    let mut projected = Vec::new();
    let mut color_planes: [Vec<[i32; 4]>; 3] = Default::default();
    for polygon in &model.polygons {
        let indices = polygon
            .vertices
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing polygon corners"))?;
        let first = indices[0];
        let normal = if animated {
            normals[first]
        } else {
            polygon
                .normal
                .ok_or_else(|| anyhow::anyhow!("missing polygon normal"))?
        };
        if dot(
            Vec3::from(positions[first]) - local_camera,
            Vec3::from(normal),
        ) >= 0.0
        {
            continue;
        }
        let admitted = clip.admits(
            indices.iter().map(|&i| {
                let point = camera_from_local.transform_point3(Vec3::from(positions[i])) * 20.0;
                ClipVertex {
                    camera: [point.x * scale.x, -point.y * scale.y, -point.z],
                    local: positions[i],
                    normal: normals[i],
                    uv: uv[i],
                    rgb: [colors[i][0], colors[i][1], colors[i][2]],
                    ..default()
                }
            }),
            [material.fog.clipping.x, material.fog.clipping.y],
        );
        if !admitted {
            continue;
        }
        let corners = clip.polygon();
        for i in 1..corners.len() - 1 {
            let vertices = [corners[0], corners[i], corners[i + 1]]
                .map(|v| crate::terrain::project(v, viewport));
            let Some(triangle) = crate::terrain::prepare_triangle(vertices) else {
                continue;
            };
            for vertex in triangle.vertices {
                let v = vertex.vertex;
                let screen = vertex.screen;
                for (channel, values) in color_planes.iter_mut().enumerate() {
                    let [start, dx, dy] = triangle.planes[channel];
                    let anchor = triangle.anchor.get(channel).copied().unwrap_or(0);
                    values.push([start, dx, dy, anchor]);
                }
                projected.push([screen[0], screen[1], v.camera[2] / 20.0, 0.0]);
                output_positions.push(v.local);
                output_normals.push(v.normal);
                output_uv.push(v.uv);
                output_colors.push([
                    v.rgb[0] / 255.0,
                    v.rgb[1] / 255.0,
                    v.rgb[2] / 255.0,
                    colors[first][3],
                ]);
            }
        }
    }
    Ok(prepared_mesh(
        output_positions,
        output_normals,
        output_uv,
        output_colors,
        projected,
        color_planes,
    ))
}

fn prepared_mesh(
    output_positions: Vec<[f32; 3]>,
    output_normals: Vec<[f32; 3]>,
    output_uv: Vec<[f32; 2]>,
    output_colors: Vec<[f32; 4]>,
    projected: Vec<[f32; 4]>,
    color_planes: [Vec<[i32; 4]>; 3],
) -> Mesh {
    let count = output_positions.len();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, output_positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, output_normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, output_uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[-1.0, 1.0]; count]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, output_colors);
    mesh.insert_attribute(PROJECTED, projected);
    for (attribute, values) in [
        crate::terrain::RED,
        crate::terrain::GREEN,
        crate::terrain::BLUE,
    ]
    .into_iter()
    .zip(color_planes)
    {
        mesh.insert_attribute(attribute, values);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material() -> ModelExtension {
        ModelExtension {
            chroma_key: Vec4::ZERO,
            fog: super::super::FogExtension {
                ambient: Vec4::ZERO,
                directional: Vec4::ZERO,
                direction: Vec4::ZERO,
                color: Vec4::ZERO,
                clipping: Vec4::new(1.0, 100.0, 0.0, 0.0),
                water: Vec4::ZERO,
                table: [Vec4::ZERO; 16],
            },
            lights: Handle::default(),
            placement: Mat4::IDENTITY,
            lighting: Vec4::ZERO,
            compositing: Vec4::splat(-1.0),
        }
    }

    fn model() -> Model {
        Model::parse(r#"{"rgpre_admission":{"version":1,"radius_fixed":768,"polygons":[{"vertices":[0,1,2],"normal":[0,0,1]}]}}"#).unwrap().unwrap()
    }

    fn mesh() -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [-0.25 / 20., 0.25 / 20., -0.5 / 20.],
                [0.25 / 20., 0.25 / 20., -2. / 20.],
                [0., -0.25 / 20., -2. / 20.],
            ],
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 3]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.], [1., 0.], [0., 1.]]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0., 1.]; 3]);
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_COLOR,
            vec![
                [20. / 255., 40. / 255., 60. / 255., 0.],
                [100. / 255., 120. / 255., 140. / 255., 0.],
                [180. / 255., 200. / 255., 220. / 255., 0.],
            ],
        );
        mesh
    }

    #[test]
    fn near_crossing_clips_source_rgb_and_keeps_pose_topology() {
        let source = mesh();
        let mut clip = Preparation::default();
        let draw = draw_mesh(
            &source,
            &model(),
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            Vec2::ONE,
            [640.0, 480.0],
            &material(),
            &Image::default(),
            false,
            &mut clip,
        )
        .unwrap();
        assert_eq!(source.count_vertices(), 3);
        assert_eq!(draw.count_vertices(), 6);
        let Some(VertexAttributeValues::Float32x4(colors)) = draw.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("missing clipped colors")
        };
        // Original-instruction near-edge RGB, before screen interpolation.
        for (actual, expected) in colors[0][..3]
            .iter()
            .zip([73.333336, 93.333336, 113.333336])
        {
            assert!((actual * 255.0 - expected).abs() < 0.00002);
        }
        assert!((colors[0][0] * 255.0 - 126.66667).abs() > 50.0);
        assert_eq!(colors[0], colors[3]);
        // A subsequent pose can replace the original three positions without clipped indices.
        let mut moved = source.clone();
        moved.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [-0.25 / 20., 0.25 / 20., -2. / 20.],
                [0.25 / 20., 0.25 / 20., -2. / 20.],
                [0., -0.25 / 20., -2. / 20.],
            ],
        );
        let draw = draw_mesh(
            &moved,
            &model(),
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            Vec2::ONE,
            [640.0, 480.0],
            &material(),
            &Image::default(),
            true,
            &mut clip,
        )
        .unwrap();
        assert_eq!(draw.count_vertices(), 3);
        assert_eq!(moved.count_vertices(), 3);
    }
    #[test]
    fn prepared_model_projects_on_the_original_sixteenth_pixel_grid() {
        let mut source = mesh();
        source.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [-0.12345 / 20.0, 0.23456 / 20.0, -2.0 / 20.0],
                [0.25 / 20.0, 0.25 / 20.0, -2.0 / 20.0],
                [0.0, -0.25 / 20.0, -2.0 / 20.0],
            ],
        );
        for (viewport, expected) in [
            ([640.0, 480.0], [300.25, 211.875, 0.1, 0.0]),
            ([1280.0, 960.0], [600.5, 423.6875, 0.1, 0.0]),
        ] {
            let draw = draw_mesh(
                &source,
                &model(),
                Mat4::IDENTITY,
                Mat4::IDENTITY,
                Vec2::ONE,
                viewport,
                &material(),
                &Image::default(),
                false,
                &mut Preparation::default(),
            )
            .unwrap();
            let Some(VertexAttributeValues::Float32x4(projected)) = draw.attribute(PROJECTED)
            else {
                panic!("missing model projection");
            };
            assert_eq!(projected[0], expected);
            assert_eq!(
                draw.attribute(Mesh::ATTRIBUTE_POSITION),
                source.attribute(Mesh::ATTRIBUTE_POSITION)
            );
        }
    }

    #[test]
    fn model_color_planes_use_the_original_top_edge_anchor() {
        let mut source = mesh();
        source.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [-8.0 / 20.0, 8.0 / 20.0, -8.0 / 20.0],
                [0.0, 8.0 / 20.0, -8.0 / 20.0],
                [-8.0 / 20.0, 0.0, -8.0 / 20.0],
            ],
        );
        let draw = draw_mesh(
            &source,
            &model(),
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            Vec2::ONE,
            [8.0, 8.0],
            &material(),
            &Image::default(),
            false,
            &mut Preparation::default(),
        )
        .unwrap();
        for (attribute, expected) in [
            (crate::terrain::RED, [532_480, 81_920, 163_840, 4]),
            (crate::terrain::GREEN, [614_400, 81_920, 163_840, 0]),
            (crate::terrain::BLUE, [696_320, 81_920, 163_840, 0]),
        ] {
            assert_eq!(
                draw.attribute(attribute),
                Some(&VertexAttributeValues::Sint32x4(vec![expected; 3]))
            );
        }
    }

    #[test]
    fn source_and_draw_keep_distinct_meshes_and_share_material_changes() {
        use bevy::asset::AssetPlugin;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<SceneMaterial>();
        register(&mut app);
        let source_mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh());
        let light = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let mut extension = material();
        extension.lights = light;
        let material = app
            .world_mut()
            .resource_mut::<Assets<SceneMaterial>>()
            .add(SceneMaterial {
                base: StandardMaterial::default(),
                extension,
            });
        app.world_mut().spawn((
            ViewCamera,
            Camera {
                viewport: Some(bevy::camera::Viewport {
                    physical_size: UVec2::new(640, 480),
                    ..default()
                }),
                ..default()
            },
            Transform::IDENTITY,
            GlobalTransform::IDENTITY,
            Projection::Perspective(PerspectiveProjection::default()),
        ));
        let source = app.world_mut().spawn((
            Mesh3d(source_mesh.clone()), MeshMaterial3d(material.clone()),
            Transform::IDENTITY, Visibility::Inherited,
            GltfExtras { value: r#"{"rgpre_admission":{"version":1,"radius_fixed":768,"polygons":[{"vertices":[0,1,2],"normal":[0,0,1]}]}}"#.into() },
        )).id();
        app.update();
        let draw = app.world().get::<Source>(source).unwrap().draw;
        let draw_mesh = app.world().get::<Mesh3d>(draw).unwrap().0.clone();
        assert_ne!(source_mesh, draw_mesh);
        assert_eq!(app.world().get::<Mesh3d>(source).unwrap().0, source_mesh);
        assert_eq!(
            *app.world().get::<RenderLayers>(source).unwrap(),
            RenderLayers::none()
        );
        assert_eq!(
            *app.world().get::<RenderLayers>(draw).unwrap(),
            RenderLayers::default()
        );
        assert_eq!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&draw_mesh)
                .unwrap()
                .count_vertices(),
            6
        );
        let projected_before = app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&draw_mesh)
            .unwrap()
            .attribute(PROJECTED)
            .unwrap()
            .clone();
        {
            let world = app.world_mut();
            let mut query = world.query_filtered::<&mut Camera, With<ViewCamera>>();
            query
                .single_mut(world)
                .unwrap()
                .viewport
                .as_mut()
                .unwrap()
                .physical_size = UVec2::new(1280, 960);
        }
        app.update();
        assert_ne!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&draw_mesh)
                .unwrap()
                .attribute(PROJECTED)
                .unwrap(),
            &projected_before
        );
        let replacement = app
            .world()
            .resource::<Assets<SceneMaterial>>()
            .get(&material)
            .unwrap()
            .clone();
        let replacement = app
            .world_mut()
            .resource_mut::<Assets<SceneMaterial>>()
            .add(replacement);
        app.world_mut()
            .entity_mut(source)
            .insert(MeshMaterial3d(replacement.clone()));
        app.update();
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<SceneMaterial>>(draw)
                .unwrap()
                .0,
            replacement
        );
        app.world_mut().entity_mut(source).despawn();
        assert!(app.world().get_entity(draw).is_err());
    }
}
