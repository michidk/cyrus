//! Original sky-plane and wave console controls, owned by world presentation.
use crate::environment::{Environment, EnvironmentRequest};
use anyhow::{Context, Result};
use bevy::prelude::{
    App, AssetId, Assets, DetectChanges, In, Local, MeshMaterial3d, Query, Res, ResMut, Resource,
    With, World,
};
use cyrus_console::{Command, ConsoleApp, Invocation};
use cyrus_world::lifecycle::CurrentScene;
use std::collections::BTreeMap;

#[derive(Resource, Default)]
pub(crate) struct Edits(BTreeMap<u32, WorldEdits>);

#[derive(Default)]
struct WorldEdits {
    scalars: [Option<f32>; 5],
    sky_speed: Option<u8>,
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<Edits>();
    app.console_command(
        Command {
            name: "show".into(),
            alias: Some("sh".into()),
            usage: "show world (other topics unsupported)".into(),
            cheat: false,
        },
        show,
    );
    app.console_command(
        Command {
            name: "fxskyspeed".into(),
            alias: Some("skysp".into()),
            usage: "fxskyspeed value".into(),
            cheat: false,
        },
        speed,
    );
    for (name, alias) in [
        ("fxskyscale", "skysc"),
        ("fxskylevel", "skyl"),
        ("fxwaveamp", "wamp"),
        ("fxwavespeed", "wsp"),
        ("fxwavefreq", "wfrq"),
    ] {
        app.console_command(
            Command {
                name: name.into(),
                alias: Some(alias.into()),
                usage: format!("{name} value"),
                cheat: false,
            },
            edit,
        );
    }
}

fn show(
    In(command): In<Invocation>,
    scene: Res<CurrentScene>,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
) -> Result<Vec<String>> {
    anyhow::ensure!(
        command.args.trim() == "world",
        "show {:?} is unsupported; use show world",
        command.args
    );
    let id = scene.world.context("show world requires a loaded world")?;
    let environment = environments
        .get(&request.0)
        .context("world environment is still loading")?;
    let mut lines = vec![
        format!("World {id}: {}", scene.name),
        format!("fog RGB {:?}", environment.background),
    ];
    if let Some(sky) = &environment.sky {
        lines.push(format!(
            "sky scale={} level={} speed={}",
            sky.scale, sky.level, sky.speed
        ));
    }
    if let Some(water) = &environment.water {
        let [amplitude, speed, frequency] = water.parameters;
        lines.push(format!(
            "waves amplitude={amplitude} speed={speed} frequency={frequency}"
        ));
    }
    Ok(lines)
}

fn slot(environment: &mut Environment, index: usize) -> Result<&mut f32> {
    if index < 2 {
        let sky = environment
            .sky
            .as_mut()
            .context("current world has no sky")?;
        Ok(if index == 0 {
            &mut sky.scale
        } else {
            &mut sky.level
        })
    } else {
        Ok(&mut environment
            .water
            .as_mut()
            .context("current world has no waves")?
            .parameters[index - 2])
    }
}

fn edit(In(command): In<Invocation>, world: &mut World) -> Result<Vec<String>> {
    let index = match command.name.as_str() {
        "fxskyscale" => 0,
        "fxskylevel" => 1,
        "fxwaveamp" => 2,
        "fxwavespeed" => 3,
        "fxwavefreq" => 4,
        _ => unreachable!("only registered environment commands reach this handler"),
    };
    let mut output = Vec::new();
    let value = cyrus_console::value(world, command.args.trim(), &mut output)? as i32 as f32;
    let id = world
        .get_resource::<CurrentScene>()
        .and_then(|scene| scene.world)
        .context("console environment requires a loaded world")?;
    let handle = world
        .get_resource::<EnvironmentRequest>()
        .context("console environment is not prepared")?
        .0
        .clone();
    {
        let mut environments = world.resource_mut::<Assets<Environment>>();
        let mut environment = environments
            .get_mut(&handle)
            .context("console environment is still loading")?;
        *slot(&mut environment, index)? = value;
    }
    world
        .resource_mut::<Edits>()
        .0
        .entry(id)
        .or_default()
        .scalars[index] = Some(value);
    if index >= 2 {
        world
            .resource_mut::<crate::water::WaterPlayback>()
            .invalidate();
    }
    output.push(format!("{} = {value}", command.name));
    Ok(output)
}

