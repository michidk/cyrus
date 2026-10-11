//! Terrain support in original map coordinates, per preservation's engine/player.md.
use bevy::math::DVec3;

/// Decoded WLD heights in source row order. Engine Y is positive down.
/// Water vertices must be excluded until moving-water support is available.
pub struct TerrainGround {
    heights: Vec<i16>,
    water: Vec<bool>,
}

/// A terrain plane result before converting its height to the actor origin.
#[derive(Debug, PartialEq)]
pub struct GroundSupport {
    pub height: f64,
    /// Face normal in original engine axes, scaled by 256 and rounded to even.
    pub normal: [i32; 3],
}

impl GroundSupport {
    /// Original contact classification. A steep surface is not ordinary walkable ground.
    pub fn classification(&self) -> u8 {
        match self.normal[1] {
            ..=-161 => 4,
            -160..=-42 => 2,
            -41..=160 => 1,
            _ => 8,
        }
    }

    /// Actor origin in runtime units with eight fractional bits.
    /// Extent and standing offset are installed values in map units.
    pub fn actor_height(&self, positive_y_extent: i32, standing_height: i32) -> i64 {
        (self.height.trunc() as i64 - i64::from(positive_y_extent) + i64::from(standing_height))
            * 256
    }
}

impl TerrainGround {
    /// Accepts exactly one original 256-by-256 grid and its water-vertex mask.
    pub fn new(heights: Vec<i16>, water: Vec<bool>) -> anyhow::Result<Self> {
        anyhow::ensure!(
            heights.len() == 256 * 256 && water.len() == heights.len(),
            "terrain support: expected 256-by-256 height and water grids"
        );
        Ok(Self { heights, water })
    }

    /// Samples dry interior terrain. Returns `None` for unsupported water or grid edges.
    /// Input coordinates retain the original eight fractional bits.
    pub fn sample(&self, position: [i64; 3]) -> Option<GroundSupport> {
        let x = (position[0] >> 8) as f64;
        let z = (position[2] >> 8) as f64;
        let i = (x / 256.0 - 0.5).round_ties_even() as i32;
        let j = (z / 256.0 + 0.5).round_ties_even() as i32;
        if !(0..255).contains(&i) || !(2..=255).contains(&j) {
            return None;
        }
        let corner = |column: i32, row: i32| {
            let index = (256 - row) as usize * 256 + column as usize;
            (!self.water[index]).then(|| {
                DVec3::new(
                    f64::from(column * 256),
                    f64::from(self.heights[index]),
                    f64::from(row * 256),
                )
            })
        };
        let a = corner(i, j)?;
        let diagonal = corner(i + 1, j - 1)?;
        let (b, c) = if x - f64::from(i * 256) > f64::from(j * 256) - z {
            (corner(i + 1, j)?, diagonal)
        } else {
            (diagonal, corner(i, j - 1)?)
        };
        let plane = (c - a).cross(b - a);
        let normal = plane.normalize();
        let height = a.y - (plane.x * (x - a.x) + plane.z * (z - a.z)) / plane.y;
        Some(GroundSupport {
            height,
            normal: normal
                .to_array()
                .map(|value| (value * 256.0).round_ties_even() as i32),
        })
    }
}
