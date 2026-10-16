//! Native-only WebAssembly mods: discovery, activation, hot reload and the hook API.
//!
//! A mod is a folder under `cyrus-config/mods/<id>/` with a `mod.toml`
//! (`name`, `description`) and any of:
//!
//! - `mod.wasm`: a WebAssembly component run by wasvy against `wit/cyrus.wit`.
//! - `water_surface.wesl`: a replacement for cyrus-render's water shading library.
//!
//! Without an active mod every hook keeps the original behaviour. Activation is
//! saved to `cyrus-config/mods.json`; the F1 debug menu lists the mods next to it.
//! Active mod files are polled and reloaded when they change.
mod api;

use anyhow::Context;
use bevy::asset::io::AssetSourceBuilder;
// Explicit imports: the Bevy prelude's `ContextExt` collides with `anyhow::Context`.
use bevy::asset::{AssetApp, AssetServer, Assets, Handle};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::prelude::{Entity, IntoScheduleConfigs, Local, Res, ResMut, Resource};
use bevy::log::{error, info, warn};
use bevy::prelude::{App, Plugin, PreUpdate, Real, Result, Startup, Time, Update, default};
use bevy::shader::Shader;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use wasvy::prelude::*;

wasvy::auto_host_components! {
    path = "wit",
    world = "cyrus:mods/host",
    module = cyrus_bindings,
}

const MODS_DIR: &str = "cyrus-config/mods";
const ACTIVE_FILE: &str = "cyrus-config/mods.json";
const WATER_SHADER: &str = "embedded://cyrus_render/water_surface.wesl";

/// Registers the `mods://` asset source; call before the asset plugin is added.
pub fn register_source(app: &mut App) {
    app.register_asset_source(
        "mods",
        AssetSourceBuilder::platform_default(&absolute(MODS_DIR), None),
    );
}

pub struct ModsPlugin;

impl Plugin for ModsPlugin {
    fn build(&self, app: &mut App) {
        let catalog = Catalog::discover().unwrap_or_else(|error| {
            error!("mods: {error:#}");
            Catalog::default()
        });
        app.add_plugins(ModLoaderPlugin::default().add_functionality(add_components_to_linker))
            .register_type::<Vec<f32>>()
            .insert_resource(catalog)
            .init_resource::<cyrus_render::WaterWaves>()
            .init_resource::<cyrus_render::camera::CameraTakeover>()
            .add_systems(Startup, api::spawn_host)
            .add_systems(PreUpdate, api::exchange)
            .add_systems(Update, (watch, apply).chain())
            .add_systems(
                EguiPrimaryContextPass,
                window.run_if(|menu: Res<cyrus_debug::DebugMenu>| menu.open),
            );
    }
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    description: String,
}

struct Entry {
    id: String,
    manifest: Manifest,
    active: bool,
    has_wasm: bool,
    has_shader: bool,
    /// The running wasvy instance while active.
    instance: Option<Entity>,
    modified: Option<SystemTime>,
}

#[derive(Resource, Default)]
struct Catalog {
    entries: Vec<Entry>,
    /// The built-in water library, kept to restore it when no mod supplies one.
    original_water: Option<Shader>,
    /// Whether a mod's water library is installed.
    water_replaced: bool,
    status: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    active: Vec<String>,
}

impl Catalog {
    fn discover() -> anyhow::Result<Self> {
        let saved: Saved = match std::fs::read(ACTIVE_FILE) {
            Ok(bytes) => serde_json::from_slice(&bytes).context(ACTIVE_FILE)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Saved::default(),
            Err(error) => return Err(error).context(ACTIVE_FILE),
        };
        let mut entries = Vec::new();
        let dirs = match std::fs::read_dir(MODS_DIR) {
            Ok(dirs) => dirs,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error).context(MODS_DIR),
        };
        for dir in dirs {
            let dir = dir.context(MODS_DIR)?.path();
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
            entries.push(Entry {
                active: saved.active.contains(&id),
                has_wasm: dir.join("mod.wasm").is_file(),
                has_shader: dir.join("water_surface.wesl").is_file(),
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
        std::fs::write(ACTIVE_FILE, serde_json::to_vec_pretty(&saved)?).context(ACTIVE_FILE)
    }
}

fn absolute(path: &str) -> String {
    std::path::absolute(path).map_or_else(|_| path.to_owned(), |p| p.to_string_lossy().into_owned())
}

fn mod_file(id: &str, file: &str) -> PathBuf {
    Path::new(MODS_DIR).join(id).join(file)
}

/// Starts and stops wasm instances and installs the active water shader.
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
    // The water shader is one slot: the first active mod in id order supplies it.
    let mut suppliers = catalog
        .entries
        .iter()
        .filter(|entry| entry.active && entry.has_shader);
    let supplier = suppliers.next().map(|entry| entry.id.clone());
    if let Some(other) = suppliers.next() {
        warn!(
            "mods: {} and {} both replace the water shader; using {}",
            supplier.as_deref().unwrap_or_default(),
            other.id,
            supplier.as_deref().unwrap_or_default()
        );
    }
    let handle: Handle<Shader> = asset_server.load(WATER_SHADER);
    // The embedded library may not have loaded yet; try again next frame.
    let Some(current) = shaders.get(&handle).cloned() else {
        *shader_pending = true;
        return;
    };
    *shader_pending = false;
    let original = catalog.original_water.get_or_insert(current).clone();
    if supplier.is_none() && !catalog.water_replaced {
        return;
    }
    catalog.water_replaced = supplier.is_some();
    let shader = match &supplier {
        Some(id) => match std::fs::read_to_string(mod_file(id, "water_surface.wesl")) {
            Ok(source) => Shader::from_wesl(source, WATER_SHADER),
            Err(error) => {
                catalog.status = Some(format!("{id}: water_surface.wesl: {error}"));
                original
            }
        },
        None => original,
    };
    match shaders.insert(&handle, shader) {
        Ok(()) => info!(
            "mods: water shader from {}",
            supplier.as_deref().unwrap_or("the original")
        ),
        Err(error) => error!("mods: water shader: {error}"),
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
        let modified = ["mod.wasm", "water_surface.wesl"]
            .iter()
            .filter_map(|file| std::fs::metadata(mod_file(&entry.id, file)).ok())
            .filter_map(|meta| meta.modified().ok())
            .max();
        if entry.modified.is_some() && modified != entry.modified {
            info!("mods: reloading {}", entry.id);
            if entry.instance.is_some() {
                asset_server.reload(format!("mods://{}/mod.wasm", entry.id));
            }
            shader_changed |= entry.has_shader;
        }
        entry.modified = modified;
    }
    if shader_changed {
        catalog.set_changed();
    }
}

fn window(mut contexts: EguiContexts, mut catalog: ResMut<Catalog>) -> Result {
    egui::Window::new("Mods").show(contexts.ctx_mut()?, |ui| {
        if catalog.entries.is_empty() {
            ui.label(format!(
                "No mods in {MODS_DIR}. `just mods` installs the bundled ones."
            ));
        }
        let mut toggled = false;
        for entry in &mut catalog.bypass_change_detection().entries {
            toggled |= ui
                .checkbox(&mut entry.active, &entry.manifest.name)
                .on_hover_text(&entry.manifest.description)
                .changed();
        }
        if toggled {
            catalog.set_changed();
            catalog.status = catalog.save().err().map(|error| format!("{error:#}"));
        }
        if let Some(status) = &catalog.status {
            ui.colored_label(egui::Color32::LIGHT_RED, status);
        }
    });
    Ok(())
}
