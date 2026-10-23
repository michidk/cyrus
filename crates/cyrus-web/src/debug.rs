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
        // The running world stays until every sidecar of the destination has loaded; a failure
        // reports `SceneLoadError` and leaves it in place.
        let prepared =
            cyrus_world::lifecycle::PreparedWorld::from_cache(requested.clone(), &assets);
        let handles = [
            prepared.animations.clone().untyped(),
            prepared.scripts.clone().untyped(),
            assets
                .load::<cyrus_game::player::data::PlayerScene>(prepared.player_path.clone())
                .untyped(),
            assets
                .load::<bevy::gltf::Gltf>(prepared.scene_path.clone())
                .untyped(),
            assets
                .load::<cyrus_assets::environment::Environment>(prepared.environment_path.clone())
                .untyped(),
        ]
        .into();
        commands.remove_resource::<cyrus_world::lifecycle::SceneLoadError>();
        commands.insert_resource(cyrus_world::lifecycle::WorldPreparationPending);
        commands.insert_resource(StagedWorld {
            requested,
            prepared,
            handles,
        });
    } else {
        #[cfg(target_arch = "wasm32")]
        if let Some(window) = web_sys::window()
            && let Err(error) = set_scene(&window, &requested, true)
        {
            error!("load world {requested}: {error:?}");
        }
    }
}

/// A world selection waiting for its sidecars. Replaced by a newer selection.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Resource)]
struct StagedWorld {
    requested: String,
    prepared: cyrus_world::lifecycle::PreparedWorld,
    handles: Vec<UntypedHandle>,
}

/// Sidecar handles of the committed world, kept so the consumers reuse the loaded assets.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Resource)]
#[allow(
    dead_code,
    reason = "owns the handles that keep the committed world's assets loaded"
)]
struct CommittedWorldHandles(Vec<UntypedHandle>);

/// Swap the world caches once the staged world has loaded, or report why it cannot.
#[cfg(any(target_arch = "wasm32", test))]
fn commit_staged_world(
    mut commands: Commands,
    assets: Res<AssetServer>,
    staged: Option<Res<StagedWorld>>,
) {
    use bevy::asset::LoadState;
    let Some(staged) = staged else {
        return;
    };
    let mut loading = false;
    for handle in &staged.handles {
        match assets.get_load_state(handle.id()) {
            Some(LoadState::Failed(error)) => {
                let message = format!("World {}: {error}", staged.requested);
                error!("{message}");
                commands.remove_resource::<StagedWorld>();
                commands.remove_resource::<cyrus_world::lifecycle::WorldPreparationPending>();
                commands.insert_resource(cyrus_world::lifecycle::SceneLoadError(message));
                return;
            }
            Some(LoadState::Loaded) => {}
            _ => loading = true,
        }
    }
    if loading {
        return;
    }
    commands.remove_resource::<cyrus_render::camera::InitialView>();
    commands.insert_resource(cyrus_world::lifecycle::SceneRequest(
        staged.requested.clone(),
    ));
    commands.insert_resource(staged.prepared.clone());
    commands.insert_resource(CommittedWorldHandles(staged.handles.clone()));
    commands.remove_resource::<StagedWorld>();
    commands.remove_resource::<cyrus_world::lifecycle::WorldPreparationPending>();
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window()
        && let Err(error) = set_scene(&window, &staged.requested, false)
    {
        error!("load world {}: {error:?}", staged.requested);
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
        (update_web_world, commit_staged_world)
            .chain()
            .before(cyrus_world::lifecycle::WorldChange),
    );
    #[cfg(not(target_arch = "wasm32"))]
    let _ = app;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selection_app(requested: Option<&str>, authored: bool) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::state::app::StatesPlugin,
            AssetPlugin::default(),
        ))
        .insert_state(cyrus_game::AppState::InGame)
        .init_asset::<cyrus_assets::animation::Animations>()
        .init_asset::<cyrus_assets::script::Scripts>()
        .init_asset::<cyrus_game::player::data::PlayerScene>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<cyrus_assets::environment::Environment>()
        .insert_resource(cyrus_game::random::EngineRandom::from_state(42))
        .insert_resource(cyrus_world::lifecycle::CurrentScene {
            name: "ISLAND".into(),
            world: Some(1),
        })
        .insert_resource(cyrus_world::lifecycle::SceneRequest("ISLAND".into()))
        .insert_resource(cyrus_render::camera::InitialView(Transform::IDENTITY))
        .insert_resource(WorldPicker {
            pending: requested.map(str::to_owned),
            ..default()
        })
        .add_systems(Update, (update_web_world, commit_staged_world).chain());
        if authored {
            app.insert_resource(cyrus_world::lifecycle::ReloadScene);
        }
        let assets = app.world().resource::<AssetServer>().clone();
        app.insert_resource(cyrus_world::lifecycle::PreparedWorld::from_cache(
            "ISLAND".into(),
            &assets,
        ));
        app
    }

    #[test]
    fn in_game_world_selection_keeps_the_running_world_until_the_destination_loads() {
        let mut app = selection_app(Some("0"), false);
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
        // The selection is staged: input is held and the running world is untouched.
        let staged = app.world().resource::<StagedWorld>();
        assert_eq!(staged.requested, "0");
        assert_eq!(staged.prepared.player_path.to_string(), "cache://0.player");
        assert!(
            app.world()
                .contains_resource::<cyrus_world::lifecycle::WorldPreparationPending>()
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::SceneRequest>()
                .0,
            "ISLAND"
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::PreparedWorld>()
                .requested_name,
            "ISLAND"
        );
        assert!(
            app.world()
                .contains_resource::<cyrus_render::camera::InitialView>()
        );
        assert!(app.world().resource::<WorldPicker>().pending.is_none());
        assert_eq!(
            *app.world().resource::<State<cyrus_game::AppState>>().get(),
            cyrus_game::AppState::InGame
        );
    }

    #[test]
    fn a_destination_that_fails_to_load_reports_the_error_and_keeps_the_running_world() {
        // The unit app has no `cache` asset source, so every sidecar of the destination fails.
        let mut app = selection_app(Some("0"), false);
        for _ in 0..4 {
            app.update();
        }
        assert!(!app.world().contains_resource::<StagedWorld>());
        assert!(
            !app.world()
                .contains_resource::<cyrus_world::lifecycle::WorldPreparationPending>()
        );
        let error = app
            .world()
            .resource::<cyrus_world::lifecycle::SceneLoadError>();
        assert!(error.0.starts_with("World 0:"), "{}", error.0);
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::PreparedWorld>()
                .requested_name,
            "ISLAND"
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::SceneRequest>()
                .0,
            "ISLAND"
        );
        assert!(
            app.world()
                .contains_resource::<cyrus_render::camera::InitialView>()
        );
    }

    #[test]
    fn authored_world_load_stages_the_destination_once() {
        let mut app = selection_app(None, true);
        app.world_mut()
            .insert_resource(cyrus_world::lifecycle::SceneRequest("13".into()));
        app.update();
        assert!(
            !app.world()
                .contains_resource::<cyrus_world::lifecycle::ReloadScene>()
        );
        let staged = app.world().resource::<StagedWorld>();
        assert_eq!(staged.requested, "13");
        assert_eq!(staged.prepared.scene_path.to_string(), "cache://13.glb");
        // No further request: the next update only polls the staged load.
        app.world_mut().remove_resource::<StagedWorld>();
        app.update();
        assert!(!app.world().contains_resource::<StagedWorld>());
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
