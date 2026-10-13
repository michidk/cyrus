//! Empty-map Wander follows preservation's SOUP navigation and distance contracts.
use super::spatial::{ActorSpatial, rounded_distance};
use crate::random::EngineRandom;
use anyhow::{Result, ensure};

pub(super) fn wander_without_maps(
    home: [i32; 3],
    radius: i32,
    frame_scale: u32,
    spatial: &mut ActorSpatial,
    random: &mut EngineRandom,
) -> Result<i32> {
    let radius = radius >> 8;
    ensure!(
        radius > 0,
        "Wander radius must contain a positive map-unit radius"
    );
    let current = spatial.query_pose()?.position.map(|v| v / 256);
    loop {
        let x = home[0]
            .wrapping_add(radius)
            .wrapping_sub(random.rnd(radius * 2)?);
        let z = home[2]
            .wrapping_add(radius)
            .wrapping_sub(random.rnd(radius * 2)?);
        if rounded_distance([x.wrapping_sub(current[0]), 0, z.wrapping_sub(current[2])])?
            > radius / 2
        {
            break;
        }
    }
    let previous = spatial.previous_local_position;
    let displacement = rounded_distance([
        spatial.local.position[0].wrapping_sub(previous[0]),
        0,
        spatial.local.position[2].wrapping_sub(previous[2]),
    ])?;
    let limit = (frame_scale.wrapping_shl(13) as i32) >> 8;
    if displacement > limit {
        spatial.local.position = previous;
    }
    Ok(displacement)
}