fn speed(In(command): In<Invocation>, world: &mut World) -> Result<Vec<String>> {
    let mut output = Vec::new();
    let value = cyrus_console::value(world, command.args.trim(), &mut output)? as i32;
    let id = world
        .get_resource::<CurrentScene>()
        .and_then(|scene| scene.world)
        .context("fxskyspeed requires a loaded world")?;
    let handle = world
        .get_resource::<EnvironmentRequest>()
        .context("fxskyspeed environment is not prepared")?
        .0
        .clone();
    anyhow::ensure!(
        world
            .resource::<Assets<Environment>>()
            .get(&handle)
            .and_then(|environment| environment.sky.as_ref())
            .is_some(),
        "world {id}: fxskyspeed requires a loaded sky"
    );
    let mut skies = world.query::<&mut crate::sky::Sky>();
    anyhow::ensure!(
        skies.iter(world).count() == 1,
        "world {id}: sky is still loading"
    );
    let direction = (world
        .get_resource_mut::<cyrus_game::random::EngineRandom>()
        .context("fxskyspeed requires the shared engine random stream")?
        .draw()
        & 2047) as u16;
    skies.single_mut(world)?.set_speed(direction, value);
    world
        .resource_mut::<Edits>()
        .0
        .entry(id)
        .or_default()
        .sky_speed = Some(value as u8);
    world
        .resource_mut::<Assets<Environment>>()
        .get_mut(&handle)
        .unwrap()
        .sky
        .as_mut()
        .unwrap()
        .speed = value as u8;
    output.push(format!("fxskyspeed = {value}"));
    Ok(output)
}

