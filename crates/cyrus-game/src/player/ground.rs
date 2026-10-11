//! Ground support in original map coordinates, per preservation's engine/player.md.
use bevy::math::DVec3;

/// Decoded WLD heights in source row order. Engine Y is positive down.
/// Water vertices must be excluded until moving-water support is available.
pub struct TerrainGround {
    heights: Vec<i16>,
    water: Vec<bool>,
}

/// A terrain plane result before converting its height to the actor origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundSupport {
    pub height: f64,
    /// Face normal in original engine axes, scaled by 256 and rounded to even.
    pub normal: [i32; 3],
}

/// A model contact after placement transformation and integer probe conversion.
pub struct ModelSupport {
    pub model: usize,
    pub face: usize,
    pub support: GroundSupport,
    /// Stored segment fraction scaled by 32,768, unchanged by the integer adapter.
    pub rank: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// Identity of the selected surface, retained independently of its height.
pub enum SupportSource {
    Terrain,
    Model { model: usize, face: usize },
}

/// Selected support and its identity for subsequent actor/contact state updates.
#[derive(Debug, PartialEq)]
pub enum SelectedSupport {
    Primary {
        source: SupportSource,
        support: GroundSupport,
    },
    /// Secondary sphere support retains the actor origin instead of using a plane height.
    Fallback {
        model: usize,
        face: usize,
        normal: [i32; 3],
        actor_height: i64,
    },
}

impl SelectedSupport {
    /// Identity used for texture/surface lookup and supporting-model state.
    pub fn source(&self) -> SupportSource {
        match self {
            Self::Primary { source, .. } => *source,
            Self::Fallback { model, face, .. } => SupportSource::Model {
                model: *model,
                face: *face,
            },
        }
    }

    /// Actor origin in runtime units; fallback retains even its fractional Y bits.
    pub fn actor_height(&self, positive_y_extent: i32, standing_height: i32) -> i64 {
        match self {
            Self::Primary { support, .. } => {
                support.actor_height(positive_y_extent, standing_height)
            }
            Self::Fallback { actor_height, .. } => *actor_height,
        }
    }

    /// Stored support class, including the original collision-attribute steep-ground override.
    pub fn classification(&self, collision_attribute: u8) -> u8 {
        let class = match self {
            Self::Primary { support, .. } => support.classification(),
            Self::Fallback { normal, .. } => classify_normal(normal[1]),
        };
        if class == 2 && collision_attribute & 2 == 0 {
            4
        } else {
            class
        }
    }

    /// Final forward admission after the support updater has run.
    /// `surface_veto` is the updater's retained shared value, not a recomputed material flag.
    pub fn allows_forward(&self, collision_attribute: u8, surface_veto: i32) -> bool {
        self.classification(collision_attribute) != 2 && surface_veto != 1
    }
}

/// Ordinary vertical probe in integer map units. Engine Y increases downward.
pub struct GroundProbe {
    start: [i32; 3],
    end: [i32; 3],
    distance: i32,
}

/// Selected-player secondary sphere, used only after the primary support query misses.
pub struct FallbackProbe {
    pub(crate) center: [i32; 3],
    pub(crate) radius: f32,
    pub(crate) actor_height: i64,
}

impl FallbackProbe {
    /// Position has eight fractional bits; collision offsets/extents use map units.
    /// `support_history_active` means the original shared support counter is nonzero.
    pub fn new(
        position: [i64; 3],
        center_offset: [i32; 3],
        positive_y_extent: i32,
        support_history_active: bool,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            positive_y_extent >= 0,
            "fallback probe: negative positive Y extent"
        );
        let mut center = [0; 3];
        for axis in 0..3 {
            let mut coordinate = (position[axis] >> 8) + i64::from(center_offset[axis]);
            if axis == 1 {
                coordinate += i64::from(positive_y_extent) - 3;
            }
            center[axis] = i32::try_from(coordinate)?;
        }
        Ok(Self {
            center,
            radius: if support_history_active { 14.0 } else { 10.0 },
            actor_height: position[1],
        })
    }
}

