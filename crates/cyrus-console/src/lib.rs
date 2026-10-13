//! Console editing and command dispatch independent of gameplay, rendering and storage.
mod catalog;
mod editor;
mod input;
mod number;
mod preferences;
pub use preferences::{Preferences, Store};

use anyhow::{Result, bail};
use bevy::{ecs::system::SystemId, input::InputSystems, prelude::*};
pub use editor::{Console, Line};
pub use number::number;

/// Hidden gameplay entry accepts only registered cheats and produces no console output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Interactive,
    Gameplay,
    Programmatic,
}

/// Normalized command name and handler-specific arguments. Semicolons end input.
#[derive(Clone, Debug)]
pub struct Invocation {
    pub name: String,
    pub args: String,
    pub source: Source,
}

/// One canonical name and, optionally, one exact interactive alias.
#[derive(Clone)]
pub struct Command {
    pub name: String,
    pub alias: Option<String>,
    pub usage: String,
    pub cheat: bool,
}
struct Registered {
    command: Command,
    handler: SystemId<In<Invocation>, Result<Vec<String>>>,
}
#[derive(Resource, Default)]
struct Registry(Vec<Registered>);

#[derive(Resource)]
struct Symbols(SystemId<In<String>, Result<Option<u32>>>);

/// Register handlers as ordinary Bevy systems; ownership remains with their plugins.
pub trait ConsoleApp {
    /// Install the game-owned live-symbol lookup used by all numeric commands.
    fn console_symbols<M>(
        &mut self,
        handler: impl IntoSystem<In<String>, Result<Option<u32>>, M> + 'static,
    ) -> &mut Self;
    fn console_command<M>(
        &mut self,
        command: Command,
        handler: impl IntoSystem<In<Invocation>, Result<Vec<String>>, M> + 'static,
    ) -> &mut Self;
}
impl ConsoleApp for App {
    fn console_symbols<M>(
        &mut self,
        handler: impl IntoSystem<In<String>, Result<Option<u32>>, M> + 'static,
    ) -> &mut Self {
        let handler = self.world_mut().register_system(handler);
        self.insert_resource(Symbols(handler))
    }
    fn console_command<M>(
        &mut self,
        command: Command,
        handler: impl IntoSystem<In<Invocation>, Result<Vec<String>>, M> + 'static,
    ) -> &mut Self {
        self.init_resource::<Registry>();
        let existing = self.world().resource::<Registry>();
        for name in std::iter::once(command.name.as_str()).chain(command.alias.as_deref()) {
            assert!(
                !name.is_empty() && name == name.to_ascii_lowercase(),
                "console names must be lowercase and nonempty"
            );
            assert!(
                existing.0.iter().all(|entry| entry.command.name != name
                    && entry.command.alias.as_deref() != Some(name)),
                "duplicate console command {name}"
            );
        }
        let handler = self.world_mut().register_system(handler);
        self.world_mut()
            .resource_mut::<Registry>()
            .0
            .push(Registered { command, handler });
        self
    }
}

/// Read a number using the active host's live symbols; unknown names diagnose and yield one.
pub fn value(world: &mut World, token: &str, diagnostics: &mut Vec<String>) -> Result<u32> {
    number(token, |name| {
        let handler = world
            .get_resource::<Symbols>()
            .ok_or_else(|| anyhow::anyhow!("console symbol lookup is unavailable"))?
            .0;
        match world
            .run_system_with(handler, name.to_owned())
            .map_err(|error| anyhow::anyhow!("console symbol lookup: {error}"))??
        {
            Some(value) => Ok(value),
            None => {
                diagnostics.push(format!("unknown console symbol {name:?}; using 1"));
                Ok(1)
            }
        }
    })
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConsoleInput;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConsoleDispatch;

pub struct ConsolePlugin;
impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Console>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .configure_sets(
                PreUpdate,
                (ConsoleInput, ConsoleDispatch).chain().after(InputSystems),
            )
            .add_systems(PreUpdate, input::capture.in_set(ConsoleInput))
            .add_systems(PreUpdate, dispatch.in_set(ConsoleDispatch))
            .console_command(
                Command {
                    name: "commands".into(),
                    alias: Some("?".into()),
                    usage: "commands".into(),
                    cheat: false,
                },
                list,
            )
            .console_command(
                Command {
                    name: "console".into(),
                    alias: Some("con".into()),
                    usage: "console clear / write / save / show / font / shadow / paper / col0..3 / hex,number / limit,number".into(),
                    cheat: false,
                },
                preferences::command,
            );
    }
}