/// Restore scalar edits after revisiting a world and refresh already-created sky material.
// Asset readiness, retained edits and live sky materials have separate Bevy owners.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply(
    scene: Option<Res<CurrentScene>>,
    request: Res<EnvironmentRequest>,
    edits: Res<Edits>,
    mut environments: ResMut<Assets<Environment>>,
    mut water: ResMut<crate::water::WaterPlayback>,
    skies: Query<&MeshMaterial3d<crate::sky::SkyMaterial>, With<crate::sky::Sky>>,
    mut materials: ResMut<Assets<crate::sky::SkyMaterial>>,
    mut applied: Local<Option<(AssetId<Environment>, u32)>>,
) {
    let Some(id) = scene.as_ref().and_then(|scene| scene.world) else {
        *applied = None;
        return;
    };
    let Some(values) = edits.0.get(&id) else {
        *applied = None;
        return;
    };
    let key = (request.0.id(), id);
    if *applied == Some(key) && !edits.is_changed() {
        return;
    }
    let Some(mut environment) = environments.get_mut(&request.0) else {
        return;
    };
    *applied = Some(key);
    for (index, value) in values.scalars.iter().enumerate() {
        if let Some(value) = value
            && let Ok(slot) = slot(&mut environment, index)
            && *slot != *value
        {
            *slot = *value;
            if index >= 2 {
                water.invalidate();
            }
        }
    }
    if let Some(value) = values.sky_speed
        && let Some(sky) = &mut environment.sky
    {
        sky.speed = value;
    }
    if let Some(sky) = &environment.sky {
        for handle in &skies {
            if let Some(mut material) = materials.get_mut(&handle.0) {
                material.plane.x = sky.level;
                material.plane.y = sky.scale;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{Update, default};

    fn environment() -> Environment {
        Environment {
            sky: Some(crate::environment::SkyImage {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
                level: -3500.0,
                scale: 2000.0,
                speed: 1,
                sun: None,
            }),
            background: [0; 3],
            clipping: [1.0, 10000.0],
            fog_table: [[0; 4]; 16],
            water: Some(crate::environment::Water {
                parameters: [24.0, 1.0, 2.0],
                side: 1,
                animated_vertices: vec![true],
            }),
            terrain_lighting: None,
            model_lighting: default(),
            prelighting: default(),
            object_lights: Vec::new(),
        }
    }

    #[test]
    fn speed_commands_consume_one_draw_and_retain_only_the_world_byte() {
        use cyrus_game::random::EngineRandom;
        let mut app = App::new();
        app.add_plugins(cyrus_console::ConsolePlugin)
            .init_resource::<EngineRandom>()
            .init_resource::<Assets<Environment>>()
            .init_resource::<Assets<crate::sky::SkyMaterial>>()
            .init_resource::<crate::water::WaterPlayback>()
            .insert_resource(CurrentScene {
                name: "test".into(),
                world: Some(7),
            });
        register(&mut app);
        app.add_systems(Update, apply);
        let first = app
            .world_mut()
            .resource_mut::<Assets<Environment>>()
            .add(environment());
        app.insert_resource(EnvironmentRequest(first.clone()));
        let submit = |app: &mut App, line| {
            cyrus_console::submit(app.world_mut(), line, cyrus_console::Source::Interactive)
        };
        assert!(submit(&mut app, "skysp 0").is_err());
        assert_eq!(app.world().resource::<EngineRandom>().state(), 1);
        app.world_mut().spawn(crate::sky::Sky::new(1));
        assert!(submit(&mut app, "skysp").is_err());
        assert_eq!(app.world().resource::<EngineRandom>().state(), 1);
        submit(&mut app, "skysp -257").unwrap();
        assert_eq!(app.world().resource::<EngineRandom>().state(), 1103527590);
        assert_eq!(
            app.world()
                .resource::<Assets<Environment>>()
                .get(&first)
                .unwrap()
                .sky
                .as_ref()
                .unwrap()
                .speed,
            255
        );
        let second = app
            .world_mut()
            .resource_mut::<Assets<Environment>>()
            .add(environment());
        app.insert_resource(EnvironmentRequest(second.clone()));
        app.world_mut().resource_mut::<CurrentScene>().world = Some(8);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<Environment>>()
                .get(&second)
                .unwrap()
                .sky
                .as_ref()
                .unwrap()
                .speed,
            1
        );
        app.world_mut().resource_mut::<CurrentScene>().world = Some(7);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<Environment>>()
                .get(&second)
                .unwrap()
                .sky
                .as_ref()
                .unwrap()
                .speed,
            255
        );
        submit(&mut app, "fxskyspeed 0").unwrap();
        assert_eq!(app.world().resource::<EngineRandom>().state(), 2524885223);
        assert_eq!(
            app.world()
                .resource::<Assets<Environment>>()
                .get(&second)
                .unwrap()
                .sky
                .as_ref()
                .unwrap()
                .speed,
            0
        );
    }

    #[test]
    fn commands_mutate_current_inputs_and_restore_only_the_edited_world_on_revisit() {
        let mut app = App::new();
        app.add_plugins(cyrus_console::ConsolePlugin)
            .init_resource::<Assets<Environment>>()
            .init_resource::<Assets<crate::sky::SkyMaterial>>()
            .init_resource::<crate::water::WaterPlayback>()
            .insert_resource(CurrentScene {
                name: "test".into(),
                world: Some(7),
            });
        register(&mut app);
        app.add_systems(Update, apply);
        let first = app
            .world_mut()
            .resource_mut::<Assets<Environment>>()
            .add(environment());
        app.insert_resource(EnvironmentRequest(first.clone()));
        for command in ["skysc -800", "skyl -4500", "wamp 0", "wsp -2", "wfrq 512"] {
            cyrus_console::submit(app.world_mut(), command, cyrus_console::Source::Interactive)
                .unwrap();
        }
        let assets = app.world().resource::<Assets<Environment>>();
        let sky = assets.get(&first).unwrap().sky.as_ref().unwrap();
        assert_eq!((sky.scale, sky.level), (-800.0, -4500.0));
        assert_eq!(
            assets
                .get(&first)
                .unwrap()
                .water
                .as_ref()
                .unwrap()
                .parameters,
            [0.0, -2.0, 512.0]
        );
        assert!(
            cyrus_console::submit(
                app.world_mut(),
                "fxwaveamp",
                cyrus_console::Source::Interactive
            )
            .is_err()
        );
        let second = app
            .world_mut()
            .resource_mut::<Assets<Environment>>()
            .add(environment());
        app.insert_resource(EnvironmentRequest(second.clone()));
        app.world_mut().resource_mut::<CurrentScene>().world = Some(8);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<Environment>>()
                .get(&second)
                .unwrap()
                .sky
                .as_ref()
                .unwrap()
                .scale,
            2000.0
        );
        app.world_mut().resource_mut::<CurrentScene>().world = Some(7);
        app.update();
        let assets = app.world().resource::<Assets<Environment>>();
        assert_eq!(
            assets.get(&second).unwrap().sky.as_ref().unwrap().scale,
            -800.0
        );
        assert_eq!(
            assets
                .get(&second)
                .unwrap()
                .water
                .as_ref()
                .unwrap()
                .parameters,
            [0.0, -2.0, 512.0]
        );
    }
}
