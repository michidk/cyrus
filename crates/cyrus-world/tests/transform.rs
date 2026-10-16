//! Public original-matrix contracts, independent of rendering and host FPU mode.
use cyrus_world::transform::{Matrix, rotation_sample, translate_forward};

#[test]
fn upright_translation_truncates_diagonal_motion_and_preserves_height() {
    assert_eq!(
        translate_forward([0, 513, 0], 256, 3).unwrap(),
        [-2, 513, 2]
    );
    assert_eq!(
        translate_forward([0, -513, 0], 256, -3).unwrap(),
        [2, -513, -2]
    );
    assert_eq!(
        translate_forward([0, 513, 0], 2304, 3).unwrap(),
        [-2, 513, 2]
    );
}

#[test]
fn upright_translation_stores_coordinates_and_sum_at_single_precision() {
    assert_eq!(
        translate_forward([16_777_217, 7, 16_777_216], 0, 1).unwrap(),
        [16_777_216, 7, 16_777_216]
    );
    assert!(translate_forward([0, 0, i32::MAX], 0, 1).is_err());
}

#[test]
fn identity_preserves_exact_q8_coordinates_and_rounds_input_storage() {
    assert_eq!(
        Matrix::IDENTITY.transform([256, -513, 0]).unwrap(),
        [256, -513, 0]
    );
    assert_eq!(
        Matrix::IDENTITY.transform([16_777_217, 0, 0]).unwrap(),
        [16_777_216, 0, 0]
    );
}

#[test]
fn final_integer_conversion_is_nearest_even_in_both_directions() {
    let half = Matrix([[0.5, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 0.0, 0.5]]);
    assert_eq!(half.transform([1, 3, 5]).unwrap(), [0, 2, 2]);
    assert_eq!(half.transform([-1, -3, -5]).unwrap(), [0, -2, -2]);
}

#[test]
fn ordinary_and_transposed_products_keep_their_distinct_axes() {
    let matrix = Matrix([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
    assert_eq!(matrix.transform([2, 3, 5]).unwrap(), [23, 53, 83]);
    assert_eq!(matrix.inverse_transform([2, 3, 5]).unwrap(), [49, 59, 69]);
}

#[test]
fn extended_addition_retains_a_term_lost_by_f64() {
    let large = 1_152_921_504_606_846_976.0_f32;
    let matrix = Matrix([[large, 1.0, -large], [0.0; 3], [0.0; 3]]);
    assert_eq!((1.0 + f64::from(large)) - f64::from(large), 0.0);
    assert_eq!(matrix.transform([1; 3]).unwrap(), [1, 0, 0]);
}

#[test]
fn orientation_uses_negative_yaw_then_pitch_then_roll() {
    assert_eq!(
        Matrix::orientation([0, 512, 0])
            .transform([256, 0, 0])
            .unwrap(),
        [0, 0, 256]
    );
    assert_eq!(
        Matrix::orientation([512, 0, 0])
            .transform([0, 256, 0])
            .unwrap(),
        [0, 0, -256]
    );
    assert_eq!(
        Matrix::orientation([0, 0, 512])
            .transform([256, 0, 0])
            .unwrap(),
        [0, -256, 0]
    );
    assert_eq!(
        Matrix::orientation([512, 512, 512])
            .transform([256, 512, 768])
            .unwrap(),
        [-256, 768, 512]
    );
    assert_eq!(
        Matrix::orientation([2048, 2560, 2048]),
        Matrix::orientation([0, 512, 0])
    );
    assert_eq!(rotation_sample(0), 0.0);
    assert_eq!(rotation_sample(512), 1.0);
}

#[test]
fn invalid_integer_results_report_the_axis_and_input() {
    for value in [f32::NAN, f32::INFINITY, f32::MAX] {
        let error = Matrix([[value, 0.0, 0.0], [0.0; 3], [0.0; 3]])
            .transform([256, 0, 0])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("axis 0") && error.contains("[256, 0, 0]"),
            "{error}"
        );
    }
    assert!(Matrix::IDENTITY.transform([i32::MAX, 0, 0]).is_err());
}

#[test]
fn collision_orientation_rebuilds_shape_angles_with_distinct_yaw_direction() {
    assert_eq!(
        Matrix::collision_orientation([0; 3])
            .transform([256, 512, 768])
            .unwrap(),
        [256, 512, 768]
    );
    assert_eq!(
        Matrix::collision_orientation([0, 512, 0])
            .transform([256, 0, 0])
            .unwrap(),
        [0, 0, -256]
    );
    assert_eq!(
        Matrix::collision_orientation([512, 0, 0])
            .transform([0, 256, 0])
            .unwrap(),
        [0, 0, -256]
    );
    assert_eq!(
        Matrix::collision_orientation([0, 0, 512])
            .transform([256, 0, 0])
            .unwrap(),
        [0, -256, 0]
    );
    assert_eq!(
        Matrix::collision_orientation([2048, 2560, 2048]),
        Matrix::collision_orientation([0, 512, 0])
    );
    assert_ne!(
        Matrix::collision_orientation([0, 512, 0]),
        Matrix::orientation([0, 512, 0])
    );
}

#[test]
fn stored_sphere_transform_keeps_fractional_centers_and_extended_products() {
    let half = Matrix([[0.5, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 0.0, 0.5]]);
    assert_eq!(
        half.transform_stored([1.0, 3.0, -5.0]).unwrap(),
        [0.5, 1.5, -2.5]
    );
    let large = 1_152_921_504_606_846_976.0_f32;
    let matrix = Matrix([[large, 1.0, -large], [0.0; 3], [0.0; 3]]);
    assert_eq!(matrix.transform_stored([1.0; 3]).unwrap(), [1.0, 0.0, 0.0]);
    assert!(
        Matrix::IDENTITY
            .transform_stored([f32::NAN, 0.0, 0.0])
            .is_err()
    );
    assert!(half.transform_stored([f32::INFINITY, 0.0, 0.0]).is_err());
}

#[test]
fn shared_rotation_table_retains_native_angle_precision_and_endpoint() {
    for (index, bits) in [
        (1, 0x3b49_0fc6),
        (256, 0x3f35_04f3),
        (1024, 0x3366_2a9a),
        (2048, 0xb3e6_2a9a),
        (2560, 0x3f80_0000),
    ] {
        assert_eq!(rotation_sample(index).to_bits(), bits, "sample {index}");
    }
}
