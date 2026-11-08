//! Native-only WebAssembly mods: discovery, activation, hot reload and the hook API.
//!
//! A mod is a folder under `mods/<id>/` in the game's config directory (`cyrus_config::config_dir`) with a `mod.toml`
//! (`name`, `description`) and any of:
//!
//! - `mod.wasm`: a WebAssembly component run by wasvy against `wit/cyrus.wit`
//!   (vertex sets, scene values, camera, movement and input hooks).
//! - any shader files that `mod.toml` lists as `[[shader]]` replacements: `replaces` is
//!   the asset path of the shader to swap (for example
//!   `embedded://cyrus_render/water_surface.wesl`) and `file` is the replacement in the
//!   mod folder. Each shader is one slot: the first active mod in id order supplies it.
//!
//! - a `files/` folder that mirrors the game install (`fxart/…`, `sound/…`): its files are read
//!   instead of the install's, or added to it. See [`file_overlays`]; applies after a restart.
//!
//! `mod.toml` may also give a `version`, an `author` and `[[setting]]` entries (see
//! [`settings`]); the Mods window draws the settings of each active mod.
//!
//! Without an active mod every hook keeps the original behaviour. Activation is
//! saved to `mods.json` there; the F1 debug menu lists the mods next to it.
//! Active mod files are polled and reloaded when they change.
mod api;
mod settings;

use anyhow::{Context, ensure};
use bevy::asset::io::AssetSourceBuilder;
// Explicit imports: the Bevy prelude's `ContextExt` collides with `anyhow::Context`.
use bevy::asset::{AssetApp, AssetServer, Assets, Handle};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::prelude::{Entity, IntoScheduleConfigs, Local, Query, Res, ResMut, Resource};
use bevy::log::{error, info, warn};
use bevy::prelude::{App, Plugin, PreUpdate, Real, Result, Startup, Time, Update, default};
use bevy::shader::Shader;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use serde::{Deserialize, Serialize};
use settings::{ModSettings, Setting};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use wasvy::prelude::*;

wasvy::auto_host_components! {
    path = "wit",
    world = "cyrus:mods/host",
    module = cyrus_bindings,
}

/// The installed mods, one folder each, in the game's config directory.
fn mods_dir() -> PathBuf {
    cyrus_config::config_dir().join("mods")
}

/// Which mods are active.
fn active_file() -> PathBuf {
    cyrus_config::config_dir().join("mods.json")
}
/// A mod's folder of replacement game files.
const FILES_DIR: &str = "files";

/// Registers the `mods://` asset source; call before the asset plugin is added.
pub fn register_source(app: &mut App) {
    app.register_asset_source(
        "mods",
        AssetSourceBuilder::platform_default(&absolute(&mods_dir()), None),
    );
}

/// The `files/` folders of the active mods in id order, for the game-data reader to overlay on
/// the install. Read straight from disk, so it can run before any plugin is built.
pub fn file_overlays() -> Vec<PathBuf> {
    let active = match std::fs::read(active_file()) {
        Ok(bytes) => serde_json::from_slice::<Saved>(&bytes).map_err(anyhow::Error::from),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Saved::default()),
        Err(error) => Err(error.into()),
    };
    let mut active = active
        .unwrap_or_else(|error| {
            error!("mods: {}: {error:#}", active_file().display());
            Saved::default()
        })
        .active;
    active.sort();
    active
        .iter()
        .map(|id| mod_file(id, FILES_DIR))
        .filter(|folder| folder.is_dir())
        .collect()
}

pub struct ModsPlugin;

