//! Native-only WebAssembly mods: discovery, activation, hot reload and the hook API.
//!
//! A mod is a folder under `mods/<id>/` in the game's config directory (`cyrus_config::config_dir`) with a `mod.toml`
//! (`name`, `description`) and any of:
//!
//! - `mod.wasm`: a WebAssembly component run by wasvy against `wit/cyrus.wit`
//!   (world facts, actors, effect slots, camera, movement and input hooks).
//! - any shader files that `mod.toml` lists as `[[shader]]` replacements: `replaces` is
//!   the asset path of the shader to swap (for example
//!   `embedded://cyrus_render/terrain_surface.wesl`) and `file` is the replacement in the
//!   mod folder. Each shader is one slot: the first active mod in id order supplies it.
//!
//! - a `files/` folder that mirrors the game install (`fxart/…`, `sound/…`): its files are read
//!   instead of the install's, or added to it. See [`file_overlays`]; applies after a restart.
//!
//! `mod.toml` may also give a `version`, an `author` and `[[setting]]` entries (see
//! [`settings`]); the Mods window draws the settings of each active mod.
//!
//! Without an active mod every hook keeps the original behaviour. Activation is
//! saved to `mods.json` there; the backtick debug menu lists the mods next to it.
//! Active mod files are polled and reloaded when they change.
mod api;
pub mod gameplay;
mod ownership;
mod particles;
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

/// The ids of the active mods in id order, read straight from disk so it can run before any
/// plugin is built.
fn active_ids() -> Vec<String> {
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
}

/// The `files/` folders of the active mods in id order, for the game-data reader to overlay on
/// the install.
pub fn file_overlays() -> Vec<PathBuf> {
    active_ids()
        .iter()
        .map(|id| mod_file(id, FILES_DIR))
        .filter(|folder| folder.is_dir())
        .collect()
}

/// The `[[script-patch]]` entries of the active mods in id order, for the script preparation to
/// apply. Like the file overlays they are fixed for the process, so a change applies after a
/// restart. A manifest that does not read is skipped with an error in the log.
pub fn script_patches() -> Vec<cyrus_assets::script_patch::ScriptPatch> {
    active_ids()
        .iter()
        .flat_map(|id| {
            let path = mod_file(id, "mod.toml");
            match std::fs::read_to_string(&path)
                .map_err(anyhow::Error::from)
                .and_then(|text| toml::from_str::<Manifest>(&text).map_err(anyhow::Error::from))
            {
                Ok(manifest) => manifest.script_patches,
                Err(error) => {
                    error!("mods: {}: {error:#}", path.display());
                    Vec::new()
                }
            }
        })
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
        app.add_plugins(bevy_hanabi::HanabiPlugin)
            .add_plugins(ModLoaderPlugin::default().add_functionality(add_components_to_linker))
            .register_type::<Vec<f32>>()
            .insert_resource(catalog)
            .insert_resource(settings)
            .init_resource::<cyrus_render::hooks::WorldFacts>()
            .init_resource::<cyrus_render::hooks::EffectSlots>()
            .init_resource::<cyrus_render::camera::CameraTakeover>()
            .init_resource::<cyrus_script::player::input::MovementWish>()
            .init_resource::<cyrus_script::player::input::LocomotionClaimed>()
            .init_resource::<cyrus_script::player::input::PlayerRequests>()
            .init_resource::<cyrus_script::player::input::CameraOrbit>()
            .init_resource::<cyrus_script::player::input::CollisionProbes>()
            .init_resource::<particles::ParticleState>()
            .init_resource::<ownership::Owners>()
            .add_systems(
                bevy::prelude::PostUpdate,
                particles::update
                    .before(bevy::transform::TransformSystems::Propagate)
                    .before(bevy_hanabi::EffectSystems::CompileEffects)
                    .before(bevy_hanabi::EffectSystems::TickSpawners),
            )
            .add_systems(Startup, api::spawn_host)
            .add_systems(
                PreUpdate,
                (
                    ownership::publish,
                    api::exchange,
                    particles::publish,
                    gameplay::publish,
                )
                    .chain(),
            )
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
    #[serde(default, rename = "script-patch")]
    script_patches: Vec<cyrus_assets::script_patch::ScriptPatch>,
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
    /// When mod.toml was last read.
    manifest_modified: Option<SystemTime>,
    /// The folder was deleted; the instance is stopped, then the entry dropped.
    gone: bool,
}

