//! Game-owned console availability and cheat command handlers.
mod cheats;
pub mod markers;
use crate::script::{ScriptActors, ScriptWorld};
use anyhow::{Context, ensure};
use bevy::prelude::{
    App, Assets, Commands, DetectChangesMut, In, IntoScheduleConfigs, Local, Plugin, PreUpdate,
    Real, Res, ResMut, Resource, State, Time, Update, World,
};
use cyrus_assets::script::Scripts;
use cyrus_console::{Command, Console, ConsoleApp, ConsoleInput, Invocation, Source};
use cyrus_world::heading::wrap;
use cyrus_world::lifecycle::PreparedWorld;

pub struct GameConsolePlugin;
impl Plugin for GameConsolePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(cyrus_console::ConsolePlugin)
            .add_systems(PreUpdate, availability.before(ConsoleInput))
            .init_resource::<PendingConsoleLoad>()
            .init_resource::<MainLoopDelay>()
            .add_systems(Update, apply_console_load)
            .console_symbols(symbol);
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(bevy::prelude::Last, apply_main_loop_delay);
        app.console_command(
            Command {
                name: "delay".into(),
                alias: Some("dl".into()),
                usage: "delay [number]".into(),
                cheat: false,
            },
            delay,
        );
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
                name: "getmapmarker".into(),
                alias: Some("gmm".into()),
                usage: "getmapmarker number".into(),
                cheat: false,
            },
            get_map_marker,
        );
        app.console_command(
            Command {
                name: "setmarker".into(),
                alias: Some("sm".into()),
                usage: "setmarker number".into(),
                cheat: false,
            },
            set_marker,
        );
        app.console_command(
            Command {
                name: "getmarker".into(),
                alias: Some("gm".into()),
                usage: "getmarker number".into(),
                cheat: false,
            },
            get_marker,
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
/// The `load` request typed into the open console: the original holds it until the console closes,
/// applies it after F12 and discards it after Escape; a later `load` replaces it (preservation
/// `engine/console.md`).
#[derive(Resource, Default)]
struct PendingConsoleLoad(Option<[i32; 3]>);

/// `load world[,marker][,angle]` requests a world load like the `LoadWorld` native: the world index
/// must name a WORLD.INI entry, the marker supplies the start position and the angle (masked to
/// 0 through 2047) the heading. Omitted marker and angle are zero. Symbols are not supported.
/// While the console is open the request waits for it to close; a command run with the console
/// already closed (the control port) applies at once.
fn load(
    In(command): In<Invocation>,
    mut commands: Commands,
    console: Res<Console>,
    mut pending: ResMut<PendingConsoleLoad>,
) -> anyhow::Result<Vec<String>> {
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
    if console.open {
        pending.0 = Some(request);
    } else {
        crate::script::request_world(&mut commands, request)?;
    }
    Ok(vec![format!(
        "load: world {}, marker {}, angle {}",
        request[0],
        request[1],
        wrap(request[2])
    )])
}

/// Applies the held `load` when the console closes with F12 and drops it when Escape closed it.
fn apply_console_load(
    mut console: ResMut<Console>,
    mut pending: ResMut<PendingConsoleLoad>,
    mut commands: Commands,
    mut was_open: Local<bool>,
) {
    let open = console.open;
    if *was_open && !open {
        let escaped = console.take_escape();
        if let Some(request) = pending.0.take()
            && !escaped
            && let Err(error) = crate::script::request_world(&mut commands, request)
        {
            bevy::log::error!("console load: {error:#}");
        }
    }
    *was_open = open;
}
/// The wait `delay` asks each main-loop pass to add, in milliseconds; zero is none (preservation
/// `engine/console.md`).
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct MainLoopDelay(pub u32);

/// The decimal value of a `delay` argument: digits up to the first other byte, so no digits is
/// zero, which clears the delay. A leading minus is refused, since the original would turn the
/// negative number into an enormous wait. Values beyond `u32::MAX` saturate.
#[cfg(not(target_arch = "wasm32"))]
fn delay_milliseconds(arguments: &str) -> anyhow::Result<u32> {
    let arguments = arguments.trim();
    ensure!(
        !arguments.starts_with('-'),
        "delay does not accept a negative number"
    );
    Ok(arguments
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0u32, |value, digit| {
            value
                .saturating_mul(10)
                .saturating_add(u32::from(digit - b'0'))
        }))
}

/// `delay [number]` sets the wait each main-loop pass adds, in milliseconds; omitting the number,
/// zero or a non-number clears it (preservation `engine/console.md`). The wait blocks the loop, so
/// the browser, which cannot block, reports it unavailable.
fn delay(
    In(command): In<Invocation>,
    mut state: ResMut<MainLoopDelay>,
) -> anyhow::Result<Vec<String>> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&command, &mut state);
        anyhow::bail!("delay is unavailable in the browser: its wait would block the page")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let milliseconds = delay_milliseconds(&command.args)?;
        state.0 = milliseconds;
        Ok(vec![if milliseconds == 0 {
            "delay: cleared".to_owned()
        } else {
            format!("delay: {milliseconds} ms per loop")
        }])
    }
}

