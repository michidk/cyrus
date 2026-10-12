//! Sound intents; only a playback consumer registers the message queue.
use bevy::prelude::*;
/// A sound trigger from game code. Consumed by the presentation sound plugin on
/// native and browser builds; senders never play audio themselves.
#[derive(Message, Debug)]
pub enum SoundCue {
    /// The original's `FlatSound`: pan 64, volume `clamp(base_volume + volume, 0, 127)`,
    /// rate `clamp(sample_rate + rate_offset_hz, 512, 44000)` Hz, no owner.
    Flat {
        effect: usize,
        volume: i32,
        rate_offset_hz: i32,
    },
    /// An authored animation sound of `actor`, which must carry a `GlobalTransform` and
    /// a [`SoundSurface`]; without the surface its starts are reported and not played.
    Animation {
        actor: Entity,
        sound: crate::animation::AnimationSound,
    },
}

/// The SURFACE.INI surface type of the face an actor stands on (preservation
/// `docs/engine/sound.md`), set by the code that knows the ground; 0 means no remap.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundSurface(pub u8);
