//! Grass on the terrain's grass tiles.
//!
//! **Modern only**, a tuning choice, and built only with the `grass` cargo feature. The
//! original draws the terrain as flat textured tiles with no blades. While `Area::Lighting` is
//! Modern, raster or ray-traced, and the grass effect is on, blades stand on the terrain wherever
//! its texture is grass: every terrain triangle whose tile image shows grass
//! (`cyrus_assets::terrain_grass`) is sampled at random points, a blade grows where the texel
//! there is grass, and takes that texel's colour, so the grass blends into the ground it grows
//! from and thins out where a tile blends into sand. Blades are merged into one mesh per square
//! chunk of the terrain near the camera, built a few per frame and dropped when the camera
//! leaves; placement is deterministic, so a chunk grows the same blades each time.
//!
//! Under raster Modern lighting the blades have a material of their own (`grass.wesl`): Bevy's
//! lights, the scene fog, and wind that bends each tip along the heading the swaying plants use
//! (`foliage`), which the Swaying plants switch turns off. Under ray tracing the blades are drawn
//! the same way, and a sparse share of the nearest ones is traced as well, so the meadow shades
//! the ground and bounces light; the traced stand-ins cannot move each frame, so they stand still
//! while the drawn blades sway. Blades cast no raster shadows, and Solari does not light them.
use super::{
    Fidelity, FogExtension, ModernEffects, SceneFog, TerrainMaterial, define_modern_lighting,
    is_prepass, skip_prepass,
};
use crate::{camera::ViewCamera, environment::EnvironmentRequest};
use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    light::NotShadowCaster,
    material::OpaqueRendererMethod,
    mesh::{Indices, PrimitiveTopology},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use cyrus_assets::terrain_grass::{grass_share, has_grass, is_grass};
use cyrus_game::fidelity::{Area, Mode};
use std::collections::HashMap;

/// Side of a chunk of blades, in world units.
const CHUNK: f32 = 24.0;
/// Chunks built per frame.
const CHUNKS_PER_FRAME: usize = 2;
/// Height and base width of a blade, in world units; the player is about three tall.
const BLADE_HEIGHT: f32 = 0.75;
const BLADE_WIDTH: f32 = 0.13;

/// How blades grow within a range of distances from the camera: fewer and larger the farther
/// they are, so the meadow keeps its coverage at a fraction of the blades.
struct Band {
    /// The farthest distance, in world units, the band reaches.
    reach: f32,
    /// Blades per square world unit of grass tile.
    density: f32,
    /// Factors on a blade's width and height.
    width: f32,
    height: f32,
}

const BANDS: [Band; 3] = [
    Band {
        reach: 30.0,
        density: 50.0,
        width: 1.0,
        height: 1.0,
    },
    Band {
        reach: 70.0,
        density: 12.0,
        width: 2.0,
        height: 1.2,
    },
    Band {
        reach: 150.0,
        density: 2.5,
        width: 3.5,
        height: 1.5,
    },
];

/// The band that holds a distance from the camera.
fn band_of(distance: f32) -> Option<usize> {
    BANDS.iter().position(|band| distance <= band.reach)
}

/// How far past its edges a chunk keeps its band, so a camera on a boundary does not rebuild the
/// chunk every frame.
const BAND_MARGIN: f32 = 6.0;

/// Whether a chunk grown for `band` still suits its distance from the camera.
fn keeps(band: usize, distance: f32) -> bool {
    distance <= BANDS[band].reach + BAND_MARGIN
        && (band == 0 || distance >= BANDS[band - 1].reach - BAND_MARGIN)
}
/// Brightness of a blade's base and tip against the ground colour, so blades read as depth.
const BASE_SHADE: f32 = 0.45;
const TIP_SHADE: f32 = 1.0;
/// How far a blade's own tone strays from the ground's, either way.
const TONE_SPREAD: f32 = 0.25;

/// How far the wind bends a blade's tip, in world units.
const WIND_BEND: f32 = 0.18;

/// The blades' material while raster Modern lighting draws them.
type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassExtension>;

// Bindless where the platform allows, as the terrain's is.
#[derive(Asset, TypePath, AsBindGroup, Clone, PartialEq)]
#[data(50, GrassUniform, binding_array(101))]
#[bindless(index_table(range(50..51), binding(100)))]
struct GrassExtension {
    fog: FogExtension,
    /// How far the wind bends a blade's tip, in world units; 0 holds the blades still.
    wind: f32,
}

