//! Platform world-picker sources and navigation.
use bevy::prelude::*;
use cyrus_debug::WorldPicker;
#[cfg(not(target_arch = "wasm32"))]
fn initialize_native_worlds(
    install: Res<cyrus_assets::install::GameInstall>,
    mut picker: ResMut<WorldPicker>,
) -> Result {
    picker.options = cyrus_assets::scene::world_options(&install)?;
    Ok(())
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
    app.add_systems(
        Update,
        initialize_native_worlds.run_if(resource_added::<cyrus_assets::install::GameInstall>),
    )
    .add_systems(Update, update_native_world);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_picker_waits_while_no_install_is_selected() {
        let mut app = App::new();
        app.init_resource::<WorldPicker>()
            .init_resource::<cyrus_world::lifecycle::CurrentScene>()
            .insert_resource(cyrus_world::lifecycle::SceneRequest("ISLAND".into()));
        register(&mut app);
        for _ in 0..3 {
            app.update();
            assert!(app.world().resource::<WorldPicker>().options.is_empty());
        }
    }

    #[test]
    fn installed_worlds_are_available_without_exporting_them() {
        let Ok(install) = cyrus_assets::install::GameInstall::locate(None) else {
            eprintln!("skipped: no install (debug world picker)");
            return;
        };
        let mut app = App::new();
        app.init_resource::<WorldPicker>()
            .init_resource::<cyrus_world::lifecycle::CurrentScene>()
            .insert_resource(cyrus_world::lifecycle::SceneRequest("ISLAND".into()));
        register(&mut app);
        for _ in 0..3 {
            app.update();
            assert!(app.world().resource::<WorldPicker>().options.is_empty());
        }
        app.insert_resource(install);
        app.update();
        let picker = app.world().resource::<WorldPicker>();
        for name in ["ISLAND", "START", "BELLTOWR"] {
            assert!(picker.options.iter().any(|[_, map]| map == name), "{name}");
        }
        assert_eq!(picker.options[0], ["0", "START"]);
        app.world_mut().resource_mut::<WorldPicker>().pending = Some("START".into());
        app.update();
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::SceneRequest>()
                .0,
            "START"
        );
    }
}
