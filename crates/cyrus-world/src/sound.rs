//! Sound intents; only a playback consumer registers the message queue.
use bevy::prelude::*;
/// A sound trigger from game code. Consumed by the presentation sound plugin on
/// native and browser builds; senders never play audio themselves.
#[derive(Message, Debug)]
pub enum SoundCue {
    /// A logical script receiver owns this command independently of scene bindings.
    Script {
        generation: u64,
        index: usize,
        sound: ScriptSound,
    },
    /// The original's `FlatSound`: pan 64, volume `clamp(base_volume + volume, 0, 127)`,
    /// rate `clamp(sample_rate + rate_offset_hz, 512, 44000)` Hz, no owner.
    Flat {
        effect: usize,
        volume: i32,
        rate_offset_hz: i32,
    },
    /// An authored animation sound of `actor`, which must carry a `GlobalTransform`.
    /// Capture the stored surface when the command runs, before later ground updates.
    Animation {
        actor: Entity,
        sound: crate::animation::AnimationSound,
        /// None leaves Play unsupported; Stop and Forget do not need a surface.
        /// Zero bypasses remapping. Other codes select the installed SURFACE.INI row.
        surface: Option<u8>,
    },
}

/// The SURFACE.INI surface type of the face an actor stands on (preservation
/// `docs/engine/sound.md`), set by the code that knows the ground; 0 means no remap.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundSurface(pub u8);

/// Synchronous sound effects from a logical script actor. Effect indices retain
/// their signed authored value; invalid starts allocate no channel.
#[derive(Debug, PartialEq, Eq)]
pub enum ScriptSound {
    /// Surface captured by the logical actor host when its animation command runs.
    Animation {
        sound: crate::animation::AnimationSound,
        surface: Option<u8>,
    },
    Flat {
        effect: i32,
        volume: i32,
        rate_offset_hz: i32,
    },
    StopEffect {
        effect: i32,
        owned: bool,
    },
    StopOwned,
}
