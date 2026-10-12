//! shortcut: the browser loads GLBs that the native game exported into
//! `cyrus-config/cache/` (served next to the page); parsing the install's own
//! files in the browser needs the preservation crate to run on wasm first.

mod bindings;
mod debug;
mod preferences;
use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;
use cyrus_game::GamePlugin;
use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;

fn scene_from_query() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window()
        && let Ok(search) = window.location().search()
        && let Some(name) = search
            .trim_start_matches('?')
            .split('&')
            .find_map(|kv| kv.strip_prefix("scene="))
    {
        return Some(name.to_ascii_uppercase());
    }
    None
}

fn modern_area(_area: &str) -> bool {
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window()
        && let Ok(search) = window.location().search()
    {
        return search
            .trim_start_matches('?')
            .split('&')
            .any(|part| part == format!("{_area}=modern"));
    }
    false
}

fn main() {
    let scene = scene_from_query();
    let mut app = App::new();
    app.insert_resource(cyrus_game::preferences::Preferences::new(
        preferences::LocalPreferences,
    ));
    app.register_asset_source(
        "cache",
        AssetSourceBuilder::platform_default("cyrus-config/cache", None),
    );
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "cyrus".into(),
                    fit_canvas_to_parent: true,
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                meta_check: bevy::asset::AssetMetaCheck::Never,
                ..default()
            }),
    )
    .insert_resource(Fidelity {
        water: if modern_area("water") {
            Mode::Modern
        } else {
            Mode::Faithful
        },
        fog: if modern_area("fog") {
            Mode::Modern
        } else {
            Mode::Faithful
        },
        sky: if modern_area("sky") {
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
    if let Some(scene) = scene {
        app.insert_resource(cyrus_render::startup::StartupPresentation::without_splash())
            .insert_resource(cyrus_render::startup::StartGame::default())
            .insert_resource(cyrus_world::lifecycle::SceneRequest(scene))
            .add_plugins(cyrus_debug::DebugMenuPlugin);
        debug::register(&mut app);
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
