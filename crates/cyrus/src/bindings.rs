//! Host initialization of saved and installed player bindings.
use bevy::prelude::*;
use cyrus_game::player::input::PlayerBindings;
pub(crate) fn load(
    preferences: Res<cyrus_game::preferences::Preferences>,
    mut commands: Commands,
    install: Option<Res<cyrus_assets::install::GameInstall>>,
) {
    let Some(install) = install else {
        return;
    };
    let path = install.root.join("KEYS.INI");
    let result = cyrus_assets::install::keyboard_codes(&install).and_then(|ini| {
        let local = preferences.menu.clone().map_err(|e| e.to_string())?;
        PlayerBindings::from_codes(
            local.map_or_else(|| ini, |p| std::array::from_fn(|i| p.bindings[i])),
        )
    });
    match result {
        Ok(bindings) => {
            commands.insert_resource(bindings);
        }
        Err(error) => error!(
            "{}: {error}; player keyboard input disabled",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_game::{
        AppState,
        fidelity::Fidelity,
        player::{PlayerPlugin, input::PlayerInput},
    };
    struct Missing;
    impl cyrus_game::preferences::PreferenceStore for Missing {
        fn load(
            &self,
            _: cyrus_game::preferences::PreferenceKey,
        ) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
        fn save(&self, _: cyrus_game::preferences::PreferenceKey, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn installed_keyboard_bindings_load_through_plugin() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let install = match cyrus_assets::install::GameInstall::open(&root) {
            Ok(install) => install,
            Err(error) => {
                eprintln!("skipped: no install ({error})");
                return;
            }
        };
        let codes = cyrus_assets::install::keyboard_codes(&install).unwrap();
        PlayerBindings::from_codes(codes).unwrap();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<AppState>()
            .init_resource::<Fidelity>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(install)
            .insert_resource(cyrus_game::preferences::Preferences::new(Missing))
            .add_plugins(PlayerPlugin)
            .add_systems(Startup, load);
        app.update();
        // Exercise the plugin loader with the install's configured bindings.
        let mut keys = ButtonInput::default();
        for key in [
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ShiftLeft,
            KeyCode::Space,
            KeyCode::AltLeft,
            KeyCode::ControlLeft,
        ] {
            keys.press(key);
        }
        let expected = PlayerBindings::from_codes(codes).unwrap().sample(&keys);
        *app.world_mut().resource_mut::<ButtonInput<KeyCode>>() = keys;
        app.update();
        assert_eq!(*app.world().resource::<PlayerInput>(), expected);
    }
}
