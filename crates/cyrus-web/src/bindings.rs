#![cfg(any(target_arch = "wasm32", test))]
//! Host initialization of saved and installed player bindings.
use bevy::prelude::*;
use cyrus_script::player::input::PlayerBindings;
pub(crate) fn load_scene_bindings(
    preferences: Res<cyrus_game::preferences::Preferences>,
    request: Option<Res<cyrus_script::player::data::PlayerSceneRequest>>,
    scenes: Res<Assets<cyrus_script::player::data::PlayerScene>>,
    mut bindings: ResMut<PlayerBindings>,
    mut applied: Local<Option<AssetId<cyrus_script::player::data::PlayerScene>>>,
) {
    let Some(request) = request else {
        return;
    };
    if *applied == Some(request.0.id()) {
        return;
    }
    *bindings = PlayerBindings::default();
    let Some(scene) = scenes.get(&request.0) else {
        return;
    };
    let result = (|| {
        let local = preferences.menu().clone().map_err(|e| e.to_string())?;
        let codes = match local {
            Some(preferences) => preferences.bindings,
            None => scene
                .control_codes
                .ok_or("missing keyboard defaults; re-export world")?,
        };
        let bindings = PlayerBindings::from_codes(codes)?;
        match scene.settings.joystick {
            Some(joystick) => bindings.with_joystick(joystick.codes, joystick.tolerance),
            None => Ok(bindings),
        }
    })();
    match result {
        Ok(loaded) => *bindings = loaded,
        Err(error) => error!(
            "world {} player keyboard input disabled: {error}",
            scene.world
        ),
    }
    *applied = Some(request.0.id());
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_game::preferences::{PreferenceKey, PreferenceStore, Preferences};
    use cyrus_script::player::data::{MovementSettings, PlayerScene, PlayerSceneRequest};
    struct Missing;
    impl PreferenceStore for Missing {
        fn load(&self, _: PreferenceKey) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
        fn save(&self, _: PreferenceKey, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }
    fn scene() -> PlayerScene {
        let collision = cyrus_assets::collision::CollisionScene::default();
        PlayerScene {
            source_identity: None,
            actor_collision: None,
            model_radii: Default::default(),
            world: 17,
            control_codes: Some([72, 80, 75, 77, 42, 57, 56, 29, 23, 31, 35, 50, 38, 52, 51]),
            settings: MovementSettings {
                standing_height: -2,
                post_collide_height: 50,
                slide_range: -60000,
                slide_speed: None,
                slide_hurt: None,
                tap_time: None,
                sheath_sword_delay: None,
                defend_interval: None,
                swim_depth: None,
                max_remap_objects: None,
                turn_speed: 12,
                turn_max_speed: 48,
                poly_push_units: 16,
                joystick: None,
            },
            definition: None,
            terrain: None,
            unavailable: Some("synthetic actor".into()),
            static_collision: cyrus_collision::collision_world::StaticCollision::new(&collision)
                .unwrap(),
            collision,
        }
    }
    #[test]
    fn pending_sidecar_clears_old_bindings_and_loaded_sidecar_uses_saved_override() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<PlayerScene>>()
            .init_resource::<PlayerBindings>()
            .insert_resource(Preferences::new(Missing))
            .add_systems(Update, load_scene_bindings);
        let old = app
            .world_mut()
            .resource_mut::<Assets<PlayerScene>>()
            .add(scene());
        app.insert_resource(PlayerSceneRequest(old)).update();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::ArrowUp);
        assert!(
            app.world()
                .resource::<PlayerBindings>()
                .sample(&keys)
                .forward
        );
        let pending = app
            .world()
            .resource::<Assets<PlayerScene>>()
            .reserve_handle();
        app.insert_resource(PlayerSceneRequest(pending.clone()))
            .update();
        assert!(
            !app.world()
                .resource::<PlayerBindings>()
                .sample(&keys)
                .forward
        );
        let mut saved = cyrus_assets::preferences::MenuPreferences {
            values: [(504, 1), (505, 0), (601, 10), (602, 10), (603, 1)].into(),
            bindings: [0; 15],
        };
        saved.bindings[0] = 17;
        app.world_mut()
            .resource_mut::<Preferences>()
            .save_menu(saved)
            .unwrap();
        app.world_mut()
            .resource_mut::<Assets<PlayerScene>>()
            .insert(pending.id(), scene())
            .unwrap();
        app.update();
        assert!(
            !app.world()
                .resource::<PlayerBindings>()
                .sample(&keys)
                .forward
        );
        keys.press(KeyCode::KeyW);
        assert!(
            app.world()
                .resource::<PlayerBindings>()
                .sample(&keys)
                .forward
        );
    }
}
