//! Marker queries use preservation's SOUP node-marker distance contract: a loaded world's own
//! marker table, an index below its count and a presence byte equal to one.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;

#[derive(Deserialize)]
struct Marker {
    map_position: [i32; 3],
    reserved: u8,
}

pub(super) struct Markers(Vec<Option<[i32; 3]>>);
impl Markers {
    pub fn prepare(metadata: &serde_json::Value) -> Result<Self> {
        let records = metadata["markers"]
            .as_array()
            .context("marker records absent")?;
        let mut markers = Vec::with_capacity(records.len());
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
            markers.push(
                (marker.reserved == 1)
                    .then_some(marker.map_position.map(|value| value.wrapping_shl(8))),
            );
        }
        Ok(Self(markers))
    }

    /// The marker's Q8 position; absent, unreserved or out-of-table markers are fatal in the original.
    pub fn position(&self, marker: i32) -> Result<[i32; 3]> {
        usize::try_from(marker)
            .ok()
            .and_then(|index| self.0.get(index))
            .with_context(|| format!("marker {marker}: outside the world's marker table"))?
            .with_context(|| format!("marker {marker}: unavailable"))
    }

    pub fn contains(&self, marker: i32, position: [i32; 3], distance: i32) -> Result<bool> {
        let target = usize::try_from(marker)
            .ok()
            .and_then(|index| self.0.get(index))
            .with_context(|| format!("marker {marker}: outside the world's marker table"))?
            .with_context(|| format!("marker {marker}: unavailable"))?;
        let length = super::spatial::rounded_distance(std::array::from_fn(|axis| {
            target[axis].wrapping_sub(position[axis])
        }))
        .with_context(|| format!("marker {marker}"))?;
        Ok(length <= distance)
    }
}

#[cfg(test)]
mod tests {
    use super::Markers;

    #[test]
    fn a_marker_needs_presence_one_and_an_index_below_the_map_count() {
        let metadata = serde_json::json!({"markers": [
            {"map_position": [1, 2, 3], "reserved": 1},
            {"map_position": [4, 5, 6], "reserved": 0},
            {"map_position": [7, 8, 9], "reserved": 2},
        ]});
        let markers = Markers::prepare(&metadata).unwrap();
        assert_eq!(markers.position(0).unwrap(), [256, 512, 768]);
        for unavailable in [1, 2] {
            let error = markers.position(unavailable).unwrap_err().to_string();
            assert!(error.contains("unavailable"), "{error}");
        }
        for outside in [3, 200, -1] {
            let error = markers.position(outside).unwrap_err().to_string();
            assert!(error.contains("marker table"), "{error}");
        }
    }
}
