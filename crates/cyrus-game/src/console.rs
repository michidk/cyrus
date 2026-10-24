//! Game-owned console availability and cheat command handlers.
mod cheats;
use crate::script::{ScriptActors, ScriptWorld};
use anyhow::{Context, ensure};
use bevy::prelude::{
    App, Assets, Commands, DetectChangesMut, In, IntoScheduleConfigs, Plugin, PreUpdate, Real, Res,
    ResMut, State, Time, World,
};
use cyrus_assets::script::Scripts;
use cyrus_console::{Command, Console, ConsoleApp, ConsoleInput, Invocation, Source};
use cyrus_world::lifecycle::PreparedWorld;

pub struct GameConsolePlugin;
impl Plugin for GameConsolePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(cyrus_console::ConsolePlugin)
            .add_systems(PreUpdate, availability.before(ConsoleInput))
            .console_symbols(symbol);
        for (name, alias, usage) in [
            ("get", "g", "get pattern"),
            ("set", "s", "set pattern,value[,value...]"),
        ] {
            app.console_command(
                Command {
                    name: name.into(),
                    alias: Some(alias.into()),
                    usage: usage.into(),
                    cheat: false,
                },
                variables,
            );
        }
        app.console_command(
            Command {
                name: "load".into(),
                alias: Some("l".into()),
                usage: "load world[,marker][,angle]".into(),
                cheat: false,
            },
            load,
        );
        app.console_command(
            Command {
                name: "item".into(),
                alias: Some("i".into()),
                usage: "item add,id".into(),
                cheat: false,
            },
            item,
        );
        for (name, alias) in [("getflag", "gf"), ("setflag", "sf")] {
            app.console_command(
                Command {
                    name: name.into(),
                    alias: Some(alias.into()),
                    usage: if name == "getflag" {
                        "getflag pattern"
                    } else {
                        "setflag pattern,value[,nonzero status]"
                    }
                    .into(),
                    cheat: false,
                },
                flags,
            );
        }
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
/// `load world[,marker][,angle]` requests a world load like the `LoadWorld` native: the world index
/// must name a WORLD.INI entry, the marker supplies the start position and the angle (masked to
/// 0 through 2047) the heading. Omitted marker and angle are zero. Symbols are not supported.
fn load(In(command): In<Invocation>, mut commands: Commands) -> anyhow::Result<Vec<String>> {
    let arguments: Vec<&str> = command.args.split(',').map(str::trim).collect();
    ensure!(
        !command.args.is_empty() && arguments.len() <= 3,
        "usage: load world[,marker][,angle]"
    );
    let mut request = [0i32; 3];
    for (slot, value) in arguments.into_iter().enumerate() {
        request[slot] = cyrus_console::number(value, |name| {
            anyhow::bail!("load accepts numeric literals, not the symbol {name:?}")
        })? as i32;
    }
    crate::script::request_world(&mut commands, request)?;
    Ok(vec![format!(
        "load: world {}, marker {}, angle {}",
        request[0],
        request[1],
        request[2] & 2047
    )])
}
/// `item add,id` gives the selected player one item through the session inventory. The other
/// original operations (`player`, `show`, `sub`, `drop`) are not implemented.
fn item(In(command): In<Invocation>, world: &mut World) -> anyhow::Result<Vec<String>> {
    let (operation, argument) = command
        .args
        .split_once(',')
        .map_or((command.args.as_str(), ""), |(a, b)| (a.trim(), b.trim()));
    ensure!(
        operation == "add",
        "item {operation:?} is unsupported: only item add,id is implemented"
    );
    let id = cyrus_console::number(argument, |name| {
        anyhow::bail!("item add accepts numeric literals, not the symbol {name:?}")
    })?;
    let id = u16::try_from(id).context("item add: item ID out of range")?;
    world.resource_scope(|world, mut actors: bevy::prelude::Mut<ScriptActors>| {
        let hud = world
            .get_resource::<crate::script::ScriptHud>()
            .context("item add: inventory unavailable")?;
        actors
            .current
            .as_mut()
            .context("item add requires a loaded script world")?
            .add_player_item(hud, id)
    })?;
    Ok(vec![format!("item add: {id}")])
}
fn availability(
    state: Res<State<crate::AppState>>,
    time: Option<Res<Time<Real>>>,
    mut console: ResMut<Console>,
    logbook: Option<Res<crate::logbook::LogbookScreen>>,
) {
    if let Some(time) = time {
        let visible = console.cursor_visible();
        console
            .bypass_change_detection()
            .tick(crate::clock::bios_tick(time.elapsed()));
        if console.cursor_visible() != visible {
            console.set_changed();
        }
    }
    let enabled = *state.get() == crate::AppState::InGame
        && !logbook.is_some_and(|screen| screen.suspended());
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
        cheats::execute(&mut cheats, &command.name, &command.args, |name| {
            lookup(
                name,
                actors.as_deref(),
                prepared.as_deref(),
                scripts.as_deref(),
            )
        })
    }
}

