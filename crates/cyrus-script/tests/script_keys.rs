//! The keyboard reaches the script inputs: the production sampling system fills `ScriptKeys`
//! with the navigation keys the `yeahbaby` handler reads.
use bevy::{asset::AssetApp, input::ButtonInput, prelude::*, state::app::StatesPlugin};
use cyrus_assets::animation::Animations;
use cyrus_assets::script::Scripts;
use cyrus_game::AppState;
use cyrus_script::script::{ScriptKeys, ScriptPlugin};
use cyrus_world::animation::AnimationTick;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .init_asset::<cyrus_script::player::data::PlayerScene>()
        .init_asset::<cyrus_assets::environment::Environment>()
        .add_message::<AnimationTick>()
        .add_message::<cyrus_world::sound::SoundCue>()
        .add_plugins(ScriptPlugin)
        .init_resource::<ButtonInput<KeyCode>>();
    app
}

fn sampled(app: &mut App) -> ScriptKeys {
    app.update();
    *app.world().resource::<ScriptKeys>()
}

#[test]
fn page_keys_and_insert_are_held_states_and_home_is_a_press_edge() {
    let mut app = app();
    assert_eq!(sampled(&mut app), ScriptKeys::default());
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::PageUp);
        keys.press(KeyCode::PageDown);
        keys.press(KeyCode::Insert);
        keys.press(KeyCode::Home);
    }
    let keys = sampled(&mut app);
    assert!(keys.page_up && keys.page_down && keys.insert && keys.home_pressed);
    // The frame ends: Home stays down but its press edge is gone; the other keys stay held.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    let keys = sampled(&mut app);
    assert!(keys.page_up && keys.page_down && keys.insert);
    assert!(!keys.home_pressed);
    // Released keys read as released.
    {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release(KeyCode::PageUp);
        input.release(KeyCode::Insert);
        input.release(KeyCode::Home);
        input.clear();
    }
    let keys = sampled(&mut app);
    assert!(!keys.page_up && keys.page_down && !keys.insert && !keys.home_pressed);
    // A second Home press is a second edge.
    {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.press(KeyCode::Home);
    }
    assert!(sampled(&mut app).home_pressed);
}

#[test]
fn space_is_a_held_state_across_frames_and_clears_on_release_or_input_suspension() {
    let mut app = app();
    assert!(!sampled(&mut app).space);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    assert!(sampled(&mut app).space);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    assert!(sampled(&mut app).space);
    app.insert_resource(cyrus_script::player::input::InputSuppressed(true));
    assert!(!sampled(&mut app).space);
    app.world_mut()
        .resource_mut::<cyrus_script::player::input::InputSuppressed>()
        .0 = false;
    assert!(sampled(&mut app).space);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Space);
    assert!(!sampled(&mut app).space);
}
