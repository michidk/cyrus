//! Scene loading. shortcut: the scene goes through the preservation crate's GLB
//! export and Bevy's glTF loader (materials, lights and unit scale are whatever
//! that export decides); upgrade to direct mesh/texture construction when
//! rendering fidelity work starts.

use crate::player::FrameScene;
use crate::{AppState, install::GameInstall, settings::Settings};
use bevy::prelude::*;
use rgpre::import::world_ini::{self, WorldIni};
use std::path::{Path, PathBuf};

/// Engine-to-export unit divisor used by the preservation crate's GLB export.
pub const ENGINE_UNIT_SCALE: f32 = 20.0;
pub struct ScenePlugin;

/// Present when there is no renderer: scenes are exported but not spawned.
#[derive(Resource)]
pub struct Headless;

/// Must run before `AssetPlugin` is added (Bevy builds sources at that point).
pub fn register_cache_source(app: &mut App) -> anyhow::Result<()> {
    cyrus_app::register_cache(app)
}

/// WORLD.INI world ID or map stem to load, e.g. `27` or `ISLAND`.
#[derive(Resource, Debug, Clone)]
pub struct SceneRequest(pub String);

#[derive(Resource, Debug, Clone, Default)]
pub struct CurrentScene {
    pub name: String,
}

#[derive(Component)]
pub struct SceneEntity;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentScene>()
            .add_systems(OnEnter(AppState::InGame), load_requested)
            .add_systems(
                Update,
                load_requested
                    .run_if(in_state(AppState::InGame))
                    .run_if(resource_changed::<SceneRequest>),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn load_requested(
    mut commands: Commands,
    request: Res<SceneRequest>,
    install: Res<GameInstall>,
    assets: Res<AssetServer>,
    mut current: ResMut<CurrentScene>,
    old: Query<Entity, With<SceneEntity>>,
    headless: Option<Res<Headless>>,
    mut frame: ResMut<FrameScene>,
) {
    let name = request.0.to_ascii_uppercase();
    if current.name == name {
        return;
    }
    let glb = Settings::cache_dir().join(format!("{name}.glb"));
    if let Err(e) = export_scene(&install, &name, &glb) {
        error!("scene {name}: {e}");
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    commands.insert_resource(crate::animation::AnimationRequest(
        assets.load(format!("cache://{name}.animations")),
    ));
    commands.insert_resource(crate::player::data::PlayerSceneRequest(
        assets.load(format!("cache://{name}.player")),
    ));
    if let Err(error) = crate::sound_export::export(&install, &Settings::cache_dir()) {
        error!("sound export: {error}");
    }
    if headless.is_some() {
        current.name = name;
        return;
    }
    commands.insert_resource(crate::environment::EnvironmentRequest(
        assets.load(format!("cache://{name}.environment")),
    ));
    frame.0 = true;
    commands.spawn((
        SceneEntity,
        WorldAssetRoot(
            assets.load(GltfAssetLabel::Scene(0).from_asset(format!("cache://{name}.glb"))),
        ),
    ));
    commands.spawn((
        SceneEntity,
        DirectionalLight {
            illuminance: 8000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));
    current.name = name;
}

/// WLD scenes (outdoor) get terrain plus their RGM placements; indoor scenes are RGM only.
pub fn export_scene(install: &GameInstall, name: &str, out: &Path) -> anyhow::Result<()> {
    let world = cyrus_assets::native::geometry::world(&install.root, name)?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    crate::animation_export::export(
        &world.rgm,
        &world.models,
        &world.registry,
        world.entry.index,
        out,
    )?;
    crate::environment_export::export(install, &world.entry, world.terrain.as_deref(), out)?;
    crate::player::export::export(
        &install.root,
        &world.rgm,
        world.terrain.as_deref(),
        world.entry.index,
        out,
    )?;
    cyrus_assets::native::publish(out, &world.glb)?;
    info!(
        "scene {name}: exported {} models to {}",
        world.models.len(),
        out.display()
    );
    Ok(())
}

pub(crate) use cyrus_assets::native::geometry::find_file;

/// `WORLD.INI` index of a scene name: a world ID or a map stem.
pub fn world_index(install: &GameInstall, name: &str) -> anyhow::Result<u32> {
    Ok(resolve_world(&read_worlds(install)?, name)?.index)
}

fn read_worlds(install: &GameInstall) -> anyhow::Result<WorldIni> {
    let path = world_ini::find_world_ini(&install.root)
        .ok_or_else(|| anyhow::anyhow!("missing WORLD.INI"))?;
    Ok(WorldIni::parse(&std::fs::read_to_string(path)?))
}

use cyrus_assets::native::geometry::resolve_world;

fn map_path(install: &GameInstall, path: &str) -> anyhow::Result<PathBuf> {
    let filename = path
        .rsplit(['\\', '/'])
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty map path"))?;
    find_file(&install.maps(), filename).ok_or_else(|| anyhow::anyhow!("missing map {path}"))
}

/// Rebuild every world variant plus the traditional map-stem aliases.
/// Numeric filenames preserve worlds that share geometry but use different palettes.
pub fn export_all_worlds(install: &GameInstall, out: &Path) -> anyhow::Result<()> {
    crate::sound_export::export(install, out)?;
    let worlds = read_worlds(install)?;
    anyhow::ensure!(!worlds.entries.is_empty(), "WORLD.INI contains no worlds");
    let mut aliases = std::collections::HashSet::new();
    let mut failures = Vec::new();
    for entry in &worlds.entries {
        let name = entry.index.to_string();
        let path = out.join(format!("{name}.glb"));
        match export_scene(install, &name, &path) {
            Ok(()) => {
                let map = map_path(install, &entry.map)?;
                let stem = map
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| anyhow::anyhow!("invalid map stem"))?
                    .to_ascii_uppercase();
                if aliases.insert(stem.clone()) {
                    std::fs::copy(&path, out.join(format!("{stem}.glb")))?;
                    std::fs::copy(
                        path.with_extension("animations"),
                        out.join(format!("{stem}.animations")),
                    )?;
                    std::fs::copy(
                        path.with_extension("player"),
                        out.join(format!("{stem}.player")),
                    )?;
                    std::fs::copy(
                        path.with_extension("environment"),
                        out.join(format!("{stem}.environment")),
                    )?;
                }
                println!("world {name}: {} -> {}", entry.map, path.display());
            }
            Err(error) => failures.push(format!("world {name}: {error:#}")),
        }
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgpre::import::palette::Palette;

    #[test]
    fn world_selection_preserves_terrain_and_palette_variants() {
        let worlds = WorldIni::parse(
            "world_map[0]=MAPS\\intro.rgm\nworld_world[0]=MAPS\\shore.WLD\nworld_palette[0]=3DART\\dusk.COL\n\
             world_map[1]=MAPS\\town.rgm\nworld_palette[1]=3DART\\day.COL\n\
             world_map[27]=MAPS\\town.rgm\nworld_palette[27]=3DART\\night.COL\n",
        );
        let intro = resolve_world(&worlds, "INTRO").unwrap();
        assert_eq!(intro.world.as_deref(), Some("MAPS\\shore.WLD"));
        assert_eq!(intro.palette, "3DART\\dusk.COL");
        assert_eq!(resolve_world(&worlds, "town").unwrap().index, 1);
        assert_eq!(
            resolve_world(&worlds, "27").unwrap().palette,
            "3DART\\night.COL"
        );
        assert!(resolve_world(&worlds, "town").unwrap().world.is_none());
        for invalid in ["", "../town", "unknown", "999", "/tmp/town"] {
            assert!(resolve_world(&worlds, invalid).is_err());
        }
    }

    #[test]
    fn installed_worlds_resolve_all_required_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
        let Ok(install) = GameInstall::locate(Some(&root)) else {
            eprintln!("skipping installed world check: no Redguard install");
            return;
        };
        let worlds = read_worlds(&install).unwrap();
        assert!(!worlds.entries.is_empty());
        for entry in worlds.entries {
            map_path(&install, &entry.map).unwrap();
            if let Some(path) = entry.world {
                map_path(&install, &path).unwrap();
            }
            let path = world_ini::find_palette_on_disk(&install.root, &entry.palette).unwrap();
            Palette::parse(&std::fs::read(path).unwrap()).unwrap();
        }
    }
}
