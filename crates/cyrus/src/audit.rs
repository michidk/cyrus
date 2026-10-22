//! Headless WORLD.INI audit through the normal preload, export and script preparation paths.
use anyhow::{Context, Result};
use bevy::prelude::{default, App, Assets, MinimalPlugins};
use cyrus_assets::{
    animation::Animations,
    install::GameInstall,
    loading::{Preloaded, Progress},
    script::Scripts,
    source::PreparationDiagnostic,
};
use cyrus_game::script::{PlaybackDiagnostics, ScriptActors, ScriptPlugin, ScriptStatus};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap, path::Path};

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    scope: &'static str,
    preload: Preloaded,
    preload_diagnostics: Vec<PreloadDiagnostic>,
    worlds: Vec<WorldReport>,
}
#[derive(Serialize)]
struct PreloadDiagnostic {
    /// A coverage scan does not establish which world references a file.
    world: Option<u32>,
    #[serde(flatten)]
    diagnostic: PreparationDiagnostic,
}
#[derive(Default, Serialize)]
struct WorldReport {
    world: u32,
    map: String,
    palette: String,
    exported: bool,
    script_status: serde_json::Value,
    /// Authored occurrence counts, separate from runtime observations.
    script_inventory: Option<cyrus_assets::script::inventory::Inventory>,
    actors: usize,
    animations_initialized: usize,
    animations_not_initialized: usize,
    counts: BTreeMap<String, usize>,
    diagnostics: Vec<PreparationDiagnostic>,
}
impl WorldReport {
    fn issue(&mut self, category: &str, file: &str, field: &str, reason: String) {
        if matches!(
            category,
            "runtime_failure" | "script_initialization" | "animation_initialization"
        ) && reason.contains("unsupported")
        {
            self.issue("unsupported_operation", file, field, reason.clone());
        }
        self.diagnostics.push(PreparationDiagnostic {
            category: category.into(),
            file: file.into(),
            field: field.into(),
            reason,
            count: 1,
        });
    }
    fn finish(&mut self) {
        self.diagnostics.sort();
        self.diagnostics.dedup();
        for category in [
            "skipped_placement",
            "referenced_asset",
            "animation_initialization",
            "script_initialization",
            "unsupported_operation",
            "script_decode",
            "runtime_failure",
            "export_failure",
        ] {
            self.counts.insert(category.into(), 0);
        }
        for issue in &self.diagnostics {
            *self.counts.entry(issue.category.clone()).or_default() += issue.count;
        }
    }

    fn has_preparation_failure(&self) -> bool {
        [
            "animation_initialization",
            "script_initialization",
            "script_decode",
            "unsupported_operation",
            "runtime_failure",
            "export_failure",
        ]
        .iter()
        .any(|category| self.counts.get(*category).copied().unwrap_or_default() != 0)
    }
}

