use bevy::prelude::*;
use cyrus_console::{Command, Console, ConsoleApp, ConsolePlugin, Invocation, Source, submit};

#[test]
fn numeric_literals_preserve_raw_values_and_original_binary_quirk() {
    for (input, expected) in [
        ("4294967295", u32::MAX),
        ("-1", u32::MAX),
        ("0x80000000", 0x80000000), // publish-check: allow; 32-bit console literal.
        ("%1011", 0),
        ("4294967296", 0),
        ("4294967297tail", 1),
        ("-4294967297tail", u32::MAX),
        ("-", 0),
        ("--1", 0),
        ("12garbage", 12),
        ("0x", 0),
        ("0xgarbage", 0),
        ("0x1g", 1),
        ("0x100000001", 1), // publish-check: allow; wrapping console literal.
        ("\"abcd\"", 0x6463_6261),
        ("\"abcdef", 0x6463_6261),
    ] {
        assert_eq!(
            cyrus_console::number(input, |_| panic!("literal resolved as symbol")).unwrap(),
            expected
        );
    }
    for input in ["", "\"ab", "é"] {
        assert!(
            cyrus_console::number(input, |_| panic!("invalid literal resolved as symbol")).is_err()
        );
    }
}

#[test]
fn numeric_symbols_are_resolved_by_the_command_owner() {
    for input in ["counter", "+1"] {
        assert_eq!(
            cyrus_console::number(input, |name| {
                assert_eq!(name, input);
                Ok(42)
            })
            .unwrap(),
            42
        );
    }
    assert!(cyrus_console::number("counter", |_| anyhow::bail!("world unavailable")).is_err());
}

#[derive(Resource, Default)]
struct Counter(u32);
fn increment(
    In(command): In<Invocation>,
    mut counter: ResMut<Counter>,
) -> anyhow::Result<Vec<String>> {
    counter.0 += command.args.parse::<u32>()?;
    Ok(vec![format!("value {}", counter.0)])
}
#[test]
fn separate_plugins_register_handlers_without_a_game_or_renderer() {
    let mut app = App::new();
    app.add_plugins(ConsolePlugin)
        .init_resource::<Counter>()
        .console_command(
            Command {
                name: "increment".into(),
                alias: Some("inc".into()),
                usage: "increment value".into(),
                cheat: false,
            },
            increment,
        );
    assert_eq!(
        submit(app.world_mut(), "INC 2; increment 100", Source::Interactive).unwrap(),
        ["value 2"]
    );
    assert!(submit(app.world_mut(), "inc 3", Source::Programmatic).is_err());
    submit(app.world_mut(), "increment 3", Source::Programmatic).unwrap();
    assert_eq!(app.world().resource::<Counter>().0, 5);
    submit(app.world_mut(), "increment 9", Source::Gameplay).unwrap();
    assert_eq!(app.world().resource::<Counter>().0, 5);
    assert!(submit(app.world_mut(), "increment invalid", Source::Interactive).is_err());
    assert_eq!(app.world().resource::<Counter>().0, 5);
    assert!(
        app.world()
            .resource::<Console>()
            .lines
            .iter()
            .any(|line| line.report && line.text.contains("invalid digit"))
    );
    let commands = submit(app.world_mut(), "?", Source::Interactive).unwrap();
    assert!(commands.iter().any(|line| line.contains("increment [inc]")));
    submit(app.world_mut(), "con clear", Source::Interactive).unwrap();
    assert_eq!(app.world().resource::<Console>().lines.len(), 1);
}

#[test]
fn invalid_input_reports_an_error_and_leaves_an_editable_prompt() {
    let mut app = App::new();
    app.add_plugins(ConsolePlugin);
    for input in ["é".to_owned(), "x".repeat(101)] {
        assert!(submit(app.world_mut(), &input, Source::Interactive).is_err());
        let console = app.world().resource::<Console>();
        assert!(
            console
                .lines
                .iter()
                .any(|line| line.report && line.text.contains("ASCII"))
        );
        assert!(!console.lines[console.selected].report);
        app.world_mut()
            .resource_mut::<Console>()
            .key(KeyCode::KeyA, Some("a"));
    }
}

#[test]
fn hidden_input_accepts_only_the_complete_cheat_name() {
    let mut app = App::new();
    app.add_plugins(ConsolePlugin)
        .init_resource::<Counter>()
        .console_command(
            Command {
                name: "toggle".into(),
                alias: Some("tog".into()),
                usage: "toggle".into(),
                cheat: true,
            },
            |_: In<Invocation>, mut counter: ResMut<Counter>| {
                counter.0 += 1;
                Ok(Vec::new())
            },
        );
    for input in ["tog", "toggle ", " toggle", "toggle;", "toggle=1"] {
        submit(app.world_mut(), input, Source::Gameplay).unwrap();
    }
    assert_eq!(app.world().resource::<Counter>().0, 0);
    submit(app.world_mut(), "TOGGLE", Source::Gameplay).unwrap();
    assert_eq!(app.world().resource::<Counter>().0, 1);
    assert_eq!(app.world().resource::<Console>().lines.len(), 2);
}

