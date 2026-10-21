//! Shared original integer-distance arithmetic for spatial and collision queries.
use anyhow::{Result, ensure};

/// Original mixed-precision Euclidean distance, rounded to a signed integer.
pub fn rounded_distance(differences: [i32; 3]) -> Result<i32> {
    let length = single_precision_length(differences).round_ties_even();
    ensure!(
        f64::from(length) <= f64::from(i32::MAX),
        "distance exceeds signed integer range"
    );
    Ok(length as i32)
}

/// The sphere/segment query for a segment whose endpoints coincide: whether the point lies inside
/// or on the sphere. Coordinate differences wrap at 32 bits and are stored as single precision;
/// the squares add, the sum is stored as single precision and truncates to an integer, the signed
/// minimum when it does not fit. The radius square and the residual wrap at 32 bits; a
/// nonnegative residual is contact. Follows preservation's engine/SOUP.md sphere/segment query.
pub fn point_in_sphere(point: [i32; 3], center: [i32; 3], radius: i32) -> bool {
    let squared: f64 = std::array::from_fn::<_, 3, _>(|axis| {
        let difference = f64::from(point[axis].wrapping_sub(center[axis]) as f32);
        difference * difference
    })
    .into_iter()
    .sum();
    let single = squared as f32;
    let distance = if single.is_finite() && single >= i32::MIN as f32 && single < i32::MAX as f32 {
        single as i32
    } else {
        i32::MIN
    };
    radius.wrapping_mul(radius).wrapping_sub(distance) >= 0
}

/// Euclidean length with original single-precision inputs and stored result.
pub fn single_precision_length(differences: [i32; 3]) -> f32 {
    // Stored f32 differences are integers; their squared sum fits u64 exactly.
    let squared: u64 = differences
        .into_iter()
        .map(|difference| {
            let difference = difference as f32 as i64;
            (difference * difference) as u64
        })
        .sum();
    let squared = squared as f64;
    let root = squared.sqrt();
    let mut single = root as f32;
    let neighbor = if f64::from(single) > root {
        single.next_down()
    } else {
        single.next_up()
    };
    let midpoint = (f64::from(single) + f64::from(neighbor)) / 2.0;
    // Correct a double-rounded square root at a single-precision midpoint.
    if root == midpoint {
        let boundary = midpoint * midpoint;
        if squared < boundary {
            single = single.min(neighbor);
        } else if squared > boundary {
            single = single.max(neighbor);
        }
    }
    single
}

#[cfg(test)]
mod point_in_sphere_tests {
    use super::point_in_sphere;

    #[test]
    fn equality_is_contact_and_an_overflowing_sum_wraps_to_the_minimum() {
        assert!(point_in_sphere([3, 4, 0], [0, 0, 0], 5));
        assert!(!point_in_sphere([3, 4, 1], [0, 0, 0], 5));
        assert!(point_in_sphere([0; 3], [0; 3], 0));
        // The sum no longer fits an integer: the signed minimum makes the residual wrap negative.
        assert!(!point_in_sphere([0, 0, 0], [50_000, 50_000, 0], 100));
    }
}
