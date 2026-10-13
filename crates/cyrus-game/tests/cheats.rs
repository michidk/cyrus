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
    let mut cheats = Cheats::new(Store::default());
    cyrus_game::cheat_command::execute(&mut cheats, " ODDJOB = 0x2 ; oddjob = 99").unwrap();
    assert_eq!(cheats.state().unwrap().value("oddjob"), Some(2));
    cyrus_game::cheat_command::execute(&mut cheats, "oddjob").unwrap();
    assert_eq!(cheats.state().unwrap().value("oddjob"), Some(1));
    cheats.execute("oddjob", Some(2)).unwrap();
    cheats.execute("oddjob", None).unwrap();
    assert_eq!(cheats.state().unwrap().value("oddjob"), Some(0));
    cyrus_game::cheat_command::execute(&mut cheats, "neversaydie = -1").unwrap();
    assert_eq!(cheats.state().unwrap().value("neversaydie"), Some(u32::MAX));
    assert!(cheats.distance_override().unwrap());
    let before = cheats.inspect();
    for invalid in [
        "oddjob =",
        "oddjob = 4294967296",
        "oddjob = unknown",
        "oddjob 2",
    ] {
        assert!(cyrus_game::cheat_command::execute(&mut cheats, invalid).is_err());
        assert_eq!(cheats.inspect(), before);
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

#[test]
fn console_editor_preserves_commands_and_never_edits_report_lines() {
    use bevy::prelude::KeyCode;
    use cyrus_game::console::Console;
    let mut cheats = Cheats::new(Store::default());
    let mut console = Console::default();
    console.key(KeyCode::KeyO, Some("oddjob = 2"), &mut cheats);
    console.key(KeyCode::Enter, None, &mut cheats);
    assert_eq!(cheats.state().unwrap().value("oddjob"), Some(2));
    console.key(KeyCode::Tab, None, &mut cheats);
    assert_eq!(console.lines[console.selected].text, "oddjob = 2");
    console.key(KeyCode::Backspace, None, &mut cheats);
    console.key(KeyCode::Digit3, Some("3"), &mut cheats);
    console.key(KeyCode::Enter, None, &mut cheats);
    assert_eq!(cheats.state().unwrap().value("oddjob"), Some(3));
    console.key(KeyCode::ArrowUp, None, &mut cheats);
    assert!(console.lines[console.selected].report);
    let report = console.lines[console.selected].text.clone();
    console.key(KeyCode::KeyA, Some("a"), &mut cheats);
    console.key(KeyCode::Delete, None, &mut cheats);
    assert_eq!(console.lines[console.selected].text, report);
    console.submit(&mut cheats, "CON clear; ignored");
    assert_eq!(console.lines.len(), 1);
    assert_eq!(console.selected, 0);
    console.key(KeyCode::KeyA, Some(&"a".repeat(101)), &mut cheats);
    assert_eq!(console.lines[0].text.len(), 100);
    for _ in 0..600 {
        console.submit(&mut cheats, "oracle");
    }
    assert_eq!(console.lines.len(), 512);
    assert!(console.selected < console.top + console.page_lines);
}

#[test]
fn hidden_entry_is_case_insensitive_and_requires_key_release() {
    use bevy::prelude::KeyCode;
    let mut cheats = Cheats::new(Store::default());
    let mut console = cyrus_game::console::Console::default();
    for ch in "NEVERSAYDIE".chars() {
        console.gameplay(KeyCode::KeyA, Some(&ch.to_string()), &mut cheats);
        console.gameplay(KeyCode::KeyA, Some("ignored repeat"), &mut cheats);
        console.release();
    }
    console.gameplay(KeyCode::Enter, None, &mut cheats);
    assert!(cheats.distance_override().unwrap());
    console.release();
    console.gameplay(KeyCode::Enter, None, &mut cheats);
    assert!(cheats.distance_override().unwrap());
}

#[test]
fn action_commands_ignore_assignment_text() {
    let mut cheats = Cheats::new(Store::default());
    assert_eq!(
        cyrus_game::cheat_command::execute(&mut cheats, "oracle = ignored")
            .unwrap()
            .len(),
        13
    );
    cyrus_game::cheat_command::execute(&mut cheats, "savecheats = ignored").unwrap();
    assert_eq!(cheats.state().unwrap().value("savecheats"), Some(1));
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
    use cyrus_game::{
        AppState,
        console::{Console, ConsolePlugin},
    };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, InputPlugin))
        .init_state::<AppState>()
        .insert_resource(Cheats::new(Store::default()))
        .add_plugins(ConsolePlugin);
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
