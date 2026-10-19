//! Equipped-model geometry follows preservation's engine/attachment.md transform rules.
use anyhow::Result;
use cyrus_world::transform::Matrix;
use rustc_apfloat::{
    Float, FloatConvert, Round, Status,
    ieee::{Single, X87DoubleExtended},
};

/// Published weapon pose and actor-retained geometry, independent of renderer axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponAttachment {
    /// Original map-unit floating attachment position.
    pub position: [f32; 3],
    pub matrix: Matrix,
    /// Actor-retained first transformed vertex plus floating origin, in Q8 units.
    pub retained_point: [i32; 3],
    /// Actor-retained pitch, heading and roll in 1/2048 turns.
    pub retained_angles: [u16; 3],
}

/// Retained attachment selected by the ordinary equipped-weapon owner.
#[derive(Debug, Clone, PartialEq)]
pub struct EquippedWeapon {
    /// ITEM.INI identity and original hand/hilt model stem.
    pub item: u16,
    pub model: String,
    pub geometry: WeaponAttachment,
    /// Retained base followed by tip, in integer map units.
    pub endpoints: [[i32; 3]; 2],
}

/// Prepare a drawn grip pair or stowed scabbard pair after attachment allocation.
/// Vertices and local offsets are original signed Q8 values. The actor origin is
/// the retained floating map-unit position, rather than the integer collision pose.
/// The caller owns vertex selection, model selection, allocation and publication.
pub fn prepare_weapon_attachment(
    vertices: [[i32; 3]; 2],
    offsets: [i32; 3],
    actor_matrix: Matrix,
    actor_position: [f32; 3],
    actor_roll: u16,
    drawn: bool,
) -> Result<WeaponAttachment> {
    let mut points = [[0; 3]; 2];
    for (point, vertex) in points.iter_mut().zip(vertices) {
        *point = actor_matrix.transform(std::array::from_fn(|axis| {
            vertex[axis].wrapping_add(offsets[axis])
        }))?;
    }
    let delta: [i32; 3] = std::array::from_fn(|axis| points[0][axis].wrapping_sub(points[1][axis]));
    let horizontal = super::spatial::rounded_distance([delta[0], 0, delta[2]])?;
    // The shared query bearing uses the opposite X sign from attachment direction.
    let bearing = |x: i32, z: i32| i32::from(super::spatial::query_bearing(x, z)).wrapping_neg();
    let pitch = (bearing(horizontal, delta[1]) - 512) & 2047;
    let heading = (1024 - bearing(delta[0], delta[2])) & 2047;
    let matrix = Matrix::orientation([
        ((pitch - 512) & 2047) as u16,
        heading as u16,
        actor_roll & 2047,
    ]);
    let retained_point = std::array::from_fn(|axis| {
        let origin = (extended(actor_position[axis]) * integer(256)).value;
        truncate((integer(points[0][axis]) + origin).value)
    });
    let position = std::array::from_fn(|axis| {
        if drawn {
            let midpoint = points[1][axis]
                .wrapping_sub(points[0][axis])
                .wrapping_shr(1)
                .wrapping_add(points[0][axis])
                >> 8;
            stored((extended(actor_position[axis]) + integer(midpoint)).value)
        } else {
            (retained_point[axis] >> 8) as f32
        }
    });
    Ok(WeaponAttachment {
        position,
        matrix,
        retained_point,
        retained_angles: [
            ((pitch + 512) & 2047) as u16,
            heading as u16,
            actor_roll & 2047,
        ],
    })
}

impl WeaponAttachment {
    /// Retained combat base and tip in integer map units, using signed ROB Y length.
    /// The transformed negative-length axis is shifted before the floating-origin add.
    pub fn combat_endpoints(self, length: i32) -> Result<[[i32; 3]; 2]> {
        let offset = self
            .matrix
            .transform([0, length.wrapping_neg().wrapping_shl(8), 0])?;
        Ok([
            self.position.map(|value| truncate(extended(value))),
            std::array::from_fn(|axis| {
                truncate((extended(self.position[axis]) + integer(offset[axis] >> 8)).value)
            }),
        ])
    }
}