fn symbol(
    In(name): In<String>,
    actors: Option<Res<ScriptActors>>,
    prepared: Option<Res<PreparedWorld>>,
    scripts: Option<Res<Assets<Scripts>>>,
) -> anyhow::Result<Option<u32>> {
    lookup(
        &name,
        actors.as_deref(),
        prepared.as_deref(),
        scripts.as_deref(),
    )
}
fn lookup(
    name: &str,
    actors: Option<&ScriptActors>,
    prepared: Option<&PreparedWorld>,
    scripts: Option<&Assets<Scripts>>,
) -> anyhow::Result<Option<u32>> {
    let world = actors
        .and_then(|actors| actors.current.as_ref())
        .context("console symbol lookup requires a loaded script world")?;
    let definition = definition(world, prepared, scripts)?;
    // Preservation's console rules: temporary equates and console variables are empty here.
    Ok(definition
        .definition
        .flags
        .iter()
        .zip(world.flags())
        .rev()
        .find(|(flag, _)| flag.name.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value as u32))
}

fn variables(In(command): In<Invocation>) -> anyhow::Result<Vec<String>> {
    let pattern = command.args.split(',').next().unwrap_or_default().trim();
    ensure!(
        !pattern.is_empty(),
        "{} requires a variable pattern",
        command.name
    );
    if command.name == "set" {
        ensure!(
            command
                .args
                .split_once(',')
                .is_some_and(|(_, value)| !value.trim().is_empty()),
            "usage: set pattern,value[,value...]"
        );
    }
    Ok(vec![format!("No console variables match {pattern:?}.")])
}

fn definition<'a>(
    world: &ScriptWorld,
    prepared: Option<&PreparedWorld>,
    scripts: Option<&'a Assets<Scripts>>,
) -> anyhow::Result<&'a Scripts> {
    let definition = prepared
        .and_then(|prepared| scripts?.get(&prepared.scripts))
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
    Ok(definition)
}

fn flag_matches(name: &str, pattern: &str) -> bool {
    if pattern.is_empty() || pattern.starts_with('*') {
        return true;
    }
    if name.is_empty() {
        return false;
    }
    let mut pattern = pattern.bytes();
    for byte in name.bytes() {
        match pattern.next() {
            Some(b'*') => return true,
            Some(b'?') => {}
            Some(expected) if byte.eq_ignore_ascii_case(&expected) => {}
            _ => return false,
        }
    }
    matches!(pattern.find(|byte| *byte != b'?'), None | Some(b'*'))
}

fn flags(In(command): In<Invocation>, world: &mut World) -> anyhow::Result<Vec<String>> {
    let parts: Vec<_> = command.args.split(',').map(str::trim).collect();
    ensure!(
        !parts[0].is_empty(),
        "{} requires a flag pattern",
        command.name
    );
    let mut output = Vec::new();
    let assignment = if command.name == "setflag" {
        ensure!(
            (2..=3).contains(&parts.len()),
            "usage: setflag pattern,value[,status]"
        );
        let value = cyrus_console::value(world, parts[1], &mut output)? as i32;
        let mark = if parts.len() == 3 {
            let status = cyrus_console::value(world, parts[2], &mut output)?;
            ensure!(
                status != 0,
                "setflag status zero is unsupported: the original XOR changes flag type metadata; no values changed"
            );
            true
        } else {
            false
        };
        Some((value, mark))
    } else {
        ensure!(parts.len() == 1, "usage: getflag pattern");
        None
    };
    let actors = world
        .get_resource::<ScriptActors>()
        .and_then(|actors| actors.current.as_ref())
        .context("console flag commands require a loaded script world")?;
    let definition = definition(
        actors,
        world.get_resource::<PreparedWorld>(),
        world.get_resource::<Assets<Scripts>>(),
    )?;
    let matches: Vec<_> = definition
        .definition
        .flags
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, flag)| flag_matches(&flag.name, parts[0]))
        .map(|(index, flag)| (index, flag.name.clone(), flag.flag_type.clone()))
        .collect();
    let mut actors = world.resource_mut::<ScriptActors>();
    let actors = actors
        .current
        .as_mut()
        .expect("validated current script world");
    for (index, name, kind) in &matches {
        if let Some((value, mark)) = assignment {
            actors.assign_global_flag(*index, value, mark)?;
        }
        output.push(format!(
            "{name} {kind} {} set={}",
            actors.flags()[*index],
            actors.global_flag_is_set(*index)?
        ));
    }
    if matches.is_empty() {
        output.push(format!("No SOUP flags match {:?}.", parts[0]));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    #[test]
    fn original_name_patterns_stop_at_star_and_allow_trailing_question_marks() {
        for (name, pattern, expected) in [
            ("Counter", "counter", true),
            ("Counter", "count", false),
            ("Counter", "count*ignored", true),
            ("Counter", "c?unter??", true),
            ("Counter", "*suffix", true),
            ("Counter", "counte?x", false),
            ("", "?", false),
            ("Counter", "", true),
        ] {
            assert_eq!(super::flag_matches(name, pattern), expected);
        }
    }
}
