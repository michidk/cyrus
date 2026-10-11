//! Scene loading. shortcut: the scene goes through the preservation crate's GLB
//! export and Bevy's glTF loader (materials, lights and unit scale are whatever
//! that export decides); upgrade to direct mesh/texture construction when
//! rendering fidelity work starts.

use crate::settings::Settings;
use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;
use cyrus_assets::install::GameInstall;
use cyrus_assets::scene::export_scene;
use cyrus_game::AppState;
pub struct ScenePlugin;

/// Must run before `AssetPlugin` is added (Bevy builds sources at that point).
pub fn register_cache_source(app: &mut App) -> anyhow::Result<()> {
    let cache = std::path::absolute(Settings::cache_dir())
        .map_err(|error| anyhow::anyhow!("resolve scene cache directory: {error}"))?;
    std::fs::create_dir_all(&cache)
        .map_err(|error| anyhow::anyhow!("create scene cache {}: {error}", cache.display()))?;
    let path = cache
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("scene cache path is not UTF-8"))?;
    app.register_asset_source("cache", AssetSourceBuilder::platform_default(path, None));
    Ok(())
}

use cyrus_world::lifecycle::{CurrentScene, PreparedWorld, SceneRequest, WorldChange};

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentScene>()
            .add_systems(OnEnter(AppState::InGame), load_requested)
            .add_systems(
                Update,
                load_requested
                    .before(WorldChange)
                    .run_if(in_state(AppState::InGame))
                    .run_if(resource_changed::<SceneRequest>),
            );
    }
}

fn load_requested(
    mut commands: Commands,
    request: Res<SceneRequest>,
    install: Res<GameInstall>,
    assets: Res<AssetServer>,
    mut current: ResMut<CurrentScene>,
) {
    let name = request.0.to_ascii_uppercase();
    if current.name == name {
        return;
    }
    let world = match cyrus_assets::scene::world_index(&install, &name) {
        Ok(world) => world,
        Err(error) => {
            error!("scene {name}: {error}");
            return;
        }
    };
    let glb = Settings::cache_dir().join(format!("{name}.glb"));
    if let Err(error) = export_scene(&install, &name, &glb) {
        error!("scene {name}: {error}");
        return;
    }
    if let Err(error) = cyrus_assets::sound_export::export(&install, &Settings::cache_dir()) {
        error!("sound export: {error}");
    }
    let mut prepared = PreparedWorld::from_cache(name.clone(), &assets);
    prepared.world = Some(world);
    current.name.clone_from(&name);
    current.world = prepared.world;
    commands.insert_resource(prepared);
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
        ))
        .insert_resource(GameInstall {
            root: std::env::temp_dir().join("cyrus-absent-install"),
        })
        .insert_resource(SceneRequest("NEW".into()))
        .insert_resource(CurrentScene {
            name: "OLD".into(),
            world: Some(17),
        })
        .add_systems(Update, load_requested);
        let old = PreparedWorld::from_cache("OLD".into(), app.world().resource::<AssetServer>());
        let animations = old.animations.clone();
        app.insert_resource(old).update();
        assert_eq!(app.world().resource::<CurrentScene>().name, "OLD");
        assert_eq!(app.world().resource::<CurrentScene>().world, Some(17));
        assert_eq!(
            app.world().resource::<PreparedWorld>().animations,
            animations
        );
    }
}