/// Reports observations from loading, not a census of reachable gameplay operations.
/// Generated reports and exported original content remain below `magic/`.
pub fn run(install: &GameInstall) -> Result<()> {
    let directory = Path::new("magic/artifacts/world-audit");
    let cache = directory.join("cache");
    std::fs::create_dir_all(&cache)?;
    install.cache_path(&cache)?;
    let (sender, receiver) = std::sync::mpsc::channel();
    cyrus_assets::loading::preload(&install.root, &sender);
    drop(sender);
    let mut preload = receiver
        .into_iter()
        .find_map(|message| match message {
            Progress::Done(report) => Some(*report),
            _ => None,
        })
        .context("preload did not return its coverage report")?;
    preload.failed.sort();
    let entries = cyrus_assets::scene::read_worlds(install)?;
    anyhow::ensure!(
        !entries.entries.is_empty(),
        "WORLD.INI field world_map: no worlds"
    );
    let mut report = Report {
        schema_version: 1,
        scope: "Verified loading observations only. Normal script preparation runs; this command does not invent actor eligibility, execute native calls, or supply animation ticks. Uninitialized animations and unexercised operations are not successful checks.",
        preload_diagnostics: preload
            .failed
            .iter()
            .map(|(file, reason)| PreloadDiagnostic {
                world: None,
                diagnostic: PreparationDiagnostic {
                    category: "preload_parse".into(),
                    file: file.clone(),
                    field: "preload.parse".into(),
                    reason: reason.clone(),
                    count: 1,
                },
            })
            .collect(),
        preload,
        worlds: Vec::new(),
    };
    for entry in entries.entries {
        let mut world = WorldReport {
            world: entry.index,
            map: entry.map.replace('\\', "/"),
            palette: entry.palette,
            ..default()
        };
        let name = entry.index.to_string();
        let output = cache.join(format!("{name}.glb"));
        let observed = RefCell::new(Vec::new());
        let stage = RefCell::new(String::from("source"));
        let result = cyrus_assets::scene::export_scene_observing(
            install,
            &name,
            &output,
            &|_, _, label| *stage.borrow_mut() = label.into(),
            &|issue| observed.borrow_mut().push(issue),
        );
        world.diagnostics = observed.into_inner();
        match result {
            Ok(()) => {
                world.exported = true;
                if let Err(error) = prepare_runtime(&output, &mut world) {
                    let map = world.map.clone();
                    world.issue(
                        "script_initialization",
                        &map,
                        "runtime.prepare",
                        format!("{error:#}"),
                    );
                }
            }
            Err(error) => {
                let map = world.map.clone();
                world.issue(
                    "export_failure",
                    &map,
                    &format!("export.{}", stage.into_inner()),
                    format!("{error:#}"),
                );
            }
        }
        world.finish();
        println!("{}", summary(&world));
        report.worlds.push(world);
    }
    report.worlds.sort_by_key(|world| world.world);
    let mut text = format!(
        "{}\nPreload: {} parsed, {} failed\n",
        report.scope,
        report.preload.parsed,
        report.preload.failed.len()
    );
    for (file, reason) in &report.preload.failed {
        text.push_str(&format!(
            "  world * file {file} field preload.parse: {reason}\n"
        ));
    }
    for world in &report.worlds {
        text.push_str(&summary(world));
        text.push('\n');
        for issue in &world.diagnostics {
            text.push_str(&format!(
                "  world {} file {} field {} [{} x{}]: {}\n",
                world.world, issue.file, issue.field, issue.category, issue.count, issue.reason
            ));
        }
    }
    cyrus_assets::native::publish(
        &directory.join("report.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    cyrus_assets::native::publish(&directory.join("report.txt"), text.as_bytes())?;
    println!("Reports: {}", directory.display());
    anyhow::ensure!(
        report.worlds.iter().all(|world| world.exported),
        "world audit contains export failures; see report.json and report.txt"
    );
    anyhow::ensure!(
        report
            .worlds
            .iter()
            .all(|world| !world.has_preparation_failure()),
        "world audit contains preparation or runtime failures; see report.json and report.txt"
    );
    Ok(())
}
fn summary(world: &WorldReport) -> String {
    format!(
        "world {} {}: exported={} skipped={} asset_references={} animation_init_failures={} script_init_failures={} unsupported={} export_failures={} animations_uninitialized={} script_decode={} runtime_failures={}",
        world.world,
        world.map,
        world.exported,
        world.counts["skipped_placement"],
        world.counts["referenced_asset"],
        world.counts["animation_initialization"],
        world.counts["script_initialization"],
        world.counts["unsupported_operation"],
        world.counts["export_failure"],
        world.animations_not_initialized,
        world.counts["script_decode"],
        world.counts["runtime_failure"]
    )
}
fn prepare_runtime(path: &Path, report: &mut WorldReport) -> Result<()> {
    let animations: Animations =
        serde_json::from_slice(&std::fs::read(path.with_extension("animations"))?)?;
    animations.require_version()?;
    let scripts: Scripts = serde_json::from_slice(&std::fs::read(path.with_extension("scripts"))?)?;
    scripts.validate()?;
    report.script_inventory = Some(cyrus_assets::script::inventory::inspect(&scripts));
    let map = report.map.clone();
    for (field, reason) in &animations.unsupported {
        report.issue(
            "referenced_asset",
            field,
            &format!("referenced_by.{map}.animations.models"),
            reason.clone(),
        );
    }
    for (field, reason) in &scripts.unsupported {
        report.issue(
            "script_decode",
            &map,
            &format!("scripts.{field}"),
            reason.clone(),
        );
    }
    // Use the production preparation systems, with already exported assets and no window.
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        cyrus_world::animation::AnimationPlugin,
        cyrus_world::lifecycle::WorldPlugin,
        ScriptPlugin,
    ));
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(animations);
    let scripts = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(scripts);
    app.insert_resource(cyrus_world::lifecycle::PreparedWorld {
        requested_name: report.world.to_string(),
        world: Some(report.world),
        animations,
        scripts,
        player_path: "unused.player".into(),
        scene_path: "unused.glb".into(),
        environment_path: "unused.environment".into(),
    });
    app.update();
    app.update();
    report.script_status = serde_json::to_value(app.world().resource::<ScriptStatus>())?;
    if let ScriptStatus::Failed { error } = app.world().resource::<ScriptStatus>() {
        report.issue(
            "script_initialization",
            &map,
            "scripts.prepare",
            error.clone(),
        );
    }
    if let Some(world) = &app.world().resource::<ScriptActors>().current {
        report.actors = world.actors().len();
        for (index, actor) in world.actors().iter().enumerate() {
            if let Some(reason) = actor.unavailable() {
                report.issue(
                    "script_initialization",
                    &map,
                    &format!("scripts.instances[{index}]"),
                    reason.into(),
                );
            }
            if let Some(reason) = actor.runtime().and_then(|runtime| runtime.failure()) {
                report.issue(
                    "runtime_failure",
                    &map,
                    &format!("scripts.instances[{index}].runtime"),
                    reason.into(),
                );
            }
            if let Some(reason) = actor.animation_failure() {
                report.issue(
                    "animation_initialization",
                    &map,
                    &format!("scripts.instances[{index}].animation"),
                    reason.into(),
                );
            }
            if actor.animation_initialized() {
                report.animations_initialized += 1;
            } else {
                report.animations_not_initialized += 1;
            }
        }
    }
    for issue in &app.world().resource::<PlaybackDiagnostics>().0 {
        report.issue(
            "runtime_failure",
            &map,
            &format!("actors.{}.{}", issue.actor, issue.stage),
            issue.reason.clone(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_report_deduplicates_observations_but_counts_placements() {
        let mut report = WorldReport::default();
        report.issue(
            "skipped_placement",
            "maps/test.rgm",
            "MPSO[4].model_name",
            "absent".into(),
        );
        report.diagnostics[0].count = 3;
        report.diagnostics.push(report.diagnostics[0].clone());
        report.issue(
            "referenced_asset",
            "TEXBSI.001",
            "images[2]",
            "missing".into(),
        );
        report.finish();
        assert_eq!(report.counts["skipped_placement"], 3);
        assert_eq!(report.counts["referenced_asset"], 1);
        assert_eq!(report.counts["script_initialization"], 0);
        assert_eq!(report.diagnostics.len(), 2);
    }

    #[test]
    fn preparation_failures_make_audit_unsuccessful() {
        let mut report = WorldReport::default();
        report.issue(
            "script_initialization",
            "maps/test.rgm",
            "scripts.prepare",
            "failed".into(),
        );
        report.finish();
        assert!(report.has_preparation_failure());

        let mut report = WorldReport::default();
        report.issue("referenced_asset", "WATERSND", "models", "missing".into());
        report.finish();
        assert!(!report.has_preparation_failure());
    }
}
