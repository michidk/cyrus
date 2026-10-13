use bevy::prelude::*;
use cyrus_console::{Command, Console, ConsoleApp, ConsolePlugin, Invocation, Source, submit};

#[test]
fn numeric_literals_preserve_raw_values_and_original_binary_quirk() {
    for (input, expected) in [
        ("4294967295", u32::MAX),
        ("-1", u32::MAX),
        ("0x80000000", 0x80000000), // publish-check: allow; 32-bit console literal.
        ("%1011", 0),
        ("\"abcd\"", 0x6463_6261),
        ("\"abcdef", 0x6463_6261),
    ] {
        assert_eq!(cyrus_console::number(input).unwrap(), expected);
    }
    for input in ["", "4294967296", "\"ab", "unknown"] {
        assert!(cyrus_console::number(input).is_err());
    }
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
