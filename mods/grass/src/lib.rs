//! PR255 grass policy, entirely in a downloadable guest. No installed art ships here.
use cyrus_mod_sdk::{
    Cx, Material, Mesh, Mod, Persist, Pixels, Region, TerrainRevision, TerrainTriangle, TextureId,
    Vec3, Vertex,
};
use std::collections::BTreeMap;
const CHUNK: f32 = 24.0;
const HEIGHT: f32 = 0.75;
const WIDTH: f32 = 0.13;
struct Band {
    reach: f32,
    density: f32,
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
fn band_of(distance: f32) -> Option<usize> {
    BANDS.iter().position(|b| distance <= b.reach)
}
fn keeps(band: usize, distance: f32) -> bool {
    distance <= BANDS[band].reach + 6.0 && (band == 0 || distance >= BANDS[band - 1].reach - 6.0)
}
fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}
fn unit(value: u64) -> f32 {
    (mix(value) >> 40) as f32 / (1u64 << 24) as f32
}
fn is_grass([r, g, b, a]: [u8; 4]) -> bool {
    a != 0 && g > r && g > b
}
fn has_grass(image: &Pixels) -> bool {
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|v| is_grass([v[0], v[1], v[2], v[3]]))
        .count() as f32
        / (image.width * image.height) as f32
        >= 0.01
}
fn linear(byte: u8) -> f32 {
    let value = f32::from(byte) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn blades(
    patches: &[TerrainTriangle],
    tiles: &BTreeMap<TextureId, Pixels>,
    band: &Band,
    density: f32,
) -> Mesh {
    let mut mesh = Mesh::default();
    let grassy: BTreeMap<_, _> = tiles
        .iter()
        .filter(|(_, image)| has_grass(image))
        .map(|(id, image)| (*id, image))
        .collect();
    for patch in patches {
        let Some(image) = grassy.get(&patch.texture) else {
            continue;
        };
        append_triangle(
            &mut mesh,
            patch.id,
            patch.positions,
            patch.uvs,
            &|uv| image.sample(uv).unwrap_or_default(),
            band,
            density,
        );
    }
    mesh
}
fn append_triangle(
    mesh: &mut Mesh,
    id: u32,
    positions: [Vec3; 3],
    uvs: [[f32; 2]; 3],
    sample: &impl Fn([f32; 2]) -> [u8; 4],
    band: &Band,
    density: f32,
) {
    let [a, b, c] = positions;
    let seed = u64::from(id) << 20;
    let wanted = (b - a).cross(c - a).length() * 0.5 * band.density * density;
    let count = wanted.floor() as u64 + u64::from(unit(seed) < wanted.fract());
    for blade in 0..count {
        let key = seed | (blade + 1);
        let (r1, r2) = (unit(key ^ 0x1111).sqrt(), unit(key ^ 0x2222));
        let weights = [1.0 - r1, r1 * (1.0 - r2), r1 * r2];
        let base = a * weights[0] + b * weights[1] + c * weights[2];
        let uv = std::array::from_fn(|j| (0..3).map(|i| uvs[i][j] * weights[i]).sum());
        let texel = sample(uv);
        if !is_grass(texel) {
            continue;
        }
        let yaw = unit(key ^ 0x3333) * std::f32::consts::TAU;
        let across = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        let lean = Vec3::new(unit(key ^ 0x4444) - 0.5, 0.0, unit(key ^ 0x5555) - 0.5)
            * (HEIGHT * band.height * 0.5);
        let height = HEIGHT * band.height * (0.7 + 0.6 * unit(key ^ 0x6666));
        let width = WIDTH * band.width * (0.8 + 0.4 * unit(key ^ 0x7777));
        let tip = base + Vec3::UP * height + lean;
        let normal = (Vec3::UP + across.cross(Vec3::UP) * 0.3)
            .normalized()
            .unwrap_or(Vec3::UP);
        let phase = unit(key ^ 0x8888);
        let tone = 1.0 + 0.25 * (2.0 * unit(key ^ 0x9999) - 1.0);
        for (position, shade, weight) in [
            (base - across * (width * 0.5), 0.45, 0.0),
            (base + across * (width * 0.5), 0.45, 0.0),
            (tip, 1.0, 1.0),
        ] {
            mesh.indices.push(mesh.vertices.len() as u32);
            mesh.vertices.push(Vertex {
                position,
                normal,
                uv: [phase, weight],
                color: [
                    (linear(texel[0]) * shade * tone).min(1.0),
                    (linear(texel[1]) * shade * tone).min(1.0),
                    (linear(texel[2]) * shade * tone).min(1.0),
                    1.0,
                ],
            });
        }
    }
}

#[derive(Default)]
struct Chunks(Vec<(i32, i32, usize, bool)>);
impl Persist for Chunks {
    fn pack(&self) -> Vec<f32> {
        self.0
            .iter()
            .flat_map(|&(x, z, b, empty)| {
                [x as f32, z as f32, b as f32, if empty { 1.0 } else { 0.0 }]
            })
            .collect()
    }
    fn unpack(v: &[f32]) -> Option<Self> {
        v.len().is_multiple_of(4).then(|| {
            Self(
                v.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|v| (v[0] as i32, v[1] as i32, v[2] as usize, v[3] != 0.0))
                    .filter(|v| v.2 < 3)
                    .collect(),
            )
        })
    }
}
fn key(x: i32, z: i32) -> String {
    format!("chunk/{x}/{z}")
}
struct Grass;
impl Mod for Grass {
    const ID: &'static str = "grass";
    fn frame(cx: &Cx) {
        let (Some(revision), Some(generation), Some(camera)) = (
            cx.terrain.revision(),
            cx.geometry.generation(),
            cx.camera.pose(),
        ) else {
            return;
        };
        let density = cx.settings.number("density").clamp(0.1, 1.0);
        let wind = cx.settings.number("wind").clamp(0.0, 0.5);
        let mut chunks = cx.memory.load::<Chunks>("chunks").unwrap_or_default();
        let reset = cx.memory.load::<TerrainRevision>("revision") != Some(revision)
            || cx.memory.load::<[f32; 2]>("look") != Some([density, wind]);
        let distance = |x: i32, z: i32| {
            let dx = (x as f32 + 0.5) * CHUNK - camera.position.x;
            let dz = (z as f32 + 0.5) * CHUNK - camera.position.z;
            (dx * dx + dz * dz).sqrt()
        };
        chunks.0.retain(|&(x, z, b, empty)| {
            let retain =
                !reset && keeps(b, distance(x, z)) && (empty || cx.geometry.contains(&key(x, z)));
            if !retain {
                cx.geometry.remove(&key(x, z));
            }
            retain
        });
        let centre = (
            (camera.position.x / CHUNK).floor() as i32,
            (camera.position.z / CHUNK).floor() as i32,
        );
        let mut wanted = Vec::new();
        for x in centre.0 - 7..=centre.0 + 7 {
            for z in centre.1 - 7..=centre.1 + 7 {
                if !chunks.0.iter().any(|c| c.0 == x && c.1 == z)
                    && band_of(distance(x, z)).is_some()
                {
                    wanted.push((x, z));
                }
            }
        }
        wanted.sort_by(|a, b| distance(a.0, a.1).total_cmp(&distance(b.0, b.1)));
        let mut tiles = BTreeMap::new();
        for (x, z) in wanted.into_iter().take(2) {
            let region = Region {
                min: [x as f32 * CHUNK, z as f32 * CHUNK],
                max: [(x + 1) as f32 * CHUNK, (z + 1) as f32 * CHUNK],
            };
            let mut patches = Vec::new();
            let mut offset = 0;
            loop {
                let Ok(page) = cx.terrain.triangles(revision, region, offset) else {
                    return;
                };
                let done = page.len() < 512;
                offset += page.len() as u32;
                patches.extend(page);
                if done {
                    break;
                }
            }
            for patch in &patches {
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    tiles.entry(patch.texture)
                {
                    let Ok(image) = cx.terrain.pixels(revision, patch.texture) else {
                        return;
                    };
                    entry.insert(image);
                }
            }
            let band = band_of(distance(x, z)).unwrap_or(2);
            let mesh = blades(&patches, &tiles, &BANDS[band], density);
            let empty = mesh.vertices.is_empty();
            if empty
                || cx
                    .geometry
                    .replace(
                        &key(x, z),
                        generation,
                        &mesh,
                        Material {
                            parameters: [wind, 0.0, 0.0, 0.0],
                            bounds_padding: wind,
                        },
                    )
                    .is_ok()
            {
                chunks.0.push((x, z, band, empty));
            }
        }
        cx.memory.save("chunks", Some(&chunks));
        cx.memory.save("revision", Some(&revision));
        cx.memory.save("look", Some(&[density, wind]));
    }
}
cyrus_mod_sdk::export_mod!(Grass);

