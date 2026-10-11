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
        && let Err(error) = window.location().set_search(&format!("?scene={requested}"))
    {
        error!("load world {requested}: {error:?}");
    }
}

#[cfg(target_arch = "wasm32")]
fn web_scene() -> String {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .and_then(|search| {
            search
                .trim_start_matches('?')
                .split('&')
                .find_map(|part| part.strip_prefix("scene="))
                .map(str::to_ascii_uppercase)
        })
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