impl Plugin for ModsPlugin {
    fn build(&self, app: &mut App) {
        let catalog = Catalog::discover().unwrap_or_else(|error| {
            error!("mods: {error:#}");
            Catalog::default()
        });
        let settings = ModSettings::load(
            catalog
                .entries
                .iter()
                .map(|entry| (entry.id.as_str(), entry.manifest.settings.as_slice())),
        )
        .unwrap_or_else(|error| {
            error!("mods: {error:#}");
            ModSettings::default()
        });
        app.add_plugins(ModLoaderPlugin::default().add_functionality(add_components_to_linker))
            .register_type::<Vec<f32>>()
            .insert_resource(catalog)
            .insert_resource(settings)
            .init_resource::<cyrus_render::hooks::VertexSets>()
            .init_resource::<cyrus_render::hooks::SceneValues>()
            .init_resource::<cyrus_render::camera::CameraTakeover>()
            .init_resource::<cyrus_script::player::input::MovementWish>()
            .init_resource::<cyrus_script::player::input::LocomotionClaimed>()
            .init_resource::<cyrus_script::player::input::PlayerRequests>()
            .init_resource::<cyrus_script::player::input::CameraOrbit>()
            .init_resource::<cyrus_script::player::input::CollisionProbes>()
            .add_systems(Startup, api::spawn_host)
            .add_systems(PreUpdate, api::exchange)
            .add_systems(Update, (watch, apply).chain())
            .add_systems(
                EguiPrimaryContextPass,
                (
                    hud,
                    window.run_if(|menu: Res<cyrus_debug::DebugMenu>| menu.open),
                ),
            );
    }
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    description: String,
    version: Option<String>,
    author: Option<String>,
    #[serde(default, rename = "setting")]
    settings: Vec<Setting>,
    #[serde(default, rename = "shader")]
    shaders: Vec<ShaderReplacement>,
}

/// One shader a mod swaps for another.
#[derive(Deserialize)]
struct ShaderReplacement {
    /// Asset path of the shader to replace.
    replaces: String,
    /// Replacement source, a plain file name in the mod folder.
    file: String,
}

struct Entry {
    id: String,
    manifest: Manifest,
    active: bool,
    has_wasm: bool,
    has_files: bool,
    /// The running wasvy instance while active.
    instance: Option<Entity>,
    modified: Option<SystemTime>,
}

#[derive(Resource, Default)]
struct Catalog {
    entries: Vec<Entry>,
    /// Each replaced shader as it was before any mod, by asset path, to restore it.
    original_shaders: HashMap<String, Shader>,
    /// The asset paths whose shader a mod currently supplies.
    replaced_shaders: HashSet<String>,
    status: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    active: Vec<String>,
}

impl Catalog {
    fn discover() -> anyhow::Result<Self> {
        let saved: Saved = match std::fs::read(active_file()) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| active_file().display().to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Saved::default(),
            Err(error) => return Err(error).with_context(|| active_file().display().to_string()),
        };
        let mut entries = Vec::new();
        let dirs = match std::fs::read_dir(mods_dir()) {
            Ok(dirs) => dirs,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error).with_context(|| mods_dir().display().to_string()),
        };
        for dir in dirs {
            let dir = dir
                .with_context(|| mods_dir().display().to_string())?
                .path();
            let manifest = dir.join("mod.toml");
            if !manifest.is_file() {
                continue;
            }
            let id = dir
                .file_name()
                .and_then(|name| name.to_str())
                .with_context(|| format!("{}: mod folder name is not UTF-8", dir.display()))?
                .to_owned();
            let manifest: Manifest = toml::from_str(
                &std::fs::read_to_string(&manifest)
                    .with_context(|| manifest.display().to_string())?,
            )
            .with_context(|| manifest.display().to_string())?;
            for setting in &manifest.settings {
                setting.validate().with_context(|| format!("mod {id}"))?;
            }
            for shader in &manifest.shaders {
                ensure!(
                    Path::new(&shader.file).file_name() == Some(shader.file.as_ref()),
                    "mod {id}: shader file {:?} must be a plain file name",
                    shader.file
                );
            }
            entries.push(Entry {
                active: saved.active.contains(&id),
                has_wasm: dir.join("mod.wasm").is_file(),
                has_files: dir.join(FILES_DIR).is_dir(),
                id,
                manifest,
                instance: None,
                modified: None,
            });
        }
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Self {
            entries,
            ..default()
        })
    }

    fn save(&self) -> anyhow::Result<()> {
        let saved = Saved {
            active: self
                .entries
                .iter()
                .filter(|entry| entry.active)
                .map(|entry| entry.id.clone())
                .collect(),
        };
        std::fs::write(active_file(), serde_json::to_vec_pretty(&saved)?)
            .with_context(|| active_file().display().to_string())
    }
}

fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_owned())
        .to_string_lossy()
        .into_owned()
}

fn mod_file(id: &str, file: &str) -> PathBuf {
    mods_dir().join(id).join(file)
}

/// Starts and stops wasm instances and installs the active shader replacements.
fn apply(
    mut catalog: ResMut<Catalog>,
    mut mods: Mods,
    asset_server: Res<AssetServer>,
    mut shaders: ResMut<Assets<Shader>>,
    mut shader_pending: Local<bool>,
) {
    if !catalog.is_changed() && !*shader_pending {
        return;
    }
    for entry in &mut catalog.entries {
        match (entry.active && entry.has_wasm, entry.instance) {
            (true, None) => {
                let instance = mods.spawn(
                    format!("mods://{}/mod.wasm", entry.id),
                    Some(entry.manifest.name.clone()),
                );
                mods.enable_access(instance, ModAccess::World);
                entry.instance = Some(instance);
            }
            (false, Some(instance)) => {
                mods.despawn(instance);
                entry.instance = None;
            }
            _ => {}
        }
    }
    // Each shader is one slot: the first active mod in id order supplies it.
    let mut suppliers: BTreeMap<String, (String, String)> = BTreeMap::new();
    for entry in catalog.entries.iter().filter(|entry| entry.active) {
        for shader in &entry.manifest.shaders {
            match suppliers.get(&shader.replaces) {
                Some((first, _)) => warn!(
                    "mods: {first} and {} both replace {}; using {first}",
                    entry.id, shader.replaces
                ),
                None => {
                    suppliers.insert(
                        shader.replaces.clone(),
                        (entry.id.clone(), shader.file.clone()),
                    );
                }
            }
        }
    }
    let targets: BTreeSet<String> = suppliers
        .keys()
        .chain(&catalog.replaced_shaders)
        .cloned()
        .collect();
    let mut status = None;
    let mut installed = Vec::new();
    for target in targets {
        let handle: Handle<Shader> = asset_server.load(&target);
        // An embedded library may not have loaded yet; try again next frame.
        let Some(current) = shaders.get(&handle).cloned() else {
            *shader_pending = true;
            continue;
        };
        let original = catalog
            .original_shaders
            .entry(target.clone())
            .or_insert(current)
            .clone();
        let supplier = suppliers.get(target.as_str());
        if supplier.is_none() && !catalog.replaced_shaders.contains(&target) {
            continue;
        }
        let shader = match supplier {
            Some((id, file)) => match std::fs::read_to_string(mod_file(id, file)) {
                Ok(source) => Shader::from_wesl(source, target.clone()),
                Err(error) => {
                    status = Some(format!("{id}: {file}: {error}"));
                    original
                }
            },
            None => original,
        };
        match shaders.insert(&handle, shader) {
            Ok(()) => {
                info!(
                    "mods: {target} from {}",
                    supplier.map_or("the original", |(id, _)| id)
                );
                installed.push((target, supplier.is_some()));
            }
            Err(error) => error!("mods: {target}: {error}"),
        }
    }
    for (target, replaced) in installed {
        if replaced {
            catalog.replaced_shaders.insert(target);
        } else {
            catalog.replaced_shaders.remove(&target);
        }
    }
    if status.is_some() {
        catalog.status = status;
    }
}

