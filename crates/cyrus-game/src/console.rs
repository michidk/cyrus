//! Game-owned console availability and cheat command handlers.
use bevy::prelude::*;
use cyrus_console::{Command, Console, ConsoleApp, ConsoleInput, Invocation, Source};

pub struct GameConsolePlugin;
impl Plugin for GameConsolePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(cyrus_console::ConsolePlugin)
            .add_systems(PreUpdate, availability.before(ConsoleInput));
        // Handler names follow preservation's engine/cheats.md; no CHT parser in the game core.
        for name in [
            "oracle",
            "nodemarker",
            "moonraker",
            "task",
            "animation",
            "magiccarpet",
            "savecheats",
            "drevil",
            "drno",
            "goldfinger",
            "neversaydie",
            "oddjob",
            "yeahbaby",
        ] {
            app.console_command(
                Command {
                    name: name.into(),
                    alias: None,
                    usage: "name [= value]".into(),
                    cheat: true,
                },
                cheat,
            );
        }
    }
}
fn availability(state: Res<State<crate::AppState>>, mut console: ResMut<Console>) {
    let enabled = *state.get() == crate::AppState::InGame;
    if console.enabled != enabled {
        console.enabled = enabled;
    }
}
fn cheat(
    In(command): In<Invocation>,
    mut cheats: ResMut<crate::cheats::Cheats>,
) -> anyhow::Result<Vec<String>> {
    if command.source == Source::Gameplay {
        cheats.execute(&command.name, None).inspect_err(|error| {
            log::error!("cheat {}: {error:#}", command.name);
        })?;
        Ok(Vec::new())
    } else {
        crate::cheat_command::execute(&mut cheats, &command.name, &command.args)
    }
}
