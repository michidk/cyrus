use bevy::prelude::*;
use cyrus_game::random::EngineRandom;
use cyrus_script::GamePlugin;

#[test]
fn game_plugin_initializes_once_and_preserves_a_supplied_state() {
    for supplied in [None, Some(42)] {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        if let Some(state) = supplied {
            app.insert_resource(EngineRandom::from_state(state));
        }
        app.add_plugins(GamePlugin);
        assert_eq!(
            app.world().resource::<EngineRandom>().state(),
            supplied.unwrap_or(1)
        );
    }
}
