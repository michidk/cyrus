//! Startup images rendered from the install's authored splash and title assets.
mod book_motion;
mod loading;
mod menu;
mod menu_raster;
mod movie;
mod settings;
use bevy::{asset::LoadState, prelude::*};
use cyrus_game::AppState;
pub use menu::MenuState;
pub use movie::MovieAudio;

/// Plays installed startup images through the installed book menu, before gameplay.
pub struct StartupPlugin;

/// Presence selects startup presentation instead of the scene-viewer preload UI.
#[derive(Resource, Default, serde::Serialize)]
pub struct StartupPresentation {
    /// Bypass splash playback while retaining required game preparation.
    pub skip: bool,
    pub(crate) title_visible: bool,
    pub(crate) error: Option<String>,
    #[serde(skip)]
    menu: Handle<cyrus_assets::menu::MenuAsset>,
}

impl StartupPresentation {
    /// Bypass splash playback while preserving required loading presentation.
    pub fn without_splash() -> Self {
        Self {
            skip: true,
            ..default()
        }
    }
    /// True after the installed loading artwork is selected for presentation.
    pub fn loading_visible(&self) -> bool {
        self.title_visible
    }
}

/// A fresh menu entry waits on the loading screen before entering its world.
#[derive(Resource, Default)]
pub struct StartGame {
    /// Menu entry uses installed transition artwork; direct inspections use the title.
    image: Handle<Image>,
    background: Vec<u8>,
    font: cyrus_text::Font,
    percent: u8,
}

/// Browser host attempts to close its tab without terminating the game loop.
#[derive(Message)]
pub struct QuitRequest;

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
        app.init_resource::<StartupPresentation>()
            .add_message::<QuitRequest>()
            .insert_resource(ClearColor(Color::BLACK))
            .add_systems(OnEnter(AppState::Loading), setup)
            .add_systems(
                Update,
                (fit_image, advance).run_if(in_state(AppState::Loading)),
            )
            .add_systems(OnExit(AppState::Loading), remove_content)
            .add_systems(OnExit(AppState::Menu), remove_content);
        menu::register(app);
        movie::register(app);
        loading::register(app);
    }
}

use cyrus_assets::SPLASH_FRAMES;

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut startup: ResMut<StartupPresentation>,
    game: Option<Res<StartGame>>,
) {
    startup.title_visible = false;
    startup.error = None;
    let frames: Vec<_> = (0..if startup.skip || game.is_some() {
        0
    } else {
        SPLASH_FRAMES
    })
        .map(|frame| assets.load(format!("cache://startup/splash-{frame:02}.png")))
        .collect();
    commands.spawn((Camera2d, StartupContent));
    let title = game
        .as_ref()
        .filter(|game| game.image != Handle::default())
        .map_or_else(
            || assets.load("cache://startup/title.png"),
            |game| game.image.clone(),
        );
    commands.spawn((
        Sprite::from_image(frames.first().unwrap_or(&title).clone()),
        StartupContent,
    ));
    commands.insert_resource(Playback {
        frames,
        title,
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
    rendering: (Query<&mut Sprite, With<StartupContent>>, Res<Assets<Image>>),
    next: Res<NextState<AppState>>,
    mut startup: ResMut<StartupPresentation>,
) {
    let (mut image, images) = rendering;
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
        if images.contains(handle.id()) {
            continue;
        }
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
    if startup.skip || (skip && !was_title) {
        playback.elapsed = splash_duration;
    } else {
        playback.elapsed += input.0.delta_secs();
    }
    if playback.elapsed < splash_duration {
        sprite.image = playback.frames[(playback.elapsed * 30.0) as usize].clone();
    } else {
        sprite.image = playback.title.clone();
        startup.title_visible = true;
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
