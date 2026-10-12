//! Browser game host using the selected install service for prepared assets.

#[cfg(target_arch = "wasm32")]
mod assets;
mod bindings;
mod debug;
mod preferences;

use bevy::prelude::*;
use cyrus_game::GamePlugin;
use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;

fn main() {
    #[cfg(target_arch = "wasm32")]
    let options = cyrus_app::launch::from_query::<cyrus_app::launch::GameOptions>()
        .expect("validated game URL options");
    #[cfg(not(target_arch = "wasm32"))]
    let options = cyrus_app::launch::GameOptions::default();
    let scene = options.scene;
    let mut app = App::new();
    app.insert_resource(cyrus_game::preferences::Preferences::new(
        preferences::LocalPreferences,
    ));
    #[cfg(target_arch = "wasm32")]
    assets::register(&mut app);
    #[cfg(not(target_arch = "wasm32"))]
    cyrus_app::register_cache(&mut app).expect("cache source");

    app.add_plugins(
        DefaultPlugins
            .set(cyrus_app::window("cyrus"))
            .set(AssetPlugin {
                meta_check: bevy::asset::AssetMetaCheck::Never,
                ..default()
            }),
    )
    .insert_resource(Fidelity {
        water: if options.water.as_deref() == Some("modern") {
            Mode::Modern
        } else {
            Mode::Faithful
        },
        fog: if options.fog.as_deref() == Some("modern") {
            Mode::Modern
        } else {
            Mode::Faithful
        },
        sky: if options.sky.as_deref() == Some("modern") {
            Mode::Modern
        } else {
            Mode::Faithful
        },
        ..default()
    })
    .add_plugins((
        GamePlugin,
        cyrus_render::WorldRenderPlugin,
        cyrus_world::lifecycle::WorldPlugin,
        cyrus_world::animation::AnimationPlugin,
        cyrus_render::hud::HudPlugin,
        cyrus_render::sound::SoundPlugin,
        cyrus_render::music::MusicPlugin,
    ));
    app.add_plugins(cyrus_render::startup::StartupPlugin)
        .add_systems(
            Update,
            load_new_game
                .run_if(in_state(cyrus_game::AppState::Loading))
                .run_if(resource_exists::<cyrus_render::startup::StartGame>),
        );
    if options.skip_startup {
        app.insert_resource(cyrus_render::startup::StartupPresentation::without_splash());
    }
    app.add_plugins(cyrus_debug::DebugMenuPlugin);
    debug::register(&mut app);
    if let Some(scene) = scene {
        app.insert_resource(cyrus_render::startup::StartupPresentation::without_splash())
            .insert_resource(cyrus_render::startup::StartGame::default())
            .insert_resource(cyrus_world::lifecycle::SceneRequest(scene));
    }
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Update, quit_tab);
    #[cfg(target_arch = "wasm32")]
    app.add_systems(
        Update,
        bindings::load_scene_bindings
            .after(cyrus_game::player::data::PlayerSceneLoading)
            .before(cyrus_game::player::input::InputSample),
    );
    app.run();
    let _ = Area::ALL;
}

fn load_new_game(
    mut commands: Commands,
    assets: Res<AssetServer>,
    request: Res<cyrus_world::lifecycle::SceneRequest>,
    startup: Res<cyrus_render::startup::StartupPresentation>,
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
fn quit_tab(mut requests: MessageReader<cyrus_render::startup::QuitRequest>) {
    for _ in requests.read() {
        if let Some(window) = web_sys::window() {
            // Browsers refuse to close ordinary user-opened tabs; keep the menu running.
            let _ = window.close();
        }
    }
}