impl Entry {
    /// Reads a mod folder: its manifest, validated, and which parts it has.
    fn read(dir: &Path, active: bool) -> anyhow::Result<Self> {
        let id = folder_id(dir)?;
        let path = dir.join("mod.toml");
        let manifest: Manifest = toml::from_str(
            &std::fs::read_to_string(&path).with_context(|| path.display().to_string())?,
        )
        .with_context(|| path.display().to_string())?;
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
        Ok(Self {
            active,
            has_wasm: dir.join("mod.wasm").is_file(),
            has_files: dir.join(FILES_DIR).is_dir(),
            manifest_modified: std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok(),
            id,
            manifest,
            instance: None,
            modified: None,
            gone: false,
        })
    }
}

/// The folders of the mods folder that hold a mod.toml; none when the folder does not exist.
fn mod_folders() -> anyhow::Result<Vec<PathBuf>> {
    let dirs = match std::fs::read_dir(mods_dir()) {
        Ok(dirs) => dirs,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| mods_dir().display().to_string()),
    };
    let mut folders = Vec::new();
    for dir in dirs {
        let dir = dir
            .with_context(|| mods_dir().display().to_string())?
            .path();
        if dir.join("mod.toml").is_file() {
            folders.push(dir);
        }
    }
    Ok(folders)
}

/// A mod's id: its folder name.
/// Records why the mod folder `id` cannot be loaded, logging it once per distinct reason.
fn note_problem(
    problems: &mut std::collections::BTreeMap<String, String>,
    id: &str,
    error: &anyhow::Error,
) {
    let message = format!("{error:#}");
    if problems.get(id) != Some(&message) {
        error!("mods: {message}");
    }
    problems.insert(id.to_owned(), message);
}

fn folder_id(dir: &Path) -> anyhow::Result<String> {
    dir.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .with_context(|| format!("{}: mod folder name is not UTF-8", dir.display()))
}

