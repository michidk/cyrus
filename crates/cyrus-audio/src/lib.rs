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

/// Copies exactly one host-selected listener pose; absent or ambiguous selections clear it.
/// Hosts schedule this before [`ApplyPreferences`] to retain cue preparation order.
pub fn follow_listener<T: Component>(
    mut commands: Commands,
    camera: Query<&GlobalTransform, With<T>>,
) {
    if let Ok(pose) = camera.single() {
        commands.insert_resource(AudioListener(*pose));
    } else {
        commands.remove_resource::<AudioListener>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Component)]
    struct ListenerMarker;

    #[test]
    fn audio_listener_requires_one_camera_and_clears_stale_pose() {
        let mut app = App::new();
        app.add_systems(
            Update,
            follow_listener::<ListenerMarker>.before(ApplyPreferences),
        );
        app.update();
        assert!(!app.world().contains_resource::<AudioListener>());
        let pose = GlobalTransform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let first = app.world_mut().spawn((ListenerMarker, pose)).id();
        app.update();
        assert_eq!(app.world().resource::<AudioListener>().0, pose);
        let second = app
            .world_mut()
            .spawn((ListenerMarker, GlobalTransform::IDENTITY))
            .id();
        app.update();
        assert!(!app.world().contains_resource::<AudioListener>());
        app.world_mut().despawn(second);
        app.update();
        assert!(app.world().contains_resource::<AudioListener>());
        app.world_mut().despawn(first);
        app.update();
        assert!(!app.world().contains_resource::<AudioListener>());
    }
}
