//! Ordinary-world marker queries use preservation's SOUP node-marker distance contract.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;

#[derive(Deserialize)]
struct Marker {
    map_position: [i32; 3],
    reserved: u8,
}

pub(super) struct Markers([Option<[i32; 3]>; 128]);
impl Markers {
    pub fn prepare(metadata: &serde_json::Value) -> Result<Self> {
        let records = metadata["markers"]
            .as_array()
            .context("marker records absent")?;
        ensure!(
            records.len() <= 128,
            "ordinary marker table exceeds 128 slots; alternate map markers unsupported"
        );
        let mut markers = [None; 128];
        for (index, record) in records.iter().enumerate() {
            let marker: Marker = serde_json::from_value(record.clone())
                .with_context(|| format!("marker {index}: invalid record"))?;
            ensure!(
                marker
                    .map_position
                    .iter()
                    .all(|value| (-8_388_608..=8_388_607).contains(value)),
                "marker {index}: map position outside signed 24-bit range"
            );
            markers[index] = (marker.reserved != 0)
                .then_some(marker.map_position.map(|value| value.wrapping_shl(8)));
        }
        Ok(Self(markers))
    }

    pub fn contains(&self, marker: i32, position: [i32; 3], distance: i32) -> Result<bool> {
        let target = usize::try_from(marker)
            .ok()
            .and_then(|index| self.0.get(index))
            .with_context(|| format!("marker {marker}: outside ordinary world table"))?
            .with_context(|| format!("marker {marker}: unavailable"))?;
        // Stored f32 differences are integers; their squared sum fits u64 exactly.
        let squared: u64 = target
            .into_iter()
            .zip(position)
            .map(|(target, current)| {
                let difference = target.wrapping_sub(current) as f32 as i64;
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
        let length = single.round_ties_even();
        ensure!(
            f64::from(length) <= f64::from(i32::MAX),
            "marker {marker}: distance exceeds signed integer range"
        );
        Ok((length as i32) <= distance)
    }
}
