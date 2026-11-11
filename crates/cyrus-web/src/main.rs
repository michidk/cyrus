//! Browser game host using the selected local folder or service for prepared assets.

#[cfg(target_arch = "wasm32")]
mod assets;
mod bindings;
#[cfg(target_arch = "wasm32")]
mod capture;
mod cheats;
mod console;
mod debug;
mod preferences;
#[cfg(target_arch = "wasm32")]
mod session;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "session/selection.rs"]
mod session_selection;
mod startup;
mod storage;

use bevy::prelude::*;
use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;
use cyrus_script::GamePlugin;

fn main() {
    #[cfg(target_arch = "wasm32")]
    let options = cyrus_app::launch::from_query::<cyrus_app::launch::GameOptions>()
        .unwrap_or_else(|error| wasm_bindgen::throw_str(&error.to_string()));
    #[cfg(not(target_arch = "wasm32"))]
    let options = cyrus_app::launch::GameOptions::default();
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(async move {
        let entry = if startup::automatic(&options) {
            Some(startup::installed_entry().await.unwrap_or_else(|error| {
                wasm_bindgen::throw_str(&format!("browser startup: {error:#}"))
            }))
        } else {
            None
        };
        run(options, entry);
    });
    #[cfg(not(target_arch = "wasm32"))]
    run(options, None);
}

fn run(
    options: cyrus_app::launch::GameOptions,
    entry: Option<cyrus_assets::startup::StartupEntry>,
) {
    let launch = startup::launch(&options, entry);
    let scene = options.scene;
    let mut app = App::new();
    if let Some(view) = options.view {
        app.insert_resource(cyrus_render::camera::InitialView(view));
    }
    app.insert_resource(cyrus_game::preferences::Preferences::new(
        preferences::LocalPreferences,
    ));
    #[cfg(target_arch = "wasm32")]
    assets::register(&mut app);
    #[cfg(not(target_arch = "wasm32"))]
    cyrus_app::register_cache(&mut app).expect("cache source");

    app.insert_resource(cheats::initial());
    app.insert_resource(console::initial());
    app.insert_resource(console::markers());
    app.add_plugins(
        DefaultPlugins
            .set(cyrus_app::window("cyrus"))
            .set(bevy::audio::AudioPlugin {
                global_volume: bevy::audio::GlobalVolume::new(bevy::audio::Volume::Linear(
                    cyrus_app::launch::volume_gain(options.volume),
                )),
                ..default()
            })
            .set(AssetPlugin {
                meta_check: bevy::asset::AssetMetaCheck::Never,
                ..default()
            })
            .set(bevy::log::LogPlugin {
                filter: cyrus_app::log_filter(),
                ..default()
            }),
    )
    .insert_resource({
        // `--mode` sets every area the browser supports; the per-area options override it. Modern
        // lighting is native only.
        let modern = |own: &Option<String>| {
            if own.as_ref().or(options.mode.as_ref()).map(String::as_str) == Some("modern") {
                Mode::Modern
            } else {
                Mode::Faithful
            }
        };
        Fidelity {
            water: modern(&options.water),
            fog: modern(&options.fog),
            sky: modern(&options.sky),
            music: modern(&options.music),
            player_controller: modern(&options.controls),
            camera: modern(&options.camera),
            ..default()
        }
    })
    .add_plugins((
        GamePlugin,
        cyrus_session::SessionBoundaryPlugin,
        cyrus_render::WorldRenderPlugin,
        cyrus_world::lifecycle::WorldPlugin,
        cyrus_world::animation::AnimationPlugin,
        cyrus_render::hud::HudPlugin,
        cyrus_audio::sound::SoundPlugin,
        cyrus_audio::music::MusicPlugin,
    ));
    app.add_systems(
        Update,
        cyrus_audio::follow_listener::<cyrus_render::camera::ViewCamera>
            .before(cyrus_audio::ApplyPreferences),
    );
    app.add_plugins(cyrus_startup::StartupPlugin).add_systems(
        Update,
        load_new_game
            .run_if(in_state(cyrus_game::AppState::Loading))
            .run_if(resource_exists::<cyrus_startup::StartGame>),
    );
    app.insert_resource(launch);
    let presentation = if options.skip_startup || options.new_game || options.movie.is_some() {
        cyrus_startup::StartupPresentation::without_splash()
    } else {
        cyrus_startup::StartupPresentation::default()
    };
    app.insert_resource(presentation);
    app.add_plugins(cyrus_debug::DebugMenuPlugin);
    cheats::register(&mut app);
    debug::register(&mut app);
    if let Some(scene) = scene {
        app.insert_resource(cyrus_startup::StartupPresentation::without_splash())
            .insert_resource(cyrus_startup::StartGame::default())
            .insert_resource(cyrus_world::lifecycle::SceneRequest(scene));
    }
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Update, quit_tab);
    #[cfg(target_arch = "wasm32")]
    app.add_systems(
        Update,
        bindings::load_scene_bindings
            .after(cyrus_script::player::data::PlayerSceneLoading)
            .before(cyrus_script::player::input::InputSample),
    );
    #[cfg(target_arch = "wasm32")]
    capture::register(&mut app);
    app.run();
    let _ = Area::ALL;
}

fn load_new_game(
    mut commands: Commands,
    assets: Res<AssetServer>,
    request: Res<cyrus_world::lifecycle::SceneRequest>,
    startup: Res<cyrus_startup::StartupPresentation>,
    prepared: Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
) {
    if !startup.loading_visible() || prepared.is_some() {
        return;
    }
    commands.insert_resource(cyrus_world::lifecycle::PreparedWorld::from_cache(
        request.0.clone(),
        &assets,
    ));
}

#[cfg(target_arch = "wasm32")]
fn quit_tab(mut requests: MessageReader<cyrus_startup::QuitRequest>) {
    for _ in requests.read() {
        if let Some(window) = web_sys::window() {
            // Browsers refuse to close ordinary user-opened tabs; keep the menu running.
            let _ = window.close();
        }
    }
}
