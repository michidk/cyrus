//! Scene loading. shortcut: the scene goes through the preservation crate's GLB
//! export and Bevy's glTF loader (materials, lights and unit scale are whatever
//! that export decides); upgrade to direct mesh/texture construction when
//! rendering fidelity work starts.

use crate::player::FrameScene;
use crate::{AppState, install::GameInstall, settings::Settings};
use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;
use rgpre::gltf::{
    TextureCache, convert_positioned_models_to_gltf, convert_wld_scene_to_gltf, to_glb,
};
use rgpre::import::{palette::Palette, registry, rgm, wld};
use std::path::{Path, PathBuf};

/// Engine-to-export unit divisor used by the preservation crate's GLB export.
pub const ENGINE_UNIT_SCALE: f32 = 20.0;
/// Terrain texture bank the original engine hard-wires for WLD terrain (per the
/// preservation crate's WLD exporter).
const TERRAIN_TEXBSI_ID: u16 = 500;

pub struct ScenePlugin;

/// Present when there is no renderer: scenes are exported but not spawned.
#[derive(Resource)]
pub struct Headless;

/// Must run before `AssetPlugin` is added (Bevy builds sources at that point).
pub fn register_cache_source(app: &mut App) {
    std::fs::create_dir_all(Settings::cache_dir()).ok();
    app.register_asset_source(
        "cache",
        AssetSourceBuilder::platform_default(Settings::cache_dir().to_str().unwrap(), None),
    );
}

/// Stem of the scene to (re)load, e.g. `ISLAND`.
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
    if !glb.is_file()
        && let Err(e) = export_scene(&install, &name, &glb)
    {
        error!("scene {name}: {e}");
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    if headless.is_some() {
        current.name = name;
        return;
    }
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
    let maps = install.maps();
    let registry = registry::scan_dir(&install.root)?;
    let palette = find_palette(install, name);
    let mut cache = TextureCache::new(
        install.root.clone(),
        palette.as_ref().map(|p| Palette { colors: p.colors }),
    );
    let rgm_path = find_file(&maps, &format!("{name}.RGM"))
        .ok_or_else(|| anyhow::anyhow!("no {name}.RGM in {}", maps.display()))?;
    let rgm_bytes = std::fs::read(&rgm_path)?;
    let (_rgm, models, lights) =
        rgm::parse_rgm_with_models_for_stem(&rgm_bytes, &registry, Some(name))?;
    let (root, buffer) = match find_file(&maps, &format!("{name}.WLD")) {
        Some(wld_path) => {
            let wld_file = wld::parse_wld_file(&std::fs::read(wld_path)?)?;
            convert_wld_scene_to_gltf(
                &wld_file,
                TERRAIN_TEXBSI_ID,
                &models,
                palette.as_ref(),
                Some(&mut cache),
                false,
            )?
        }
        None => convert_positioned_models_to_gltf(
            &models,
            &lights,
            palette.as_ref(),
            Some(&mut cache),
            false,
        )?,
    };
    let glb = to_glb(&root, &buffer)?;
    std::fs::create_dir_all(out.parent().unwrap())?;
    std::fs::write(out, glb)?;
    info!(
        "scene {name}: exported {} models to {}",
        models.len(),
        out.display()
    );
    Ok(())
}

/// Case-insensitive lookup, because the GOG tree mixes upper and lower case.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
}

/// shortcut: palette picked by scene stem, falling back to ISLAND.COL; the
/// engine's own mapping lives in WORLD.INI and should be read from there.
fn find_palette(install: &GameInstall, name: &str) -> Option<Palette> {
    let fxart = install.fxart();
    let path =
        find_file(&fxart, &format!("{name}.COL")).or_else(|| find_file(&fxart, "ISLAND.COL"))?;
    Palette::parse(&std::fs::read(path).ok()?).ok()
}
