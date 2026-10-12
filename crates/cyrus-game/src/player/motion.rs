//! Original heading projection, per preservation's player and water engine specs.
use std::sync::LazyLock;

use cyrus_world::transform::rotation_sample;

/// Upright actor rotation in original axes, with heading in 1/2048 turns.
/// Retains table residuals per preservation player.md's upright orientation rules.
pub fn upright_rotation(heading: u16) -> bevy::math::Mat3 {
    use bevy::math::{Mat3, Vec3};
    let index = usize::from(heading.wrapping_neg() & 2047);
    let (sine, cosine) = (rotation_sample(index), rotation_sample(index + 512));
    Mat3::from_cols(
        Vec3::new(cosine, 0.0, -sine),
        Vec3::Y,
        Vec3::new(sine, 0.0, cosine),
    )
}

/// Refresh an admitted upright mode-zero matrix without consuming the angle snapshot.
/// Per preservation PR49, each output component is stored at single precision.
/// The caller snapshots headings at update start and chooses the refresh stage.
pub fn refresh_upright_rotation(
    rotation: &mut bevy::math::Mat3,
    previous_heading: u16,
    heading: u16,
) {
    if heading == previous_heading {
        return;
    }
    let index = usize::from(previous_heading.wrapping_sub(heading) & 2047);
    let (sine, cosine) = (
        f64::from(rotation_sample(index)),
        f64::from(rotation_sample(index + 512)),
    );
    let x = rotation.x_axis.as_dvec3();
    let z = rotation.z_axis.as_dvec3();
    rotation.x_axis = (x * cosine - z * sine).as_vec3();
    rotation.z_axis = (x * sine + z * cosine).as_vec3();
}

/// Translate in original axes using an integer runtime distance. Y is unchanged.
/// Positions and distance have eight fractional bits; heading uses 1/2048 turns.
/// Single-precision coordinate addition is part of the original movement rule.
pub fn translate(position: [i64; 3], heading: u16, distance: i64) -> [i64; 3] {
    let sine = usize::from(heading.wrapping_neg() & 2047);
    let mut result = position;
    result[0] = ((f64::from(position[0] as f32)
        + distance as f64 * f64::from(rotation_sample(sine))) as f32) as i64;
    result[2] = ((f64::from(position[2] as f32)
        + distance as f64 * f64::from(rotation_sample(sine + 512))) as f32) as i64;
    result
}

static OCTANT: LazyLock<[i32; 512]> = LazyLock::new(|| {
    #[allow(clippy::approx_constant)] // Published original table-generation constant.
    std::array::from_fn(|index| ((index as f64 / 512.0).atan() * 65536.0 / 3.141592654) as i32 >> 6)
});

/// Quantized actor heading from an original X/Z displacement, in 1/2048 turns.
/// This preserves the integer reduction before the angle-table lookup.
pub fn displacement_heading(x: i64, z: i64) -> u16 {
    let (mut ax, mut az) = (x.unsigned_abs(), z.unsigned_abs());
    while ax.max(az) > 511 {
        ax >>= 1;
        az >>= 1;
    }
    let angle = if ax == 0 {
        0
    } else if az == 0 {
        512
    } else if ax == az {
        256
    } else if ax < az {
        OCTANT[(ax * 512 / az) as usize]
    } else {
        512 - OCTANT[(az * 512 / ax) as usize]
    };
    let angle = match (x < 0, z < 0) {
        (false, false) => angle,
        (true, false) => -angle,
        (false, true) => 1024 - angle,
        (true, true) => 1024 + angle,
    };
    (-angle).rem_euclid(2048) as u16
}
