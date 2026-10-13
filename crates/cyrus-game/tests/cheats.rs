use cyrus_assets::cheats::{self, CheatFile};
use cyrus_game::cheats::{CheatEffect, CheatStore, Cheats};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Store(Arc<Mutex<Option<Vec<u8>>>>);
impl CheatStore for Store {
    fn load(&self) -> anyhow::Result<CheatFile> {
        self.0
            .lock()
            .unwrap()
            .as_deref()
            .map(cheats::decode)
            .unwrap_or_else(|| Ok(cheats::empty()))
    }
    fn save(&self, state: &CheatFile) -> anyhow::Result<()> {
        *self.0.lock().unwrap() = Some(cheats::encode(state));
        Ok(())
    }
}

#[test]
fn toggles_assigns_lists_and_restores_only_after_savecheats() {
    let store = Store::default();
    let mut runtime = Cheats::new(store.clone());
    assert!(!runtime.distance_override().unwrap());
    assert_eq!(
        runtime.execute("neversaydie", None).unwrap(),
        CheatEffect::PlayerDistance
    );
    assert!(runtime.distance_override().unwrap());
    assert!(!Cheats::new(store.clone()).distance_override().unwrap());
    runtime.execute("neversaydie", Some(42)).unwrap();
    assert_eq!(runtime.state().unwrap().value("neversaydie"), Some(42));
    runtime.execute("savecheats", None).unwrap();
    let mut restored = Cheats::new(store);
    assert_eq!(restored.state().unwrap().value("neversaydie"), Some(42));
    let before = restored.inspect();
    assert_eq!(restored.execute("oracle", None).unwrap(), CheatEffect::List);
    assert_eq!(restored.inspect(), before);
    assert!(restored.execute("unknown", None).is_err());
    assert_eq!(restored.inspect(), before);
    restored.execute("neversaydie", None).unwrap();
    assert!(!restored.distance_override().unwrap());
    assert_eq!(
        restored.execute("oddjob", None).unwrap(),
        CheatEffect::Unsupported
    );
    assert_eq!(
        restored.execute("magiccarpet", None).unwrap(),
        CheatEffect::Unsupported
    );
}

#[test]
fn preserves_raw_values_and_unnamed_slots() {
    let mut bytes = vec![0; 256];
    bytes[252..].copy_from_slice(&[0x78, 0x56, 0x34, 0x12]);
    let store = Store(Arc::new(Mutex::new(Some(bytes.clone()))));
    let mut runtime = Cheats::new(store.clone());
    runtime.execute("oddjob", Some(u32::MAX)).unwrap();
    runtime.execute("savecheats", None).unwrap();
    bytes[44..48].copy_from_slice(&[255; 4]);
    assert_eq!(store.0.lock().unwrap().as_ref().unwrap(), &bytes);
}

#[test]
fn malformed_state_cannot_be_overwritten_by_commands() {
    let store = Store(Arc::new(Mutex::new(Some(vec![1, 2, 3]))));
    let mut runtime = Cheats::new(store.clone());
    assert_eq!(runtime.inspect()["ready"], false);
    assert!(runtime.execute("neversaydie", None).is_err());
    assert!(runtime.execute("savecheats", None).is_err());
    assert_eq!(store.0.lock().unwrap().as_deref(), Some(&[1, 2, 3][..]));
}

#[test]
fn failed_write_reports_error_and_retains_runtime_state() {
    struct ReadOnly;
    impl CheatStore for ReadOnly {
        fn load(&self) -> anyhow::Result<CheatFile> {
            Ok(cheats::empty())
        }
        fn save(&self, _: &CheatFile) -> anyhow::Result<()> {
            anyhow::bail!("disk full")
        }
    }
    let mut runtime = Cheats::new(ReadOnly);
    runtime.execute("neversaydie", None).unwrap();
    assert!(
        runtime
            .execute("savecheats", None)
            .unwrap_err()
            .to_string()
            .contains("disk full")
    );
    assert!(runtime.distance_override().unwrap());
    let before = runtime.inspect();
    assert!(runtime.import(cheats::empty()).is_err());
    assert_eq!(runtime.inspect(), before);
}

