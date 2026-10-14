//! Actor-style sphere overlap follows preservation's ordered shape query rules.
use anyhow::{Result, ensure};
use rustc_apfloat::{
    Float, FloatConvert, Round, Status,
    ieee::{Single, X87DoubleExtended},
};

/// A selected runtime sphere after the owner's matrix and rounded-origin transform.
/// Centers and radius are stored single-precision original map units. The host
/// owns shape selection, transforms, identity exclusion and candidate admission.
#[derive(Clone, Copy, Debug)]
pub struct OverlapSphere {
    pub center: [f32; 3],
    pub radius: f32,
}

/// Return horizontal separation for the first strictly overlapping pair, or zero.
/// Visits at most the first 32 spheres of each selected shape in shape order.
/// The actor-to-actor query truncates the summed radius, not each radius separately.
/// The caller must exclude self before submitting two shapes.
pub fn actor_separation(left: &[OverlapSphere], right: &[OverlapSphere]) -> Result<i32> {
    separation(left, right, RadiusRule::Sum)
}

/// Query an actor's selected spheres against one actor-style static placement.
/// Unlike actor-to-actor overlap, truncates both radii separately before their sum.
/// The placement's sphere is centered on its stored map-coordinate position.
pub fn static_separation(actor: &[OverlapSphere], placement: OverlapSphere) -> Result<i32> {
    separation(actor, &[placement], RadiusRule::Separate)
}

#[derive(Clone, Copy)]
enum RadiusRule {
    Sum,
    Separate,
}

fn separation(left: &[OverlapSphere], right: &[OverlapSphere], rule: RadiusRule) -> Result<i32> {
    let left = &left[..left.len().min(32)];
    let right = &right[..right.len().min(32)];
    for sphere in left.iter().chain(right) {
        ensure!(
            sphere.center.iter().all(|v| v.is_finite())
                && sphere.radius.is_finite()
                && sphere.radius >= 0.0,
            "actor overlap: invalid world sphere"
        );
    }
    for a in left {
        for b in right {
            let radius = match rule {
                RadiusRule::Sum => integer((extended(a.radius) + extended(b.radius)).value)?,
                RadiusRule::Separate => integer(extended(a.radius))?
                    .checked_add(integer(extended(b.radius))?)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "static overlap: summed radius outside signed integer range"
                        )
                    })?,
            };
            let mut difference = [0; 3];
            for (axis, value) in difference.iter_mut().enumerate() {
                *value = integer((extended(a.center[axis]) - extended(b.center[axis])).value)?;
            }
            if crate::distance::rounded_distance(difference)? < radius {
                difference[1] = 0;
                return Ok(radius - crate::distance::rounded_distance(difference)?);
            }
        }
    }
    Ok(0)
}

fn extended(value: f32) -> X87DoubleExtended {
    Single::from_bits(u128::from(value.to_bits()))
        .convert(&mut false)
        .value
}

fn integer(value: X87DoubleExtended) -> Result<i32> {
    let value = value.to_i128_r(32, Round::TowardZero, &mut false);
    ensure!(
        !value.status.contains(Status::INVALID_OP),
        "actor overlap: coordinate difference or radius outside signed integer range"
    );
    Ok(value.value as i32)
}
