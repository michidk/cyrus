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
                    && texture.rgba.len() == (texture.width * texture.height * 4) as usize,
                "splash: a texture's pixel count does not match its size"
            );
        }
        for draw in self.frames.iter().flatten().chain(&self.final_draws) {
            ensure!(
                draw.mesh < self.meshes.len() && draw.texture < self.textures.len(),
                "splash: a draw uses a missing mesh or texture"
            );
        }
        Ok(())
    }
}
