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

/// One synchronous AmbientSound invocation, using original Q8 emitter coordinates.
/// The remembered channel is a sample slot, not an effect or scene entity identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbientSoundRequest {
    pub generation: u64,
    pub actor_index: usize,
    pub channel: Option<u8>,
    pub effect: i32,
    pub volume: i32,
    pub rate_offset_hz: i32,
    pub position: [i32; 3],
    pub object_update_excluded: bool,
}

/// Updated native task scratch and the handler's active/completed result.
/// An active task may have no channel when sample allocation fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbientSoundUpdate {
    /// A remembered slot must be within the original sixteen sample channels.
    pub channel: Option<u8>,
    pub active: bool,
}

/// Synchronous sample status, positional mixing and channel ownership service.
/// Implementations follow preservation's engine/sound.md: zero volume or object
/// exclusion stops the remembered channel; status two restarts it; otherwise its
/// volume and pan are updated. Script effects bypass animation surface remapping.
pub trait AmbientSoundDriver: std::fmt::Debug + Send + Sync {
    fn update(&self, request: AmbientSoundRequest) -> Result<AmbientSoundUpdate, String>;
}

/// Synchronous sound effects from a logical script actor. Effect indices retain
/// their signed authored value; invalid starts allocate no channel.
#[derive(Debug, PartialEq, Eq)]
pub enum ScriptSound {
    /// Surface captured by the logical actor host when its animation command runs.
    Animation {
        sound: crate::animation::AnimationSound,
        surface: Option<u8>,
    },
    /// Positional start using the receiver's original Q8 emitter coordinates.
    /// Script Sound bypasses SURFACE.INI, retaining its authored effect on every surface.
    Positional {
        effect: i32,
        volume: i32,
        rate_offset_hz: i32,
        position: [i32; 3],
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
