//! Game scripting host, player controller, simulation clock and in-game screens, built on the portable SOUP runtime.
pub mod clock;
pub mod console;
pub mod inventory_screen;
pub mod island_map;
pub mod item_viewer;
pub mod logbook;
pub mod player;
pub mod script;
use bevy::prelude::*;
use cyrus_game::{
    AppState, MovieUnlocks, PauseBookEntry, RestoredSessionEntry, cheats, fidelity, preferences,
    random,
};
use log::info;

/// Gameplay continuation retained while an eligible book menu owns presentation.
/// Startup and terminal death menus must not create this admission token.
#[derive(Resource)]
pub struct PausedGameplay {
    pub clock: clock::SimulationClock,
    pub active_cameras: Vec<Entity>,
}

pub struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        logbook::register(app);
        inventory_screen::register(app);
        item_viewer::register(app);
        island_map::register(app);
        app.init_resource::<preferences::Preferences>()
            .init_resource::<MovieUnlocks>()
            .init_resource::<random::EngineRandom>()
            .init_resource::<cheats::Cheats>()
            .init_resource::<cheats::CheatTransfer>()
            .init_state::<AppState>()
            .init_resource::<fidelity::Fidelity>()
            .add_plugins((
                player::PlayerPlugin,
                script::ScriptPlugin,
                console::GameConsolePlugin,
            ))
            .add_systems(Update, log_state_changes)
            .add_systems(
                Update,
                (request_pause_book, request_information)
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Last,
                clear_restored_entry.run_if(in_state(AppState::InGame)),
            );
    }
}

/// Gameplay book shortcuts in sampling order: Save, Load, Display, Sound, Controls.
const BOOK_SHORTCUTS: [(KeyCode, u8); 5] = [
    (KeyCode::F2, 1),
    (KeyCode::F3, 2),
    (KeyCode::F4, 5),
    (KeyCode::F5, 6),
    (KeyCode::F6, 7),
];

/// With `final_version` set, Escape is processed after its release (preservation
/// engine/menu.md); the original without it ignores Escape in a world.
#[allow(clippy::too_many_arguments)] // Shared keyboard gates and the book transition have one owner.
fn request_pause_book(
    keys: Res<ButtonInput<KeyCode>>,
    mut armed: Local<bool>,
    suppressed: Option<Res<player::input::InputSuppressed>>,
    logbook: Option<Res<logbook::LogbookScreen>>,
    console: Option<Res<cyrus_console::Console>>,
    actors: Option<Res<script::ScriptActors>>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    let held_elsewhere = suppressed.is_some_and(|input| input.0)
        || !actors.is_some_and(|actors| actors.final_version())
        || logbook.is_some_and(|screen| screen.suspended())
        || console.is_some_and(|console| console.open || console.blocked)
        || matches!(*next, NextState::Pending(_));
    // Escape outranks the shortcuts (preservation engine/menu.md).
    let page = if escape_opens_book(&keys, &mut armed, held_elsewhere) {
        Some(0)
    } else {
        BOOK_SHORTCUTS
            .into_iter()
            .find(|(key, _)| !held_elsewhere && keys.just_pressed(*key))
            .map(|(_, page)| page)
    };
    if let Some(page) = page {
        commands.insert_resource(PauseBookEntry { page });
        next.set(AppState::Menu);
    }
}

/// The press must have started while no gate held the keyboard, so the release that closes a
/// screen or the console does not also open the book.
fn escape_opens_book(keys: &ButtonInput<KeyCode>, armed: &mut bool, held_elsewhere: bool) -> bool {
    if keys.just_pressed(KeyCode::Escape) {
        *armed = !held_elsewhere;
    }
    keys.just_released(KeyCode::Escape) && std::mem::take(armed) && !held_elsewhere
}

fn clear_restored_entry(mut commands: Commands) {
    commands.remove_resource::<RestoredSessionEntry>();
}

