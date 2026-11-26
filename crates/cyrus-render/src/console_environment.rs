//! Original sky-plane and wave console controls, owned by world presentation.
use crate::environment::{Environment, EnvironmentRequest};
use anyhow::{Context, Result};
use bevy::prelude::{
    App, AssetId, Assets, DetectChanges, In, Local, MeshMaterial3d, Query, Res, ResMut, Resource,
    With, World,
};
use cyrus_console::{Command, ConsoleApp, Invocation};
use cyrus_world::heading::wrap;
use cyrus_world::lifecycle::CurrentScene;
use std::{collections::BTreeMap, str::FromStr};

#[derive(Resource, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Edits(BTreeMap<u32, WorldEdits>);

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnvironmentField {
    SkyScale,
    SkyLevel,
    WaveAmplitude,
    WaveSpeed,
    WaveFrequency,
}

impl FromStr for EnvironmentField {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self> {
        match name {
            "fxskyscale" => Ok(Self::SkyScale),
            "fxskylevel" => Ok(Self::SkyLevel),
            "fxwaveamp" => Ok(Self::WaveAmplitude),
            "fxwavespeed" => Ok(Self::WaveSpeed),
            "fxwavefreq" => Ok(Self::WaveFrequency),
            _ => anyhow::bail!("unsupported environment field {name:?}"),
        }
    }
}

impl EnvironmentField {
    const ALL: [Self; 5] = [
        Self::SkyScale,
        Self::SkyLevel,
        Self::WaveAmplitude,
        Self::WaveSpeed,
        Self::WaveFrequency,
    ];

    // Version-one snapshots retain their scalar array order and numeric diagnostics.
    fn storage_slot(self) -> usize {
        match self {
            Self::SkyScale => 0,
            Self::SkyLevel => 1,
            Self::WaveAmplitude => 2,
            Self::WaveSpeed => 3,
            Self::WaveFrequency => 4,
        }
    }

    fn is_wave(self) -> bool {
        matches!(
            self,
            Self::WaveAmplitude | Self::WaveSpeed | Self::WaveFrequency
        )
    }

    fn scalar(self, environment: &mut Environment) -> Result<&mut f32> {
        match self {
            Self::SkyScale | Self::SkyLevel => {
                let sky = environment
                    .sky
                    .as_mut()
                    .context("current world has no sky")?;
                Ok(match self {
                    Self::SkyScale => &mut sky.scale,
                    _ => &mut sky.level,
                })
            }
            Self::WaveAmplitude | Self::WaveSpeed | Self::WaveFrequency => {
                let [amplitude, speed, frequency] = &mut environment
                    .water
                    .as_mut()
                    .context("current world has no waves")?
                    .parameters;
                Ok(match self {
                    Self::WaveAmplitude => amplitude,
                    Self::WaveSpeed => speed,
                    _ => frequency,
                })
            }
        }
    }
}

