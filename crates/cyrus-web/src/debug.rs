//! Platform world-picker sources and navigation.
use bevy::prelude::*;
#[cfg(target_arch = "wasm32")]
use cyrus_debug::WorldPicker;
#[cfg(target_arch = "wasm32")]
fn initialize_web_world(mut picker: ResMut<WorldPicker>) {
    let current = web_scene();
    picker.current.clone_from(&current);
    picker.selected.clone_from(&current);
    picker.options = web_worlds();
    if !picker.options.contains(&current) {
        picker.options.push(current);
    }
}

#[cfg(target_arch = "wasm32")]
fn update_web_world(mut picker: ResMut<WorldPicker>) {
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
    window
        .location()
        .set_search(&params.to_string().as_string().unwrap_or_default())
}

#[cfg(target_arch = "wasm32")]
fn web_scene() -> String {
    cyrus_app::launch::from_query::<cyrus_app::launch::GameOptions>()
        .ok()
        .and_then(|options| options.scene)
        .unwrap_or_else(|| "ISLAND".into())
}

#[cfg(target_arch = "wasm32")]
fn web_worlds() -> Vec<String> {
    let worlds = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.query_selector("meta[name='cyrus-worlds']").ok()?)
        .and_then(|element| element.get_attribute("content"))
        .unwrap_or_default();
    parse_worlds(&worlds)
}

#[cfg(any(target_arch = "wasm32", test))]
fn parse_worlds(worlds: &str) -> Vec<String> {
    let mut worlds = worlds
        .split(',')
        .filter(|world| !world.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    worlds.sort();
    worlds.dedup();
    worlds
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
    fn world_options_are_sorted_and_unique() {
        assert_eq!(
            parse_worlds("START,2,ISLAND,START"),
            ["2", "ISLAND", "START"]
        );
    }
}
