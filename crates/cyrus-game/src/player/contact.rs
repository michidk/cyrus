//! Ordinary grounded body response from preservation's engine/player.md.

/// Result of a facing wall contact. Other actor states must use their own response.
#[derive(Debug, PartialEq, Eq)]
pub enum WallResponse {
    Rollback([i64; 3]),
    Projected([i64; 3]),
}

/// Original integer contact inputs, after collision-query conversion.
pub struct GroundedContact {
    pub normal: [i32; 3],
    /// Stored classification from the query, not recomputed after transformation.
    pub classification: u8,
}

/// Process primary contacts in authored query order for an ordinary grounded actor.
/// Sliding, airborne and special-state response must be dispatched separately.
/// `attempted` is the position after the ground response. Residual separation follows.
pub fn resolve_grounded_contacts(
    previous: [i64; 3],
    attempted: [i64; 3],
    heading: &mut u16,
    contacts: &[GroundedContact],
    slide_range: i32,
) -> [i64; 3] {
    let movement = if previous[0] == attempted[0] && previous[2] == attempted[2] {
        *heading
    } else {
        super::motion::displacement_heading(attempted[0] - previous[0], attempted[2] - previous[2])
    };
    let mut current = attempted;
    for contact in contacts {
        if contact.classification == 4 || facing_difference(movement, contact.normal).abs() >= 512 {
            continue;
        }
        match resolve_wall(previous, attempted, current, contact.normal, slide_range) {
            WallResponse::Rollback(position) => current = position,
            WallResponse::Projected(position) => {
                current = position;
                if current != previous {
                    *heading =
                        wall_heading(*heading, movement, contact.normal).rem_euclid(2048) as u16;
                }
            }
        }
    }
    current
}

fn normalized(vector: [i64; 3]) -> [i64; 3] {
    let length = vector
        .iter()
        .map(|&v| (v as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    if length == 0.0 {
        return [0; 3];
    }
    let divisor = (length / 65536.0) as f32;
    vector.map(|v| ((v as f64 / f64::from(divisor)) as f32).round_ties_even() as i64)
}

/// Original residual displacement length in runtime units, before the map-unit shift.
/// Components and the square-root result are stored at single precision.
pub fn residual_distance(previous: [i64; 3], current: [i64; 3]) -> anyhow::Result<u32> {
    let x = f64::from((current[0] - previous[0]) as f32);
    let z = f64::from((current[2] - previous[2]) as f32);
    let distance = ((x * x + z * z).sqrt() as f32).round_ties_even();
    anyhow::ensure!(
        distance.is_finite() && f64::from(distance) <= f64::from(i32::MAX),
        "residual displacement length outside original integer range"
    );
    Ok(distance as u32)
}

/// Resolve one eligible facing wall against the update's original displacement.
/// Positions use eight fractional bits; normals have length 256. `slide_range`
/// comes from SYSTEM.INI. Y always retains the current support response.
pub fn resolve_wall(
    previous: [i64; 3],
    attempted: [i64; 3],
    current: [i64; 3],
    normal: [i32; 3],
    slide_range: i32,
) -> WallResponse {
    let movement = normalized(std::array::from_fn(|axis| attempted[axis] - previous[axis]));
    let dot = (movement[0] * i64::from(normal[0]) + movement[2] * i64::from(normal[2])) >> 8;
    let mut position = current;
    if dot <= i64::from(slide_range) {
        position[0] = previous[0];
        position[2] = previous[2];
        WallResponse::Rollback(position)
    } else {
        for axis in [0, 2] {
            let projected = movement[axis] - ((dot * i64::from(normal[axis])) >> 8);
            position[axis] = previous[axis] + (projected >> 4);
        }
        WallResponse::Projected(position)
    }
}

/// Apply residual static contacts after the primary body response.
/// `horizontal_distance` is the resolved displacement length in runtime units.
/// `minimum_push` is the installed `poly_push_units` in map units.
/// Returns whether a nonempty residual contact set was processed.
pub fn separate_residual(
    horizontal_distance: u32,
    current: &mut [i64; 3],
    normals: &[[i32; 3]],
    minimum_push: u32,
) -> bool {
    if normals.is_empty() {
        return false;
    }
    let distance = i64::from((horizontal_distance >> 8).max(minimum_push));
    let average = std::array::from_fn(|axis| {
        normals
            .iter()
            .map(|normal| i64::from(normal[axis]))
            .sum::<i64>()
            / normals.len() as i64
    });
    let normal = normalized(average);
    for axis in [0, 2] {
        current[axis] += (normal[axis] >> 8) * distance;
    }
    true
}

fn facing_difference(movement: u16, normal: [i32; 3]) -> i32 {
    let facing = (i32::from(super::motion::displacement_heading(
        i64::from(normal[0]),
        i64::from(normal[2]),
    )) + 1024)
        & 2047;
    let movement = i32::from(movement & 2047);
    let mut difference = facing - movement;
    if difference < -1024 {
        difference += 2047;
    } else if difference > 1024 {
        difference -= 2047;
    }
    difference
}

/// Facing adjustment after a projected ordinary grounded contact, in heading units.
pub fn wall_heading(actor: u16, movement: u16, normal: [i32; 3]) -> i32 {
    let difference = facing_difference(movement, normal);
    let movement = i32::from(movement & 2047);
    let result = movement - difference.clamp(-32, 32);
    let mut backward = i32::from(actor & 2047) - movement;
    if backward < -1024 {
        backward += 2048;
    } else if backward > 1024 {
        backward -= 2048;
    }
    if backward.abs() > 512 {
        (result + 1024) & 2047
    } else {
        result
    }
}