/// Reloads active mods whose files changed on disk.
fn watch(
    mut catalog: ResMut<Catalog>,
    asset_server: Res<AssetServer>,
    time: Res<Time<Real>>,
    mut next: Local<f32>,
) {
    if time.elapsed_secs() < *next {
        return;
    }
    *next = time.elapsed_secs() + 0.5;
    let mut shader_changed = false;
    for entry in catalog.bypass_change_detection().entries.iter_mut() {
        if !entry.active {
            continue;
        }
        let modified = std::iter::once("mod.wasm")
            .chain(
                entry
                    .manifest
                    .shaders
                    .iter()
                    .map(|shader| shader.file.as_str()),
            )
            .filter_map(|file| std::fs::metadata(mod_file(&entry.id, file)).ok())
            .filter_map(|meta| meta.modified().ok())
            .max();
        if entry.modified.is_some() && modified != entry.modified {
            info!("mods: reloading {}", entry.id);
            if entry.instance.is_some() {
                asset_server.reload(format!("mods://{}/mod.wasm", entry.id));
            }
            shader_changed |= !entry.manifest.shaders.is_empty();
        }
        entry.modified = modified;
    }
    if shader_changed {
        catalog.set_changed();
    }
}

/// Draws the lines mods wrote this frame in the top-left corner.
fn hud(mut contexts: EguiContexts, mut host: Query<&mut api::Hud>) -> Result {
    let Ok(mut hud) = host.single_mut() else {
        return Ok(());
    };
    let lines = hud.take();
    if lines.is_empty() {
        return Ok(());
    }
    egui::Area::new(egui::Id::new("mod hud"))
        .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
        .interactable(false)
        .show(contexts.ctx_mut()?, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for line in lines {
                    ui.label(line);
                }
            });
        });
    Ok(())
}

fn window(
    mut contexts: EguiContexts,
    mut catalog: ResMut<Catalog>,
    mut settings: ResMut<ModSettings>,
) -> Result {
    egui::Window::new("Mods").show(contexts.ctx_mut()?, |ui| {
        if catalog.entries.is_empty() {
            ui.label(format!(
                "No mods in {}. `just mods` installs the bundled ones.",
                mods_dir().display()
            ));
        }
        let mut toggled = false;
        let mut save_error = None;
        for entry in &mut catalog.bypass_change_detection().entries {
            let manifest = &entry.manifest;
            toggled |= ui
                .checkbox(&mut entry.active, &manifest.name)
                .on_hover_text(&manifest.description)
                .changed();
            let credit = [
                manifest
                    .version
                    .as_deref()
                    .map(|version| format!("v{version}")),
                manifest
                    .author
                    .as_ref()
                    .map(|author| format!("by {author}")),
            ];
            let credit: Vec<_> = credit.into_iter().flatten().collect();
            if !credit.is_empty() {
                ui.small(credit.join(" "));
            }
            if entry.has_files {
                ui.small("Replaces game files; restart to apply a change.");
            }
            if entry.active && !manifest.settings.is_empty() {
                ui.indent(&entry.id, |ui| {
                    save_error = settings
                        .edit(ui, &entry.id, &manifest.settings)
                        .or(save_error.take());
                });
            }
        }
        if toggled {
            catalog.set_changed();
            catalog.status = catalog.save().err().map(|error| format!("{error:#}"));
        }
        if let Some(error) = save_error {
            catalog.bypass_change_detection().status = Some(format!("{error:#}"));
        }
        if let Some(status) = &catalog.status {
            ui.colored_label(egui::Color32::LIGHT_RED, status);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_lists_any_number_of_shader_replacements() {
        let manifest: Manifest = toml::from_str(
            r#"
            name = "Looks"
            [[shader]]
            replaces = "embedded://cyrus_render/water_surface.wesl"
            file = "water.wesl"
            [[shader]]
            replaces = "embedded://cyrus_render/sky.wesl"
            file = "sky.wesl"
            "#,
        )
        .unwrap();
        let files: Vec<_> = manifest.shaders.iter().map(|s| s.file.as_str()).collect();
        assert_eq!(files, ["water.wesl", "sky.wesl"]);
    }
}