#[test]
fn console_and_gameplay_have_distinct_raw_toggle_rules() {
    use cyrus_console::{Source, submit};
    let mut app = console_app();
    submit(
        app.world_mut(),
        " ODDJOB = 0x2 ; oddjob = 99",
        Source::Interactive,
    )
    .unwrap();
    assert_eq!(
        app.world()
            .resource::<Cheats>()
            .state()
            .unwrap()
            .value("oddjob"),
        Some(2)
    );
    submit(app.world_mut(), "oddjob", Source::Interactive).unwrap();
    assert_eq!(
        app.world()
            .resource::<Cheats>()
            .state()
            .unwrap()
            .value("oddjob"),
        Some(1)
    );
    app.world_mut()
        .resource_mut::<Cheats>()
        .execute("oddjob", Some(2))
        .unwrap();
    submit(app.world_mut(), "oddjob", Source::Gameplay).unwrap();
    assert_eq!(
        app.world()
            .resource::<Cheats>()
            .state()
            .unwrap()
            .value("oddjob"),
        Some(0)
    );
    submit(app.world_mut(), "neversaydie = -1", Source::Interactive).unwrap();
    let cheats = app.world().resource::<Cheats>();
    assert_eq!(cheats.state().unwrap().value("neversaydie"), Some(u32::MAX));
    assert!(cheats.distance_override().unwrap());
    let before = cheats.inspect();
    for invalid in [
        "oddjob =",
        "oddjob = 4294967296",
        "oddjob = unknown",
        "oddjob 2",
    ] {
        assert!(submit(app.world_mut(), invalid, Source::Interactive).is_err());
        assert_eq!(app.world().resource::<Cheats>().inspect(), before);
    }
}

#[test]
fn savecheats_writes_zero_for_its_slot_then_sets_live_slot_to_one() {
    let store = Store::default();
    let mut cheats = Cheats::new(store.clone());
    cheats.execute("savecheats", Some(42)).unwrap();
    cheats.console("savecheats", None).unwrap();
    assert_eq!(cheats.state().unwrap().value("savecheats"), Some(1));
    assert_eq!(
        &store.0.lock().unwrap().as_ref().unwrap()[24..28],
        &[0, 0, 0, 0]
    );
}

#[test]
fn explicit_import_recovers_failed_load_without_discarding_unnamed_slots() {
    let store = Store(Arc::new(Mutex::new(Some(vec![1, 2, 3]))));
    let mut cheats = Cheats::new(store.clone());
    let mut bytes = vec![0; 256];
    bytes[255] = 128;
    cheats
        .import(cyrus_assets::cheats::decode(&bytes).unwrap())
        .unwrap();
    assert_eq!(cheats.inspect()["ready"], true);
    assert_eq!(*store.0.lock().unwrap(), Some(bytes));
}

fn console_app() -> bevy::prelude::App {
    use bevy::prelude::*;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<cyrus_game::AppState>()
        .insert_resource(Cheats::new(Store::default()))
        .add_plugins(cyrus_game::console::GameConsolePlugin);
    app.world_mut()
        .resource_mut::<NextState<cyrus_game::AppState>>()
        .set(cyrus_game::AppState::InGame);
    app.update();
    app
}

#[test]
fn every_preservation_cheat_has_a_registered_handler() {
    let mut app = console_app();
    for entry in cyrus_assets::cheats::empty().entries() {
        if let Some(name) = &entry.name {
            cyrus_console::submit(app.world_mut(), name, cyrus_console::Source::Programmatic)
                .unwrap_or_else(|error| panic!("{name}: {error:#}"));
        }
    }
}

#[test]
fn console_editor_preserves_commands_and_never_edits_report_lines() {
    use bevy::prelude::*;
    use cyrus_console::{Console, Source};
    let mut app = console_app();
    let key = |app: &mut App, code, text| {
        app.world_mut().resource_mut::<Console>().key(code, text);
        app.update();
    };
    key(&mut app, KeyCode::KeyO, Some("oddjob = 2"));
    key(&mut app, KeyCode::Enter, None);
    assert_eq!(
        app.world()
            .resource::<Cheats>()
            .state()
            .unwrap()
            .value("oddjob"),
        Some(2)
    );
    key(&mut app, KeyCode::Tab, None);
    let console = app.world().resource::<Console>();
    assert_eq!(console.lines[console.selected].text, "oddjob = 2");
    key(&mut app, KeyCode::Backspace, None);
    key(&mut app, KeyCode::Digit3, Some("3"));
    key(&mut app, KeyCode::Enter, None);
    assert_eq!(
        app.world()
            .resource::<Cheats>()
            .state()
            .unwrap()
            .value("oddjob"),
        Some(3)
    );
    key(&mut app, KeyCode::ArrowUp, None);
    let console = app.world().resource::<Console>();
    assert!(console.lines[console.selected].report);
    let report = console.lines[console.selected].text.clone();
    key(&mut app, KeyCode::KeyA, Some("a"));
    key(&mut app, KeyCode::Delete, None);
    let console = app.world().resource::<Console>();
    assert_eq!(console.lines[console.selected].text, report);
    cyrus_console::submit(app.world_mut(), "CON clear; ignored", Source::Interactive).unwrap();
    assert_eq!(app.world().resource::<Console>().lines.len(), 1);
    key(&mut app, KeyCode::KeyA, Some(&"a".repeat(101)));
    assert_eq!(app.world().resource::<Console>().lines[0].text.len(), 100);
    for _ in 0..600 {
        cyrus_console::submit(app.world_mut(), "oracle", Source::Interactive).unwrap();
    }
    let console = app.world().resource::<Console>();
    assert_eq!(console.lines.len(), 512);
    assert!(console.selected < console.top + console.page_lines);
}