/// `GrassExtension` as the shader reads it, `GrassMaterial` in `grass.wesl`.
#[derive(ShaderType)]
struct GrassUniform {
    fog: FogExtension,
    wind: Vec4,
}

impl From<&GrassExtension> for GrassUniform {
    fn from(extension: &GrassExtension) -> Self {
        Self {
            fog: extension.fog.clone(),
            wind: Vec4::new(extension.wind, 0.0, 0.0, 0.0),
        }
    }
}

impl MaterialExtension for GrassExtension {
    fn vertex_shader() -> ShaderRef {
        "embedded://cyrus_render/fog/grass.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://cyrus_render/fog/grass.wesl".into()
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if is_prepass(descriptor) {
            // Blades sway in the main pass only; a prepass depth of the resting blades would
            // reject the swayed ones.
            skip_prepass(descriptor);
        } else {
            define_modern_lighting(descriptor);
        }
        Ok(())
    }
}

pub(super) struct GrassPlugin;

impl Plugin for GrassPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "grass.wesl");
        app.add_plugins(MaterialPlugin::<GrassMaterial>::default())
            .init_resource::<GrassField>()
            .add_systems(
                Update,
                (
                    (index_terrain, grow).chain().run_if(grassy),
                    clear.run_if(not(grassy)),
                ),
            );
    }
}

/// Whether grass is on: the grass effect under Modern lighting, raster or traced.
fn grassy(fidelity: Res<Fidelity>, effects: Option<Res<ModernEffects>>) -> bool {
    effects.is_some_and(|effects| effects.grass) && fidelity.mode(Area::Lighting) == Mode::Modern
}

/// A terrain triangle on a grass tile, in world space, with its texture coordinates.
#[derive(Clone, Copy)]
struct Patch {
    positions: [Vec3; 3],
    uvs: [Vec2; 3],
    /// Index into `GrassField::tiles`.
    tile: usize,
}

impl Patch {
    fn centre(&self) -> Vec3 {
        (self.positions[0] + self.positions[1] + self.positions[2]) / 3.0
    }

    fn area(&self) -> f32 {
        let [a, b, c] = self.positions;
        (b - a).cross(c - a).length() * 0.5
    }
}

/// The chunk of the terrain's horizontal plane that holds a point.
fn chunk_of(position: Vec3) -> IVec2 {
    IVec2::new(
        (position.x / CHUNK).floor() as i32,
        (position.z / CHUNK).floor() as i32,
    )
}

/// The grass of the loaded world: its patches by chunk and the chunks grown from them.
#[derive(Resource, Default)]
struct GrassField {
    /// The world the patches were read from.
    world: Option<AssetId<cyrus_assets::environment::Environment>>,
    /// Tile images the patches sample.
    tiles: Vec<Handle<Image>>,
    patches: Vec<Patch>,
    buckets: HashMap<IVec2, Vec<u32>>,
    /// Whether the terrain has been read.
    indexed: bool,
    /// The blade meshes standing now, with the band each was grown for.
    chunks: HashMap<IVec2, (Entity, usize)>,
    /// How the standing chunks are drawn; they are grown again when it changes.
    look: Option<Look>,
    /// The raster blades' material.
    leaf: Option<Handle<GrassMaterial>>,
    /// The ray-traced blades' material as bounced light sees them.
    #[cfg_attr(not(feature = "raytracing"), allow(dead_code))]
    bounce: Option<Handle<StandardMaterial>>,
}

/// How blades are drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// Drawn alone, under raster Modern lighting.
    Raster,
    /// Drawn the same, with a sparse stand-in traced up to `TRACED_BANDS`, while Solari lights
    /// the scene.
    Traced,
}

/// The nearest bands, as many as this, are in the ray-traced scene; farther blades are only drawn.
#[cfg_attr(not(feature = "raytracing"), allow(dead_code))]
const TRACED_BANDS: usize = 1;

/// A chunk of blades. The scene material pass leaves it alone.
#[derive(Component)]
pub(super) struct GrassChunk;

