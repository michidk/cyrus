//! The installed ropes with terminal models (the CATACOMB torture frame and the NECRTOWR
//! chandelier) carry prepared terminal collision geometry and become transient candidates.
use anyhow::{Context, Result, ensure};
use cyrus_assets::{animation::Animations, install::GameInstall, script::Scripts};
use cyrus_script::{player::data::PlayerScene, script::ScriptWorld};

#[test]
fn installed_rope_terminals_are_transient_collision_candidates() -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (installed rope terminal models)");
        return Ok(());
    };
    let files = cyrus_assets::native::Files::open(&install)?;
    for (world, terminal) in [("NECRTOWR", "CC#CHAND"), ("CATACOMB", "CTTORT01")] {
        let player = format!("{world}.player");
        let animations = format!("{world}.animations");
        let scripts = format!("{world}.scripts");
        let prepared = cyrus_assets::game::prepare(&files, &player)
            .with_context(|| format!("{world}: prepare"))?;
        let scene = PlayerScene::decode(&prepared[&player])?;
        let data: Animations = serde_json::from_slice(&prepared[&animations])?;
        let scripts: Scripts = serde_json::from_slice(&prepared[&scripts])?;
        let mut script_world = ScriptWorld::prepare(&scripts, 1)?;
        script_world.prepare_spatial()?;
        script_world.prepare_ropes(&data)?;
        let ropes = script_world.ropes();
        let rope = ropes
            .iter()
            .position(|rope| {
                rope.end_model()
                    .is_some_and(|name| name.eq_ignore_ascii_case(terminal))
            })
            .with_context(|| format!("{world}: no rope ends in {terminal}"))?;
        let records = script_world.transient_collision_models(&scene)?;
        ensure!(
            records.len()
                == ropes
                    .iter()
                    .filter(|rope| rope.end_model().is_some())
                    .count(),
            "{world}: one transient record per terminal"
        );
        ensure!(
            records
                .iter()
                .all(|record| record.active && record.flags & 0x10 != 0 && record.model.is_some()),
            "{world}: every terminal is an active transient with a loaded model"
        );
        let models = scene
            .actor_collision
            .as_ref()
            .context("player scene without live collision geometry")?;
        ensure!(
            models.has_transient(rope),
            "{world} rope {rope}: terminal geometry exported"
        );
        ensure!(
            models.transient_radius(cyrus_collision::candidates::transient_model(rope))? > 0.0,
            "{world} rope {rope}: terminal radius"
        );
    }
    Ok(())
}
