//! Installed splash, title, book menu, settings, cinematics and world-entry presentation.
mod book_motion;
mod fade;
mod loading;
mod logbook;
pub use logbook::LogbookPlugin;
mod menu;
mod menu_raster;
mod movie;
#[cfg(not(target_arch = "wasm32"))]
mod movie_native;
#[cfg(target_arch = "wasm32")]
mod movie_stream;
#[cfg(not(target_arch = "wasm32"))]
mod preparation;
mod settings;
use bevy::{asset::LoadState, prelude::*};
use cyrus_game::AppState;
pub use menu::MenuState;
#[cfg(not(target_arch = "wasm32"))]
pub use preparation::LoadingPlugin;

/// Plays installed startup images through the installed book menu, before gameplay.
pub struct StartupPlugin;

/// Installed 3dfx splash variant played before the title. Both ship with the GOG release.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Splash {
    /// `3DfxSpl2.dll`, which GOG's Windows DOSBox plays when the game opens Glide.
    #[default]
    Windows,
    /// The older shield animation in the DOS Glide overlay `glide2x_emu.ovl`.
    Dos,
}

impl Splash {
    /// Parses the shared launch option; absent selects the Windows default.
    pub fn from_option(value: Option<&str>) -> Self {
        if value == Some("dos") {
            Self::Dos
        } else {
            Self::Windows
        }
    }
}

/// Presence selects startup presentation instead of the scene-viewer preload UI.
#[derive(Resource, Default, serde::Serialize)]
pub struct StartupPresentation {
    /// Bypass splash playback while retaining required game preparation.
    pub skip: bool,
    pub splash: Splash,
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
    progress: (usize, usize),
    /// Menu entries fade the loading screen in, then out before entering the world.
    faded_in: bool,
    fade_out: Option<Entity>,
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
            .insert_resource(cyrus_audio::AudioReadiness { prepare: false })
            .add_message::<QuitRequest>()
            .insert_resource(ClearColor(Color::BLACK))
            .add_systems(OnEnter(AppState::Loading), setup)
            .add_systems(
                Update,
                (fit_image, advance).run_if(in_state(AppState::Loading)),
            )
            .add_systems(OnExit(AppState::Loading), remove_content)
            .add_systems(OnExit(AppState::Menu), remove_content);
        app.add_systems(
            Update,
            sync_audio
                .after(advance)
                .after(menu::drive)
                .before(cyrus_audio::ApplyPreferences),
        );
        menu::register(app);
        movie::register(app);
        loading::register(app);
        app.add_plugins(LogbookPlugin);
    }
}

fn sync_audio(
    startup: Res<StartupPresentation>,
    menu: Option<Res<MenuState>>,
    mut readiness: ResMut<cyrus_audio::AudioReadiness>,
    settings: Option<ResMut<cyrus_audio::sound::SoundSettings>>,
) {
    readiness.prepare = startup.title_visible;
    if let (Some(preferences), Some(mut settings)) = (
        menu.as_ref().and_then(|menu| menu.preferences.as_ref()),
        settings,
    ) {
        let mut next = settings.clone();
        next.apply_menu(preferences);
        if (
            next.master_volume,
            next.redbook_volume,
            next.speech,
            next.subtitles,
        ) != (
            settings.master_volume,
            settings.redbook_volume,
            settings.speech,
            settings.subtitles,
        ) {
            *settings = next;
        }
    }
}

use cyrus_assets::{DOS_SPLASH_FRAMES, SPLASH_FRAMES};

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut startup: ResMut<StartupPresentation>,
    game: Option<Res<StartGame>>,
) {
    startup.title_visible = false;
    startup.error = None;
    let (prefix, count) = match startup.splash {
        Splash::Windows => ("splash", SPLASH_FRAMES),
        Splash::Dos => ("splash-dos", DOS_SPLASH_FRAMES),
    };
    let count = if startup.skip || game.is_some() {
        0
    } else {
        count
    };
    let frames: Vec<_> = (0..count)
        .map(|frame| assets.load(format!("cache://startup/{prefix}-{frame:02}.png")))
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
    fn audio_host_copies_committed_preferences_before_cues() {
        use cyrus_assets::{preferences::MenuPreferences, sound::SoundSettings};
        use cyrus_audio::{ApplyPreferences, AudioReadiness, PlayCues};
        let mut app = App::new();
        app.init_resource::<StartupPresentation>()
            .init_resource::<AudioReadiness>()
            .insert_resource(SoundSettings::parse(""))
            .configure_sets(Update, (ApplyPreferences, PlayCues).chain())
            .add_systems(Update, sync_audio.before(ApplyPreferences));
        app.update();
        assert!(!app.world().resource::<AudioReadiness>().prepare);
        app.init_resource::<MenuState>();
        let preferences = MenuPreferences {
            values: [(601, 0), (602, 0), (603, 0), (504, 0), (505, 0)].into(),
            bindings: [0; 15],
        };
        app.world_mut().resource_mut::<MenuState>().preferences = Some(preferences);
        app.world_mut()
            .resource_mut::<StartupPresentation>()
            .title_visible = true;
        app.add_systems(
            Update,
            (|settings: Res<SoundSettings>,
              readiness: Res<AudioReadiness>,
              menu: Res<MenuState>| {
                assert!(readiness.prepare);
                assert_eq!(
                    (
                        settings.master_volume,
                        settings.redbook_volume,
                        settings.speech
                    ),
                    (0, 0, false)
                );
                assert_eq!(
                    settings.subtitles,
                    menu.preferences.as_ref().unwrap().values[&504] != 0
                );
            })
            .in_set(PlayCues),
        );
        app.update();
        app.world_mut()
            .resource_mut::<MenuState>()
            .preferences
            .as_mut()
            .unwrap()
            .values
            .insert(504, 1);
        app.update();
    }

    #[test]
    fn original_viewport_fits_without_stretching() {
        assert_eq!(viewport_scale(Vec2::new(640.0, 480.0)), 1.0);
        assert_eq!(viewport_scale(Vec2::new(1280.0, 720.0)), 1.5);
        assert_eq!(viewport_scale(Vec2::new(480.0, 800.0)), 0.75);
    }
}
