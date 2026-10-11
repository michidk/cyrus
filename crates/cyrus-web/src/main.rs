//! shortcut: the browser loads GLBs that the native game exported into
//! `cyrus-config/cache/` (served next to the page); parsing the install's own
//! files in the browser needs the preservation crate to run on wasm first.

use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;
use cyrus_game::fidelity::{Area, Fidelity, Mode};
use cyrus_game::player::PlayerPlugin;

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
        PlayerPlugin,
        cyrus_game::hud::HudPlugin,
        cyrus_game::sound::SoundPlugin,
    ));
    if direct_scene {
        app.add_plugins((
            cyrus_game::environment::EnvironmentPlugin,
            cyrus_game::animation::AnimationPlugin,
            cyrus_debug::DebugMenuPlugin,
        ))
        .add_systems(Startup, spawn_scene);
    } else {
        app.add_plugins(cyrus_game::startup::StartupPlugin);
    }
    app.run();
    let _ = Area::ALL;
}

fn spawn_scene(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut frame: ResMut<cyrus_game::player::FrameScene>,
) {
    let Some(name) = scene_from_query() else {
        return;
    };
    commands.insert_resource(cyrus_game::animation::AnimationRequest(
        assets.load(format!("cache://{name}.animations")),
    ));
    commands.insert_resource(cyrus_game::player::data::PlayerSceneRequest(
        assets.load(format!("cache://{name}.player")),
    ));
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
