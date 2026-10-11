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
    let direct_scene = scene_from_query().is_some();
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
    .insert_state(if direct_scene {
        cyrus_game::AppState::InGame
    } else {
        cyrus_game::AppState::Loading
    })
    .add_plugins((
        GamePlugin,
        cyrus_render::hud::HudPlugin,
        cyrus_render::sound::SoundPlugin,
    ));
    if direct_scene {
        app.add_plugins((
            cyrus_render::WorldRenderPlugin,
            cyrus_world::lifecycle::WorldPlugin,
            cyrus_world::animation::AnimationPlugin,
            cyrus_debug::DebugMenuPlugin,
        ))
        .add_systems(Startup, spawn_scene);
        debug::register(&mut app);
    } else {
        app.add_plugins(cyrus_render::startup::StartupPlugin);
    }
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

fn spawn_scene(mut commands: Commands, assets: Res<AssetServer>) {
    if let Some(name) = scene_from_query() {
        commands.insert_resource(cyrus_world::lifecycle::PreparedWorld::from_cache(
            name, &assets,
        ));
    }
}
