//! Ordinary grounded body response from preservation's engine/player.md.

/// Result of a facing wall contact. Other actor states must use their own response.
#[derive(Debug, PartialEq, Eq)]
pub enum WallResponse {
    Rollback([i64; 3]),
    Projected([i64; 3]),
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

/// Facing adjustment after an eligible grounded wall projection.
/// The result is unwrapped unless the backward-facing branch applies, matching
/// the response rule. Heading consumers wrap it modulo 2048 when needed.
pub fn wall_heading(actor: u16, movement: u16, normal: [i32; 3]) -> i32 {
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
