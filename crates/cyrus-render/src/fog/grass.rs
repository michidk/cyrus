//! Grass on the terrain's grass tiles.
//!
//! **Modern only**, a tuning choice. The original draws the terrain as flat textured tiles with
//! no blades. While `Area::Lighting` is Modern or ray tracing lights the scene, and the grass
//! effect is on, blades stand on the
//! terrain wherever its texture is grass: every terrain triangle whose tile image shows grass
//! (`cyrus_assets::terrain_grass`) is sampled at random points, a blade grows where the texel
//! there is grass, and takes that texel's colour, so the grass blends into the ground it grows
//! from and thins out where a tile blends into sand. Blades are merged into one mesh per square
//! chunk of the terrain near the camera, built a few per frame and dropped when the camera
//! leaves; placement is deterministic, so a chunk grows the same blades each time. Blades are
//! plain lit meshes: ray tracing shades them but does not trace them, and they cast no shadows.
use super::{Fidelity, ModernEffects, TerrainMaterial};
use crate::{camera::ViewCamera, environment::EnvironmentRequest};
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use cyrus_assets::terrain_grass::{grass_share, has_grass, is_grass};
use cyrus_game::fidelity::{Area, Mode};
use std::collections::HashMap;

/// Side of a chunk of blades, in world units.
const CHUNK: f32 = 24.0;
/// Chunks built per frame.
const CHUNKS_PER_FRAME: usize = 2;
/// Height and base width of a blade, in world units; the player is about three tall.
const BLADE_HEIGHT: f32 = 1.3;
const BLADE_WIDTH: f32 = 0.3;

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
        reach: 40.0,
        density: 20.0,
        width: 1.0,
        height: 1.0,
    },
    Band {
        reach: 90.0,
        density: 5.0,
        width: 2.0,
        height: 1.15,
    },
    Band {
        reach: 150.0,
        density: 1.2,
        width: 3.5,
        height: 1.3,
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
const BASE_SHADE: f32 = 0.55;
const TIP_SHADE: f32 = 1.1;

pub(super) struct GrassPlugin;

impl Plugin for GrassPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GrassField>().add_systems(
            Update,
            (
                (index_terrain, grow).chain().run_if(grassy),
                clear.run_if(not(grassy)),
            ),
        );
    }
}

/// Whether grass is on: the grass effect with Modern lighting, or with ray tracing, which lights
/// the scene in its place.
fn grassy(
    fidelity: Res<Fidelity>,
    ray_tracing: Option<Res<super::RayTracingSupport>>,
    effects: Option<Res<ModernEffects>>,
) -> bool {
    effects.is_some_and(|effects| effects.grass)
        && (fidelity.mode(Area::Lighting) == Mode::Modern
            || ray_tracing
                .is_some_and(|support| super::ray_tracing::ray_traced(&fidelity, &support)))
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
    material: Option<Handle<StandardMaterial>>,
}

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
            material: field.material.take(),
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
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(camera) = cameras.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    if !field.indexed || field.patches.is_empty() {
        return;
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
    let material = field
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                double_sided: true,
                cull_mode: None,
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
                commands
                    .spawn((
                        GrassChunk,
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(material.clone()),
                        NotShadowCaster,
                    ))
                    .id()
            }
            // A chunk of tiles whose texels held no grass keeps an empty entity, so it is not
            // tried again every frame.
            None => commands.spawn(GrassChunk).id(),
        };
        field.chunks.insert(chunk, (entity, band));
    }
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
            for (point, shade) in [
                (base - across * width * 0.5, BASE_SHADE),
                (base + across * width * 0.5, BASE_SHADE),
                (tip, TIP_SHADE),
            ] {
                positions.push(point.to_array());
                // Lit as the ground is, with the blade's own facing mixed in.
                normals.push((Vec3::Y + facing * 0.3).normalize().to_array());
                colors.push([
                    ground.red * shade,
                    ground.green * shade,
                    ground.blue * shade,
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
        assert!((1990..=2010).contains(&blades), "{blades} blades");
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
        // A base is darker than the ground, a tip brighter.
        assert!((colors[0][1] - ground.green * BASE_SHADE).abs() < 1e-5);
        assert!((colors[2][1] - ground.green * TIP_SHADE).abs() < 1e-5);
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