#[cfg(test)]
mod tests {
    use super::*;
    const GREEN: [u8; 4] = [40, 120, 50, 255];
    fn triangle(sample: impl Fn([f32; 2]) -> [u8; 4]) -> Mesh {
        let mut mesh = Mesh::default();
        append_triangle(
            &mut mesh,
            7,
            [Vec3::ZERO, Vec3::new(10., 0., 0.), Vec3::new(0., 0., 10.)],
            [[1., 0.], [1., 1.], [0., 0.]],
            &sample,
            &BANDS[0],
            1.0,
        );
        mesh
    }
    #[test]
    fn deterministic_blades_follow_rotated_texture_coordinates() {
        let first = triangle(|_| GREEN);
        let second = triangle(|_| GREEN);
        assert_eq!(first, second);
        assert_eq!(first.vertices.len(), 2500 * 3);
        // Rotated U is 1-z/10. Grass on U<0.5 must grow at z>5.
        let rotated = triangle(|uv| {
            if uv[0] < 0.5 {
                GREEN
            } else {
                [200, 160, 110, 255]
            }
        });
        assert!(!rotated.vertices.is_empty());
        for blade in rotated.vertices.as_chunks::<3>().0 {
            let root = (blade[0].position + blade[1].position) * 0.5;
            assert!(root.z > 5.0);
            assert_eq!(blade[0].uv[1], 0.0);
            assert_eq!(blade[2].uv[1], 1.0);
            assert!(blade[2].position.y > 0.5);
        }
        assert!(triangle(|_| [200, 160, 110, 255]).vertices.is_empty());
        assert!(triangle(|_| [40, 120, 50, 0]).vertices.is_empty());
        assert!(keeps(0, 35.0));
        assert!(!keeps(0, 37.0));
        assert!(band_of(151.0).is_none());
    }
}
