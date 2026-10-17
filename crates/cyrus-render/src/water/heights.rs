//! Read-only binding of exact engine heights to logical camera terrain sampling.
use anyhow::{Result, ensure};
use cyrus_game::script::camera::CameraTerrainHeights;

/// Borrowed view of one settled terrain owner. The host validates source and epoch.
/// Both arrays use original WLD row order; engine Z reverses those rows.
pub struct CameraTerrainView<'a> {
    side: usize,
    map: &'a [f32],
    retained: &'a [f32],
    center: [i32; 2],
}

impl CameraTerrainView<'_> {
    fn index(&self, [x, z]: [i32; 2]) -> Result<usize> {
        let row = (self.side as i64) - i64::from(z);
        ensure!(
            (0..self.side as i32).contains(&x) && (0..self.side as i64).contains(&row),
            "camera terrain cell [{x}, {z}]: outside {}-wide source",
            self.side
        );
        Ok(row as usize * self.side + x as usize)
    }
}

impl CameraTerrainHeights for CameraTerrainView<'_> {
    fn retained_center(&self) -> [i32; 2] {
        self.center
    }
    fn retained_height(&self, cell: [i32; 2]) -> Result<f32> {
        ensure!(cell.into_iter().zip(self.center).all(|(value, center)| (-17..=16).contains(&(i64::from(value)-i64::from(center)))),
            "camera terrain cell {cell:?}: outside retained storage around {:?}", self.center);
        Ok(self.retained[self.index(cell)?])
    }
    fn map_height(&self, cell: [i32; 2]) -> Result<f32> {
        Ok(self.map[self.index(cell)?])
    }
}

impl super::WaterPlayback {
    /// Read exact terrain heights from this settled owner, without advancing waves.
    /// The supplied environment handle must be the one currently bound to playback.
    /// Modern render-window expansion and water styling do not change these heights.
    pub fn camera_terrain<'a>(
        &'a self,
        source: bevy::prelude::AssetId<cyrus_assets::environment::Environment>,
        environment: &'a cyrus_assets::environment::Environment,
    ) -> Result<CameraTerrainView<'a>> {
        use anyhow::Context;
        ensure!(
            self.source == Some(source),
            "camera terrain: environment binding differs"
        );
        let terrain = environment
            .terrain_heights
            .as_ref()
            .context("camera terrain: world has no terrain")?;
        let window = self
            .window
            .context("camera terrain: update has not settled")?;
        window.validate(terrain.side)?;
        ensure!(
            (2..=256).contains(&terrain.side)
                && terrain.heights.len() == terrain.side * terrain.side
                && self.tick.is_some()
                && self.engine_heights.len() == terrain.heights.len(),
            "camera terrain: retained heights have not settled"
        );
        Ok(CameraTerrainView {
            side: terrain.side,
            map: &terrain.heights,
            retained: &self.engine_heights,
            center: window.engine_center(),
        })
    }
}