/// Reads the terrain's grass triangles once per world, when its meshes and images are ready.
#[allow(clippy::too_many_arguments)] // Bevy system parameters.
fn index_terrain(
    request: Res<EnvironmentRequest>,
    mut field: ResMut<GrassField>,
    mut commands: Commands,
    terrain: Query<(
        Entity,
        &Mesh3d,
        &GlobalTransform,
        &MeshMaterial3d<TerrainMaterial>,
    )>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<TerrainMaterial>>,
    images: Res<Assets<Image>>,
) {
    if field.world != Some(request.0.id()) {
        for (_, (chunk, _)) in field.chunks.drain().collect::<Vec<_>>() {
            commands.entity(chunk).despawn();
        }
        *field = GrassField {
            world: Some(request.0.id()),
            leaf: field.leaf.take(),
            bounce: field.bounce.take(),
            look: field.look,
            ..default()
        };
    }
    if field.indexed {
        return;
    }
    let mut patches = Vec::new();
    let mut tiles = Vec::new();
    let mut seen = 0;
    for (entity, mesh, transform, material) in &terrain {
        let on_terrain = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|node| names.get(node).is_ok_and(|name| name.as_str() == "Terrain"));
        if !on_terrain {
            continue;
        }
        let (Some(mesh), Some(material)) = (meshes.get(&mesh.0), materials.get(&material.0)) else {
            return;
        };
        let Some(handle) = &material.base.base_color_texture else {
            continue;
        };
        let Some(image) = images.get(handle) else {
            return;
        };
        seen += 1;
        let Some(data) = &image.data else {
            continue;
        };
        if !has_grass(grass_share(data)) {
            continue;
        }
        let tile = tiles.len();
        tiles.push(handle.clone());
        patches.extend(terrain_patches(mesh, transform, tile));
    }
    if seen == 0 {
        return;
    }
    let mut buckets = HashMap::<IVec2, Vec<u32>>::new();
    for (index, patch) in patches.iter().enumerate() {
        buckets
            .entry(chunk_of(patch.centre()))
            .or_default()
            .push(index as u32);
    }
    let (low, high) = patches
        .iter()
        .fold((Vec3::MAX, Vec3::MIN), |(low, high), patch| {
            patch
                .positions
                .iter()
                .fold((low, high), |(low, high), point| {
                    (low.min(*point), high.max(*point))
                })
        });
    info!(
        "grass: {} triangles on {} tile images, from {low} to {high}",
        patches.len(),
        tiles.len()
    );
    field.tiles = tiles;
    field.patches = patches;
    field.buckets = buckets;
    field.indexed = true;
}

/// The triangles of a terrain mesh in world space with their texture coordinates.
fn terrain_patches(mesh: &Mesh, transform: &GlobalTransform, tile: usize) -> Vec<Patch> {
    let Some(positions) = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|values| values.as_float3())
    else {
        return Vec::new();
    };
    let Some(uvs) = mesh
        .attribute(Mesh::ATTRIBUTE_UV_0)
        .and_then(|values| match values {
            bevy::mesh::VertexAttributeValues::Float32x2(uvs) => Some(uvs),
            _ => None,
        })
    else {
        return Vec::new();
    };
    let indices: Vec<usize> = match mesh.indices() {
        Some(indices) => indices.iter().collect(),
        None => (0..positions.len()).collect(),
    };
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|triangle| {
            let corner = |slot: usize| -> Option<(Vec3, Vec2)> {
                let index = triangle[slot];
                Some((
                    transform.transform_point(Vec3::from(*positions.get(index)?)),
                    Vec2::from(*uvs.get(index)?),
                ))
            };
            let (a, b, c) = (corner(0)?, corner(1)?, corner(2)?);
            Some(Patch {
                positions: [a.0, b.0, c.0],
                uvs: [a.1, b.1, c.1],
                tile,
            })
        })
        .collect()
}

