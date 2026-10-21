//! Platform world-picker sources and navigation.
use bevy::prelude::*;
#[cfg(any(target_arch = "wasm32", test))]
use cyrus_debug::WorldPicker;
#[cfg(target_arch = "wasm32")]
fn initialize_web_world(mut picker: ResMut<WorldPicker>) {
    picker.options = web_worlds();
}

#[cfg(any(target_arch = "wasm32", test))]
fn update_web_world(
    mut commands: Commands,
    assets: Res<AssetServer>,
    state: Res<State<cyrus_game::AppState>>,
    mut picker: ResMut<WorldPicker>,
    current: Res<cyrus_world::lifecycle::CurrentScene>,
    authored: Option<(
        Res<cyrus_world::lifecycle::ReloadScene>,
        Res<cyrus_world::lifecycle::SceneRequest>,
    )>,
) {
    if picker.current != current.name {
        picker.current.clone_from(&current.name);
        picker.selected.clone_from(&current.name);
    }
    // Script `LoadWorld` and console `load` raise `ReloadScene` with the request; the browser has
    // no native preparation thread, so both swap the world caches like a picker selection.
    let authored = authored.map(|(_, request)| request.0.clone());
    if authored.is_some() {
        commands.remove_resource::<cyrus_world::lifecycle::ReloadScene>();
    }
    let Some(requested) = picker.pending.take().or(authored) else {
        return;
    };
    let in_game = *state.get() == cyrus_game::AppState::InGame;
    if in_game {
        commands.remove_resource::<cyrus_render::camera::InitialView>();
        commands.insert_resource(cyrus_world::lifecycle::SceneRequest(requested.clone()));
        commands.insert_resource(cyrus_world::lifecycle::PreparedWorld::from_cache(
            requested.clone(),
            &assets,
        ));
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window()
        && let Err(error) = set_scene(&window, &requested, !in_game)
    {
        error!("load world {requested}: {error:?}");
    }
}

#[cfg(target_arch = "wasm32")]
fn set_scene(
    window: &web_sys::Window,
    scene: &str,
    reload: bool,
) -> Result<(), wasm_bindgen::JsValue> {
    let params = web_sys::UrlSearchParams::new_with_str(&window.location().search()?)?;
    params.set("scene", scene);
    params.delete("view");
    let query = params.to_string().as_string().unwrap_or_default();
    if reload {
        window.location().set_search(&query)
    } else {
        window.history()?.replace_state_with_url(
            &wasm_bindgen::JsValue::NULL,
            "",
            Some(&format!("?{query}")),
        )
    }
}

#[cfg(target_arch = "wasm32")]
fn web_worlds() -> Vec<[String; 2]> {
    let worlds = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.query_selector("meta[name='cyrus-worlds']").ok()?)
        .and_then(|element| element.get_attribute("content"))
        .unwrap_or_default();
    parse_worlds(&worlds)
}

#[cfg(any(target_arch = "wasm32", test))]
fn parse_worlds(worlds: &str) -> Vec<[String; 2]> {
    serde_json::from_str(worlds).unwrap_or_default()
}

pub fn register(app: &mut App) {
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Startup, initialize_web_world).add_systems(
        Update,
        update_web_world.before(cyrus_world::lifecycle::WorldChange),
    );
    #[cfg(not(target_arch = "wasm32"))]
    let _ = app;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn in_game_world_selection_keeps_the_session_and_queues_shared_preparation() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::state::app::StatesPlugin,
            AssetPlugin::default(),
        ))
        .insert_state(cyrus_game::AppState::InGame)
        .init_asset::<cyrus_assets::animation::Animations>()
        .init_asset::<cyrus_assets::script::Scripts>()
        .insert_resource(cyrus_game::random::EngineRandom::from_state(42))
        .insert_resource(cyrus_world::lifecycle::CurrentScene {
            name: "ISLAND".into(),
            world: Some(1),
        })
        .insert_resource(cyrus_world::lifecycle::SceneRequest("ISLAND".into()))
        .insert_resource(cyrus_render::camera::InitialView(Transform::IDENTITY))
        .insert_resource(WorldPicker {
            pending: Some("0".into()),
            ..default()
        })
        .add_systems(Update, update_web_world);
        let retained = app
            .world_mut()
            .spawn(Name::new("retained session entity"))
            .id();
        app.update();
        assert!(app.world().get::<Name>(retained).is_some());
        assert_eq!(
            app.world()
                .resource::<cyrus_game::random::EngineRandom>()
                .state(),
            42
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::SceneRequest>()
                .0,
            "0"
        );
        let prepared = app
            .world()
            .resource::<cyrus_world::lifecycle::PreparedWorld>();
        assert_eq!(prepared.requested_name, "0");
        assert_eq!(prepared.player_path.to_string(), "cache://0.player");
        assert!(
            !app.world()
                .contains_resource::<cyrus_render::camera::InitialView>()
        );
        assert!(app.world().resource::<WorldPicker>().pending.is_none());
        app.update();
        assert!(app.world().get::<Name>(retained).is_some());
        assert_eq!(
            *app.world().resource::<State<cyrus_game::AppState>>().get(),
            cyrus_game::AppState::InGame
        );
    }

    #[test]
    fn authored_world_load_swaps_the_world_caches_once() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::state::app::StatesPlugin,
            AssetPlugin::default(),
        ))
        .insert_state(cyrus_game::AppState::InGame)
        .init_asset::<cyrus_assets::animation::Animations>()
        .init_asset::<cyrus_assets::script::Scripts>()
        .insert_resource(cyrus_world::lifecycle::CurrentScene {
            name: "13".into(),
            world: Some(13),
        })
        .insert_resource(cyrus_world::lifecycle::SceneRequest("13".into()))
        .insert_resource(cyrus_world::lifecycle::ReloadScene)
        .insert_resource(cyrus_render::camera::InitialView(Transform::IDENTITY))
        .init_resource::<WorldPicker>()
        .add_systems(Update, update_web_world);
        app.update();
        assert!(
            !app.world()
                .contains_resource::<cyrus_world::lifecycle::ReloadScene>()
        );
        assert!(
            !app.world()
                .contains_resource::<cyrus_render::camera::InitialView>()
        );
        let prepared = app
            .world()
            .resource::<cyrus_world::lifecycle::PreparedWorld>();
        assert_eq!(prepared.requested_name, "13");
        assert_eq!(prepared.scene_path.to_string(), "cache://13.glb");
        app.world_mut()
            .remove_resource::<cyrus_world::lifecycle::PreparedWorld>();
        app.update();
        assert!(
            !app.world()
                .contains_resource::<cyrus_world::lifecycle::PreparedWorld>()
        );
    }

    #[test]
    fn world_options_preserve_ids_for_shared_maps() {
        assert_eq!(
            parse_worlds(r#"[["0","ISLAND"],["27","ISLAND"]]"#),
            [
                ["0".to_owned(), "ISLAND".to_owned()],
                ["27".to_owned(), "ISLAND".to_owned()]
            ]
        );
    }
}
