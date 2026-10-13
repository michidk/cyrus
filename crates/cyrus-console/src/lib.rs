//! Console editing and command dispatch independent of gameplay, rendering and storage.
mod editor;
mod input;
mod number;

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

/// Register handlers as ordinary Bevy systems; ownership remains with their plugins.
pub trait ConsoleApp {
    fn console_command<M>(
        &mut self,
        command: Command,
        handler: impl IntoSystem<In<Invocation>, Result<Vec<String>>, M> + 'static,
    ) -> &mut Self;
}
impl ConsoleApp for App {
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
                    usage: "console clear".into(),
                    cheat: false,
                },
                options,
            );
    }
}

fn list(_: In<Invocation>, registry: Res<Registry>) -> Result<Vec<String>> {
    Ok(registry
        .0
        .iter()
        .filter(|entry| !entry.command.cheat)
        .map(|entry| {
            let command = &entry.command;
            format!(
                "{} [{}]  {}",
                command.name,
                command.alias.as_deref().unwrap_or(""),
                command.usage
            )
        })
        .collect())
}
fn options(In(command): In<Invocation>, mut console: ResMut<Console>) -> Result<Vec<String>> {
    match command.args.as_str() {
        "clear" => {
            console.clear();
            Ok(Vec::new())
        }
        _ => bail!("usage: console clear"),
    }
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
    while let Some((line, source)) = world.resource_mut::<Console>().pending.pop_front() {
        let _ = submit(world, &line, source);
    }
}