fn edit(In(command): In<Invocation>, world: &mut World) -> Result<Vec<String>> {
    let field = command.name.parse::<EnvironmentField>()?;
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
        *field.scalar(&mut environment)? = value;
    }
    world
        .resource_mut::<Edits>()
        .0
        .entry(id)
        .or_default()
        .scalars[field.storage_slot()] = Some(value);
    if field.is_wave() {
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
    let direction = wrap(
        world
            .get_resource_mut::<cyrus_game::random::EngineRandom>()
            .context("fxskyspeed requires the shared engine random stream")?
            .draw(),
    ) as u16;
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
    for (field, value) in EnvironmentField::ALL.into_iter().zip(&values.scalars) {
        if let Some(value) = value
            && let Ok(slot) = field.scalar(&mut environment)
            && *slot != *value
        {
            *slot = *value;
            if field.is_wave() {
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
            effects: None,
            terrain_heights: Some(cyrus_assets::environment::TerrainHeights {
                side: 2,
                offset: 0,
                heights: vec![0.0; 4],
            }),
            texture_scale: [1, 1],
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
                side: 2,
                animated_vertices: vec![true; 4],
            }),
            terrain_lighting: None,
            model_lighting: default(),
            prelighting: default(),
            world_palette: Vec::new(),
            water_textures: Vec::new(),
            object_lights: Vec::new(),
        }
    }

    #[test]
    fn scalar_commands_preserve_names_output_snapshot_slots_and_owner_errors() {
        let mut app = App::new();
        app.add_plugins(cyrus_console::ConsolePlugin)
            .init_resource::<Assets<Environment>>()
            .init_resource::<crate::water::WaterPlayback>()
            .insert_resource(CurrentScene {
                name: "test".into(),
                world: Some(7),
            });
        register(&mut app);
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Environment>>()
            .add(environment());
        app.insert_resource(EnvironmentRequest(handle.clone()));
        let submit = |app: &mut App, line: &str| {
            cyrus_console::submit(app.world_mut(), line, cyrus_console::Source::Interactive)
        };
        for (name, alias, value) in [
            ("fxskyscale", "skysc", -800),
            ("fxskylevel", "skyl", -4500),
            ("fxwaveamp", "wamp", 0),
            ("fxwavespeed", "wsp", -2),
            ("fxwavefreq", "wfrq", 512),
        ] {
            let expected = vec![format!("{name} = {value}")];
            assert_eq!(
                submit(&mut app, &format!("{name} {value}")).unwrap(),
                expected
            );
            assert_eq!(
                submit(&mut app, &format!("{alias} {value}")).unwrap(),
                expected
            );
        }
        let saved =
            serde_json::to_value(EnvironmentEditsSnapshot::capture(app.world()).unwrap()).unwrap();
        assert_eq!(
            saved["edits"]["7"]["scalars"],
            serde_json::json!([-800.0, -4500.0, 0.0, -2.0, 512.0])
        );
        assert!("skysc".parse::<EnvironmentField>().is_err());
        assert!("fxskyspeed".parse::<EnvironmentField>().is_err());
        app.world_mut()
            .resource_mut::<Assets<Environment>>()
            .get_mut(&handle)
            .unwrap()
            .sky = None;
        assert_eq!(
            submit(&mut app, "skysc 1").unwrap_err().to_string(),
            "current world has no sky"
        );
        app.world_mut()
            .resource_mut::<Assets<Environment>>()
            .get_mut(&handle)
            .unwrap()
            .water = None;
        assert_eq!(
            submit(&mut app, "wamp 1").unwrap_err().to_string(),
            "current world has no waves"
        );
        let snapshot: EnvironmentEditsSnapshot = serde_json::from_value(saved).unwrap();
        assert_eq!(
            snapshot
                .restore(&BTreeMap::from([(7, &environment())]))
                .unwrap()
                .prepare_environment(7, environment())
                .unwrap()
                .water
                .unwrap()
                .parameters,
            [0.0, -2.0, 512.0]
        );
    }

    #[test]
    fn replacement_environment_applies_edits_before_owner_reconstruction() -> Result<()> {
        let installed = environment();
        let saved = EnvironmentEditsSnapshot {
            version: 1,
            edits: Edits(BTreeMap::from([(
                7,
                WorldEdits {
                    scalars: [None, Some(-1234.0), Some(48.0), None, None],
                    sky_speed: Some(9),
                },
            )])),
        };
        let candidate = saved.restore(&BTreeMap::from([(7, &installed)]))?;
        let prepared = candidate.prepare_environment(7, environment())?;
        assert_eq!(prepared.sky.as_ref().unwrap().level, -1234.0);
        assert_eq!(prepared.sky.as_ref().unwrap().speed, 9);
        assert_eq!(prepared.water.as_ref().unwrap().parameters[0], 48.0);
        assert_eq!(installed.sky.as_ref().unwrap().level, -3500.0);
        assert_eq!(installed.water.as_ref().unwrap().parameters[0], 24.0);
        let mut missing = environment();
        missing.water = None;
        assert!(candidate.prepare_environment(7, missing).is_err());
        Ok(())
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
        app.world_mut().spawn(crate::sky::Sky::new(1, 0));
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
    fn environment_edits_replacement_validates_all_worlds_before_publication() {
        let mut live = World::new();
        live.insert_resource(Edits(BTreeMap::from([(
            7,
            WorldEdits {
                scalars: [Some(-800.0), None, Some(0.0), Some(-2.0), None],
                sky_speed: Some(255),
            },
        )])));
        let saved = EnvironmentEditsSnapshot::capture(&live).unwrap();
        let bytes = serde_json::to_vec(&saved).unwrap();
        assert!(saved.clone().restore(&BTreeMap::new()).is_err());
        let installed = environment();
        let sources = BTreeMap::from([(7, &installed)]);
        let mut malformed = serde_json::to_value(&saved).unwrap();
        malformed["version"] = 99.into();
        assert!(
            serde_json::from_value::<EnvironmentEditsSnapshot>(malformed)
                .unwrap()
                .restore(&sources)
                .is_err()
        );
        let candidate = saved.restore(&sources).unwrap();
        assert_eq!(
            serde_json::to_vec(&EnvironmentEditsSnapshot::capture(&live).unwrap()).unwrap(),
            bytes
        );
        let mut replacement = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        candidate.queue(&mut bevy::prelude::Commands::new(&mut queue, &replacement));
        assert!(EnvironmentEditsSnapshot::capture(&replacement).is_err());
        queue.apply(&mut replacement);
        assert_eq!(
            serde_json::to_vec(&EnvironmentEditsSnapshot::capture(&replacement).unwrap()).unwrap(),
            bytes
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

/// Retained per-world console edits, independent of loaded asset handles.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct EnvironmentEditsSnapshot {
    version: u32,
    edits: Edits,
}

/// Validated replacement; queued only by the complete session commit.
pub struct EnvironmentEditsCandidate(Edits);

impl EnvironmentEditsSnapshot {
    pub fn capture(world: &World) -> Result<Self> {
        Ok(Self {
            version: 1,
            edits: world
                .get_resource::<Edits>()
                .context("environment edits owner absent")?
                .clone(),
        })
    }

    /// Validate every retained world and affected field against prepared installed inputs.
    pub fn restore(
        self,
        sources: &BTreeMap<u32, &Environment>,
    ) -> Result<EnvironmentEditsCandidate> {
        anyhow::ensure!(
            self.version == 1,
            "environment edits snapshot version unsupported"
        );
        for (id, edits) in &self.edits.0 {
            let environment = sources.get(id).with_context(|| {
                format!("environment edits world {id}: installed source absent")
            })?;
            for (field, value) in EnvironmentField::ALL.into_iter().zip(&edits.scalars) {
                if let Some(value) = value {
                    let index = field.storage_slot();
                    anyhow::ensure!(
                        value.is_finite(),
                        "environment edits world {id} field {index}: nonfinite scalar"
                    );
                    anyhow::ensure!(
                        if !field.is_wave() {
                            environment.sky.is_some()
                        } else {
                            environment.water.is_some()
                        },
                        "environment edits world {id} field {index}: installed owner absent"
                    );
                }
            }
            anyhow::ensure!(
                edits.sky_speed.is_none() || environment.sky.is_some(),
                "environment edits world {id}: installed sky absent"
            );
        }
        Ok(EnvironmentEditsCandidate(self.edits))
    }
}

impl EnvironmentEditsCandidate {
    /// Apply retained edits to an owned off-live installed environment before preparing
    /// sky, terrain and materials. The caller publishes this private asset at commit.
    pub fn prepare_environment(&self, world: u32, mut source: Environment) -> Result<Environment> {
        if let Some(edits) = self.0.0.get(&world) {
            for (field, value) in EnvironmentField::ALL.into_iter().zip(&edits.scalars) {
                if let Some(value) = value {
                    let index = field.storage_slot();
                    anyhow::ensure!(
                        value.is_finite(),
                        "environment world {world} field {index}: nonfinite edit"
                    );
                    *field
                        .scalar(&mut source)
                        .with_context(|| format!("environment world {world} field {index}"))? =
                        *value;
                }
            }
            if let Some(speed) = edits.sky_speed {
                source
                    .sky
                    .as_mut()
                    .with_context(|| format!("environment world {world}: sky absent"))?
                    .speed = speed;
            }
        }
        Ok(source)
    }

    /// Queue the validated owner alongside the other session replacements.
    pub fn queue(self, commands: &mut bevy::prelude::Commands) {
        commands.insert_resource(self.0);
    }
}