impl GroundProbe {
    /// Converts runtime coordinates before applying the body extent and search distance.
    pub fn new(position: [i64; 3], positive_y_extent: i32, distance: u32) -> anyhow::Result<Self> {
        anyhow::ensure!(
            distance > 0,
            "ground probe: search distance must be positive"
        );
        let distance = i32::try_from(distance)?;
        let x = i32::try_from(position[0] >> 8)?;
        let z = i32::try_from(position[2] >> 8)?;
        let center = (position[1] >> 8) + i64::from(positive_y_extent);
        let start_y = i32::try_from(center - i64::from(distance))?;
        let end_y = i32::try_from(center + i64::from(distance))?;
        Ok(Self {
            start: [x, start_y, z],
            end: [x, end_y, z],
            distance,
        })
    }

    /// Upper and lower segment endpoints in integer map coordinates.
    pub fn endpoints(&self) -> ([i32; 3], [i32; 3]) {
        (self.start, self.end)
    }

    /// Select the proposed-position surface for the separate forward slope check.
    /// Uses distance from segment midpoint, preserving the first equal model contact.
    pub fn slope_surface(
        &self,
        terrain: Option<GroundSupport>,
        models: &[ModelSupport],
    ) -> Option<GroundSupport> {
        let center = i64::from(self.start[1]) + i64::from(self.distance);
        let mut rank = 999_999;
        let mut height = 9_999_999;
        let mut selected = None;
        for model in models {
            let distance = i64::from(model.rank).abs_diff(16_384);
            if matches!(model.support.classification(), 2 | 4) && distance < rank {
                rank = distance;
                height = model.support.height as i64;
                selected = Some(model.support);
            }
        }
        if let Some(support) = terrain.filter(|support| support.height <= f64::from(self.end[1])) {
            let distance = (support.height - center as f64).trunc().abs();
            if distance < height.abs_diff(center) as f64 {
                selected = Some(support);
            }
        }
        selected
    }

    /// Selects from caller-admitted model contacts in original traversal order.
    /// A missing result still requires the original secondary probe/falling response.
    pub fn select(
        &self,
        terrain: Option<GroundSupport>,
        models: &[ModelSupport],
    ) -> Option<SelectedSupport> {
        let mut rank = 65_536;
        let mut selected = None;
        if let Some(support) = terrain.filter(|support| support.height <= f64::from(self.end[1])) {
            rank = if support.height < f64::from(self.start[1]) {
                65_535
            } else {
                let delta = support.height.trunc() as i64 - i64::from(self.start[1]);
                (((16_777_216 / (2 * i64::from(self.distance))) * delta) >> 8) as u32
            };
            selected = Some(SelectedSupport::Primary {
                source: SupportSource::Terrain,
                support,
            });
        }
        for model in models {
            if matches!(model.support.classification(), 2 | 4) && model.rank <= rank {
                rank = model.rank;
                selected = Some(SelectedSupport::Primary {
                    source: SupportSource::Model {
                        model: model.model,
                        face: model.face,
                    },
                    support: model.support,
                });
            }
        }
        selected
    }
}

impl GroundSupport {
    /// Original contact classification. A steep surface is not ordinary walkable ground.
    pub fn classification(&self) -> u8 {
        classify_normal(self.normal[1])
    }

    /// Actor origin in runtime units with eight fractional bits.
    /// Extent and standing offset are installed values in map units.
    pub fn actor_height(&self, positive_y_extent: i32, standing_height: i32) -> i64 {
        (self.height.trunc() as i64 - i64::from(positive_y_extent) + i64::from(standing_height))
            * 256
    }
}

fn classify_normal(y: i32) -> u8 {
    match y {
        ..=-161 => 4,
        -160..=-42 => 2,
        -41..=160 => 1,
        _ => 8,
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
