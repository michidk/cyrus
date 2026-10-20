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

    /// The marker's Q8 position; absent, unreserved or out-of-table markers are fatal in the original.
    pub fn position(&self, marker: i32) -> Result<[i32; 3]> {
        usize::try_from(marker)
            .ok()
            .and_then(|index| self.0.get(index))
            .with_context(|| format!("marker {marker}: outside ordinary world table"))?
            .with_context(|| format!("marker {marker}: unavailable"))
    }

    pub fn contains(&self, marker: i32, position: [i32; 3], distance: i32) -> Result<bool> {
        let target = usize::try_from(marker)
            .ok()
            .and_then(|index| self.0.get(index))
            .with_context(|| format!("marker {marker}: outside ordinary world table"))?
            .with_context(|| format!("marker {marker}: unavailable"))?;
        let length = super::spatial::rounded_distance(std::array::from_fn(|axis| {
            target[axis].wrapping_sub(position[axis])
        }))
        .with_context(|| format!("marker {marker}"))?;
        Ok(length <= distance)
    }
}
