//! Game-owned console availability and cheat command handlers.
use crate::script::ScriptActors;
use anyhow::{Context, ensure};
use bevy::prelude::{App, Assets, In, IntoScheduleConfigs, Plugin, PreUpdate, Res, ResMut, State};
use cyrus_assets::script::Scripts;
use cyrus_console::{Command, Console, ConsoleApp, ConsoleInput, Invocation, Source};
use cyrus_world::lifecycle::PreparedWorld;

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
    actors: Option<Res<ScriptActors>>,
    prepared: Option<Res<PreparedWorld>>,
    scripts: Option<Res<Assets<Scripts>>>,
) -> anyhow::Result<Vec<String>> {
    if command.source == Source::Gameplay {
        cheats.execute(&command.name, None).inspect_err(|error| {
            log::error!("cheat {}: {error:#}", command.name);
        })?;
        Ok(Vec::new())
    } else {
        crate::cheat_command::execute(&mut cheats, &command.name, &command.args, |name| {
            let world = actors
                .as_ref()
                .and_then(|actors| actors.current.as_ref())
                .context("console symbol lookup requires a loaded script world")?;
            let definition = prepared
                .as_ref()
                .and_then(|prepared| scripts.as_ref()?.get(&prepared.scripts))
                .context("console symbol lookup requires loaded script definitions")?;
            ensure!(
                definition.world == world.world && definition.content_id == world.content_id,
                "world {}: console symbol definitions do not match the active script world",
                world.world
            );
            ensure!(
                definition.definition.flags.len() == world.flags().len(),
                "world {}: console flag names and values differ in length",
                world.world
            );
            // Preservation's console rules: temporary equates and console variables are empty here.
            Ok(definition
                .definition
                .flags
                .iter()
                .zip(world.flags())
                .rev()
                .find(|(flag, _)| flag.name.eq_ignore_ascii_case(name))
                .map(|(_, value)| *value as u32))
        })
    }
}