fn log_state_changes(mut transitions: MessageReader<StateTransitionEvent<AppState>>) {
    for transition in transitions.read() {
        info!("state: {:?} -> {:?}", transition.exited, transition.entered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(keys: &mut ButtonInput<KeyCode>, armed: &mut bool, held_during_press: bool) -> bool {
        keys.press(KeyCode::Escape);
        let early = escape_opens_book(keys, armed, held_during_press);
        keys.clear();
        keys.release(KeyCode::Escape);
        let opened = escape_opens_book(keys, armed, false);
        keys.clear();
        assert!(!early);
        opened
    }

    #[test]
    fn escape_release_opens_the_book_from_gameplay() {
        let mut keys = ButtonInput::<KeyCode>::default();
        assert!(tap(&mut keys, &mut false, false));
    }

    #[test]
    fn escape_that_closes_a_screen_does_not_open_the_book() {
        let mut keys = ButtonInput::<KeyCode>::default();
        assert!(!tap(&mut keys, &mut false, true));
    }
}

/// Eligibility shared by desktop and browser Quick Save keyboard handlers.
/// The console, inspection input and in-game readers own their keys while active.
pub fn quick_save_pressed(world: &World) -> bool {
    world
        .get_resource::<State<AppState>>()
        .is_some_and(|state| *state.get() == AppState::InGame)
        && world
            .get_resource::<ButtonInput<KeyCode>>()
            .is_some_and(|keys| keys.just_pressed(KeyCode::F9))
        && world
            .get_resource::<script::ScriptActors>()
            .is_some_and(|actors| actors.final_version())
        && !world
            .get_resource::<cyrus_console::Console>()
            .is_some_and(|console| console.open || console.blocked)
        && !world
            .get_resource::<logbook::LogbookScreen>()
            .is_some_and(|screen| screen.suspended())
        && !world
            .get_resource::<player::input::InputSuppressed>()
            .is_some_and(|input| input.0)
        && !world
            .get_resource::<NextState<AppState>>()
            .is_some_and(|next| matches!(next, NextState::Pending(_)))
}

#[allow(clippy::too_many_arguments)] // Shared keyboard gates and the information transition have one owner.
fn request_information(
    keys: Res<ButtonInput<KeyCode>>,
    actors: Option<Res<script::ScriptActors>>,
    console: Option<Res<cyrus_console::Console>>,
    logbook: Option<Res<logbook::LogbookScreen>>,
    suppressed: Option<Res<player::input::InputSuppressed>>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    if console.is_some_and(|console| console.open || console.blocked)
        || logbook.is_some_and(|screen| screen.suspended())
        || suppressed.is_some_and(|input| input.0)
        || matches!(*next, NextState::Pending(_))
    {
        return;
    }
    let screen = if keys.just_pressed(KeyCode::F1)
        && actors.is_some_and(|actors| actors.final_version())
        && !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
    {
        Some(cyrus_game::GameplayOverlay::Version)
    } else if keys.just_pressed(KeyCode::F10) {
        Some(cyrus_game::GameplayOverlay::Credits)
    } else {
        None
    };
    if let Some(screen) = screen {
        commands.insert_resource(screen);
        next.set(AppState::GameplayOverlay);
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    fn final_actors() -> script::ScriptActors {
        use cyrus_assets::script::*;
        let mut data = Scripts {
            version: 30,
            weapons: Default::default(),
            combat_attacks: vec![],
            combat_defenses: vec![],
            in_combat_threshold: None,
            lineup_distance: None,
            view_distance: None,
            texture_frames: Default::default(),
            non_engaged_spacing: None,
            combat_clank_effects: [None; 2],
            node_timer: None,
            camera: Default::default(),
            clock: ClockSettings {
                normal_frame_rate: 12,
                min_frame_rate: 6,
                max_frame_rate: 300,
                use_smooth_fps: true,
                use_smooth_divisor: true,
            },
            updates: UpdateSettings {
                rope: None,
                ropes_load: None,
                fall_death_ticks: 144,
                fall_landing: None,
                dummy_model_manager: false,
                task_system: true,
                animation_system: true,
                disable_slaves: false,
                disable_master_slaves: false,
                final_version: true,
            },
            dialogue: DialogueSettings {
                menu_start: [35, 25],
                max_menu_items: 300,
                max_dialog_lines: 20,
                max_text_width: 500,
                menu_traverse_delay: 4,
                print_text: true,
                use_speech: true,
                max_distance: 1600,
                rtx_filename: "SYNTHETIC.RTX".into(),
            },
            player_distance_override: false,
            actor_pool_capacity: 10,
            content_id: String::new(),
            world: 1,
            definition: cyrus_soup::program::Definition {
                functions: vec![cyrus_soup::program::Function {
                    name: cyrus_soup::Native::NullFunction,
                    param_count: 0,
                    kind: cyrus_soup::program::CallKind::Function,
                }],
                flags: vec![],
                references: vec![],
                attributes: vec![],
            },
            actors: Default::default(),
            shells: vec![],
            instances: vec![],
            unsupported: Default::default(),
        };
        data.content_id = data.content_identity().unwrap();
        let mut actors = script::ScriptActors::default();
        actors.current = Some(script::ScriptWorld::prepare(&data, 1).unwrap());
        actors
    }

    fn information_app(key: KeyCode, blocked: bool) -> App {
        let mut app = App::new();
        let mut keys = ButtonInput::default();
        keys.press(key);
        app.insert_resource(keys)
            .insert_resource(NextState::<AppState>::Unchanged)
            .insert_resource(player::input::InputSuppressed(blocked))
            .add_systems(Update, request_information);
        app
    }

    #[test]
    fn credits_key_requests_a_world_overlay_without_a_book_or_ending_movie() {
        let mut app = information_app(KeyCode::F10, false);
        app.update();
        assert_eq!(
            app.world().get_resource::<cyrus_game::GameplayOverlay>(),
            Some(&cyrus_game::GameplayOverlay::Credits)
        );
        assert!(!app.world().contains_resource::<PauseBookEntry>());
        assert!(matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Pending(AppState::GameplayOverlay)
        ));
    }

    #[test]
    fn information_keys_respect_input_ownership_and_pending_transitions() {
        let mut blocked = information_app(KeyCode::F10, true);
        blocked.update();
        assert!(
            !blocked
                .world()
                .contains_resource::<cyrus_game::GameplayOverlay>()
        );
        let mut pending = information_app(KeyCode::F10, false);
        pending
            .world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Menu);
        pending.update();
        assert!(
            !pending
                .world()
                .contains_resource::<cyrus_game::GameplayOverlay>()
        );
    }

    #[test]
    fn version_and_quick_save_require_a_final_version_world() {
        let mut app = information_app(KeyCode::F1, false);
        app.update();
        assert!(
            !app.world()
                .contains_resource::<cyrus_game::GameplayOverlay>()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F9);
        assert!(!quick_save_pressed(app.world()));
    }

    #[test]
    fn book_keys_dispatch_the_original_pages_and_version_key_opens_information() {
        for (key, page) in [
            (KeyCode::F2, 1),
            (KeyCode::F3, 2),
            (KeyCode::F4, 5),
            (KeyCode::F5, 6),
            (KeyCode::F6, 7),
        ] {
            let mut app = information_app(key, false);
            app.insert_resource(final_actors())
                .add_systems(Update, request_pause_book.before(request_information));
            app.update();
            assert_eq!(app.world().resource::<PauseBookEntry>().page, page);
            assert!(matches!(
                app.world().resource::<NextState<AppState>>(),
                NextState::Pending(AppState::Menu)
            ));
        }
        let mut app = information_app(KeyCode::F1, false);
        app.insert_resource(final_actors());
        app.update();
        assert_eq!(
            app.world().get_resource::<cyrus_game::GameplayOverlay>(),
            Some(&cyrus_game::GameplayOverlay::Version)
        );
    }

    #[test]
    fn quick_save_requires_gameplay_keyboard_ownership() {
        let mut app = information_app(KeyCode::F9, false);
        app.insert_resource(State::new(AppState::InGame))
            .insert_resource(final_actors());
        assert!(quick_save_pressed(app.world()));
        app.insert_resource(player::input::InputSuppressed(true));
        assert!(!quick_save_pressed(app.world()));
        app.insert_resource(player::input::InputSuppressed(false));
        let mut console = cyrus_console::Console::default();
        console.open = true;
        app.insert_resource(console);
        assert!(!quick_save_pressed(app.world()));
        app.world_mut()
            .resource_mut::<cyrus_console::Console>()
            .open = false;
        app.world_mut()
            .resource_mut::<cyrus_console::Console>()
            .blocked = true;
        assert!(!quick_save_pressed(app.world()));
        app.world_mut()
            .resource_mut::<cyrus_console::Console>()
            .blocked = false;
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Menu);
        assert!(!quick_save_pressed(app.world()));
    }
}
