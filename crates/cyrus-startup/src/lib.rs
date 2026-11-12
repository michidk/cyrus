//! Installed splash, title, book menu, settings, cinematics and world-entry presentation.
mod book_motion;
mod book_scene;
mod confirmation;
mod credits;
mod examine;
mod information;
mod splash;
mod view3d;
pub use confirmation::{ConfirmationChoice, ConfirmationInput, LastGameConfirmation};
mod fade;
/// Marker for installed loading and first-world black-overlay transitions.
pub use fade::Fade as WorldEntryFade;
mod last_game;
mod loading;
mod logbook;
pub use logbook::LogbookPlugin;
mod overlay;
pub use overlay::OverlayPlugin;
mod menu;
mod save_load;
pub use save_load::{GameplayThumbnail, SaveLoadHost, SaveLoadRequest, SaveSlot};
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
pub use last_game::LastGameLoader;
pub use menu::{LaunchAction, LaunchRequest, MenuContext, MenuState};
#[cfg(not(target_arch = "wasm32"))]
pub use preparation::LoadingPlugin;

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

/// A debug world selection enters through the shared loading and player-entry lifecycle.
/// Hosts provide a WORLD.INI ID or map stem. Inspection enters at marker zero, heading zero.
#[derive(Resource)]
pub struct WorldLoadRequest(pub String);

/// A world entry waits on the loading screen before entering its world.
#[derive(Resource, Default)]
pub struct StartGame {
    artwork_world: Option<u32>,
    /// Menu entry uses installed transition artwork; direct inspections use the title.
    image: Handle<Image>,
    background: Vec<u8>,
    font: cyrus_text::Font,
    progress: (usize, usize),
    /// Menu entries fade the loading screen in, then out before entering the world.
    faded_in: bool,
    fade_out: Option<Entity>,
}

impl StartGame {
    /// WORLD.INI destination supplying the selected artwork; absent for direct title inspections.
    pub fn artwork_world(&self) -> Option<u32> {
        self.artwork_world
    }
}

/// Browser host attempts to close its tab without terminating the game loop.
#[derive(Message)]
pub struct QuitRequest;

#[derive(Resource)]
struct Playback {
    /// Frames the splash plays; zero when it is skipped.
    count: usize,
    /// The 3dfx splash's exported meshes and draws, and the scene that draws them live.
    live: Option<(
        Handle<cyrus_assets::splash_asset::SplashAsset>,
        splash::SplashView,
    )>,
    title: Handle<Image>,
    elapsed: f32,
    failed: bool,
}

#[derive(Component)]
struct StartupContent;

impl Plugin for StartupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StartupPresentation>()
            .init_resource::<cyrus_script::script::DeathDispatch>()
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
        app.add_systems(
            Last,
            // The death exit requests the book in this same schedule; the view is captured after it.
            save_load::capture_gameplay
                .after(cyrus_script::script::WorldLoopExit)
                .run_if(in_state(AppState::InGame)),
        );
        app.add_systems(OnExit(AppState::Menu), save_load::discard_gameplay);
        menu::register(app);
        movie::register(app);
        credits::register(app);
        information::register(app);
        loading::register(app);
        // Hosts without the renderer draw no models, so the lit material is registered only with it.
        splash::register_assets(app);
        if app.is_plugin_added::<bevy::pbr::PbrPlugin>() {
            app.add_plugins((view3d::LitViewPlugin, splash::SplashPlugin));
        }
        app.add_plugins((LogbookPlugin, OverlayPlugin, examine::ExaminePlugin));
    }
}

fn sync_audio(
    startup: Res<StartupPresentation>,
    menu: Option<Res<MenuState>>,
    mut readiness: ResMut<cyrus_audio::AudioReadiness>,
    settings: Option<ResMut<cyrus_audio::sound::SoundSettings>>,
) {
    // A skipped startup shows no title to wait for, and a world entered before the title would
    // otherwise run without audio settings (the torch sound then fails the script world).
    readiness.prepare = startup.skip || startup.title_visible;
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

use cyrus_assets::SPLASH_FRAMES;

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut startup: ResMut<StartupPresentation>,
    game: Option<Res<StartGame>>,
    mut images: ResMut<Assets<Image>>,
) {
    startup.title_visible = false;
    startup.error = None;
    let count = if startup.skip || game.is_some() {
        0
    } else {
        SPLASH_FRAMES
    };
    let live = (count > 0).then(|| {
        (
            assets.load("cache://startup/splash.splash"),
            splash::SplashView::new(&mut images),
        )
    });
    spawn_camera(&mut commands);
    let title = game
        .as_ref()
        .filter(|game| game.image != Handle::default())
        .map_or_else(
            || assets.load("cache://startup/title.png"),
            |game| game.image.clone(),
        );
    let first = live
        .as_ref()
        .map(|(_, view)| view.target())
        .unwrap_or_else(|| title.clone());
    commands.spawn((
        Sprite {
            image: first,
            // The live splash renders at twice the size, for supersampling.
            custom_size: live.is_some().then_some(Vec2::new(640.0, 480.0)),
            ..default()
        },
        StartupContent,
    ));
    commands.insert_resource(Playback {
        count,
        live,
        title,
        elapsed: 0.0,
        failed: false,
    });
}