/// Grows the chunks near the camera and drops the ones it has left.
#[allow(clippy::too_many_arguments)] // Bevy system parameters.
fn grow(
    mut commands: Commands,
    mut field: ResMut<GrassField>,
    cameras: Query<&GlobalTransform, With<ViewCamera>>,
    images: Res<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    (mut standard, mut leaves): (
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<GrassMaterial>>,
    ),
    (fidelity, ray_tracing, scene_fog, effects): (
        Res<Fidelity>,
        Option<Res<super::RayTracing>>,
        Res<SceneFog>,
        Res<ModernEffects>,
    ),
) {
    let Some(camera) = cameras.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    if !field.indexed || field.patches.is_empty() {
        return;
    }
    let look = if super::ray_tracing::traced_if_built(&fidelity, ray_tracing.as_deref()) {
        Look::Traced
    } else {
        Look::Raster
    };
    if field.look != Some(look) {
        for (_, (chunk, _)) in field.chunks.drain().collect::<Vec<_>>() {
            commands.entity(chunk).despawn();
        }
        field.look = Some(look);
    }
    // The fog of the world, which the scene materials carry too; nothing is drawn before the scene
    // has set it.
    let Some(fog) = scene_fog.0.clone() else {
        return;
    };
    let extension = GrassExtension {
        fog,
        wind: if effects.foliage { WIND_BEND } else { 0.0 },
    };
    let leaf = field
        .leaf
        .get_or_insert_with(|| {
            leaves.add(GrassMaterial {
                base: StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 1.0,
                    reflectance: 0.0,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                },
                extension: extension.clone(),
            })
        })
        .clone();
    if leaves
        .get(&leaf)
        .is_some_and(|leaf| leaf.extension != extension)
        && let Some(mut leaf) = leaves.get_mut(&leaf)
    {
        leaf.extension = extension;
    }
    let near = |chunk: IVec2| {
        let centre = (chunk.as_vec2() + Vec2::splat(0.5)) * CHUNK;
        centre.distance(Vec2::new(camera.x, camera.z))
    };
    let stale: Vec<IVec2> = field
        .chunks
        .iter()
        .filter(|(chunk, (_, band))| !keeps(*band, near(**chunk)))
        .map(|(chunk, _)| *chunk)
        .collect();
    for chunk in stale {
        if let Some((entity, _)) = field.chunks.remove(&chunk) {
            commands.entity(entity).despawn();
        }
    }
    let mut wanted: Vec<IVec2> = field
        .buckets
        .keys()
        .copied()
        .filter(|chunk| !field.chunks.contains_key(chunk) && band_of(near(*chunk)).is_some())
        .collect();
    wanted.sort_by(|a, b| near(*a).total_cmp(&near(*b)));
    let bounce = field
        .bounce
        .get_or_insert_with(|| {
            standard.add(StandardMaterial {
                base_color: Color::srgb(0.25, 0.38, 0.12),
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                double_sided: true,
                cull_mode: None,
                opaque_render_method: OpaqueRendererMethod::Deferred,
                ..default()
            })
        })
        .clone();
    for chunk in wanted.into_iter().take(CHUNKS_PER_FRAME) {
        let Some(band) = band_of(near(chunk)) else {
            continue;
        };
        let patches: Vec<(u32, Patch)> = field.buckets[&chunk]
            .iter()
            .map(|&index| (index, field.patches[index as usize]))
            .collect();
        let sample = |tile: usize, uv: Vec2| texel(&images, field.tiles.get(tile)?, uv);
        let entity = match blades(&patches, &sample, &BANDS[band]) {
            Some(mesh) => {
                debug!(
                    "grass chunk {chunk} band {band}: {} blades",
                    mesh.count_vertices() / 3
                );
                let traced = (look == Look::Traced && band < TRACED_BANDS)
                    .then(|| traceable(&mesh))
                    .flatten();
                #[cfg_attr(not(feature = "raytracing"), allow(unused_mut))]
                let mut chunk = commands.spawn((
                    GrassChunk,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(leaf.clone()),
                    NotShadowCaster,
                ));
                #[cfg(feature = "raytracing")]
                if let Some(traced) = traced {
                    let traced = meshes.add(traced);
                    chunk.with_children(|chunk| {
                        chunk.spawn((
                            bevy::solari::prelude::RaytracingMesh3d(traced),
                            MeshMaterial3d(bounce.clone()),
                        ));
                    });
                }
                #[cfg(not(feature = "raytracing"))]
                let _ = (traced, &bounce);
                chunk.id()
            }
            // A chunk of tiles whose texels held no grass keeps an empty entity, so it is not
            // tried again every frame.
            None => commands.spawn(GrassChunk).id(),
        };
        field.chunks.insert(chunk, (entity, band));
    }
}

