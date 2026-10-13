//! Private terrain meshes, shading normals and wave playback from preservation.

use crate::environment::EnvironmentRequest;
use bevy::camera::primitives::MeshAabb;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use cyrus_assets::environment::Environment;
use cyrus_assets::environment::Water;
use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;

// Preservation's exporter uses 20 engine units per unit and 256 per grid cell.
const CELL_SIZE: f32 = 256.0 / 20.0;
const BIOS_TICKS_PER_SECOND: f64 = 1_193_182.0 / 65_536.0;

struct TerrainMesh {
    entity: Entity,
    original: Handle<Mesh>,
    animated: Handle<Mesh>,
    vertices: Vec<usize>,
    source: crate::terrain::Source,
    view: Option<crate::terrain::View>,
}

#[derive(Resource, Default)]
pub(crate) struct WaterPlayback {
    source: Option<AssetId<Environment>>,
    base: Vec<[f32; 3]>,
    terrain: Vec<TerrainMesh>,
    tick: Option<u64>,
    mode: Mode,
    grid: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    rgb: Vec<[f32; 3]>,
}

/// Modern art-style preset: two smooth crossing waves around the original mean height.
fn modern_displacement(water: &Water, position: [f32; 3], seconds: f32) -> f32 {
    let phase = (position[0] + position[2] * 0.35) / (CELL_SIZE * 6.0) * std::f32::consts::TAU
        - seconds * 0.8;
    let cross = (position[2] - position[0] * 0.6) / (CELL_SIZE * 2.5) * std::f32::consts::TAU
        - seconds * 1.3;
    water.parameters[0] / 20.0 * (0.5 + 0.35 * phase.sin() + 0.15 * cross.sin())
}

/// Vertical offset for a grid vertex in exported units with positive Y up.
/// `vertex` must be within the validated square grid; timing uses BIOS ticks.
/// Phase starts at the application's clock origin; scene changes do not reset it.
#[allow(clippy::approx_constant)]
// The documented table uses this decimal period, rather than mathematical tau.
fn displacement(water: &Water, vertex: usize, ticks: u64) -> f32 {
    let x = (vertex % water.side) as i64 - (water.side / 2) as i64;
    let row = (vertex / water.side) as i64 - (water.side / 2) as i64;
    let [amplitude, speed, frequency] = water.parameters;
    let spatial = (x * x + row * row) * frequency as i64;
    let phase = (spatial as u64).wrapping_add(ticks.wrapping_mul((speed as i64) as u64)) & 2047;
    let sample = (((phase + 512) as f64 * 6.2831852 / 2048.0).sin()) as f32;
    -(sample * amplitude - amplitude * 0.5) / 20.0
}

