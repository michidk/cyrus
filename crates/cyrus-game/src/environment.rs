//! Cached sky and water rendering shared by native and browser builds.
//! Engine inputs follow preservation's docs/engine/{sky,water}.md.
//! Projection and wave timing are approximations, restricted to Modern graphics.

use crate::fidelity::{Area, Fidelity, Mode};
use bevy::asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader};
use bevy::core_pipeline::Skybox;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use serde::{Deserialize, Serialize};

#[derive(Asset, TypePath, Serialize, Deserialize)]
/// Per-world rendering inputs extracted from WORLD.INI and the installed assets.
pub struct Environment {
    /// First decoded GXA frame, if the world declares a sky.
    pub sky: Option<SkyImage>,
    /// Palette-derived RGB background; unsupported sky-derived colors use black.
    pub background: [u8; 3],
    /// Terrain water mask and original wave parameters, if declared.
    pub water: Option<Water>,
}

#[derive(Serialize, Deserialize)]
/// Decoded sky pixels in row-major RGBA order.
pub struct SkyImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes per pixel, exactly `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
/// Wave inputs from WORLD.INI and the WLD texture-selection grid.
pub struct Water {
    /// Raw amplitude, speed, and spatial frequency, in INI order.
    pub parameters: [f32; 3],
    /// Number of vertices along each side of the square terrain grid.
    pub side: usize,
    /// Row-major mask identifying vertices belonging to four-corner water cells.
    pub vertices: Vec<bool>,
}

impl Environment {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        if let Some(sky) = &self.sky {
            anyhow::ensure!(
                sky.width > 0 && sky.height > 0 && sky.width <= 8192 && sky.height <= 8192,
                "invalid sky dimensions"
            );
            anyhow::ensure!(
                sky.rgba.len() == sky.width as usize * sky.height as usize * 4,
                "invalid sky pixels"
            );
        }
        if let Some(water) = &self.water {
            anyhow::ensure!(
                (2..=256).contains(&water.side) && water.vertices.len() == water.side * water.side,
                "invalid water grid"
            );
            anyhow::ensure!(
                water.parameters.iter().all(|v| v.is_finite() && *v >= 0.0),
                "invalid wave parameters"
            );
        }
        Ok(())
    }
}

#[derive(Default, TypePath)]
struct EnvironmentLoader;
impl AssetLoader for EnvironmentLoader {
    type Asset = Environment;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<Environment, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let environment: Environment = serde_json::from_slice(&bytes)?;
        environment.validate()?;
        Ok(environment)
    }
    fn extensions(&self) -> &[&str] {
        &["environment"]
    }
}

#[derive(Resource, Default)]
/// Environment asset associated with the currently requested world.
pub struct EnvironmentRequest(pub Handle<Environment>);

#[derive(Resource, Default)]
struct ActiveEnvironment {
    source: Option<AssetId<Environment>>,
    sky: Option<Handle<Image>>,
    modern: bool,
}

#[derive(Component)]
struct WaterMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    vertices: Vec<(usize, i32, i32)>,
    displaced: bool,
    last_frame: Option<(u64, i32, i32)>,
}

/// Renderer-only plugin; approximate sky and waves require Modern graphics.
pub struct EnvironmentPlugin;
impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Environment>()
            .init_asset_loader::<EnvironmentLoader>()
            .init_resource::<EnvironmentRequest>()
            .init_resource::<ActiveEnvironment>()
            .add_systems(Update, (apply_sky, bind_water, animate_water).chain());
    }
}

// Bevy supplies the independent asset, camera, and fidelity resources to this system.
#[allow(clippy::too_many_arguments)]
fn apply_sky(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
    fidelity: Res<Fidelity>,
    mut active: ResMut<ActiveEnvironment>,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<Entity, With<Camera3d>>,
    mut clear: ResMut<ClearColor>,
) {
    let Some(environment) = environments.get(&request.0) else {
        return;
    };
    let modern = fidelity.mode(Area::Graphics) == Mode::Modern;
    if active.source == Some(request.0.id()) && active.modern == modern {
        return;
    }
    if cameras.is_empty() {
        return;
    }
    active.sky = if modern {
        environment.sky.as_ref().map(|sky| images.add(cubemap(sky)))
    } else {
        None
    };
    for camera in &cameras {
        commands.entity(camera).remove::<Skybox>();
        if let Some(image) = &active.sky {
            commands.entity(camera).insert(Skybox {
                image: Some(image.clone()),
                brightness: 1000.0,
                ..default()
            });
        }
    }
    let [r, g, b] = environment.background;
    clear.0 = Color::srgb_u8(r, g, b);
    active.source = Some(request.0.id());
    active.modern = modern;
}

fn cubemap(sky: &SkyImage) -> Image {
    // unverified: equirectangular projection; original GXA screen mapping is undocumented.
    let side = sky.height.clamp(16, 512);
    let mut pixels = Vec::with_capacity((side * side * 6 * 4) as usize);
    for face in 0..6 {
        for y in 0..side {
            for x in 0..side {
                let u = 2.0 * (x as f32 + 0.5) / side as f32 - 1.0;
                let v = 2.0 * (y as f32 + 0.5) / side as f32 - 1.0;
                let direction = match face {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                }
                .normalize();
                let sx = ((direction.z.atan2(direction.x) / std::f32::consts::TAU + 0.5)
                    * sky.width as f32) as u32
                    % sky.width;
                let sy =
                    ((0.5 - direction.y.asin() / std::f32::consts::PI) * sky.height as f32) as u32;
                let offset = ((sy.min(sky.height - 1) * sky.width + sx) * 4) as usize;
                pixels.extend_from_slice(&sky.rgba[offset..offset + 4]);
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

fn bind_water(
    mut commands: Commands,
    fidelity: Res<Fidelity>,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut candidates: Query<(Entity, &mut Mesh3d, &ChildOf), Without<WaterMesh>>,
    names: Query<(&Name, Option<&ChildOf>)>,
) {
    if fidelity.mode(Area::Graphics) != Mode::Modern {
        return;
    }
    let Some(water) = environments.get(&request.0).and_then(|e| e.water.as_ref()) else {
        return;
    };
    for (entity, mut handle, parent) in &mut candidates {
        let mut ancestor = parent.parent();
        let mut terrain = false;
        while let Ok((name, parent)) = names.get(ancestor) {
            if name.as_str() == "Terrain" {
                terrain = true;
                break;
            }
            let Some(parent) = parent else { break };
            ancestor = parent.parent();
        }
        if !terrain {
            continue;
        }
        let Some(mesh) = meshes.get(&handle.0) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            continue;
        };
        let vertices = positions
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let x = (-p[0] / GRID_SCALE).round() as i32;
                let z = (-p[2] / GRID_SCALE).round() as i32;
                (x >= 0
                    && z >= 0
                    && (x as usize) < water.side
                    && (z as usize) < water.side
                    && water.vertices[x as usize + z as usize * water.side])
                    .then_some((i, x, z))
            })
            .collect();
        let state = WaterMesh {
            positions: positions.clone(),
            normals: normals.clone(),
            vertices,
            displaced: false,
            last_frame: None,
        };
        let owned = mesh.clone();
        handle.0 = meshes.add(owned);
        commands.entity(entity).insert(state);
    }
}

// Terrain units and the 2048-sample phase wrap follow preservation's WLD/water docs.
const GRID_SCALE: f32 = 256.0 / 20.0;
fn wave_height(x: i32, z: i32, tick: u64, parameters: [f32; 3]) -> f32 {
    // unverified: raw speed per 30 Hz tick, unit sine amplitude, zero centering bias.
    // Upgrade when preservation establishes the speed conversion and sine-table bias.
    let [amplitude, speed, frequency] = parameters;
    let phase = ((x as f64 * x as f64 + z as f64 * z as f64) * frequency as f64
        + tick as f64 * speed as f64)
        .rem_euclid(2048.0);
    static SINE: std::sync::OnceLock<[f32; 2048]> = std::sync::OnceLock::new();
    let sine = SINE
        .get_or_init(|| std::array::from_fn(|i| (i as f32 * std::f32::consts::TAU / 2048.0).sin()));
    sine[phase as usize] * amplitude / 20.0
}

fn animate_water(
    time: Res<Time>,
    fidelity: Res<Fidelity>,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut water_meshes: Query<(&Mesh3d, &mut WaterMesh)>,
) {
    let Some(water) = environments.get(&request.0).and_then(|e| e.water.as_ref()) else {
        return;
    };
    let modern = fidelity.mode(Area::Graphics) == Mode::Modern;
    let Ok(camera) = cameras.single() else { return };
    let center = camera.translation();
    let cx = (-center.x / GRID_SCALE).round() as i32;
    let cz = (-center.z / GRID_SCALE).round() as i32;
    let tick = (time.elapsed_secs_f64() * 30.0) as u64;
    for (handle, mut state) in &mut water_meshes {
        let Some(mut mesh) = meshes.get_mut(&handle.0) else {
            continue;
        };
        if !modern {
            if state.displaced {
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, state.positions.clone());
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, state.normals.clone());
                state.displaced = false;
                state.last_frame = None;
            }
            continue;
        }
        if state.last_frame == Some((tick, cx, cz)) {
            continue;
        }
        state.last_frame = Some((tick, cx, cz));
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        for &(i, x, z) in &state.vertices {
            positions[i][1] =
                state.positions[i][1] + wave_height(x - cx, z - cz, tick, water.parameters);
        }
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL)
        else {
            continue;
        };
        for &(i, x, z) in &state.vertices {
            let dx = wave_height(x - cx + 1, z - cz, tick, water.parameters)
                - wave_height(x - cx - 1, z - cz, tick, water.parameters);
            let dz = wave_height(x - cx, z - cz + 1, tick, water.parameters)
                - wave_height(x - cx, z - cz - 1, tick, water.parameters);
            normals[i] = Vec3::new(dx, 2.0 * GRID_SCALE, dz).normalize().to_array();
        }
        state.displaced = true;
    }
}

/// Four-corner water classification from preservation's water renderer documentation.
pub fn water_vertices(textures: &[u8], side: usize) -> Vec<bool> {
    let mut vertices = vec![false; textures.len()];
    for z in 0..side - 1 {
        for x in 0..side - 1 {
            let corners = [
                x + z * side,
                x + 1 + z * side,
                x + (z + 1) * side,
                x + 1 + (z + 1) * side,
            ];
            if corners
                .iter()
                .all(|&i| matches!(textures[i] & 63, 0 | 5 | 30 | 31))
            {
                for i in corners {
                    vertices[i] = true;
                }
            }
        }
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modern_water_moves_only_marked_vertices_and_faithful_restores_mesh() {
        use bevy::mesh::PrimitiveTopology;
        let mut app = App::new();
        let mut environments = Assets::<Environment>::default();
        let environment = environments.add(Environment {
            sky: None,
            background: [0; 3],
            water: Some(Water {
                parameters: [20.0, 512.0, 0.0],
                side: 2,
                vertices: vec![true, false, false, false],
            }),
        });
        let positions = vec![[0.0, 0.0, 0.0], [-12.8, 4.0, 0.0], [0.0, 0.0, -12.8]];
        let normals = vec![[0.0, 1.0, 0.0]; 3];
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone());
        let mut meshes = Assets::<Mesh>::default();
        let handle = meshes.add(mesh);
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_millis(34));
        app.insert_resource(time)
            .insert_resource(environments)
            .insert_resource(meshes)
            .insert_resource(EnvironmentRequest(environment))
            .insert_resource(Fidelity {
                graphics: Mode::Modern,
                ..default()
            })
            .add_systems(Update, (bind_water, animate_water).chain());
        app.world_mut()
            .spawn((Camera3d::default(), GlobalTransform::default()));
        let terrain = app.world_mut().spawn(Name::new("Terrain")).id();
        let entity = app
            .world_mut()
            .spawn((Mesh3d(handle.clone()), ChildOf(terrain)))
            .id();
        app.update();
        let animated_handle = app.world().get::<Mesh3d>(entity).unwrap().0.clone();
        assert_ne!(
            animated_handle, handle,
            "animation must not mutate shared assets"
        );
        let handle = animated_handle;
        let mesh = app.world().resource::<Assets<Mesh>>().get(&handle).unwrap();
        let VertexAttributeValues::Float32x3(animated) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
        else {
            panic!("position format")
        };
        assert!((animated[0][1] - 1.0).abs() < 1e-6);
        assert_eq!(animated[1], [-12.8, 4.0, 0.0]);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(34));
        app.update();
        let mesh = app.world().resource::<Assets<Mesh>>().get(&handle).unwrap();
        let VertexAttributeValues::Float32x3(animated) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
        else {
            panic!("position format")
        };
        assert!(animated[0][1].abs() < 1e-6, "water advances with time");
        assert_eq!(animated[1], positions[1]);
        app.world_mut().resource_mut::<Fidelity>().graphics = Mode::Faithful;
        app.update();
        let mesh = app.world().resource::<Assets<Mesh>>().get(&handle).unwrap();
        let VertexAttributeValues::Float32x3(restored) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
        else {
            panic!("position format")
        };
        assert_eq!(restored, &positions);
    }

    #[test]
    fn water_requires_all_four_corners_and_ignores_rotation_bits() {
        assert_eq!(water_vertices(&[0, 69, 158, 223], 2), vec![true; 4]);
        assert_eq!(water_vertices(&[0, 5, 30, 2], 2), vec![false; 4]);
        assert_eq!(
            water_vertices(&[0, 0, 2, 0, 0, 2, 2, 2, 2], 3),
            vec![true, true, false, true, true, false, false, false, false]
        );
    }
    #[test]
    fn waves_are_radial_periodic_and_bounded() {
        assert_eq!(
            wave_height(3, 4, 1, [24.0, 28.0, 5.0]),
            wave_height(4, 3, 1, [24.0, 28.0, 5.0])
        );
        assert!((wave_height(0, 0, 1, [20.0, 512.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(wave_height(0, 0, 4, [20.0, 512.0, 0.0]).abs() < 1e-6);
        assert_eq!(wave_height(5, 8, 99, [0.0, 28.0, 5.0]), 0.0);
    }
}
