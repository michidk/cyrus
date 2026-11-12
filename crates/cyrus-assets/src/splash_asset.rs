//! The installed Windows 3dfx splash's meshes, textures and authored draws, exported once so the
//! game can draw the animation live every frame, as the original's Glide splash does.
use bevy::{asset::Asset, reflect::TypePath};
use serde::{Deserialize, Serialize};

#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct SplashAsset {
    pub frames_per_second: u32,
    pub meshes: Vec<SplashMesh>,
    /// Whether each mesh takes its texture coordinates from a reflection of the light sphere.
    pub sphere_mapped: Vec<bool>,
    pub textures: Vec<SplashTexture>,
    /// The authored frames, each the draws that make it up.
    pub frames: Vec<Vec<SplashDraw>>,
    /// The draws the splash holds while it fades out.
    pub final_draws: Vec<SplashDraw>,
}

#[derive(Serialize, Deserialize)]
pub struct SplashMesh {
    pub vertices: Vec<SplashVertex>,
    pub faces: Vec<[usize; 3]>,
}

#[derive(Serialize, Deserialize)]
pub struct SplashVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Texture coordinates in a 256 by 256 space.
    pub uv: [f32; 2],
}

#[derive(Serialize, Deserialize)]
pub struct SplashTexture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct SplashDraw {
    pub mesh: usize,
    pub texture: usize,
    /// Column-major object transform and camera.
    pub transform: [f32; 16],
    pub camera: [f32; 16],
    pub horizontal_fov: f32,
    pub light_color: [f32; 3],
    pub light_position: [f32; 3],
    pub alpha: u8,
}

impl SplashAsset {
    /// Checks every index the draws and faces use, so a bad export fails here, not in the game.
    pub fn validate(&self) -> anyhow::Result<()> {
        use anyhow::ensure;
        ensure!(self.frames_per_second > 0, "splash: the frame rate is zero");
        ensure!(
            self.sphere_mapped.len() == self.meshes.len(),
            "splash: sphere-mapping flags do not match the meshes"
        );
        for (index, mesh) in self.meshes.iter().enumerate() {
            ensure!(
                mesh.faces
                    .iter()
                    .flatten()
                    .all(|vertex| *vertex < mesh.vertices.len()),
                "splash: mesh {index} has a face that uses a missing vertex"
            );
        }
        for texture in &self.textures {
            ensure!(
                texture.width > 0
                    && texture.height > 0
                    && texture.rgba.len() as u64
                        == u64::from(texture.width) * u64::from(texture.height) * 4,
                "splash: a texture's pixel count does not match its size"
            );
        }
        for draw in self.frames.iter().flatten().chain(&self.final_draws) {
            ensure!(
                draw.mesh < self.meshes.len() && draw.texture < self.textures.len(),
                "splash: a draw uses a missing mesh or texture"
            );
            // The installed splash draws every face opaque; blending is not implemented.
            ensure!(
                draw.alpha == 255,
                "splash: a translucent draw (alpha {}) is not supported",
                draw.alpha
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset() -> SplashAsset {
        SplashAsset {
            frames_per_second: 30,
            meshes: vec![SplashMesh {
                vertices: vec![SplashVertex {
                    position: [0.0; 3],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0; 2],
                }],
                faces: vec![[0, 0, 0]],
            }],
            sphere_mapped: vec![false],
            textures: vec![SplashTexture {
                width: 1,
                height: 1,
                rgba: vec![255; 4],
            }],
            frames: vec![vec![draw(255)]],
            final_draws: vec![draw(255)],
        }
    }

    fn draw(alpha: u8) -> SplashDraw {
        SplashDraw {
            mesh: 0,
            texture: 0,
            transform: [0.0; 16],
            camera: [0.0; 16],
            horizontal_fov: 1.0,
            light_color: [0.0; 3],
            light_position: [0.0; 3],
            alpha,
        }
    }

    #[test]
    fn a_consistent_asset_validates() {
        asset().validate().unwrap();
    }

    #[test]
    fn a_zero_frame_rate_is_rejected_before_the_fade_divides_by_it() {
        let mut asset = asset();
        asset.frames_per_second = 0;
        assert!(asset.validate().is_err());
    }

    #[test]
    fn a_texture_size_that_overflows_32_bits_does_not_match_a_short_buffer() {
        let mut asset = asset();
        // 65536 * 65536 * 4 wraps to zero in 32 bits, which an empty buffer would then match.
        asset.textures[0] = SplashTexture {
            width: 65536,
            height: 65536,
            rgba: Vec::new(),
        };
        assert!(asset.validate().is_err());
    }

    #[test]
    fn a_translucent_draw_is_rejected_instead_of_drawn_opaque() {
        let mut asset = asset();
        asset.frames[0][0].alpha = 128;
        assert!(format!("{:#}", asset.validate().unwrap_err()).contains("translucent"));
    }
}
