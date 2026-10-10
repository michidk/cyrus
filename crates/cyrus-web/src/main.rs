//! shortcut: the browser loads GLBs that the native game exported into
//! `cyrus-config/cache/` (served next to the page); parsing the install's own
//! files in the browser needs the preservation crate to run on wasm first.

use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;
use cyrus_game::fidelity::{Area, Fidelity, Mode};
use cyrus_game::player::PlayerPlugin;

fn scene_from_query() -> String {
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window()
        && let Ok(search) = window.location().search()
        && let Some(name) = search
            .trim_start_matches('?')
            .split('&')
            .find_map(|kv| kv.strip_prefix("scene="))
    {
        return name.to_ascii_uppercase();
    }
    "ISLAND".into()
}

fn modern_graphics() -> bool {
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window()
        && let Ok(search) = window.location().search()
    {
        return search
            .trim_start_matches('?')
            .split('&')
            .any(|part| part == "graphics=modern");
    }
    false
}

fn main() {
    let mut app = App::new();
    app.register_asset_source(
        "cache",
        AssetSourceBuilder::platform_default("cyrus-config/cache", None),
    );
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "cyrus".into(),
            fit_canvas_to_parent: true,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(Fidelity {
        graphics: if modern_graphics() {
            Mode::Modern
        } else {
            Mode::Faithful
        },
        ..default()
    })
    .insert_state(cyrus_game::AppState::InGame)
    .add_plugins((
        PlayerPlugin,
        cyrus_game::environment::EnvironmentPlugin,
        cyrus_debug::DebugMenuPlugin,
    ))
    .add_systems(Startup, spawn_scene)
    .run();
    let _ = Area::ALL;
}

fn spawn_scene(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut frame: ResMut<cyrus_game::player::FrameScene>,
) {
    let name = scene_from_query();
    commands.insert_resource(cyrus_game::environment::EnvironmentRequest(
        assets.load(format!("cache://{name}.environment")),
    ));
    frame.0 = true;
    commands.spawn(WorldAssetRoot(assets.load(
        GltfAssetLabel::Scene(0).from_asset(format!("cache://{name}.glb")),
    )));
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));
}