/// The delay's wait, at the end of each frame.
#[cfg(not(target_arch = "wasm32"))]
fn apply_main_loop_delay(delay: Res<MainLoopDelay>) {
    if delay.0 != 0 {
        std::thread::sleep(std::time::Duration::from_millis(u64::from(delay.0)));
    }
}

/// `getmapmarker number` moves the selected player to the loaded map's own marker of that number,
/// keeping its heading (preservation `engine/console.md`, marker file section). A marker outside
/// the map table or without its presence byte is reported and nothing moves. The original's
/// built-in table, used when no map is loaded, is not implemented.
fn get_map_marker(In(command): In<Invocation>, world: &mut World) -> anyhow::Result<Vec<String>> {
    let marker = marker_number(&command)?;
    let mut actors = world
        .get_resource_mut::<ScriptActors>()
        .context("getmapmarker requires a loaded script world")?;
    actors
        .current
        .as_mut()
        .context("getmapmarker requires a loaded script world")?
        .move_selected_player_to_map_marker(marker)?;
    Ok(vec![format!("getmapmarker: {marker}")])
}

/// The marker number argument of the marker commands: a numeric literal, not a symbol.
fn marker_number(command: &Invocation) -> anyhow::Result<i32> {
    ensure!(!command.args.is_empty(), "usage: {} number", command.name);
    Ok(cyrus_console::number(command.args.trim(), |name| {
        anyhow::bail!(
            "{} accepts numeric literals, not the symbol {name:?}",
            command.name
        )
    })? as i32)
}

/// `setmarker number` saves the selected player's position and orientation and the published
/// camera pose under the current world and number in the host's marker file (preservation
/// `engine/console.md`, marker file section). A same world and number is overwritten in place, and
/// a stored file with a wrong magic or tag is replaced, as the original does. Other malformed
/// files are refused. cyrus keeps the file in host local state, never in the install.
fn set_marker(In(command): In<Invocation>, world: &mut World) -> anyhow::Result<Vec<String>> {
    let number = marker_number(&command)?;
    let (world_number, placement) = {
        let script = world
            .get_resource::<ScriptActors>()
            .and_then(|actors| actors.current.as_ref())
            .context("setmarker requires a loaded script world")?;
        (script.world, script.selected_player_placement()?)
    };
    let storage = world
        .get_resource::<markers::MarkerStorage>()
        .context("setmarker: marker file storage is unavailable on this host")?;
    let mut records = match storage.0.load()? {
        Some(bytes) => {
            markers::decode_for_update(&bytes).context("setmarker: invalid marker file")?
        }
        None => Vec::new(),
    };
    markers::upsert(
        &mut records,
        markers::MarkerRecord {
            world: world_number,
            number,
            placement,
        },
    );
    storage.0.save(&markers::encode(&records))?;
    Ok(vec![format!("setmarker: {number}")])
}

