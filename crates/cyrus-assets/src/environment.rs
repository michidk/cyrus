//! Validated world environment records.
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use serde::{Deserialize, Serialize};
#[derive(Asset, TypePath, Serialize, Deserialize)]
/// Per-world rendering inputs extracted from WORLD.INI and the installed assets.
pub struct Environment {
    /// SYSTEM.INI 256-size and 128-size texture reduction factors, in that order.
    pub texture_scale: [i32; 2],
    /// Decoded Glide BSI sky and its plane parameters, if the world declares a sky.
    pub sky: Option<SkyImage>,
    /// Glide fog and clear color as encoded RGB bytes.
    pub background: [u8; 3],
    /// Scene near/far clipping distances in engine units, from the install.
    pub clipping: [f32; 2],
    /// The 64-entry Glide scene fog density table, packed in groups of four.
    pub fog_table: [[u8; 4]; 16],
    /// Terrain water mask and original wave parameters, if declared.
    pub water: Option<Water>,
    /// Original per-world terrain shading inputs, absent for worlds without terrain.
    pub terrain_lighting: Option<TerrainLighting>,
    /// Ambient and directional model inputs, including worlds without terrain.
    pub model_lighting: ModelLighting,
    /// Authored static placements and admitted map lights for the world-entry bake.
    pub prelighting: Prelighting,
    /// Authored kind-6 runtime lights in file order, before active-region selection.
    pub object_lights: Vec<LocalLight>,
}

#[derive(Default, Serialize, Deserialize)]
/// Static lighting inputs in original engine coordinates, per preservation's model-lighting rules.
pub struct Prelighting {
    /// Exported GLTF node name to original MPSO placement.
    pub instances: std::collections::BTreeMap<String, StaticLightingInstance>,
    /// Active map lights admitted for this world by the installed configuration.
    pub lights: Vec<LocalLight>,
}

#[derive(Serialize, Deserialize)]
/// Original placement retained before GLTF transform decomposition.
pub struct StaticLightingInstance {
    /// Engine coordinates, without the exported scene-origin offset.
    pub position: [i32; 3],
    /// Row-major signed Q4.28 matrix; preserve authored scale.
    pub matrix: [i32; 9],
}

#[derive(Serialize, Deserialize)]
/// Signed authored light inputs, without conversion to a physical point light.
pub struct LocalLight {
    /// Engine world coordinates.
    pub position: [i32; 3],
    /// Radius in engine units; negative source values remain meaningful.
    pub radius: i16,
    /// Signed encoded RGB strengths.
    pub rgb: [i16; 3],
}

#[derive(Serialize, Deserialize)]
/// Opaque BSI sky pixels and WORLD.INI projection inputs, per preservation's sky docs.
pub struct SkyImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes per pixel, exactly `width * height * 4` bytes.
    pub rgba: Vec<u8>,
    /// Absolute engine Y coordinate, positive down.
    pub level: f32,
    /// Half-span of plane texture coordinates in texels.
    pub scale: f32,
    /// Raw WORLD.INI scrolling speed; one unit is 1/16 texel per BIOS tick.
    pub speed: u8,
    /// Optional camera-facing sun, using the BSI bytes as opacity.
    pub sun: Option<SunImage>,
}

#[derive(Serialize, Deserialize)]
/// Glide sun billboard inputs, per preservation's sky documentation.
pub struct SunImage {
    /// Texture dimensions in pixels.
    pub size: [u32; 2],
    /// One opacity byte per pixel; no palette conversion.
    pub alpha: Vec<u8>,
    /// Camera-relative center in engine coordinates, positive Y down.
    pub center: [f32; 3],
    /// Billboard half-width and half-height in engine units.
    pub half_size: [f32; 2],
    /// Encoded RGB tint.
    pub color: [u8; 3],
}