fn list(_: In<Invocation>, registry: Res<Registry>) -> Result<Vec<String>> {
    let mut lines = catalog::ORIGINAL
        .iter()
        .map(|(name, alias)| {
            if let Some(entry) = registry.0.iter().find(|entry| entry.command.name == *name) {
                format!("{name} [{alias}]  {}", entry.command.usage)
            } else {
                format!("{name} [{alias}]  unsupported")
            }
        })
        .collect::<Vec<_>>();
    lines.extend(
        registry
            .0
            .iter()
            .filter(|entry| {
                !entry.command.cheat
                    && !catalog::ORIGINAL
                        .iter()
                        .any(|(name, _)| *name == entry.command.name)
            })
            .map(|entry| {
                format!(
                    "{} [{}]  {}",
                    entry.command.name,
                    entry.command.alias.as_deref().unwrap_or(""),
                    entry.command.usage
                )
            }),
    );
    Ok(lines)
}
/// Execute a submission immediately, retaining interactive input and diagnostics.
/// Programmatic input accepts canonical names only. Hidden input accepts exact cheat names only.
pub fn submit(world: &mut World, line: &str, source: Source) -> Result<Vec<String>> {
    let limit = if source == Source::Gameplay { 127 } else { 100 };
    if !line.is_ascii() || line.len() > limit {
        let error = anyhow::anyhow!("console input must contain at most {limit} ASCII characters");
        if source != Source::Gameplay {
            world.resource_mut::<Console>().report(error.to_string());
        }
        return Err(error);
    }
    if source != Source::Gameplay {
        world.resource_mut::<Console>().enter(line);
    }
    let normalized = if source == Source::Gameplay {
        line.to_ascii_lowercase()
    } else {
        line.split([';', '\n', '\r'])
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
    };
    let result = (|| {
        if normalized.is_empty() {
            return Ok(Vec::new());
        }
        let end = normalized
            .find(|ch: char| ch.is_ascii_whitespace() || ch == '=')
            .unwrap_or(normalized.len());
        let name = &normalized[..end];
        let entry = world.resource::<Registry>().0.iter().find(|entry| {
            (entry.command.name == name
                || (source == Source::Interactive && entry.command.alias.as_deref() == Some(name)))
                && (source != Source::Gameplay || (entry.command.cheat && name == normalized))
        });
        let Some(entry) = entry else {
            if source == Source::Gameplay {
                return Ok(Vec::new());
            }
            if let Some((canonical, _)) = catalog::ORIGINAL.iter().find(|(canonical, alias)| {
                *canonical == name || (source == Source::Interactive && *alias == name)
            }) {
                bail!(
                    "console command {canonical} is unsupported: its owning subsystem is not implemented"
                );
            }
            bail!("unknown console command {name:?}");
        };
        let handler = entry.handler;
        let invocation = Invocation {
            name: entry.command.name.clone(),
            args: normalized[end..].trim().into(),
            source,
        };
        world
            .run_system_with(handler, invocation)
            .map_err(|error| anyhow::anyhow!("console {name}: {error}"))?
    })();
    if source != Source::Gameplay {
        let lines = match &result {
            Ok(lines) => lines.clone(),
            Err(error) => vec![format!("{error:#}")],
        };
        world.resource_mut::<Console>().finish(lines);
    }
    result
}
fn dispatch(world: &mut World) {
    while let Some((line, source)) = world
        .resource_mut::<Console>()
        .bypass_change_detection()
        .pending
        .pop_front()
    {
        let _ = submit(world, &line, source);
    }
}