/// `getmarker number` moves the selected player to the first saved marker of the current world and
/// number: its position and orientation, with the previous position following. A missing file,
/// a file that does not decode, or no matching marker reports an error and moves nothing. The saved
/// camera pose is restored at the command boundary.
fn get_marker(In(command): In<Invocation>, world: &mut World) -> anyhow::Result<Vec<String>> {
    let number = marker_number(&command)?;
    let records = {
        let storage = world
            .get_resource::<markers::MarkerStorage>()
            .context("getmarker: marker file storage is unavailable on this host")?;
        let bytes = storage
            .0
            .load()?
            .context("getmarker: no marker file saved")?;
        markers::decode(&bytes).context("getmarker: invalid marker file")?
    };
    let mut actors = world
        .get_resource_mut::<ScriptActors>()
        .context("getmarker requires a loaded script world")?;
    let script = actors
        .current
        .as_mut()
        .context("getmarker requires a loaded script world")?;
    let record = *markers::find(&records, script.world, number).with_context(|| {
        format!(
            "getmarker: no marker {number} saved for world {}",
            script.world
        )
    })?;
    script.restore_selected_player_placement(&record.placement)?;
    Ok(vec![format!("getmarker: {number}")])
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
    state: Res<State<cyrus_game::AppState>>,
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
    let enabled = *state.get() == cyrus_game::AppState::InGame
        && !logbook.is_some_and(|screen| screen.suspended());
    if console.enabled != enabled {
        console.enabled = enabled;
    }
}
fn cheat(
    In(command): In<Invocation>,
    mut cheats: ResMut<cyrus_game::cheats::Cheats>,
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

    #[test]
    fn typed_load_waits_for_the_console_to_close_f12_applies_and_escape_discards() {
        use super::*;
        use bevy::input::keyboard::KeyCode;
        use cyrus_world::lifecycle::SceneRequest;
        let mut world = World::new();
        // One registered system keeps its `Local` open/closed memory between runs.
        let id = world.register_system(apply_console_load);
        let run = move |world: &mut World| world.run_system(id).unwrap();
        world.insert_resource(Console::default());
        world.init_resource::<PendingConsoleLoad>();
        // Typed while open: nothing is requested yet; a later load replaces the first.
        world.resource_mut::<Console>().open = true;
        run(&mut world);
        world.resource_mut::<PendingConsoleLoad>().0 = Some([5, 1, 0]);
        world.resource_mut::<PendingConsoleLoad>().0 = Some([4, 2, 2048 + 7]);
        run(&mut world);
        assert!(!world.contains_resource::<SceneRequest>());
        // F12 closes the console and applies the held request.
        world.resource_mut::<Console>().key(KeyCode::F12, None);
        run(&mut world);
        assert_eq!(world.resource::<SceneRequest>().0, "4");
        assert_eq!(
            world
                .resource::<crate::player::placement::MarkerEntry>()
                .heading,
            7
        );
        // Escape closes it and drops the request.
        world.remove_resource::<SceneRequest>();
        world.resource_mut::<Console>().open = true;
        run(&mut world);
        world.resource_mut::<PendingConsoleLoad>().0 = Some([3, 0, 0]);
        world.resource_mut::<Console>().key(KeyCode::Escape, None);
        run(&mut world);
        assert!(!world.contains_resource::<SceneRequest>());
        assert!(world.resource::<PendingConsoleLoad>().0.is_none());
    }

    #[test]
    fn getmapmarker_needs_a_numeric_marker_and_a_loaded_script_world() {
        use super::*;
        let mut world = World::new();
        let id = world.register_system(get_map_marker);
        let mut run = |args: &str| {
            world
                .run_system_with(
                    id,
                    Invocation {
                        name: "getmapmarker".into(),
                        args: args.into(),
                        source: Source::Interactive,
                    },
                )
                .unwrap()
                .unwrap_err()
                .to_string()
        };
        assert!(run("").contains("usage: getmapmarker number"));
        assert!(run("flag").contains("numeric literals"));
        assert!(run("3").contains("loaded script world"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn delay_reads_decimal_digits_and_clears_on_none_zero_or_a_non_number() {
        for (arguments, expected) in [
            ("", 0),
            ("0", 0),
            ("250", 250),
            (" 40 ", 40),
            ("12abc", 12),
            ("abc", 0),
            ("0x10", 0),
            ("4294967296", u32::MAX),
        ] {
            assert_eq!(
                super::delay_milliseconds(arguments).unwrap(),
                expected,
                "{arguments:?}"
            );
        }
        let error = super::delay_milliseconds("-5").unwrap_err().to_string();
        assert!(error.contains("negative"), "{error}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_delay_command_sets_replaces_and_clears_the_main_loop_wait() {
        use super::*;
        let mut world = World::new();
        world.init_resource::<MainLoopDelay>();
        let id = world.register_system(delay);
        let run = |world: &mut World, args: &str| {
            world
                .run_system_with(
                    id,
                    Invocation {
                        name: "delay".into(),
                        args: args.into(),
                        source: Source::Interactive,
                    },
                )
                .unwrap()
        };
        assert_eq!(run(&mut world, "250").unwrap(), ["delay: 250 ms per loop"]);
        assert_eq!(world.resource::<MainLoopDelay>().0, 250);
        run(&mut world, "40").unwrap();
        assert_eq!(world.resource::<MainLoopDelay>().0, 40);
        assert!(run(&mut world, "-1").is_err());
        assert_eq!(
            world.resource::<MainLoopDelay>().0,
            40,
            "a refusal keeps the delay"
        );
        assert_eq!(run(&mut world, "").unwrap(), ["delay: cleared"]);
        assert_eq!(world.resource::<MainLoopDelay>().0, 0);
    }

    #[test]
    fn marker_commands_report_missing_storage_files_and_script_worlds() {
        use super::markers::{MarkerStorage, MarkerStore};
        use super::*;
        struct Empty;
        impl MarkerStore for Empty {
            fn load(&self) -> anyhow::Result<Option<Vec<u8>>> {
                Ok(None)
            }
            fn save(&self, _: &[u8]) -> anyhow::Result<()> {
                anyhow::bail!("must not be written")
            }
        }
        let mut world = World::new();
        let set = world.register_system(set_marker);
        let get = world.register_system(get_marker);
        let run = |world: &mut World,
                   id: bevy::ecs::system::SystemId<In<Invocation>, anyhow::Result<Vec<String>>>,
                   name: &str,
                   args: &str| {
            world
                .run_system_with(
                    id,
                    Invocation {
                        name: name.into(),
                        args: args.into(),
                        source: Source::Interactive,
                    },
                )
                .unwrap()
                .unwrap_err()
                .to_string()
        };
        assert!(run(&mut world, set, "setmarker", "").contains("usage: setmarker number"));
        assert!(run(&mut world, get, "getmarker", "name").contains("numeric literals"));
        assert!(run(&mut world, set, "setmarker", "1").contains("loaded script world"));
        assert!(run(&mut world, get, "getmarker", "1").contains("storage is unavailable"));
        world.insert_resource(MarkerStorage(Box::new(Empty)));
        assert!(run(&mut world, get, "getmarker", "1").contains("no marker file saved"));
    }
}
