//! Camera-relative model geometry must not depend on the world's common origin.
use bevy::prelude::*;
use cyrus_render::camera::model_view_transform;

#[test]
fn common_world_translation_preserves_model_camera_coordinates() {
    let camera_rotation = Quat::from_euler(EulerRot::YXZ, 0.7, -0.3, 0.1);
    let model_rotation = Quat::from_rotation_y(-0.2);
    let camera_position = Vec3::new(3.0, 5.0, 7.0);
    let model_position = Vec3::new(11.0, 13.0, 17.0);
    let offset = Vec3::new(8192.0, -8192.0, 8192.0);
    let relative = |offset| {
        model_view_transform(
            Mat4::from_rotation_translation(camera_rotation, camera_position + offset),
            Mat4::from_scale_rotation_translation(
                Vec3::new(2.0, 0.5, 3.0),
                model_rotation,
                model_position + offset,
            ),
        )
    };
    let expected = relative(Vec3::ZERO);
    let shifted = relative(offset);
    for position in [Vec3::ZERO, Vec3::X, Vec3::new(-2.0, 4.0, 1.0)] {
        let expected = expected.transform_point3(position);
        let actual = shifted.transform_point3(position);
        assert!(
            actual.abs_diff_eq(expected, 0.000001),
            "{actual:?} != {expected:?}"
        );
    }
}
