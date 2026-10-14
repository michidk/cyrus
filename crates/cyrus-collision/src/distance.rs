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
