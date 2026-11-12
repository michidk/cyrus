//! Converts the parsed Windows 3dfx splash into the asset the game draws live.
use crate::splash_asset::{SplashAsset, SplashDraw, SplashMesh, SplashTexture, SplashVertex};
use rgpre::import::{
    glide_splash::SplashMesh as SourceMesh,
    glide_splash_dll::{WindowsGlideSplash, WindowsSplashDraw},
};

fn mesh(source: &SourceMesh) -> SplashMesh {
    SplashMesh {
        vertices: source
            .vertices
            .iter()
            .map(|vertex| SplashVertex {
                position: vertex.position,
                normal: vertex.normal,
                uv: vertex.uv,
            })
            .collect(),
        faces: source.faces.iter().map(|face| face.vertices).collect(),
    }
}

fn draw(draw: &WindowsSplashDraw) -> SplashDraw {
    SplashDraw {
        mesh: draw.mesh,
        texture: draw.texture,
        transform: draw.transform,
        camera: draw.camera,
        horizontal_fov: draw.horizontal_fov,
        light_color: draw.light_color,
        light_position: draw.light_position,
        alpha: draw.alpha,
    }
}

pub(super) fn export(splash: &WindowsGlideSplash) -> SplashAsset {
    SplashAsset {
        frames_per_second: splash.frames_per_second,
        meshes: splash.meshes.iter().map(mesh).collect(),
        sphere_mapped: splash.sphere_mapped.clone(),
        textures: splash
            .textures
            .iter()
            .map(|texture| SplashTexture {
                width: texture.width,
                height: texture.height,
                rgba: texture.rgba.clone(),
            })
            .collect(),
        frames: splash
            .frames
            .iter()
            .map(|frame| frame.iter().map(draw).collect())
            .collect(),
        final_draws: splash.final_draws.iter().map(draw).collect(),
    }
}