#[test]
fn hidden_entry_is_case_insensitive_and_requires_key_release() {
    use bevy::prelude::KeyCode;
    use cyrus_console::Console;
    let mut app = console_app();
    for ch in "NEVERSAYDIE".chars() {
        let mut console = app.world_mut().resource_mut::<Console>();
        console.gameplay(KeyCode::KeyA, Some(&ch.to_string()));
        console.gameplay(KeyCode::KeyA, Some("ignored repeat"));
        console.release();
    }
    app.world_mut()
        .resource_mut::<Console>()
        .gameplay(KeyCode::Enter, None);
    app.update();
    assert!(
        app.world()
            .resource::<Cheats>()
            .distance_override()
            .unwrap()
    );
    app.world_mut().resource_mut::<Console>().release();
    app.world_mut()
        .resource_mut::<Console>()
        .gameplay(KeyCode::Enter, None);
    app.update();
    assert!(
        app.world()
            .resource::<Cheats>()
            .distance_override()
            .unwrap()
    );
}

#[test]
fn action_commands_ignore_assignment_text() {
    use cyrus_console::{Source, submit};
    let mut app = console_app();
    assert_eq!(
        submit(app.world_mut(), "oracle = ignored", Source::Interactive)
            .unwrap()
            .len(),
        13
    );
    submit(app.world_mut(), "savecheats = ignored", Source::Interactive).unwrap();
    assert_eq!(
        app.world()
            .resource::<Cheats>()
            .state()
            .unwrap()
            .value("savecheats"),
        Some(1)
    );
}

#[test]
fn cheat_assignment_uses_shared_packed_and_binary_literals() {
    let mut app = console_app();
    for (command, expected) in [("oddjob = \"ABCD\"", 0x6463_6261), ("oddjob = %1011", 0)] {
        cyrus_console::submit(app.world_mut(), command, cyrus_console::Source::Interactive)
            .unwrap();
        assert_eq!(
            app.world()
                .resource::<Cheats>()
                .state()
                .unwrap()
                .value("oddjob"),
            Some(expected)
        );
    }
}

#[test]
fn keyboard_events_open_close_and_capture_player_keys() {
    use bevy::{
        input::{
            ButtonState, InputPlugin,
            keyboard::{Key, KeyboardInput},
        },
        prelude::*,
        state::app::StatesPlugin,
    };
    use cyrus_console::Console;
    use cyrus_game::{AppState, console::GameConsolePlugin};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, InputPlugin))
        .init_state::<AppState>()
        .insert_resource(Cheats::new(Store::default()))
        .add_plugins(GameConsolePlugin);
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    let send = |app: &mut App, key_code, logical_key, state, repeat| {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    };
    send(
        &mut app,
        KeyCode::F12,
        Key::F12,
        ButtonState::Pressed,
        false,
    );
    assert!(app.world().resource::<Console>().open);
    send(&mut app, KeyCode::F12, Key::F12, ButtonState::Pressed, true);
    assert!(app.world().resource::<Console>().open);
    send(
        &mut app,
        KeyCode::KeyW,
        Key::Character("w".into()),
        ButtonState::Pressed,
        false,
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    assert_eq!(
        app.world().resource::<Console>().lines.last().unwrap().text,
        "w"
    );
    send(
        &mut app,
        KeyCode::Space,
        Key::Space,
        ButtonState::Pressed,
        false,
    );
    send(
        &mut app,
        KeyCode::KeyW,
        Key::Character("w".into()),
        ButtonState::Pressed,
        false,
    );
    assert_eq!(
        app.world().resource::<Console>().lines.last().unwrap().text,
        "w w"
    );
    send(
        &mut app,
        KeyCode::Escape,
        Key::Escape,
        ButtonState::Pressed,
        false,
    );
    assert!(!app.world().resource::<Console>().open);
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Escape)
    );
    app.world_mut().resource_mut::<Console>().blocked = true;
    send(
        &mut app,
        KeyCode::F12,
        Key::F12,
        ButtonState::Pressed,
        false,
    );
    assert!(!app.world().resource::<Console>().open);
    app.world_mut().resource_mut::<Console>().blocked = false;
    app.update();
    assert!(!app.world().resource::<Console>().open);
}
