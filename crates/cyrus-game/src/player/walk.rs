//! Forward-walking startup from redguard-preservation's
//! `docs/engine/player.md`. Animation eligibility and collision are separate.

/// Startup state for ordinary forward walking with `attr_step_type = 1`.
pub struct WalkingStartup {
    phase: u8,
}

impl WalkingStartup {
    /// Begin at phase zero. Other actor step types remain unsupported.
    pub fn new(step_type: u8) -> Option<Self> {
        (step_type == 1).then_some(Self { phase: 0 })
    }

    /// Advance one eligible walking update, returning distance before collision.
    ///
    /// `forward_walk` is the actor attribute, and `frame_scale` has eight
    /// fractional bits. The result is in runtime position units, also with eight
    /// fractional bits. Call only when walking is eligible; an animation that
    /// suppresses translation still advances startup. The caller owns phase
    /// reset, whose relationship to input release is not yet specified.
    pub fn advance(
        &mut self,
        forward_walk: u8,
        frame_scale: u32,
        animation_allows_translation: bool,
    ) -> u64 {
        let speed = u64::from(forward_walk) * 256;
        let speed = match self.phase {
            0 => speed / 4,
            1 => speed / 3,
            2 => speed / 2,
            _ => speed,
        };
        self.phase = (self.phase + 1).min(3);
        if animation_allows_translation {
            (speed * u64::from(frame_scale)) >> 8
        } else {
            0
        }
    }
}
