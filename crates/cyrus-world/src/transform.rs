//! Original actor matrix arithmetic, per preservation's SOUP matrix-axis rules.
use anyhow::{Result, ensure};
use rustc_apfloat::{
    Float, FloatConvert, Round, Status,
    ieee::{Single, X87DoubleExtended},
};
use std::sync::LazyLock;

static ROTATION: LazyLock<[f32; 2561]> = LazyLock::new(|| {
    #[allow(clippy::approx_constant)] // Published original table-generation constant.
    std::array::from_fn(|index| (index as f64 * 6.2831852 / 2048.0).sin() as f32)
});

/// Original sine table sample; cosine uses the masked angle index plus 512.
/// The caller must supply an index below 2561, retaining the extended cosine tail.
pub fn rotation_sample(index: usize) -> f32 {
    ROTATION[index]
}

/// Row-major original actor matrix with single-precision stored components.
/// Operations use nearest-even extended intermediates, per [preservation PR55].
///
/// [preservation PR55]: https://github.com/michidk/redguard-preservation/pull/55
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [[f32; 3]; 3]);

impl Matrix {
    pub const IDENTITY: Self = Self([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    /// Build an orientation from pitch, yaw and roll in 1/2048 turns.
    pub fn orientation(angles: [u16; 3]) -> Self {
        let mut result = Self::IDENTITY;
        result.rotate(angles);
        result
    }

    /// Rebuild the collision shape's matrix from local pitch, yaw and roll.
    /// This differs from retained actor presentation orientation. Uses shared
    /// table samples at wrapped negative angles and extended intermediates,
    /// following preservation's per-update collision shape preparation.
    pub fn collision_orientation(angles: [u16; 3]) -> Self {
        let samples = angles.map(|angle| {
            let index = usize::from(angle.wrapping_neg() & 2047);
            (
                extended(rotation_sample(index)),
                extended(rotation_sample(index + 512)),
            )
        });
        let [(sp, cp), (sy, cy), (sr, cr)] = samples;
        let product = |a: X87DoubleExtended, b: X87DoubleExtended| (a * b).value;
        let triple = |a: X87DoubleExtended, b: X87DoubleExtended, c: X87DoubleExtended| {
            product(product(a, b), c)
        };
        let add = |a: X87DoubleExtended, b: X87DoubleExtended| stored((a + b).value);
        let subtract = |a: X87DoubleExtended, b: X87DoubleExtended| stored((a - b).value);
        Self([
            [
                add(triple(sp, sr, sy), product(cr, cy)),
                stored(product(-sr, cp)),
                subtract(triple(sr, sp, cy), product(sy, cr)),
            ],
            [
                subtract(product(sr, cy), triple(sp, sy, cr)),
                stored(product(cr, cp)),
                subtract(product(-sr, sy), triple(cr, sp, cy)),
            ],
            [stored(product(cp, sy)), stored(sp), stored(product(cp, cy))],
        ])
    }

    /// The original's closed-form Euler matrix from three table angles in 1/2048 turns, wrapped
    /// to the table without negation. Three products (`sin a0 * cos a1`, `sin a1 * cos a2` and
    /// `sin a2 * sin a1`) are stored as f32 before their second multiplication; every other
    /// product and sum stays extended until its component is stored. `SOUP sMoveAlongAxis`
    /// rotates its step by the transpose of `euler([pitch, -yaw, roll])`.
    pub fn euler(angles: [u16; 3]) -> Self {
        let [(s0, c0), (s1, c1), (s2, c2)] = angles.map(|angle| {
            let index = usize::from(angle & 2047);
            (
                extended(rotation_sample(index)),
                extended(rotation_sample(index + 512)),
            )
        });
        let product = |a: X87DoubleExtended, b: X87DoubleExtended| (a * b).value;
        let rounded = |value: X87DoubleExtended| extended(stored(value));
        let first = rounded(product(s0, c1));
        let second = rounded(product(s1, c2));
        let third = rounded(product(s2, s1));
        Self([
            [
                stored((product(c2, c1) + product(s0, third)).value),
                stored(product(-s2, c0)),
                stored((product(s2, first) - second).value),
            ],
            [
                stored((product(s2, c1) - product(s0, second)).value),
                stored(product(c2, c0)),
                stored((-third - product(c2, first)).value),
            ],
            [
                stored(product(c0, s1)),
                rotation_sample(usize::from(angles[0] & 2047)),
                stored(product(c0, c1)),
            ],
        ])
    }

    /// Apply negative yaw, pitch, then roll, wrapping each angle to 1/2048 turns.
    /// Each axis reads the old row and stores each resulting component as f32.
    pub fn rotate(&mut self, [pitch, yaw, roll]: [u16; 3]) {
        for (axis, angle) in [(1, yaw), (0, pitch), (2, roll)] {
            let index = usize::from(angle.wrapping_neg() & 2047);
            let sine = extended(rotation_sample(index));
            let cosine = extended(rotation_sample(index + 512));
            for row in &mut self.0 {
                let [x, y, z] = row.map(extended);
                match axis {
                    1 => {
                        let next_x = stored(((x * cosine).value - (z * sine).value).value);
                        row[2] = stored(((z * cosine).value + (x * sine).value).value);
                        row[0] = next_x;
                    }
                    0 => {
                        let next_y = stored(((y * cosine).value + (z * sine).value).value);
                        row[2] = stored(((z * cosine).value - (y * sine).value).value);
                        row[1] = next_y;
                    }
                    _ => {
                        let next_x = stored(((x * cosine).value + (y * sine).value).value);
                        row[1] = stored(((y * cosine).value - (x * sine).value).value);
                        row[0] = next_x;
                    }
                }
            }
        }
    }

    /// Transform original Q8 coordinates, storing inputs and each dot product as f32.
    /// Final integer conversion rounds nearest-even and rejects nonfinite/overflow results.
    pub fn transform(self, position: [i32; 3]) -> Result<[i32; 3]> {
        let inputs: [X87DoubleExtended; 3] = position.map(|value| {
            let single = Single::from_i128(i128::from(value)).value;
            single.convert(&mut false).value
        });
        let mut result = [0; 3];
        for (axis, row) in self.0.into_iter().enumerate() {
            let products: [X87DoubleExtended; 3] =
                std::array::from_fn(|i| (inputs[i] * extended(row[i])).value);
            let first = if axis == 0 {
                (products[1] + products[0]).value
            } else {
                (products[0] + products[1]).value
            };
            let single =
                Single::from_bits(u128::from(stored((first + products[2]).value).to_bits()));
            let integer = single.to_i128_r(32, Round::NearestTiesToEven, &mut false);
            ensure!(
                !integer.status.contains(Status::INVALID_OP),
                "actor matrix transform axis {axis}: position {position:?} with row {row:?} cannot convert to i32"
            );
            result[axis] = integer.value as i32;
        }
        Ok(result)
    }

    /// Transform stored single-precision coordinates without integer conversion.
    /// Sphere overlap retains these fractional centers until pair differences are
    /// rounded. Uses the same extended dot products as actor-coordinate transforms.
    pub fn transform_stored(self, position: [f32; 3]) -> Result<[f32; 3]> {
        ensure!(
            position.iter().all(|value| value.is_finite()),
            "actor stored transform: nonfinite position"
        );
        let inputs = position.map(extended);
        let mut result = [0.0; 3];
        for (axis, row) in self.0.into_iter().enumerate() {
            let products: [X87DoubleExtended; 3] =
                std::array::from_fn(|i| (inputs[i] * extended(row[i])).value);
            let first = if axis == 0 {
                (products[1] + products[0]).value
            } else {
                (products[0] + products[1]).value
            };
            result[axis] = stored((first + products[2]).value);
            ensure!(
                result[axis].is_finite(),
                "actor stored transform axis {axis}: nonfinite result for {position:?}"
            );
        }
        Ok(result)
    }

    /// Resolve a stored model vertex into local Q8 coordinates.
    /// The caller supplies the original vertex conversion to f32 in runtime Q8
    /// units before rotation. The rotated f32 and exact
    /// integer actor position add in extended precision, then store as f32 before
    /// nearest-even integer conversion. Follows preservation's actor model-point
    /// conversion; the caller owns frame/vertex selection and the current matrix.
    pub fn model_point(self, vertex: [f32; 3], position: [i32; 3]) -> Result<[i32; 3]> {
        let rotated = self.transform_stored(vertex)?;
        let mut result = [0; 3];
        for axis in 0..3 {
            let local = X87DoubleExtended::from_i128(i128::from(position[axis])).value;
            let sum = stored((local + extended(rotated[axis])).value);
            let integer = Single::from_bits(u128::from(sum.to_bits())).to_i128_r(
                32,
                Round::NearestTiesToEven,
                &mut false,
            );
            ensure!(
                !integer.status.contains(Status::INVALID_OP),
                "actor model point axis {axis}: vertex {vertex:?} at {position:?} cannot convert to i32"
            );
            result[axis] = integer.value as i32;
        }
        Ok(result)
    }

    /// Apply the transpose using the original inverse-transform arithmetic.
    /// This is an inverse only for an orthogonal matrix; it performs no matrix inversion.
    pub fn inverse_transform(self, position: [i32; 3]) -> Result<[i32; 3]> {
        Self(std::array::from_fn(|row| {
            std::array::from_fn(|column| self.0[column][row])
        }))
        .transform(position)
    }
}

/// Convert a successfully resolved animated vertex from model units to runtime Q8.
/// The original query stores the product by 256 as f32 before nearest-even i32
/// conversion. Frame lookup and query admission belong to the caller, following
/// preservation's animation-vertex wrapper rules.
pub fn animation_vertex_q8(position: [f32; 3]) -> Result<[i32; 3]> {
    let scale = Single::from_i128(256).value;
    let mut result = [0; 3];
    for axis in 0..3 {
        let input = Single::from_bits(u128::from(position[axis].to_bits()));
        let integer = (input * scale)
            .value
            .to_i128_r(32, Round::NearestTiesToEven, &mut false);
        ensure!(
            !integer.status.contains(Status::INVALID_OP),
            "animation vertex axis {axis}: position {position:?} cannot convert to runtime Q8 i32"
        );
        result[axis] = integer.value as i32;
    }
    Ok(result)
}

/// Translate an upright actor forward in Q8 coordinates using the original table.
/// The coordinate is stored as f32 before addition, the sum as f32 before truncation.
/// Per preservation's player heading-based translation and SOUP MoveObjectAxis rules.
pub fn translate_forward(position: [i32; 3], heading: u16, distance: i32) -> Result<[i32; 3]> {
    let index = usize::from(heading.wrapping_neg() & 2047);
    let mut result = position;
    for (axis, sample) in [(0, index), (2, index + 512)] {
        let coordinate: Single = Single::from_i128(i128::from(position[axis])).value;
        let coordinate: X87DoubleExtended = coordinate.convert(&mut false).value;
        let distance = X87DoubleExtended::from_i128(i128::from(distance)).value;
        let sum = stored(((distance * extended(rotation_sample(sample))).value + coordinate).value);
        let sum = Single::from_bits(u128::from(sum.to_bits()));
        let integer = sum.to_i128_r(32, Round::TowardZero, &mut false);
        ensure!(
            !integer.status.contains(Status::INVALID_OP),
            "forward translation axis {axis}: coordinate {} cannot convert to i32",
            position[axis]
        );
        result[axis] = integer.value as i32;
    }
    Ok(result)
}

/// Rotate a horizontal Q8 offset (X, Z) by a heading, as the original's offset rotation does: the
/// inputs and each result are stored as f32 around extended products, the result rounds to nearest
/// even, and the heading is negated and wrapped to the table.
pub fn rotate_offset(offset: [i32; 2], heading: u16) -> Result<[i32; 2]> {
    let index = usize::from(heading.wrapping_neg() & 2047);
    let [x, z]: [X87DoubleExtended; 2] = offset.map(|value| {
        let single = Single::from_i128(i128::from(value)).value;
        single.convert(&mut false).value
    });
    let (sine, cosine) = (
        extended(rotation_sample(index)),
        extended(rotation_sample(index + 512)),
    );
    let products = |a: X87DoubleExtended, b: X87DoubleExtended| (a * b).value;
    let results = [
        (products(x, cosine) + products(z, sine)).value,
        (products(z, cosine) - products(x, sine)).value,
    ];
    let mut rotated = [0; 2];
    for (axis, value) in results.into_iter().enumerate() {
        let single = Single::from_bits(u128::from(stored(value).to_bits()));
        let integer = single.to_i128_r(32, Round::NearestTiesToEven, &mut false);
        ensure!(
            !integer.status.contains(Status::INVALID_OP),
            "offset rotation axis {axis}: {offset:?} cannot convert to i32"
        );
        rotated[axis] = integer.value as i32;
    }
    Ok(rotated)
}

fn extended(value: f32) -> X87DoubleExtended {
    Single::from_bits(u128::from(value.to_bits()))
        .convert(&mut false)
        .value
}

fn stored(value: X87DoubleExtended) -> f32 {
    let single: Single = value.convert(&mut false).value;
    f32::from_bits(single.to_bits() as u32)
}

#[cfg(test)]
mod rotate_offset_tests {
    use super::rotate_offset;

    #[test]
    fn offset_rotation_matches_the_original_routine() {
        // Rows from the original routine in an emulator: X, Z, heading, rotated X, rotated Z.
        #[rustfmt::skip]
        let rows: &[(i32, i32, u16, i32, i32)] = &[
        (-512, 2048, 237, -1744, 1190),
        (256, 0, 1284, -179, -183),
        (2048, 256, 3119, -1990, -548),
        (1, 256, 107, -82, 243),
        (256, -512, 1491, -542, -183),
        (-1, 1, 1136, 1, -1),
        (61985, -1, 14, 61928, 2661),
        (256, -1, 1363, -130, -220),
        (2048, 256, 1488, -47, -2063),
        (256, -512, 176, 483, -308),
        (77, -1, 2161, 73, 25),
        (2048, 0, 2768, -1220, 1645),
        (-512, -512, 3938, -692, -214),
        (2048, -51563, 3871, -31254, -41062),
        (-1, -512, 187, 277, -431),
        (2048, 0, 3431, -926, -1826),
        (77, 0, 3709, 29, -71),
        (-1, 256, 975, -37, -253),
        (256, 2048, 2906, -1222, -1663),
        (-512, 1, 3008, 502, -101),
        (0, 1, 1707, 1, 1),
        (2048, 2048, 2258, 407, 2868),
        (1, 2048, 2508, -2022, 326),
        (1, -1, 2533, 1, 1),
        (-1, 0, 663, 0, -1),
        (77, 256, 2816, -235, -127),
        (-34410, 99, 1192, 29988, 16874),
        (0, 2048, 1696, 1806, 965),
        (-1, 99, 873, -43, -89),
        (77, -1, 481, 8, 77),
        (-512, -62985, 1386, -56205, 28432),
        (36890, 2048, 3924, 32903, -16806),
        (-512, 99, 1200, 490, 178),
        (77, -1, 4090, 77, -2),
        (-1, -76634, 2230, 40600, -64995),
        (2048, 2048, 281, -222, 2888),
        (-512, -512, 3978, -660, -297),
        (-512, -1, 94, -491, -147),
        (-512, -512, 3792, -716, 106),
        (2048, -512, 2831, -1168, 1758),
        (77, -1, 3684, 22, -74),
        (2048, -1, 1361, -1048, -1759),
        (2048, -512, 645, -343, 2083),
        (77, 99, 2479, -77, 99),
        (-512, 0, 1602, -103, 502),
        (-58051, -1, 1794, -41300, 40795),
        (-1, 0, 3863, -1, 1),
        (-1, -1, 907, 1, 1),
        (-1, 256, 292, -200, 159),
        (8918, 99, 589, -2183, 8647),
        (256, 2048, 5, 225, 2052),
        (77, 99, 942, -99, -77),
        (2048, -512, 776, -1130, 1783),
        (2048, -512, 2880, -1418, 1564),
        (77, 2048, 3829, 1549, 1342),
        (-1, 99, 3907, 53, 83),
        (-1, 1, 2876, 0, -1),
        (0, 1, 2670, -1, 0),
        (-1, 99, 3516, 97, -20),
        (256, -512, 3928, -30, -572),
        (77, -1, 2128, 75, 18),
        (-512, -512, 3403, -165, 705),
        (2048, -19394, 2346, 16612, -10216),
        (-62438, 1, 3632, -9161, 61762),
        (-512, -1, 3214, 464, 217),
        (-96539, -512, 1994, -95302, 15416),
        (0, -35602, 2192, 15222, -32184),
        (-512, -23143, 3888, -14198, -18284),
        (1, -1, 2463, 1, 1),
        (2048, -512, 218, 1924, 868),
        (2048, -1, 298, 1251, 1622),
        (-512, 45025, 881, -18662, -40978),
        (256, 99, 3503, 33, -272),
        (55947, 1049, 3210, -50576, -23942),
        (256, -22288, 541, 22177, 2235),
        (0, 99, 2345, -78, 61),
        (69972, -1, 1355, -36890, -59458),
        (25412, -1, 3486, -7527, -24272),
        (0, 256, 2188, -107, 233),
        (1, 0, 2728, 0, 1),
        (77, 99, 3986, 105, 68),
        (1, 256, 910, -89, -240),
        (-1, -23325, 1226, -13547, 18988),
        (-1, -1, 550, 1, -1),
        (256, 1, 3106, -255, -28),
        (1, 0, 969, -1, 0),
        (77, -1, 81, 75, 18),
        (1, -17538, 4010, -4573, -16931),
        (2048, -1, 2414, 888, 1846),
        (-512, 256, 2156, -567, 76),
        (256, -512, 1095, -361, 445),
        (2048, 99, 1468, -327, -2024),
        (0, 2048, 698, -1724, -1106),
        (0, 1, 3715, 1, 0),
        (77, 21277, 2648, -20527, -5601),
        (0, 99, 439, -97, 22),
        (77, 256, 950, -133, -232),
        (-78039, -1, 3079, 78021, 1677),
        (256, 2048, 2856, -1462, -1457),
        (77, 2048, 2424, -1841, 900),
        (77, 0, 1760, 49, -60),
        (-1, 1, 794, 0, -1),
        (0, 256, 1602, 251, 51),
        (-89658, 2048, 3, -89673, 1223),
        (11932, 99, 2648, -3278, 11473),
        (22047, 256, 1267, -16025, -15144),
        (-1, 2048, 3159, 541, -1975),
        (-1, -512, 903, 187, 477),
        (-1, -512, 1926, -188, -476),
        (-91310, 2048, 3022, 89925, -15976),
        (0, 0, 3740, 0, 0),
        (-1, 2048, 2384, -1757, 1052),
        (256, -1, 274, 171, 190),
        (0, 256, 640, -237, -98),
        (77, 2048, 2554, -2046, 115),
        (-51641, -512, 547, 6044, -51289),
        (91980, 256, 1438, -26995, -87930),
        (256, 99, 2217, 173, 213),
        (-98356, -84045, 3360, -2571, 129348),
        (0, 2048, 768, -1448, -1448)
        ];
        for &(x, z, heading, rx, rz) in rows {
            assert_eq!(
                rotate_offset([x, z], heading).unwrap(),
                [rx, rz],
                "{x} {z} {heading}"
            );
        }
    }
}

#[cfg(test)]
mod euler_tests {
    use super::Matrix;

    #[test]
    fn the_closed_form_matrix_matches_the_original_builder_bit_for_bit() {
        // Rows from the original builder in an emulator at extended precision: the three table
        // angles, then the nine stored components row by row as f32 bit patterns.
        let rows: Vec<([u16; 3], [u32; 9])> = serde_json::from_str(r#"[
[[1024,1536,0],[3018205850,0,1065353216,862333594,3212836864,667873428,1065353216,862333594,870722202]],
[[0,1536,1536],[676262036,1065353216,3018205850,870722202,3018205850,3212836864,3212836864,0,3018205850]],
[[417,0,1536],[3018205850,1049829174,3212129309,3212836864,3003401862,870086048,0,1064645661,1049829174]],
[[512,512,512],[1065353216,3009817242,0,0,659484820,3212836864,862333594,1065353216,659484820]],
[[512,0,512],[862333594,3009817242,1065353216,1065353216,659484820,3009817242,0,1065353216,862333594]],
[[512,1024,512],[0,3009817242,3212836864,3212836864,659484820,0,659484820,1065353216,3009817242]],
[[512,1536,0],[3018205850,2147483648,1065353216,1065353216,862333594,870722202,3009817242,1065353216,2815357076]],
[[1536,512,0],[862333594,0,3212836864,1065353216,3018205850,862333594,3018205850,3212836864,2815357076]],
[[1024,13,0],[1065339874,0,3173208523,2970801834,3212836864,3009805246,3173208523,862333594,3212823522]],
[[1024,1536,512],[3009817244,1065353216,862333592,3018205850,3009817242,1065353216,1065353216,862333594,870722202]],
[[1536,0,0],[1065353216,0,2147483648,0,3018205850,1065353216,2147483648,3212836864,3018205850]],
[[1536,74,1024],[3212406350,667873428,1046903905,3194387553,870722202,3212406350,2999925278,3212836864,3017818780]],
[[1541,1536,512],[1065351242,3162198232,875338876,3022822968,811725941,1065353216,3162198232,3212834890,2967598197]],
[[0,1536,1410],[858623111,1064115229,3200321313,869609143,3200321313,3211598877,3212836864,0,3018205850]],
[[1453,1536,442],[1064439238,1048315038,1046100386,3193122862,3176915385,1064967812,1048639684,3212295863,854059501]],
[[1024,1536,512],[3009817244,1065353216,862333592,3018205850,3009817242,1065353216,1065353216,862333594,870722202]],
[[1395,512,1024],[3017511118,851507894,1065353216,3211291449,1054253980,3017511118,3201737628,3211291449,2998991542]],
[[1024,1024,0],[3212836864,0,3009817242,2806968468,3212836864,862333594,3009817242,862333594,1065353216]],
[[1024,512,0],[862333594,0,3212836864,3009817242,3212836864,2806968468,3212836864,862333594,3009817242]],
[[0,972,1024],[1065140169,3009817242,1042459577,3009625694,3212836864,2987540826,1042459577,0,3212623817]],
[[1536,0,1536],[3018205850,3018205850,1065353216,3212836864,676262036,3018205850,2147483648,3212836864,3018205850]],
[[512,534,1536],[3212798663,862333594,1032462352,1032462352,2815357076,1065315015,862299248,1065353216,2977455996]],
[[512,1024,0],[3212836864,2147483648,3009817242,3009817242,862333594,1065353216,659484820,1065353216,3009817242]],
[[1024,1536,693],[842820491,1062832302,3204904581,3019907196,1057420933,1062832302,1065353216,862333594,870722202]],
[[1536,1536,1024],[875339764,667873428,3212836864,1065353216,870722202,875339764,870722202,3212836864,676262036]],
[[1536,0,1544],[1019808374,3018201307,1065348163,3212831811,2973024475,1019808374,2147483648,3212836864,3018205850]],
[[1536,512,1536],[1065353216,3018205850,875339764,3022823412,676262036,1065353216,3018205850,3212836864,2815357076]],
[[1115,0,1456],[3195588563,3211703935,1049156935,3212334072,1047473550,3179880843,2147483648,3196917694,3212187259]],
[[1024,1488,1536],[865311788,3212836864,3016935930,1041645706,870722202,3212655276,1065171628,862333594,1041645706]],
[[1024,512,1536],[3009817244,3212836864,870722202,3009817240,870722202,1065353216,3212836864,862333594,3009817242]],
[[1024,1536,1183],[872456480,3203397625,3210880026,826731127,1063396378,3203397625,1065353216,862333594,870722202]],
[[1024,1024,1024],[1065353216,862333594,862333593,3009817241,1065353216,3009817243,3009817242,862333594,1065353216]],
[[1536,1024,581],[1045899143,870385483,1064978704,3212462352,851538261,1045899143,2815357076,3212836864,870722202]],
[[512,1536,0],[3018205850,2147483648,1065353216,1065353216,862333594,870722202,3009817242,1065353216,2815357076]],
[[512,1024,0],[3212836864,2147483648,3009817242,3009817242,862333594,1065353216,659484820,1065353216,3009817242]],
[[512,1806,512],[3207402601,3009817242,1060937730,1060937730,659484820,1059918953,3004931365,1065353216,858363685]],
[[0,512,0],[862333594,2147483648,3212836864,0,1065353216,2147483648,1065353216,0,862333594]],
[[0,512,512],[659484820,3212836864,3009817242,862333594,862333594,3212836864,1065353216,0,862333594]],
[[1536,1024,1557],[3179535093,3018174555,3212802056,1065318408,2985105759,3179535093,2815357076,3212836864,870722202]],
[[294,512,0],[862333594,2147483648,3212836864,3209222323,1058978834,3006567453,1058978834,1061738675,856602467]],
[[1536,1536,471],[1065220664,870603026,1040217936,3187701584,2993094937,1065220664,870722202,3212836864,676262036]],
[[0,0,1209],[3210206336,1057595214,0,3205078862,3210206336,0,0,0,1065353216]],
[[1390,1024,1536],[874967745,3202203260,3211181772,1065353216,860315146,874595727,2999410186,3211181772,1054719612]],
[[1676,1536,512],[1063829308,3201644094,874654700,3022480880,851423799,1065353216,3201644094,3211312956,3007296055]],
[[1567,512,544],[3212680622,3183578220,1036565799,3183989994,3155722425,3212756077,1036155946,3212761044,833543147]],
[[1024,1536,1024],[870722202,862333594,3212836864,3009817244,1065353216,862333592,1065353216,862333594,870722202]],
[[1024,1536,512],[3009817244,1065353216,862333592,3018205850,3009817242,1065353216,1065353216,862333594,870722202]],
[[1024,1536,512],[3009817244,1065353216,862333592,3018205850,3009817242,1065353216,1065353216,862333594,870722202]],
[[1024,1024,0],[3212836864,0,3009817242,2806968468,3212836864,862333594,3009817242,862333594,1065353216]],
[[1024,1024,1536],[870722202,3212836864,862333596,1065353216,870722202,862333592,3009817242,862333594,1065353216]],
[[0,512,0],[862333594,2147483648,3212836864,0,1065353216,2147483648,1065353216,0,862333594]],
[[1536,1536,512],[1065353216,870722202,875339764,3022823412,2815357076,1065353216,870722202,3212836864,676262036]],
[[0,1536,512],[2815357076,3212836864,862333594,3018205850,862333594,1065353216,3212836864,0,3018205850]],
[[1024,2016,1944],[1064429729,3198196456,1035901836,3198145773,3211990087,3170623392,1036565828,862333594,3212756077]],
[[0,1024,1156],[1063996172,1053407569,861113493,1053407569,3211479820,850746897,862333594,0,3212836864]],
[[1376,1215,1536],[3204036108,3203488493,3208387388,1062554278,861470683,3205337727,1048934707,3210855831,1053366023]],
[[1024,0,1024],[3212836864,862333594,659484820,862333594,1065353216,862333594,2147483648,862333594,3212836864]],
[[0,0,1536],[3018205850,1065353216,0,3212836864,3018205850,0,0,0,1065353216]],
[[1536,1536,1024],[875339764,667873428,3212836864,1065353216,870722202,875339764,870722202,3212836864,676262036]],
[[0,0,1772],[1059689491,1061144424,2147483648,3208628072,1059689491,0,0,0,1065353216]]
]"#).unwrap();
        for (angles, expected) in rows {
            let matrix = Matrix::euler(angles);
            let bits: Vec<u32> = matrix.0.iter().flatten().map(|v| v.to_bits()).collect();
            assert_eq!(bits, expected, "angles {angles:?}");
        }
    }
}
