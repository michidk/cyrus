//! Startup images rendered from the install's authored splash and title assets.
#[cfg(not(target_arch = "wasm32"))]
mod export;
mod menu;
mod menu_data;
mod menu_preferences;
pub use menu_preferences::MenuPreferences;
#[cfg(not(target_arch = "wasm32"))]
mod menu_export;
mod menu_raster;
pub use menu::MenuState;
#[cfg(not(target_arch = "wasm32"))]
pub use menu_export::export_menu;
#[cfg(not(target_arch = "wasm32"))]
mod raster;
#[cfg(not(target_arch = "wasm32"))]
pub use export::export_startup;

use crate::AppState;
use bevy::{asset::LoadState, prelude::*};

/// Plays installed startup images through the installed book menu, before gameplay.
pub struct StartupPlugin;

/// Presence selects startup presentation instead of the scene-viewer preload UI.
#[derive(Resource)]
pub(crate) struct StartupPresentation;

#[derive(Resource)]
struct Playback {
    frames: Vec<Handle<Image>>,
    title: Handle<Image>,
    elapsed: f32,
    failed: bool,
}

#[derive(Component)]
struct StartupContent;

impl Plugin for StartupPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(StartupPresentation)
            .insert_resource(ClearColor(Color::BLACK))
            .add_systems(OnEnter(AppState::Loading), setup)
            .add_systems(
                Update,
                (fit_image, advance).run_if(in_state(AppState::Loading)),
            )
            .add_systems(OnExit(AppState::Loading), remove_content)
            .add_systems(OnExit(AppState::Menu), remove_content);
        menu::register(app);
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let frames: Vec<_> = (1..75)
        .map(|frame| assets.load(format!("cache://startup/splash-{frame:02}.png")))
        .collect();
    commands.spawn((Camera2d, StartupContent));
    commands.spawn((Sprite::from_image(frames[0].clone()), StartupContent));
    commands.insert_resource(Playback {
        frames,
        title: assets.load("cache://startup/title.png"),
        elapsed: 0.0,
        failed: false,
    });
}

/// Fits the original 640×480 viewport within any window, with black borders.
fn viewport_scale(size: Vec2) -> f32 {
    (size.x / 640.0).min(size.y / 480.0)
}

fn fit_image(
    windows: Query<&Window>,
    mut image: Query<&mut Transform, (With<StartupContent>, With<Sprite>)>,
) {
    if let (Ok(window), Ok(mut transform)) = (windows.single(), image.single_mut()) {
        transform.scale = Vec3::splat(viewport_scale(Vec2::new(window.width(), window.height())));
    }
}

fn advance(
    mut commands: Commands,
    assets: Res<AssetServer>,
    input: (Res<Time>, Res<ButtonInput<KeyCode>>),
    mut playback: ResMut<Playback>,
    mut image: Query<&mut Sprite, With<StartupContent>>,
    mut next: ResMut<NextState<AppState>>,
    #[cfg(not(target_arch = "wasm32"))] preloaded: Option<Res<crate::loading::Preloaded>>,
) {
    if matches!(*next, NextState::Pending(AppState::InGame)) {
        return;
    }
    if playback.failed {
        return;
    }
    for handle in playback
        .frames
        .iter()
        .chain(std::iter::once(&playback.title))
    {
        if let Some(LoadState::Failed(error)) = assets.get_load_state(handle.id()) {
            error!("startup: {error}");
            commands.spawn((
                Text::new("Startup assets unavailable. Export them with --export-startup."),
                StartupContent,
            ));
            playback.failed = true;
            return;
        }
        if !assets.is_loaded_with_dependencies(handle.id()) {
            return;
        }
    }
    let Ok(mut sprite) = image.single_mut() else {
        return;
    };
    let splash_duration = playback.frames.len() as f32 / 30.0;
    let skip = [KeyCode::Enter, KeyCode::Space, KeyCode::Escape]
        .iter()
        .any(|key| input.1.just_pressed(*key));
    let was_title = playback.elapsed >= splash_duration;
    if skip && !was_title {
        playback.elapsed = splash_duration;
    } else {
        playback.elapsed += input.0.delta_secs();
    }
    if playback.elapsed < splash_duration {
        sprite.image = playback.frames[(playback.elapsed * 30.0) as usize].clone();
    } else {
        sprite.image = playback.title.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let ready = preloaded.is_some();
        #[cfg(target_arch = "wasm32")]
        let ready = true;
        // Playback duration is presentation policy, not original load timing.
        if ready && (playback.elapsed >= splash_duration + 1.0 || (skip && was_title)) {
            next.set(AppState::Menu);
        }
    }
}

fn remove_content(mut commands: Commands, content: Query<Entity, With<StartupContent>>) {
    for entity in &content {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<Playback>();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_viewport_fits_without_stretching() {
        assert_eq!(viewport_scale(Vec2::new(640.0, 480.0)), 1.0);
        assert_eq!(viewport_scale(Vec2::new(1280.0, 720.0)), 1.5);
        assert_eq!(viewport_scale(Vec2::new(480.0, 800.0)), 0.75);
    }
}
