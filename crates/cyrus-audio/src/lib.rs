//! Effects, speech and CD music playback, independent of rendering and menu presentation.
pub mod music;
pub mod sound;
#[cfg(target_arch = "wasm32")]
mod sound_bank;
use bevy::prelude::*;
pub use music::MusicPlugin;
pub use sound::{ApplyPreferences, PlayCues, SoundPlugin};
#[cfg(target_arch = "wasm32")]
pub use sound_bank::{SoundBank, SoundBankHandle};

/// Host readiness to prepare installed effects and speech; standalone hosts prepare immediately.
#[derive(Resource)]
pub struct AudioReadiness {
    pub prepare: bool,
}
impl Default for AudioReadiness {
    fn default() -> Self {
        Self { prepare: true }
    }
}

/// Optional listener pose in Bevy world space; positional starts require its presence.
#[derive(Resource)]
pub struct AudioListener(pub GlobalTransform);

/// Presence pauses CD music while cinematic audio owns playback.
#[derive(Component)]
pub struct CinematicAudio;
