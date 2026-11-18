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
    mut picker: ResMut<WorldPicker>,
    mut flycam: Option<ResMut<cyrus_render::camera::DetachedView>>,
    current: Res<cyrus_world::lifecycle::CurrentScene>,
) {
    if picker.current != current.name {
        picker.current.clone_from(&current.name);
        picker.selected.clone_from(&current.name);
        #[cfg(target_arch = "wasm32")]
        if let Some(window) = web_sys::window()
            && let Err(error) = set_scene(&window, &current.name)
        {
            error!("load world {}: {error:?}", current.name);
        }
    }
    if let Some(requested) = picker.pending.take() {
        if let Some(flycam) = flycam.as_mut() {
            flycam.enabled = false;
        }
        commands.remove_resource::<cyrus_render::camera::InitialView>();
        commands.insert_resource(cyrus_startup::WorldLoadRequest(requested));
    }
}

#[cfg(target_arch = "wasm32")]
fn set_scene(window: &web_sys::Window, scene: &str) -> Result<(), wasm_bindgen::JsValue> {
    let params = web_sys::UrlSearchParams::new_with_str(&window.location().search()?)?;
    params.set("scene", scene);
    params.delete("view");
    let query = params.to_string().as_string().unwrap_or_default();
    window.history()?.replace_state_with_url(
        &wasm_bindgen::JsValue::NULL,
        "",
        Some(&format!("?{query}")),
    )
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
        .init_asset::<cyrus_script::player::data::PlayerScene>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<cyrus_assets::environment::Environment>()
        .insert_resource(cyrus_game::random::EngineRandom::from_state(42))
        .insert_resource(cyrus_world::lifecycle::CurrentScene {
            name: "ISLAND".into(),
            world: Some(1),
        })
        .insert_resource(cyrus_world::lifecycle::SceneRequest("ISLAND".into()))
        .insert_resource(cyrus_render::camera::InitialView(Transform::IDENTITY))
        .insert_resource(cyrus_render::camera::DetachedView {
            enabled: true,
            ..default()
        })
        .insert_resource(WorldPicker {
            pending: requested.map(str::to_owned),
            ..default()
        })
        .add_systems(Update, update_web_world);
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
    fn picker_selection_requests_shared_loading_instead_of_staging_a_live_swap() {
        let mut app = selection_app(Some("0"), false);
        app.update();
        assert_eq!(
            app.world().resource::<cyrus_startup::WorldLoadRequest>().0,
            "0"
        );
        assert!(
            !app.world()
                .resource::<cyrus_render::camera::DetachedView>()
                .enabled
        );
        assert!(
            !app.world()
                .contains_resource::<cyrus_render::camera::InitialView>()
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::SceneRequest>()
                .0,
            "ISLAND"
        );
        assert!(app.world().resource::<WorldPicker>().pending.is_none());
    }

    #[test]
    fn authored_world_load_is_retained_for_shared_loading_presentation() {
        let mut app = selection_app(None, false);
        let mut commands = app.world_mut().commands();
        cyrus_script::script::request_world(&mut commands, [5, 9, 2560]).unwrap();
        app.world_mut().flush();
        app.update();
        assert!(
            app.world()
                .contains_resource::<cyrus_world::lifecycle::ReloadScene>()
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::SceneRequest>()
                .0,
            "5"
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_script::player::placement::MarkerEntry>()
                .marker,
            9
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_script::player::placement::MarkerEntry>()
                .heading,
            512
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::PreparedWorld>()
                .requested_name,
            "ISLAND"
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