#[derive(Resource, Default)]
struct Catalog {
    entries: Vec<Entry>,
    /// Each replaced shader as it was before any mod, by asset path, to restore it.
    original_shaders: HashMap<String, Shader>,
    /// The asset paths whose shader a mod currently supplies.
    replaced_shaders: HashSet<String>,
    status: Option<String>,
    /// Mod folders that cannot be loaded, by folder name, with the reason. Shown in the Mods
    /// window, so a broken mod is visible where the mods are listed and does not take the others
    /// with it.
    problems: std::collections::BTreeMap<String, String>,
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
        Ok(Self::load(&mod_folders()?, &saved.active))
    }

    /// The catalog of mod folders: each one that reads is an entry, each one that does not is a
    /// problem beside the others, which still load.
    fn load(folders: &[PathBuf], active: &[String]) -> Self {
        let mut entries = Vec::new();
        let mut problems = std::collections::BTreeMap::new();
        for dir in folders {
            let id = match folder_id(dir) {
                Ok(id) => id,
                Err(error) => {
                    error!("mods: {error:#}");
                    continue;
                }
            };
            match Entry::read(dir, active.contains(&id)) {
                Ok(entry) => entries.push(entry),
                Err(error) => note_problem(&mut problems, &id, &error),
            }
        }
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Self {
            entries,
            problems,
            ..default()
        }
    }

    /// Picks up folders added to or removed from the mods folder and manifests edited since the
    /// last look. A folder that cannot be read is recorded in `problems` and left out; an
    /// installed mod whose manifest becomes unreadable keeps its last good one, with the problem
    /// shown beside it.
    fn rescan(&mut self, settings: &mut ModSettings) -> bool {
        let folders = match mod_folders() {
            Ok(folders) => folders,
            Err(error) => {
                self.status = Some(format!("{error:#}"));
                return false;
            }
        };
        let saved = std::fs::read(active_file())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            .unwrap_or_default();
        let mut changed = false;
        self.problems.retain(|id, _| {
            folders
                .iter()
                .any(|dir| folder_id(dir).is_ok_and(|name| name == *id))
        });
        for entry in &mut self.entries {
            let present = folders
                .iter()
                .any(|dir| folder_id(dir).is_ok_and(|id| id == entry.id));
            if !present && !entry.gone {
                info!("mods: {} was removed", entry.id);
                entry.gone = true;
                entry.active = false;
                changed = true;
            }
        }
        for dir in &folders {
            let Ok(id) = folder_id(dir) else { continue };
            match self.entries.iter_mut().find(|entry| entry.id == id) {
                Some(entry) if !entry.gone => {
                    let modified = std::fs::metadata(dir.join("mod.toml"))
                        .and_then(|meta| meta.modified())
                        .ok();
                    if modified != entry.manifest_modified {
                        entry.manifest_modified = modified;
                        match Entry::read(dir, entry.active) {
                            Ok(fresh) => {
                                info!("mods: reread the manifest of {id}");
                                entry.manifest = fresh.manifest;
                                entry.has_files = fresh.has_files;
                                self.problems.remove(&id);
                                changed = true;
                            }
                            Err(error) => note_problem(&mut self.problems, &id, &error),
                        }
                    }
                    entry.has_wasm = dir.join("mod.wasm").is_file();
                }
                _ => match Entry::read(dir, saved.active.contains(&id)) {
                    Ok(fresh) => {
                        info!("mods: found {id}");
                        self.entries.retain(|entry| entry.id != id);
                        self.entries.push(fresh);
                        self.problems.remove(&id);
                        changed = true;
                    }
                    Err(error) => note_problem(&mut self.problems, &id, &error),
                },
            }
        }
        if changed {
            self.entries.sort_by(|a, b| a.id.cmp(&b.id));
            for entry in &self.entries {
                settings.declare(&entry.id, &entry.manifest.settings);
            }
        }
        changed
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
    start_and_stop_instances(&mut catalog, &mut mods);
    catalog
        .bypass_change_detection()
        .entries
        .retain(|entry| !entry.gone);
    let suppliers = shader_suppliers(&catalog);
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

/// Spawns the wasm instance of each newly active mod and despawns those switched off.
fn start_and_stop_instances(catalog: &mut Catalog, mods: &mut Mods) {
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
}

/// Each shader is one slot: the first active mod in id order supplies it.
fn shader_suppliers(catalog: &Catalog) -> BTreeMap<String, (String, String)> {
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
    suppliers
}

/// Reloads active mods whose files changed on disk.
fn watch(
    mut catalog: ResMut<Catalog>,
    mut settings: ResMut<ModSettings>,
    asset_server: Res<AssetServer>,
    time: Res<Time<Real>>,
    mut next: Local<f32>,
) {
    if time.elapsed_secs() < *next {
        return;
    }
    *next = time.elapsed_secs() + 0.5;
    if catalog
        .bypass_change_detection()
        .rescan(settings.bypass_change_detection())
    {
        catalog.set_changed();
        settings.set_changed();
    }
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
        if catalog.entries.is_empty() && catalog.problems.is_empty() {
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
            if !manifest.script_patches.is_empty() {
                ui.small("Changes actor scripts; restart to apply a change.");
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
        for (id, problem) in &catalog.problems {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("{id} cannot be loaded: {problem}"),
            );
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
            replaces = "embedded://cyrus_render/terrain_surface.wesl"
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

    #[test]
    fn a_manifest_lists_script_patches_by_installed_native_name() {
        let manifest: Manifest = toml::from_str(
            r#"
            name = "Serpent"
            [[script-patch]]
            world = "ISLAND"
            actor = "SERPENT"
            remove = ["HideMe"]
            "#,
        )
        .unwrap();
        let [patch] = &manifest.script_patches[..] else {
            panic!("one patch expected");
        };
        assert!(patch.applies_to("island", "serpent"));
        assert_eq!(patch.remove, [cyrus_soup::Native::HideMe]);
    }

    #[test]
    fn a_script_patch_naming_an_unknown_native_does_not_parse() {
        let error = toml::from_str::<Manifest>(
            r#"
            name = "Bad"
            [[script-patch]]
            world = "ISLAND"
            actor = "SERPENT"
            remove = ["NoSuchNative"]
            "#,
        )
        .err()
        .expect("unknown natives are rejected");
        assert!(error.to_string().contains("NoSuchNative"), "{error}");
    }

    fn mod_folder(root: &Path, id: &str, manifest: &str) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mod.toml"), manifest).unwrap();
        dir
    }

    #[test]
    fn a_broken_mod_is_a_problem_and_the_others_still_load() {
        let root = std::env::temp_dir().join(format!("cyrus-mods-catalog-{}", std::process::id()));
        let good = mod_folder(&root, "good", "name = \"Good\"");
        let broken = mod_folder(&root, "broken", "name = [not a string");
        let bad_patch = mod_folder(
            &root,
            "bad-patch",
            "name = \"Bad\"
[[script-patch]]
world = \"ISLAND\"
actor = \"X\"
remove = [\"NoSuchNative\"]",
        );
        let catalog = Catalog::load(&[good, broken, bad_patch], &["good".into()]);
        std::fs::remove_dir_all(&root).unwrap();
        let ids: Vec<_> = catalog
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(ids, ["good"]);
        assert!(catalog.entries[0].active);
        assert_eq!(
            catalog
                .problems
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["bad-patch", "broken"]
        );
        assert!(catalog.problems["bad-patch"].contains("NoSuchNative"));
    }
}
