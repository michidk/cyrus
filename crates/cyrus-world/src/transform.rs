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

    /// Apply the transpose using the original inverse-transform arithmetic.
    /// This is an inverse only for an orthogonal matrix; it performs no matrix inversion.
    pub fn inverse_transform(self, position: [i32; 3]) -> Result<[i32; 3]> {
        Self(std::array::from_fn(|row| {
            std::array::from_fn(|column| self.0[column][row])
        }))
        .transform(position)
    }
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

fn extended(value: f32) -> X87DoubleExtended {
    Single::from_bits(u128::from(value.to_bits()))
        .convert(&mut false)
        .value
}

fn stored(value: X87DoubleExtended) -> f32 {
    let single: Single = value.convert(&mut false).value;
    f32::from_bits(single.to_bits() as u32)
}
