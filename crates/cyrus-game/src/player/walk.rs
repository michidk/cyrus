//! Forward-walking startup from redguard-preservation's
//! `docs/engine/player.md`. Animation eligibility and collision are separate.

/// Startup state for ordinary forward walking with `attr_step_type = 1`.
pub struct WalkingStartup {
    phase: u8,
}

/// Result of an admitted ordinary sheathed walk request, before collision response.
pub struct WalkingStep {
    pub distance: u64,
    /// File frame reference produced by the request's immediate animation advance.
    pub frame: Option<u32>,
}

impl WalkingStartup {
    /// Request the supported ordinary walking group before reading its movement marker.
    /// Caller must establish sheathed/non-combat eligibility and pass the forward probe.
    /// Missing groups do not advance startup; sound errors propagate without translation.
    pub fn request(
        &mut self,
        animation: &mut crate::animation::ActorAnimation,
        definition: &super::definition::PlayerDefinition,
        frame_scale: u32,
        nominal_ticks: u32,
        sounds: &mut dyn FnMut(crate::animation::AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<WalkingStep>> {
        if !animation.has_group(20) {
            return Ok(None);
        }
        let frame = animation.request_with_sounds(20, 0, nominal_ticks, sounds)?;
        let distance = self.advance(
            definition.forward_walk,
            definition.forward_speed,
            frame_scale,
            !animation.translation_suppressed(),
        );
        Ok(Some(WalkingStep { distance, frame }))
    }

    /// Begin at phase zero. Other actor step types remain unsupported.
    pub fn new(step_type: u8) -> Option<Self> {
        (step_type == 1).then_some(Self { phase: 0 })
    }

    /// Reset startup after an actor update that did not accept forward movement.
    /// Animation suppression alone does not mean movement was rejected.
    pub fn finish_update(&mut self, accepted_forward: bool) {
        if !accepted_forward {
            self.phase = 0;
        }
    }

    /// Advance one eligible walking update, returning distance before collision.
    ///
    /// Zero `forward_walk` selects `forward_speed >> 1` without multiplying by 256.
    /// Both speeds are actor attributes, and `frame_scale` has eight
    /// fractional bits. The result is in runtime position units, also with eight
    /// fractional bits. Call only when walking is eligible; an animation that
    /// suppresses translation still advances startup. Finish each actor update
    /// with [`Self::finish_update`], including updates without walking.
    pub fn advance(
        &mut self,
        forward_walk: u8,
        forward_speed: u8,
        frame_scale: u32,
        animation_allows_translation: bool,
    ) -> u64 {
        let speed = if forward_walk == 0 {
            u64::from(forward_speed >> 1)
        } else {
            u64::from(forward_walk) * 256
        };
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