// Rendering assets, scene hierarchy, and clock are independent Bevy resources.
#[allow(clippy::too_many_arguments)]
pub(crate) fn animate_water(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
    time: Res<Time<Real>>,
    fidelity: Res<Fidelity>,
    mut playback: ResMut<WaterPlayback>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut entities: Query<(Entity, &mut Mesh3d)>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::camera::ViewCamera>>,
    transforms: Query<&GlobalTransform>,
    distance: Option<Res<crate::fog::RenderDistance>>,
    mut preparation: Local<crate::terrain::Preparation>,
    mut camera_grid: Local<Vec<[f32; 3]>>,
) {
    if playback.source != Some(request.0.id())
        || playback
            .terrain
            .iter()
            .any(|terrain| entities.get(terrain.entity).is_err())
    {
        for terrain in &playback.terrain {
            if let Ok((_, mut mesh)) = entities.get_mut(terrain.entity)
                && mesh.0 == terrain.animated
            {
                mesh.0 = terrain.original.clone();
                commands
                    .entity(terrain.entity)
                    .insert(Visibility::Inherited);
                if let Some(bounds) = meshes.get(&terrain.original).and_then(Mesh::get_aabb) {
                    commands.entity(terrain.entity).insert(bounds);
                }
            }
        }
        *playback = WaterPlayback::default();
    }
    let Some(environment) = environments.get(&request.0) else {
        return;
    };
    let lighting = environment.terrain_lighting.as_ref();
    let water = environment
        .water
        .as_ref()
        .filter(|w| w.parameters[0] != 0.0);
    let Some(side) = lighting.map(|l| l.side).or_else(|| water.map(|w| w.side)) else {
        return;
    };
    if playback.terrain.is_empty() {
        let terrain: Vec<_> = entities
            .iter()
            .filter(|(entity, _)| {
                std::iter::once(*entity)
                    .chain(parents.iter_ancestors(*entity))
                    .any(|ancestor| {
                        names
                            .get(ancestor)
                            .is_ok_and(|name| name.as_str() == "Terrain")
                    })
            })
            .map(|(entity, mesh)| (entity, mesh.0.clone()))
            .collect();
        if terrain.is_empty()
            || terrain
                .iter()
                .any(|(_, handle)| meshes.get(handle).is_none())
        {
            return;
        }
        playback.base = vec![[0.0; 3]; side * side];
        for (entity, original) in terrain {
            let mesh = meshes.get(&original).unwrap();
            let Ok(VertexAttributeValues::Float32x3(positions)) =
                mesh.try_attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                error!("water terrain {entity}: missing positions");
                return;
            };
            let mut vertices = Vec::with_capacity(positions.len());
            for &position in positions {
                let x = (-position[0] / CELL_SIZE).round() as usize;
                let row = (-position[2] / CELL_SIZE).round() as usize;
                if x >= side || row >= side {
                    error!("water terrain {entity}: vertex outside {}-wide grid", side);
                    return;
                }
                let vertex = x + row * side;
                playback.base[vertex] = position;
                vertices.push(vertex);
            }
            let source = match crate::terrain::Source::new(mesh, vertices.len()) {
                Ok(source) => source,
                Err(reason) => {
                    error!(
                        "water terrain {entity}, environment {:?}: {reason}",
                        request.0.id()
                    );
                    return;
                }
            };
            let mut animated_mesh = mesh.clone();
            animated_mesh.insert_attribute(
                Mesh::ATTRIBUTE_UV_1,
                vertices
                    .iter()
                    .map(|&vertex| {
                        [
                            u8::from(water.is_some_and(|w| w.animated_vertices[vertex])) as f32,
                            0.0,
                        ]
                    })
                    .collect::<Vec<_>>(),
            );
            if let Some(mut bounds) = mesh.get_aabb() {
                bounds.half_extents.y += water.map_or(0.0, |w| w.parameters[0].abs()) * 1.5 / 20.0;
                animated_mesh.final_aabb = Some(bounds.into());
                commands.entity(entity).insert(bounds);
            }
            let animated = meshes.add(animated_mesh);
            entities.get_mut(entity).unwrap().1.0 = animated.clone();
            playback.terrain.push(TerrainMesh {
                entity,
                original,
                animated,
                vertices,
                source,
                view: None,
            });
        }
        playback.source = Some(request.0.id());
    }
    let ticks = (time.elapsed_secs_f64() * BIOS_TICKS_PER_SECOND).floor() as u64;
    let mode = fidelity.mode(Area::Water);
    let update_grid = playback.tick.is_none()
        || (water.is_some()
            && (mode != Mode::Faithful || playback.mode != mode || playback.tick != Some(ticks)));
    if update_grid {
        playback.tick = Some(ticks);
        playback.mode = mode;
        let mut grid = playback.base.clone();
        for (vertex, position) in grid.iter_mut().enumerate() {
            if let Some(water) = water.filter(|w| w.animated_vertices[vertex]) {
                position[1] += match mode {
                    Mode::Faithful => displacement(water, vertex, ticks),
                    Mode::Modern => {
                        modern_displacement(water, playback.base[vertex], time.elapsed_secs())
                    }
                };
            }
        }
        // shortcut: original water edges can retain old normals; replace full recomputation when dirty admission is established.
        playback.normals = crate::lighting::grid_normals(&grid, side);
        playback.rgb = playback
            .normals
            .iter()
            .map(|&normal| lighting.map_or([255.0; 3], |l| l.rgb(normal)))
            .collect();
        playback.grid = grid;
    }
    let view = cameras.single().ok().and_then(|(camera, transform)| {
        let viewport = camera.physical_viewport_size()?.as_vec2();
        let projection = camera.clip_from_view();
        Some(crate::terrain::View {
            camera_from_local: transform.to_matrix().inverse(),
            scale: Vec2::new(projection.x_axis.x, -projection.y_axis.y),
            viewport: viewport.to_array(),
            clipping: distance
                .as_ref()
                .and_then(|d| d.effective(&fidelity))
                .unwrap_or(environment.clipping),
        })
    });
    let WaterPlayback {
        terrain,
        grid,
        normals,
        rgb,
        ..
    } = &mut *playback;
    let mut transformed = None;

    for terrain in terrain {
        let view = view.and_then(|mut view| {
            view.camera_from_local *= transforms.get(terrain.entity).ok()?.to_matrix();
            Some(view)
        });
        let restore = view.is_none() && terrain.view.is_some();
        if !update_grid && !restore && terrain.view == view {
            continue;
        }
        if let Some(view) = view {
            let key = (view.camera_from_local, view.scale);
            if transformed != Some(key) {
                camera_grid.clear();
                camera_grid.extend(grid.iter().map(|&position| {
                    let p = view
                        .camera_from_local
                        .transform_point3(Vec3::from(position))
                        * 20.0;
                    [p.x * view.scale.x, p.y * view.scale.y, -p.z]
                }));
                transformed = Some(key);
            }
            let visible = crate::terrain::update(
                &mut terrain.source,
                &terrain.vertices,
                crate::terrain::Grid {
                    local: grid,
                    camera: &camera_grid,
                    normals,
                    rgb,
                    water: water.map(|w| w.animated_vertices.as_slice()),
                },
                view,
                &mut preparation,
            );
            commands.entity(terrain.entity).insert(if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
            if visible && let Some(mut mesh) = meshes.get_mut(&terrain.animated) {
                std::mem::swap(&mut *mesh, &mut terrain.source.prepared);
            }
            terrain.view = Some(view);
            continue;
        }
        if restore {
            terrain.view = None;
            commands
                .entity(terrain.entity)
                .insert(Visibility::Inherited);
            if let Some(original) = meshes.get(&terrain.original).cloned()
                && let Some(mut mesh) = meshes.get_mut(&terrain.animated)
            {
                *mesh = original;
            }
        }
        let Some(mut mesh) = meshes.get_mut(&terrain.animated) else {
            continue;
        };
        if let Some(lighting) = lighting {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_COLOR,
                terrain
                    .vertices
                    .iter()
                    .map(|&vertex| lighting.color(normals[vertex]))
                    .collect::<Vec<_>>(),
            );
        }
        if let Ok(VertexAttributeValues::Float32x3(positions)) =
            mesh.try_attribute_mut(Mesh::ATTRIBUTE_POSITION)
        {
            for (position, &vertex) in positions.iter_mut().zip(&terrain.vertices) {
                *position = grid[vertex];
            }
        }
        if let Ok(VertexAttributeValues::Float32x3(mesh_normals)) =
            mesh.try_attribute_mut(Mesh::ATTRIBUTE_NORMAL)
        {
            for (normal, &vertex) in mesh_normals.iter_mut().zip(&terrain.vertices) {
                *normal = normals[vertex];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::PrimitiveTopology;
    use std::time::Duration;

    #[test]
    fn documented_wave_samples_have_the_bias_and_export_sign() {
        let water = Water {
            parameters: [24.0, 512.0, 0.0],
            side: 2,
            animated_vertices: vec![false, false, false, true],
        };
        for (ticks, expected) in [(0, -0.6), (1, 0.6), (2, 1.8), (3, 0.6), (4, -0.6)] {
            assert!((displacement(&water, 3, ticks) - expected).abs() < 0.00001);
        }
        let spatial = Water {
            parameters: [24.0, 0.0, 256.0],
            ..water
        };
        assert!((displacement(&spatial, 0, 0) - 0.6).abs() < 0.00001);
        assert!((displacement(&spatial, 3, 0) + 0.6).abs() < 0.00001);
    }

    fn triangle(positions: Vec<[f32; 3]>) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            vec![[0.0, 1.0, 0.0]; positions.len()],
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0; 2]; positions.len()]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh
    }

    fn positions(app: &App, handle: &Handle<Mesh>) -> Vec<[f32; 3]> {
        let mesh = app.world().resource::<Assets<Mesh>>().get(handle).unwrap();
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            VertexAttributeValues::Float32x3(positions) => positions.clone(),
            _ => panic!("wrong position format"),
        }
    }

    #[test]
    fn terrain_playback_keeps_shared_assets_and_normals_and_restores_on_world_change() {
        let mut app = App::new();
        let mut environments = Assets::<Environment>::default();
        let outdoor = environments.add(Environment {
            object_lights: Vec::new(),
            prelighting: Default::default(),
            model_lighting: Default::default(),
            sky: None,
            background: [0; 3],
            clipping: [7.0, 3800.0],
            fog_table: [[0; 4]; 16],
            terrain_lighting: Some(cyrus_assets::environment::TerrainLighting {
                side: 2,
                ambient: [64.0; 3],
                directional: [128.0; 3],
                direction: [0.0, -1.0, 0.0],
            }),
            water: Some(Water {
                parameters: [24.0, 512.0, 0.0],
                side: 2,
                animated_vertices: vec![false, false, false, true],
            }),
        });
        app.insert_resource(environments)
            .insert_resource(EnvironmentRequest(outdoor.clone()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<WaterPlayback>()
            .init_resource::<Fidelity>()
            .init_resource::<Time<Real>>()
            .add_systems(Update, animate_water);
        let a = [
            [0.0, 0.0, 0.0],
            [-CELL_SIZE, 0.0, -CELL_SIZE],
            [-CELL_SIZE, 0.0, 0.0],
        ];
        let b = [a[1], a[0], [0.0, 0.0, -CELL_SIZE]];
        let first = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(triangle(a.to_vec()));
        let second = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(triangle(b.to_vec()));
        let parent = app.world_mut().spawn(Name::new("Terrain")).id();
        let entity = app
            .world_mut()
            .spawn((Mesh3d(first.clone()), ChildOf(parent)))
            .id();
        let other = app
            .world_mut()
            .spawn((Mesh3d(second.clone()), ChildOf(parent)))
            .id();
        // An ordinary model can share a mesh with terrain without receiving waves.
        let model = app.world_mut().spawn(Mesh3d(first.clone())).id();
        app.update();
        let animated = app.world().get::<Mesh3d>(entity).unwrap().0.clone();
        let animated_other = app.world().get::<Mesh3d>(other).unwrap().0.clone();
        assert_ne!(animated, first);
        let colors = |app: &App, handle: &Handle<Mesh>| {
            let mesh = app.world().resource::<Assets<Mesh>>().get(handle).unwrap();
            match mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap() {
                VertexAttributeValues::Float32x4(values) => values.clone(),
                _ => panic!("wrong terrain color format"),
            }
        };
        let initial_colors = colors(&app, &animated);
        assert_eq!(initial_colors[1], colors(&app, &animated_other)[0]);
        assert!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&first)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_COLOR)
                .is_none()
        );
        assert_eq!(positions(&app, &first), a);
        assert_eq!(app.world().get::<Mesh3d>(model).unwrap().0, first);
        assert!((positions(&app, &animated)[1][1] + 0.6).abs() < 0.00001);
        assert_eq!(positions(&app, &animated)[0], a[0]);
        let meshes = app.world().resource::<Assets<Mesh>>();
        let normal = |handle: &Handle<Mesh>, vertex: usize| match meshes
            .get(handle)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .unwrap()
        {
            VertexAttributeValues::Float32x3(normals) => normals[vertex],
            _ => panic!("wrong normal format"),
        };
        assert_eq!(normal(&animated, 1), normal(&animated_other, 0));
        assert_ne!(normal(&animated, 1), [0.0, 1.0, 0.0]);
        app.world_mut()
            .resource_mut::<Fidelity>()
            .set(Area::Water, Mode::Modern);
        app.update();
        assert!(positions(&app, &animated)[1][1] >= 0.0);
        assert_eq!(positions(&app, &animated)[0], a[0]);
        assert_eq!(positions(&app, &first), a);
        app.world_mut()
            .resource_mut::<Fidelity>()
            .set(Area::Water, Mode::Faithful);
        app.update();
        // Returning to Faithful within the same tick restores its exact displacement.
        assert!((positions(&app, &animated)[1][1] + 0.6).abs() < 0.00001);
        // Render updates in the same BIOS tick leave the mesh unchanged.
        app.update();
        assert!((positions(&app, &animated)[1][1] + 0.6).abs() < 0.00001);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(110));
        app.update();
        assert!((positions(&app, &animated)[1][1] - 1.8).abs() < 0.00001);
        assert_ne!(colors(&app, &animated), initial_colors);
        app.world_mut().resource_mut::<EnvironmentRequest>().0 = Handle::default();
        app.update();
        assert_eq!(app.world().get::<Mesh3d>(entity).unwrap().0, first);
        assert_eq!(app.world().get::<Mesh3d>(other).unwrap().0, second);
        app.world_mut().resource_mut::<EnvironmentRequest>().0 = outdoor;
        app.update();
        let rebound = &app.world().get::<Mesh3d>(entity).unwrap().0;
        assert!(
            (positions(&app, rebound)[1][1] - 1.8).abs() < 0.00001,
            "world changes must not reset wave time"
        );
        let disabled = app
            .world_mut()
            .resource_mut::<Assets<Environment>>()
            .add(Environment {
                object_lights: Vec::new(),
                prelighting: Default::default(),
                model_lighting: Default::default(),
                sky: None,
                background: [0; 3],
                clipping: [7.0, 3800.0],
                fog_table: [[0; 4]; 16],
                terrain_lighting: None,
                water: Some(Water {
                    parameters: [0.0, 512.0, 0.0],
                    side: 2,
                    animated_vertices: vec![false, false, false, true],
                }),
            });
        app.world_mut().resource_mut::<EnvironmentRequest>().0 = disabled;
        app.update();
        assert_eq!(app.world().get::<Mesh3d>(entity).unwrap().0, first);
        assert_eq!(positions(&app, &first), a);
        let dry = app
            .world_mut()
            .resource_mut::<Assets<Environment>>()
            .add(Environment {
                object_lights: Vec::new(),
                prelighting: Default::default(),
                model_lighting: Default::default(),
                sky: None,
                background: [0; 3],
                clipping: [7.0, 3800.0],
                fog_table: [[0; 4]; 16],
                water: None,
                terrain_lighting: Some(cyrus_assets::environment::TerrainLighting {
                    side: 2,
                    ambient: [32.0, 64.0, 128.0],
                    directional: [0.0; 3],
                    direction: [0.0, -1.0, 0.0],
                }),
            });
        app.world_mut().resource_mut::<EnvironmentRequest>().0 = dry;
        app.update();
        let dry_mesh = app.world().get::<Mesh3d>(entity).unwrap().0.clone();
        assert_eq!(positions(&app, &dry_mesh), a);
        assert_eq!(
            colors(&app, &dry_mesh),
            vec![[32.0 / 255.0, 64.0 / 255.0, 128.0 / 255.0, 1.0]; 3]
        );
        app.world_mut()
            .resource_mut::<Fidelity>()
            .set(Area::Lighting, Mode::Modern);
        app.update();
        assert_eq!(app.world().get::<Mesh3d>(entity).unwrap().0, dry_mesh);
        assert_eq!(positions(&app, &first), a);
        app.world_mut().despawn(entity);
        app.world_mut().despawn(other);
        let replacement = app
            .world_mut()
            .spawn((Mesh3d(first.clone()), ChildOf(parent)))
            .id();
        app.world_mut().spawn((Mesh3d(second), ChildOf(parent)));
        app.update();
        let rebound = app.world().get::<Mesh3d>(replacement).unwrap().0.clone();
        assert_ne!(
            rebound, first,
            "reloading the same world must bind its new terrain entities"
        );
        assert_eq!(colors(&app, &rebound), colors(&app, &dry_mesh));
        app.world_mut()
            .entity_mut(replacement)
            .insert(GlobalTransform::IDENTITY);
        let mut camera = Camera::default();
        camera.computed.clip_from_view =
            bevy::camera::CameraProjection::get_clip_from_view(&PerspectiveProjection {
                fov: std::f32::consts::FRAC_PI_2,
                aspect_ratio: 4.0 / 3.0,
                near: 0.35,
                ..Default::default()
            });
        camera.computed.target_info = Some(bevy::camera::RenderTargetInfo {
            physical_size: UVec2::new(640, 480),
            scale_factor: 1.0,
        });
        let pose = Transform::from_xyz(0.0, 50.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y);
        let camera_entity = app
            .world_mut()
            .spawn((
                crate::camera::ViewCamera,
                camera,
                GlobalTransform::from(pose),
            ))
            .id();
        app.update();
        let projected = |app: &App| {
            let meshes = app.world().resource::<Assets<Mesh>>();
            match meshes
                .get(&rebound)
                .unwrap()
                .attribute(crate::terrain::PROJECTED)
                .unwrap()
            {
                VertexAttributeValues::Float32x4(v) => v.clone(),
                _ => panic!("wrong prepared terrain format"),
            }
        };
        let initial = projected(&app);
        assert_eq!(initial.len(), 3);
        app.update();
        assert_eq!(projected(&app), initial);
        app.world_mut()
            .get_mut::<Camera>(camera_entity)
            .unwrap()
            .computed
            .target_info
            .as_mut()
            .unwrap()
            .physical_size *= 2;
        app.update();
        let resized = projected(&app);
        assert_ne!(initial, resized);
        assert_eq!(resized.len(), 3);
        app.world_mut()
            .get_mut::<GlobalTransform>(camera_entity)
            .unwrap()
            .clone_from(&GlobalTransform::from(
                Transform::from_xyz(10.0, 50.0, 50.0).with_rotation(pose.rotation),
            ));
        app.update();
        assert_ne!(projected(&app), resized);
        assert_eq!(positions(&app, &first), a);
        let forward_pose = *app.world().get::<GlobalTransform>(camera_entity).unwrap();
        *app.world_mut()
            .get_mut::<GlobalTransform>(camera_entity)
            .unwrap() = GlobalTransform::from(
            pose.with_rotation(pose.rotation * Quat::from_rotation_y(std::f32::consts::PI)),
        );
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(replacement).unwrap(),
            Visibility::Hidden
        );
        assert!(!positions(&app, &rebound).is_empty());
        *app.world_mut()
            .get_mut::<GlobalTransform>(camera_entity)
            .unwrap() = forward_pose;
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(replacement).unwrap(),
            Visibility::Inherited
        );
        app.world_mut().despawn(camera_entity);
        app.update();
        assert_eq!(positions(&app, &rebound), a);
        assert_eq!(
            colors(&app, &rebound),
            vec![[32.0 / 255.0, 64.0 / 255.0, 128.0 / 255.0, 1.0]; 3]
        );
    }
}