/// One blade in this many is in the ray-traced scene. Traced, every blade of a dense meadow shadows
/// its neighbours and the blades come out black; a sparse stand-in still lets the meadow shade
/// and bounce light onto the ground, and the drawn blades are lit by it.
const TRACED_EVERY: usize = 6;

/// A sparse share of the blades in the layout Solari traces: position, normal, UV 0 and tangent,
/// as indexed triangles.
fn traceable(blades: &Mesh) -> Option<Mesh> {
    fn every<T: Copy>(values: &[T]) -> Vec<T> {
        values
            .chunks(3)
            .step_by(TRACED_EVERY)
            .flatten()
            .copied()
            .collect()
    }
    let positions = every(blades.attribute(Mesh::ATTRIBUTE_POSITION)?.as_float3()?);
    let normals = every(blades.attribute(Mesh::ATTRIBUTE_NORMAL)?.as_float3()?);
    let uvs = match blades.attribute(Mesh::ATTRIBUTE_UV_0)? {
        bevy::mesh::VertexAttributeValues::Float32x2(uvs) => every(uvs),
        _ => return None,
    };
    let count = positions.len();
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0; 4]; count])
        .with_inserted_indices(Indices::U32((0..count as u32).collect())),
    )
}

/// The texel of a tile image at texture coordinates `uv`, nearest, wrapping.
fn texel(images: &Assets<Image>, handle: &Handle<Image>, uv: Vec2) -> Option<[u8; 4]> {
    let image = images.get(handle)?;
    let size = image.size();
    let x = (uv.x.rem_euclid(1.0) * size.x as f32) as u32;
    let y = (uv.y.rem_euclid(1.0) * size.y as f32) as u32;
    let color = image
        .get_color_at(x.min(size.x - 1), y.min(size.y - 1))
        .ok()?;
    let [red, green, blue, alpha] = Srgba::from(color).to_u8_array();
    Some([red, green, blue, alpha])
}

/// Drops all grass once it is off.
fn clear(mut commands: Commands, mut field: ResMut<GrassField>) {
    for (_, (chunk, _)) in field.chunks.drain().collect::<Vec<_>>() {
        commands.entity(chunk).despawn();
    }
}

/// A well-mixed 64-bit hash, so blade placement needs no random state.
fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

/// A hash as a number in `0.0..1.0`.
fn unit(value: u64) -> f32 {
    (mix(value) >> 40) as f32 / (1u64 << 24) as f32
}