#[test]
fn keyboard_batch_dispatches_before_editing_the_next_prompt() {
    use bevy::input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, ConsolePlugin));
    app.world_mut().resource_mut::<Console>().open = true;
    for (key_code, logical_key) in [
        (KeyCode::KeyC, Key::Character("con clear".into())),
        (KeyCode::Enter, Key::Enter),
        (KeyCode::KeyN, Key::Character("next".into())),
    ] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }
    app.update();
    let console = app.world().resource::<Console>();
    assert_eq!(console.lines.len(), 1);
    assert_eq!(console.lines[0].text, "next");
}

#[derive(Default)]
struct StoredPreferences {
    document: Option<cyrus_console::Preferences>,
    log: String,
    fail: bool,
}
#[derive(Clone, Default)]
struct MemoryStore(std::sync::Arc<std::sync::Mutex<StoredPreferences>>);
impl cyrus_console::Store for MemoryStore {
    fn load(&self) -> anyhow::Result<Option<cyrus_console::Preferences>> {
        Ok(self.0.lock().unwrap().document.clone())
    }
    fn save(&self, state: &cyrus_console::Preferences) -> anyhow::Result<()> {
        let mut stored = self.0.lock().unwrap();
        anyhow::ensure!(!stored.fail, "write failed");
        stored.document = Some(state.clone());
        Ok(())
    }
    fn write_log(&self, text: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().log = text.into();
        Ok(())
    }
}

#[test]
fn preference_commands_save_restart_and_preserve_state_on_invalid_values_and_failed_writes() {
    let store = MemoryStore::default();
    let mut app = App::new();
    app.add_plugins(ConsolePlugin)
        .insert_resource(Console::with_store(store.clone()));
    app.world_mut().resource_mut::<Console>().palette = Some([42; 16]);
    assert!(submit(app.world_mut(), "console col3", Source::Interactive).is_err());
    for command in [
        "console font",
        "console shadow",
        "console paper",
        "console col3",
        "console col1",
        "console hex,-2",
        "console limit,300",
        "console save",
        "console write",
    ] {
        submit(app.world_mut(), command, Source::Interactive).unwrap();
    }
    let saved = store.0.lock().unwrap().document.clone().unwrap();
    assert_eq!((saved.font, saved.shadow, saved.paper_disabled), (1, 1, 1));
    assert_eq!((saved.colors, saved.background), ([0, 14, 9, 1], 42));
    assert_eq!((saved.hex_lines, saved.screen_limit), (-2, 76800));
    let restored = Console::with_store(store.clone());
    assert_eq!(
        (restored.preferences.hex_lines, restored.preferences.shadow),
        (1, 0)
    );
    assert_eq!(restored.preferences.colors, [0, 14, 9, 1]);
    assert!(
        store
            .0
            .lock()
            .unwrap()
            .log
            .contains("> console save\n REDGUARD.CPF saved.\n")
    );
    assert!(submit(app.world_mut(), "console limit,301", Source::Interactive).is_err());
    assert_eq!(app.world().resource::<Console>().preferences, saved);
    store.0.lock().unwrap().fail = true;
    submit(app.world_mut(), "console font", Source::Interactive).unwrap();
    assert!(submit(app.world_mut(), "console save", Source::Interactive).is_err());
    assert_eq!(store.0.lock().unwrap().document.as_ref(), Some(&saved));
    assert_eq!(app.world().resource::<Console>().preferences.font, 0);
}

#[test]
fn catalog_recognizes_original_aliases_and_numeric_preferences_use_host_symbols() {
    let mut app = App::new();
    app.add_plugins(ConsolePlugin)
        .console_symbols(|In(name): In<String>| Ok((name == "count").then_some(25)));
    let lines = submit(app.world_mut(), "?", Source::Interactive).unwrap();
    assert_eq!(lines.len(), 82);
    assert!(lines.iter().any(|line| line == "setflag [sf]  unsupported"));
    assert!(
        submit(app.world_mut(), "sf value,1", Source::Interactive)
            .unwrap_err()
            .to_string()
            .contains("unsupported")
    );
    assert!(
        submit(app.world_mut(), "sf value,1", Source::Programmatic)
            .unwrap_err()
            .to_string()
            .contains("unknown")
    );
    submit(app.world_mut(), "console limit,count", Source::Interactive).unwrap();
    assert_eq!(
        app.world().resource::<Console>().preferences.screen_limit,
        6400
    );
    let diagnostic = submit(app.world_mut(), "console hex,unknown", Source::Interactive).unwrap();
    assert!(diagnostic[0].contains("using 1"));
    assert_eq!(app.world().resource::<Console>().preferences.hex_lines, 1);
}

#[test]
fn cursor_uses_bios_intervals_and_key_input_forces_visibility() {
    let mut console = Console::default();
    console.tick(17);
    assert!(!console.cursor_visible());
    console.tick(18);
    assert!(console.cursor_visible());
    console.tick(36);
    assert!(!console.cursor_visible());
    console.tick(72); // A delayed invocation toggles once, not once per elapsed interval.
    assert!(console.cursor_visible());
    console.tick(90);
    assert!(!console.cursor_visible());
    console.key(KeyCode::KeyA, Some("a"));
    assert!(console.cursor_visible());
}

#[test]
fn idle_input_and_empty_dispatch_do_not_mark_console_changed() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, ConsolePlugin));
    let changes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = changes.clone();
    app.add_systems(Update, move |console: Res<Console>| {
        if console.is_changed() {
            observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    });
    app.update();
    app.update();
    assert_eq!(changes.load(std::sync::atomic::Ordering::Relaxed), 1);
}
