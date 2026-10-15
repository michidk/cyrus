//! Player attributes and upright collision bounds from preservation's RGM metadata.
use anyhow::Context;
use serde::Deserialize;

/// Installed CYRUS definition. Extents are nonnegative map units in original axes.
#[derive(Clone, Debug)]
pub struct PlayerDefinition {
    /// Ordinary sheathed idle default from `attr_panic_group`, per preservation player.md.
    pub ordinary_default_group: u8,
    pub forward_walk: u8,
    pub forward_speed: u8,
    pub step_type: u8,
    pub positive_extent: [i32; 3],
    pub negative_extent: [i32; 3],
    collision_scale: i32,
}

pub use cyrus_collision::actor_shape::BodySphere;

#[derive(Deserialize)]
struct Bounds {
    total_extent: [i32; 3],
    pos_extent: [i32; 3],
    neg_extent: [i32; 3],
    center_offset: [i32; 3],
    flags: u8,
}

impl PlayerDefinition {
    /// Read metadata exported with the installed SOUP386.DEF attribute names.
    /// Missing zero-valued attributes are zero, as in rgpre's sparse export.
    pub fn from_metadata(metadata: &serde_json::Value) -> anyhow::Result<Self> {
        let actor = metadata["actors"]
            .as_array()
            .context("CYRUS: missing actor definitions")?
            .iter()
            .find(|actor| actor["script_name"].as_str() == Some("CYRUS"))
            .context("CYRUS: actor definition absent")?;
        let attributes = actor["attributes"]
            .as_object()
            .context("CYRUS: attributes absent")?;
        anyhow::ensure!(
            !attributes.keys().any(|key| key
                .strip_prefix("attr_")
                .is_some_and(|suffix| suffix.parse::<u32>().is_ok())),
            "CYRUS: unnamed attributes; export with installed SOUP386.DEF"
        );
        let attribute = |name: &str| -> anyhow::Result<u8> {
            match attributes.get(name) {
                None => Ok(0),
                Some(value) => u8::try_from(
                    value
                        .as_u64()
                        .with_context(|| format!("CYRUS {name}: expected byte"))?,
                )
                .with_context(|| format!("CYRUS {name}: outside byte range")),
            }
        };
        let index = actor["mpsz_bounds_primary"]
            .as_u64()
            .context("CYRUS: primary bounds absent")?;
        let index = usize::try_from(index).context("CYRUS: primary bounds index outside range")?;
        let bounds: Bounds = serde_json::from_value(
            metadata["mpsz_entries"]
                .as_array()
                .and_then(|entries| entries.get(index))
                .context("CYRUS: primary MPSZ record absent")?
                .clone(),
        )
        .context("CYRUS: invalid primary MPSZ record")?;
        anyhow::ensure!(
            bounds.center_offset == [0; 3] && bounds.flags == 0,
            "CYRUS MPSZ: shifted or flagged bounds unsupported"
        );
        for axis in 0..3 {
            anyhow::ensure!(
                bounds.pos_extent[axis] >= 0
                    && bounds.neg_extent[axis] >= 0
                    && i64::from(bounds.pos_extent[axis]) + i64::from(bounds.neg_extent[axis])
                        == i64::from(bounds.total_extent[axis]),
                "CYRUS MPSZ axis {axis}: inconsistent extents"
            );
        }
        let scale = actor["raex"]["unknown_12"]
            .as_i64()
            .context("CYRUS RAEX: collision scale absent")?;
        let scale =
            i16::try_from(scale).context("CYRUS RAEX: collision scale outside i16 range")?;
        Ok(Self {
            ordinary_default_group: attribute("attr_panic_group")?,
            forward_walk: attribute("attr_forward_walk")?,
            forward_speed: attribute("attr_forward_speed")?,
            step_type: attribute("attr_step_type")?,
            positive_extent: bounds.pos_extent,
            negative_extent: bounds.neg_extent,
            collision_scale: if scale > 0 { i32::from(scale) } else { 256 },
        })
    }

    /// Build the forward clearance probe in runtime units using the actor matrix.
    /// Preserves actor Y; conversion follows preservation player.md's forward probe rules.
    pub fn forward_probe(
        &self,
        position: [i64; 3],
        rotation: bevy::math::Mat3,
    ) -> anyhow::Result<[i64; 3]> {
        let distance = (i64::from(self.positive_extent[2]) + i64::from(self.forward_walk)) * 256;
        let distance = i32::try_from(distance)
            .context("CYRUS forward probe: local distance outside runtime i32 range")?
            as f32;
        let mut probe = position;
        for axis in [0, 2] {
            let offset = (rotation.z_axis[axis] * distance).round_ties_even();
            anyhow::ensure!(
                offset.is_finite() && (-2147483648.0..2147483648.0).contains(&f64::from(offset)),
                "CYRUS forward probe axis {axis}: rotated offset outside runtime i32 range"
            );
            probe[axis] = position[axis]
                .checked_add(offset as i64)
                .with_context(|| format!("CYRUS forward probe axis {axis}: position overflow"))?;
        }
        Ok(probe)
    }

    /// Build ordinary obstacle spheres from installed bounds and post-collision height.
    /// Shape construction follows preservation's actor collision sphere rules.
    pub fn body_spheres(&self, post_collide_height: u32) -> anyhow::Result<Vec<BodySphere>> {
        cyrus_collision::actor_shape::ActorBounds::new(
            self.positive_extent,
            self.negative_extent,
            self.collision_scale,
        )?
        .body_spheres(post_collide_height)
        .context("CYRUS collision shape")
    }
}
