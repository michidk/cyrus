//! Original Glide horizontal field of view, independent of viewport aspect ratio.
use bevy::{camera::CameraProjection, camera::SubCameraView, math::Vec3A, prelude::*};

/// Perspective depth/frustum behavior with preservation's Glide viewport scaling.
/// See <https://github.com/michidk/redguard-preservation/blob/6520206/docs/engine/projection.md>.
#[derive(Clone, Debug, Default)]
pub(crate) struct GameProjection(pub PerspectiveProjection);

impl CameraProjection for GameProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        self.0.get_clip_from_view()
    }

    fn get_clip_from_view_for_sub(&self, view: &SubCameraView) -> Mat4 {
        self.0.get_clip_from_view_for_sub(view)
    }

    fn update(&mut self, width: f32, height: f32) {
        self.0.update(width, height);
        self.0.fov = 2.0 * (0.788 / self.0.aspect_ratio).atan();
    }

    fn far(&self) -> f32 {
        self.0.far()
    }

    fn get_frustum_corners(&self, near: f32, far: f32) -> [Vec3A; 8] {
        self.0.get_frustum_corners(near, far)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_view_edges_survive_viewport_resize() {
        let mut projection = GameProjection::default();
        for (width, height) in [(640.0, 480.0), (1280.0, 720.0), (800.0, 600.0)] {
            projection.update(width, height);
            let matrix = projection.get_clip_from_view();
            for (point, expected) in [
                (
                    Vec3::new(0.0, 0.0, -100.0),
                    Vec2::new(width / 2.0, height / 2.0),
                ),
                (Vec3::new(78.8, 0.0, -100.0), Vec2::new(width, height / 2.0)),
                (Vec3::new(-78.8, 0.0, -100.0), Vec2::new(0.0, height / 2.0)),
                (
                    Vec3::new(0.0, 78.8, -100.0),
                    Vec2::new(width / 2.0, (height - width) / 2.0),
                ),
            ] {
                let ndc = matrix.project_point3(point);
                let screen = Vec2::new((ndc.x + 1.0) * width / 2.0, (1.0 - ndc.y) * height / 2.0);
                assert!(
                    screen.distance(expected) < 0.001,
                    "{screen:?} != {expected:?}"
                );
            }
        }
    }
}