/// Fits the original 640×480 viewport within any window, with black borders.
fn viewport_scale(size: Vec2) -> f32 {
    (size.x / 640.0).min(size.y / 480.0)
}

type FittedImages<'w, 's> = Query<
    'w,
    's,
    &'static mut Transform,
    (
        With<StartupContent>,
        With<Sprite>,
        Without<last_game::CapturedBackground>,
    ),
>;

fn fit_image(windows: Query<&Window>, mut image: FittedImages) {
    if let (Ok(window), Ok(mut transform)) = (windows.single(), image.single_mut()) {
        transform.scale = Vec3::splat(viewport_scale(Vec2::new(window.width(), window.height())));
    }
}

fn advance(
    mut commands: Commands,
    assets: Res<AssetServer>,
    input: (
        Res<Time>,
        Res<ButtonInput<KeyCode>>,
        Res<NextState<AppState>>,
    ),
    mut playback: ResMut<Playback>,
    mut image: Query<&mut Sprite, With<StartupContent>>,
    mut drawing: splash::SplashAssets,
    mut startup: ResMut<StartupPresentation>,
) {
    if matches!(*input.2, NextState::Pending(AppState::InGame)) {
        return;
    }
    if playback.failed {
        return;
    }
    let failed = |commands: &mut Commands, error: &dyn std::fmt::Display| {
        error!("startup: {error}");
        commands.spawn((
            Text::new("Startup assets unavailable. Export them with --export-startup."),
            StartupContent,
        ));
    };
    if let Some((handle, _)) = &playback.live {
        match assets.get_load_state(handle.id()) {
            Some(LoadState::Failed(error)) => {
                failed(&mut commands, &error);
                playback.failed = true;
                return;
            }
            Some(LoadState::Loaded) => {}
            _ => return,
        }
    }
    let title = playback.title.id();
    if !drawing.images.contains(title) {
        if let Some(LoadState::Failed(error)) = assets.get_load_state(title) {
            failed(&mut commands, &error);
            playback.failed = true;
            return;
        }
        if !assets.is_loaded_with_dependencies(title) {
            return;
        }
    }
    let Ok(mut sprite) = image.single_mut() else {
        return;
    };
    let splash_duration = playback.count as f32 / 30.0;
    let skip = [KeyCode::Enter, KeyCode::Space, KeyCode::Escape]
        .iter()
        .any(|key| input.1.just_pressed(*key));
    let was_title = playback.elapsed >= splash_duration;
    if startup.skip || (skip && !was_title) {
        playback.elapsed = splash_duration;
    } else {
        playback.elapsed += input.0.delta_secs();
    }
    let frame = (playback.elapsed * 30.0) as usize;
    if playback.elapsed < splash_duration {
        if let Some((handle, view)) = playback.live.as_mut() {
            view.present(&mut commands, &mut drawing, handle, frame);
        }
    } else {
        if let Some((_, view)) = playback.live.as_mut() {
            view.finish(&mut commands);
        }
        sprite.image = playback.title.clone();
        startup.title_visible = true;
    }
}

/// Presentation clears independently of the world's environment colour.
fn spawn_camera(commands: &mut Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            clear_color: bevy::camera::ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        StartupContent,
    ));
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
    fn presentation_camera_keeps_black_borders_after_world_entry() {
        let mut app = App::new();
        app.insert_resource(ClearColor(Color::srgb_u8(192, 96, 96)))
            .add_systems(Update, |mut commands: Commands| spawn_camera(&mut commands));
        app.update();
        let camera = app
            .world_mut()
            .query::<&Camera>()
            .single(app.world())
            .unwrap();
        assert!(matches!(
            camera.clear_color,
            bevy::camera::ClearColorConfig::Custom(colour) if colour == Color::BLACK
        ));
    }

    #[test]
    fn a_skipped_startup_prepares_audio_without_waiting_for_the_title() {
        use cyrus_audio::AudioReadiness;
        let mut app = App::new();
        app.insert_resource(StartupPresentation::without_splash())
            .insert_resource(AudioReadiness { prepare: false })
            .add_systems(Update, sync_audio);
        app.update();
        assert!(app.world().resource::<AudioReadiness>().prepare);
    }
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