#[derive(Serialize, Deserialize)]
/// Wave inputs from WORLD.INI and the WLD texture-selection grid.
pub struct Water {
    /// Raw amplitude, speed, and spatial frequency, in INI order.
    pub parameters: [f32; 3],
    /// Number of vertices along each side of the square terrain grid.
    pub side: usize,
    /// Row-major mask of selected wave vertices, per preservation water docs.
    pub animated_vertices: Vec<bool>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
/// Model RGB strengths and direction in exported axes, per preservation's model-lighting rules.
pub struct ModelLighting {
    /// Ambient strengths in encoded RGB units.
    pub ambient: [f32; 3],
    /// Directional strengths in encoded RGB units.
    pub directional: [f32; 3],
    /// Normalized light direction, or zero when disabled.
    pub direction: [f32; 3],
}

impl Environment {
    pub fn validate(&self) -> anyhow::Result<()> {
        let lighting = &self.model_lighting;
        anyhow::ensure!(
            lighting
                .ambient
                .iter()
                .chain(&lighting.directional)
                .chain(&lighting.direction)
                .all(|v| v.is_finite())
                && (Vec3::from(lighting.direction) == Vec3::ZERO
                    || (Vec3::from(lighting.direction).length_squared() - 1.0).abs() < 0.00001),
            "invalid model lighting strengths or direction"
        );
        anyhow::ensure!(
            self.clipping.iter().all(|value| value.is_finite())
                && self.clipping[0] > 0.0
                && self.clipping[1] > self.clipping[0],
            "invalid scene clipping distances"
        );
        if let Some(sky) = &self.sky {
            anyhow::ensure!(
                sky.width > 0 && sky.height > 0 && sky.width <= 8192 && sky.height <= 8192,
                "invalid sky dimensions"
            );
            anyhow::ensure!(
                sky.rgba.len() == sky.width as usize * sky.height as usize * 4,
                "invalid sky pixels"
            );
            anyhow::ensure!(
                sky.level.is_finite() && sky.scale.is_finite() && sky.scale != 0.0,
                "invalid sky plane parameters"
            );
        }
        if let Some(sun) = self.sky.as_ref().and_then(|sky| sky.sun.as_ref()) {
            anyhow::ensure!(
                sun.size.iter().all(|&v| (1..=256).contains(&v))
                    && sun.alpha.len() == sun.size[0] as usize * sun.size[1] as usize,
                "invalid sun pixels"
            );
            anyhow::ensure!(
                sun.center.iter().all(|v| v.is_finite())
                    && sun.half_size.iter().all(|v| v.is_finite() && *v > 0.0),
                "invalid sun geometry"
            );
        }
        if let Some(light) = &self.terrain_lighting {
            anyhow::ensure!(
                (2..=256).contains(&light.side)
                    && light
                        .ambient
                        .iter()
                        .chain(&light.directional)
                        .chain(&light.direction)
                        .all(|v| v.is_finite())
                    && (Vec3::from(light.direction) == Vec3::ZERO
                        || (Vec3::from(light.direction).length_squared() - 1.0).abs() < 0.00001),
                "invalid terrain lighting grid, strengths or direction"
            );
            anyhow::ensure!(
                self.water
                    .as_ref()
                    .is_none_or(|water| water.side == light.side),
                "terrain lighting/water grid mismatch"
            );
        }
        if let Some(water) = &self.water {
            anyhow::ensure!(
                (2..=256).contains(&water.side)
                    && water.animated_vertices.len() == water.side * water.side,
                "invalid water grid"
            );
            anyhow::ensure!(
                water.parameters.iter().all(|v| v.is_finite())
                    && water.parameters[1..].iter().all(|v| v.fract() == 0.0
                        && *v >= i32::MIN as f32
                        && *v <= i32::MAX as f32),
                "invalid wave parameters"
            );
        }
        Ok(())
    }
}

#[derive(Default, TypePath)]
pub struct EnvironmentLoader;
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

#[derive(Clone, Serialize, Deserialize)]
/// Terrain lighting in exported coordinates, shared by native and browser playback.
pub struct TerrainLighting {
    /// Width of the square WLD vertex grid.
    pub side: usize,
    /// Ambient strengths in encoded RGB units, before clamping.
    pub ambient: [f32; 3],
    /// Directional strengths in encoded RGB units, before clamping.
    pub directional: [f32; 3],
    /// Unit light direction after the engine-to-export `[-X,-Y,+Z]` conversion.
    pub direction: [f32; 3],
}

impl TerrainLighting {
    /// Encoded RGB divided by 255; preserves the supplied normal's magnitude.
    pub fn color(&self, normal: [f32; 3]) -> [f32; 4] {
        (Vec3::from(self.rgb(normal)) / 255.0)
            .extend(1.0)
            .to_array()
    }

    /// Encoded RGB in 0..=255 before clipping and raster setup, without normalization.
    pub fn rgb(&self, normal: [f32; 3]) -> [f32; 3] {
        let weight = (-Vec3::from(self.direction).dot(Vec3::from(normal))).max(0.0);
        (Vec3::from(self.ambient) + weight * Vec3::from(self.directional))
            .clamp(Vec3::ZERO, Vec3::splat(255.0))
            .to_array()
    }
}

pub fn water_vertices(textures: &[u8], side: usize) -> Vec<bool> {
    let mut vertices = vec![false; textures.len()];
    assert!(side >= 2 && textures.len() == side * side);
    for row in 1..side {
        for x in 1..side {
            let selected = x + row * side;
            let neighbours = [selected - 1, selected - side, selected - side - 1];
            vertices[selected] = matches!(textures[selected] & 63, 0 | 30 | 31)
                && neighbours
                    .iter()
                    .all(|&i| matches!(textures[i] & 63, 0 | 5 | 30 | 31));
        }
    }
    vertices
}
