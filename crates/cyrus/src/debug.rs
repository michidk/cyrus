//! Platform world-picker sources and navigation.
use bevy::prelude::*;
use cyrus_debug::WorldPicker;
#[cfg(not(target_arch = "wasm32"))]
fn initialize_native_worlds(mut picker: ResMut<WorldPicker>) {
    picker.options = std::fs::read_dir(crate::settings::Settings::cache_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("glb"))
                .then(|| path.file_stem()?.to_str().map(str::to_owned))
                .flatten()
        })
        .collect();
    picker.options.sort();
    picker.options.dedup();
}

#[cfg(not(target_arch = "wasm32"))]
fn update_native_world(
    mut picker: ResMut<WorldPicker>,
    current: Res<cyrus_world::lifecycle::CurrentScene>,
    mut request: ResMut<cyrus_world::lifecycle::SceneRequest>,
) {
    picker.current.clone_from(&current.name);
    if picker.selected.is_empty() {
        picker.selected.clone_from(&request.0);
    }
    if let Some(requested) = picker.pending.take() {
        request.0 = requested;
    }
}

pub fn register(app: &mut App) {
    app.add_systems(Startup, initialize_native_worlds)
        .add_systems(Update, update_native_world);
}