/// The blades of `patches`, merged into one mesh; `None` when no texel under a blade is grass.
/// `sample` reads a tile image at texture coordinates. Each patch grows the same blades every
/// time, keyed by its index, as `band` thins and widens them.
fn blades(
    patches: &[(u32, Patch)],
    sample: &dyn Fn(usize, Vec2) -> Option<[u8; 4]>,
    band: &Band,
) -> Option<Mesh> {
    let mut positions = Vec::<[f32; 3]>::new();
    let mut normals = Vec::<[f32; 3]>::new();
    let mut colors = Vec::<[f32; 4]>::new();
    let mut uvs = Vec::<[f32; 2]>::new();
    for (index, patch) in patches {
        let seed = u64::from(*index) << 20;
        let wanted = patch.area() * band.density;
        let count = wanted.floor() as u64 + u64::from(unit(seed) < wanted.fract());
        for blade in 0..count {
            let key = seed | (blade + 1);
            // A uniform point of the triangle.
            let (r1, r2) = (unit(key ^ 0x1111).sqrt(), unit(key ^ 0x2222));
            let weights = Vec3::new(1.0 - r1, r1 * (1.0 - r2), r1 * r2);
            let base = patch.positions[0] * weights.x
                + patch.positions[1] * weights.y
                + patch.positions[2] * weights.z;
            let uv = patch.uvs[0] * weights.x + patch.uvs[1] * weights.y + patch.uvs[2] * weights.z;
            let Some(texel) = sample(patch.tile, uv).filter(|texel| is_grass(*texel)) else {
                continue;
            };
            let ground = Color::srgb_u8(texel[0], texel[1], texel[2]).to_linear();
            let yaw = unit(key ^ 0x3333) * std::f32::consts::TAU;
            let across = Vec3::new(yaw.cos(), 0.0, yaw.sin());
            let lean = Vec3::new(unit(key ^ 0x4444) - 0.5, 0.0, unit(key ^ 0x5555) - 0.5)
                * (BLADE_HEIGHT * band.height * 0.5);
            let height = BLADE_HEIGHT * band.height * (0.7 + 0.6 * unit(key ^ 0x6666));
            let width = BLADE_WIDTH * band.width * (0.8 + 0.4 * unit(key ^ 0x7777));
            let tip = base + Vec3::Y * height + lean;
            let facing = across.cross(Vec3::Y).normalize_or_zero();
            // UV 0 carries what the wind needs: the blade's own phase, and 0 at its base to 1 at
            // its tip.
            let phase = unit(key ^ 0x8888);
            let tone = 1.0 + TONE_SPREAD * (2.0 * unit(key ^ 0x9999) - 1.0);
            for (point, shade, tip_weight) in [
                (base - across * width * 0.5, BASE_SHADE, 0.0),
                (base + across * width * 0.5, BASE_SHADE, 0.0),
                (tip, TIP_SHADE, 1.0),
            ] {
                positions.push(point.to_array());
                uvs.push([phase, tip_weight]);
                // Lit as the ground is, with the blade's own facing mixed in.
                normals.push((Vec3::Y + facing * 0.3).normalize().to_array());
                colors.push([
                    ground.red * shade * tone,
                    ground.green * shade * tone,
                    ground.blue * shade * tone,
                    1.0,
                ]);
            }
        }
    }
    if positions.is_empty() {
        return None;
    }
    let indices = (0..positions.len() as u32).collect();
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREEN: [u8; 4] = [40, 120, 50, 255];
    const SAND: [u8; 4] = [200, 160, 110, 255];

    /// A flat 10 by 10 unit patch (two triangles) with its tile texture across it.
    fn square() -> Vec<(u32, Patch)> {
        let corner = |x: f32, z: f32| (Vec3::new(x, 0.0, z), Vec2::new(x / 10.0, z / 10.0));
        let (a, b, c, d) = (
            corner(0.0, 0.0),
            corner(10.0, 0.0),
            corner(10.0, 10.0),
            corner(0.0, 10.0),
        );
        let triangle = |(p, q, r): ((Vec3, Vec2), (Vec3, Vec2), (Vec3, Vec2))| Patch {
            positions: [p.0, q.0, r.0],
            uvs: [p.1, q.1, r.1],
            tile: 0,
        };
        vec![(0, triangle((a, b, c))), (1, triangle((a, c, d)))]
    }

    fn positions(mesh: &Mesh) -> Vec<Vec3> {
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|values| values.as_float3())
            .expect("positions")
            .iter()
            .map(|point| Vec3::from(*point))
            .collect()
    }

    #[test]
    fn grass_grows_in_whole_blades_over_the_patch() {
        let mesh = blades(&square(), &|_, _| Some(GREEN), &BANDS[0]).expect("blades");
        let points = positions(&mesh);
        // 100 square units at the near band's density, give or take the rounding of each patch.
        let blades = points.len() / 3;
        assert_eq!(points.len() % 3, 0);
        assert!((4990..=5010).contains(&blades), "{blades} blades");
        for point in points {
            assert!(
                (-BLADE_HEIGHT..=10.0 + BLADE_HEIGHT).contains(&point.x)
                    && (-BLADE_HEIGHT..=10.0 + BLADE_HEIGHT).contains(&point.z)
                    && (0.0..=BLADE_HEIGHT * 1.3 + 0.01).contains(&point.y),
                "{point}"
            );
        }
    }

    #[test]
    fn blades_grow_only_where_the_texel_is_grass() {
        // Grass on the left half of the tile, sand on the right.
        let mesh = blades(
            &square(),
            &|_, uv| Some(if uv.x < 0.5 { GREEN } else { SAND }),
            &BANDS[0],
        )
        .expect("blades");
        for point in positions(&mesh).chunks(3).map(|blade| blade[0]) {
            assert!(point.x <= 5.0 + BLADE_WIDTH, "{point}");
        }
        assert!(blades(&square(), &|_, _| Some(SAND), &BANDS[0]).is_none());
        assert!(blades(&square(), &|_, _| None, &BANDS[0]).is_none());
    }

    #[test]
    fn a_patch_grows_the_same_blades_each_time() {
        let sample = |_: usize, _: Vec2| Some(GREEN);
        let first = positions(&blades(&square(), &sample, &BANDS[0]).expect("blades"));
        let second = positions(&blades(&square(), &sample, &BANDS[0]).expect("blades"));
        assert_eq!(first, second);
    }

    #[test]
    fn blades_take_the_colour_of_the_ground() {
        let mesh = blades(&square(), &|_, _| Some(GREEN), &BANDS[0]).expect("blades");
        let colors = mesh.attribute(Mesh::ATTRIBUTE_COLOR).expect("colours");
        let bevy::mesh::VertexAttributeValues::Float32x4(colors) = colors else {
            panic!("colours are float4");
        };
        let ground = Color::srgb_u8(GREEN[0], GREEN[1], GREEN[2]).to_linear();
        // A base is darker than the ground, a tip brighter, each by the blade's own tone.
        let tone = colors[0][1] / (ground.green * BASE_SHADE);
        assert!(
            (1.0 - TONE_SPREAD..=1.0 + TONE_SPREAD).contains(&tone),
            "{tone}"
        );
        assert!((colors[2][1] - ground.green * TIP_SHADE * tone).abs() < 1e-5);
    }

    #[test]
    fn blades_carry_their_phase_and_tip_weight_for_the_wind() {
        let mesh = blades(&square(), &|_, _| Some(GREEN), &BANDS[0]).expect("blades");
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("UV 0 is float2");
        };
        for blade in uvs.chunks(3) {
            // The base corners stay planted, the tip moves, and the three share one phase.
            assert_eq!([blade[0][1], blade[1][1], blade[2][1]], [0.0, 0.0, 1.0]);
            assert!(blade.iter().all(|uv| uv[0] == blade[0][0]));
            assert!((0.0..1.0).contains(&blade[0][0]));
        }
    }

    #[test]
    fn traced_blades_have_the_layout_solari_accepts() {
        let mesh = blades(&square(), &|_, _| Some(GREEN), &BANDS[0]).expect("blades");
        let traced = traceable(&mesh).expect("traced blades");
        let names: std::collections::BTreeSet<_> = traced
            .attributes()
            .map(|(attribute, _)| attribute.name)
            .collect();
        assert_eq!(
            names,
            [
                "Vertex_Normal",
                "Vertex_Position",
                "Vertex_Tangent",
                "Vertex_Uv"
            ]
            .into_iter()
            .collect()
        );
        // A sparse share, whole blades.
        assert_eq!(traced.count_vertices() % 3, 0);
        assert!(traced.count_vertices() * TRACED_EVERY <= mesh.count_vertices() + 3 * TRACED_EVERY);
        assert!(traced.count_vertices() > 0);
    }

    #[test]
    fn farther_bands_are_sparser_and_wider() {
        let blades_in = |band: &Band| {
            positions(&blades(&square(), &|_, _| Some(GREEN), band).expect("blades")).len() / 3
        };
        assert!(blades_in(&BANDS[0]) > blades_in(&BANDS[1]));
        assert!(blades_in(&BANDS[1]) > blades_in(&BANDS[2]));
        assert!(
            BANDS
                .windows(2)
                .all(|pair| { pair[0].reach < pair[1].reach && pair[0].width <= pair[1].width })
        );
    }

    #[test]
    fn a_chunk_keeps_its_band_a_little_past_the_edges() {
        assert_eq!(band_of(10.0), Some(0));
        assert_eq!(band_of(BANDS[0].reach + 1.0), Some(1));
        assert_eq!(band_of(BANDS[2].reach + 1.0), None);
        assert!(keeps(0, BANDS[0].reach + BAND_MARGIN - 1.0));
        assert!(!keeps(0, BANDS[0].reach + BAND_MARGIN + 1.0));
        assert!(keeps(1, BANDS[0].reach - BAND_MARGIN + 1.0));
        assert!(!keeps(1, BANDS[0].reach - BAND_MARGIN - 1.0));
    }

    #[test]
    fn chunks_tile_the_horizontal_plane() {
        assert_eq!(chunk_of(Vec3::new(0.0, 5.0, 0.0)), IVec2::ZERO);
        assert_eq!(
            chunk_of(Vec3::new(-0.1, 0.0, CHUNK + 0.1)),
            IVec2::new(-1, 1)
        );
    }
}