fn extended(value: f32) -> X87DoubleExtended {
    Single::from_bits(u128::from(value.to_bits()))
        .convert(&mut false)
        .value
}
fn integer(value: i32) -> X87DoubleExtended {
    X87DoubleExtended::from_i128(i128::from(value)).value
}
fn stored(value: X87DoubleExtended) -> f32 {
    let single: Single = value.convert(&mut false).value;
    f32::from_bits(single.to_bits() as u32)
}
fn truncate(value: X87DoubleExtended) -> i32 {
    let converted = value.to_i128_r(32, Round::TowardZero, &mut false);
    if converted.status.contains(Status::INVALID_OP) {
        i32::MIN
    } else {
        converted.value as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coincident_pair_keeps_original_direction_and_distinct_stowed_position() {
        let drawn = prepare_weapon_attachment(
            [[-257, 513, 0]; 2],
            [0; 3],
            Matrix::IDENTITY,
            [0.5, -0.5, 1.25],
            0,
            true,
        )
        .unwrap();
        assert_eq!(drawn.retained_angles, [0, 1024, 0]);
        assert_eq!(drawn.retained_point, [-129, 385, 320]);
        assert_eq!(drawn.position, [-1.5, 1.5, 1.25]);
        let stowed = prepare_weapon_attachment(
            [[-257, 513, 0]; 2],
            [0; 3],
            Matrix::IDENTITY,
            [0.5, -0.5, 1.25],
            0,
            false,
        )
        .unwrap();
        assert_eq!(stowed.position, [-1.0, 1.0, 1.0]);
    }

    #[test]
    fn endpoints_shift_before_adding_fractional_origin() {
        let attachment = WeaponAttachment {
            position: [-0.5, 0.5, 1.25],
            matrix: Matrix::IDENTITY,
            retained_point: [0; 3],
            retained_angles: [0; 3],
        };
        assert_eq!(
            attachment.combat_endpoints(17).unwrap(),
            [[0, 0, 1], [0, -16, 1]]
        );
        assert_eq!(
            attachment.combat_endpoints(-17).unwrap(),
            [[0, 0, 1], [0, 17, 1]]
        );
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct EquippedWeaponSnapshot {
    item: u16,
    model: String,
    position_bits: [u32; 3],
    matrix_bits: [[u32; 3]; 3],
    retained_point: [i32; 3],
    retained_angles: [u16; 3],
    endpoints: [[i32; 3]; 2],
}
impl EquippedWeapon {
    pub(super) fn snapshot(&self) -> EquippedWeaponSnapshot {
        EquippedWeaponSnapshot {
            item: self.item,
            model: self.model.clone(),
            position_bits: self.geometry.position.map(f32::to_bits),
            matrix_bits: self.geometry.matrix.0.map(|row| row.map(f32::to_bits)),
            retained_point: self.geometry.retained_point,
            retained_angles: self.geometry.retained_angles,
            endpoints: self.endpoints,
        }
    }
}
impl EquippedWeaponSnapshot {
    pub(super) fn restore(self) -> Result<EquippedWeapon> {
        let position = self.position_bits.map(f32::from_bits);
        let matrix = self.matrix_bits.map(|row| row.map(f32::from_bits));
        anyhow::ensure!(
            self.item <= 255 && !self.model.is_empty(),
            "saved weapon identity invalid"
        );
        anyhow::ensure!(
            position
                .iter()
                .chain(matrix.iter().flatten())
                .all(|value| value.is_finite()),
            "saved weapon geometry is nonfinite"
        );
        anyhow::ensure!(
            self.retained_angles.iter().all(|&angle| angle < 2048),
            "saved weapon angle outside wrapped range"
        );
        Ok(EquippedWeapon {
            item: self.item,
            model: self.model,
            geometry: WeaponAttachment {
                position,
                matrix: Matrix(matrix),
                retained_point: self.retained_point,
                retained_angles: self.retained_angles,
            },
            endpoints: self.endpoints,
        })
    }
}
