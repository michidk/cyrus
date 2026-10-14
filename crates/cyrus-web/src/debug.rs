//! Platform world-picker sources and navigation.
use bevy::prelude::*;
#[cfg(target_arch = "wasm32")]
use cyrus_debug::WorldPicker;
#[cfg(target_arch = "wasm32")]
fn initialize_web_world(mut picker: ResMut<WorldPicker>) {
    picker.options = web_worlds();
}

#[cfg(target_arch = "wasm32")]
fn update_web_world(
    mut picker: ResMut<WorldPicker>,
    current: Res<cyrus_world::lifecycle::CurrentScene>,
) {
    if picker.current != current.name {
        picker.current.clone_from(&current.name);
        picker.selected.clone_from(&current.name);
    }
    let Some(requested) = picker.pending.take() else {
        return;
    };
    if let Some(window) = web_sys::window()
        && let Err(error) = set_scene(&window, &requested)
    {
        error!("load world {requested}: {error:?}");
    }
}

#[cfg(target_arch = "wasm32")]
fn set_scene(window: &web_sys::Window, scene: &str) -> Result<(), wasm_bindgen::JsValue> {
    let params = web_sys::UrlSearchParams::new_with_str(&window.location().search()?)?;
    params.set("scene", scene);
    params.delete("view");
    window
        .location()
        .set_search(&params.to_string().as_string().unwrap_or_default())
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
    app.add_systems(Startup, initialize_web_world)
        .add_systems(Update, update_web_world);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = app;
}

#[cfg(test)]
mod tests {
    use super::*;
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
