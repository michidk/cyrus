//! Background native scene export; commit cache references only after preparation succeeds.

use crate::settings::Settings;
use bevy::prelude::*;
use cyrus_assets::install::GameInstall;
use cyrus_assets::scene::export_scene_reporting;
use cyrus_game::AppState;
pub struct ScenePlugin;

/// Must run before `AssetPlugin` is added (Bevy builds sources at that point).
pub fn register_cache_source(app: &mut App) -> anyhow::Result<()> {
    cyrus_app::register_cache(app)
}

use cyrus_world::lifecycle::{
    CurrentScene, PreparationProgress, PreparedWorld, SceneLoadError, SceneRequest, WorldChange,
};
use std::sync::{Mutex, mpsc};

enum Message {
    Stage(usize, usize, String),
    Done(anyhow::Result<u32>),
}
#[derive(Resource)]
struct Preparation {
    name: String,
    result: Mutex<mpsc::Receiver<Message>>,
}

#[derive(Resource)]
struct LastAttempt(String);

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentScene>()
            .add_systems(OnEnter(AppState::InGame), load_requested)
            .add_systems(
                OnEnter(AppState::Loading),
                (clear_attempt, load_requested)
                    .chain()
                    .run_if(resource_exists::<cyrus_startup::StartGame>),
            )
            .add_systems(Update, finish.before(WorldChange))
            .add_systems(
                Update,
                load_requested.before(WorldChange).after(finish).run_if(
                    in_state(AppState::InGame).or_else(resource_exists::<cyrus_startup::StartGame>),
                ),
            );
    }
}

fn clear_attempt(mut commands: Commands) {
    commands.remove_resource::<LastAttempt>();
}

fn load_requested(
    mut commands: Commands,
    request: Res<SceneRequest>,
    install: Res<GameInstall>,
    current: Res<CurrentScene>,
    pending: Option<Res<Preparation>>,
    last: Option<Res<LastAttempt>>,
    game: Option<Res<cyrus_startup::StartGame>>,
) {
    let name = request.0.to_ascii_uppercase();
    if pending.is_some()
        || (!request.is_changed() && last.is_some_and(|last| last.0 == name))
        || (game.is_none() && current.name == name)
    {
        return;
    }
    commands.remove_resource::<SceneLoadError>();
    commands.remove_resource::<PreparationProgress>();
    commands.insert_resource(LastAttempt(name.clone()));
    let install = install.clone();
    let requested = name.clone();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name(format!("prepare world {name}"))
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let stages = sender.clone();
            let result = (|| {
                let world = cyrus_assets::scene::world_index(&install, &requested)?;
                let glb = Settings::cache_dir().join(format!("{requested}.glb"));
                export_scene_reporting(&install, &requested, &glb, &|done, total, stage| {
                    let _ = stages.send(Message::Stage(done, total, stage.into()));
                })?;
                cyrus_assets::sound_export::export(&install, &Settings::cache_dir())?;
                Ok(world)
            })();
            let _ = sender.send(Message::Done(result));
        });
    match worker {
        Ok(_) => {
            commands.insert_resource(Preparation {
                name,
                result: Mutex::new(receiver),
            });
        }
        Err(error) => {
            commands.insert_resource(SceneLoadError(format!(
                "World {name} preparation worker: {error}"
            )));
        }
    }
}

fn finish(
    mut commands: Commands,
    pending: Option<Res<Preparation>>,
    request: Res<SceneRequest>,
    assets: Res<AssetServer>,
) {
    let Some(pending) = pending else {
        return;
    };
    let result = loop {
        match pending.result.lock().unwrap().try_recv() {
            Ok(Message::Stage(done, total, label)) => {
                commands.insert_resource(PreparationProgress { done, total, label });
            }
            Ok(Message::Done(result)) => break result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                break Err(anyhow::anyhow!("preparation worker disconnected"));
            }
        }
    };
    commands.remove_resource::<Preparation>();
    if pending.name != request.0.to_ascii_uppercase() {
        return;
    }
    match result {
        Ok(world) => {
            let mut prepared = PreparedWorld::from_cache(pending.name.clone(), &assets);
            for path in [
                &prepared.player_path,
                &prepared.environment_path,
                &prepared.scene_path,
            ] {
                assets.reload(path.clone());
            }
            assets.reload(format!("cache://{}.animations", pending.name));
            assets.reload(format!("cache://{}.scripts", pending.name));
            prepared.world = Some(world);
            commands.insert_resource(prepared);
        }
        Err(error) => {
            let error = format!("World {}: {error:#}", pending.name);
            error!("{error}");
            commands.insert_resource(SceneLoadError(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_preparation_preserves_the_committed_scene_and_references() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            cyrus_world::animation::AnimationPlugin,
            cyrus_world::lifecycle::WorldPlugin,
        ))
        .insert_resource(GameInstall {
            root: std::env::temp_dir().join("cyrus-absent-install"),
        })
        .insert_resource(SceneRequest("NEW".into()))
        .insert_resource(CurrentScene {
            name: "OLD".into(),
            world: Some(17),
        })
        .add_systems(Update, (load_requested, finish).chain());
        let mut old =
            PreparedWorld::from_cache("OLD".into(), app.world().resource::<AssetServer>());
        old.world = Some(17);
        let animations = old.animations.clone();
        app.insert_resource(old);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app.world().contains_resource::<SceneLoadError>() {
            app.update();
            assert!(
                std::time::Instant::now() < deadline,
                "scene failure not reported"
            );
            std::thread::yield_now();
        }
        assert_eq!(app.world().resource::<CurrentScene>().name, "OLD");
        assert_eq!(app.world().resource::<CurrentScene>().world, Some(17));
        assert_eq!(
            app.world().resource::<PreparedWorld>().animations,
            animations
        );
    }
}
