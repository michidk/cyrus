//! The installed ropes with terminal models (the CATACOMB torture frame and the NECRTOWR
//! chandelier) carry prepared terminal collision geometry and become transient candidates.
use anyhow::{Context, Result, ensure};
use cyrus_assets::{animation::Animations, install::GameInstall, script::Scripts};
use cyrus_collision::candidates::{ModelInstance, transient_model};
use cyrus_script::{
    player::data::PlayerScene,
    script::{ScriptWorld, WorldRestoreContext, WorldSnapshotSources, camera::CameraPose},
};

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
        let base = cyrus_game::rope::engine_position(ropes[rope].position)
            .map(|axis| (axis.round() as i32) * 256);
        let key = ModelInstance::Runtime(transient_model(rope));
        let contains_terminal = |world: &ScriptWorld| -> Result<bool> {
            Ok(world
                .transient_collision_models(&scene)?
                .iter()
                .any(|record| record.model.is_some_and(|model| model.instance == key)))
        };
        let selected = script_world.selected_player().context("no player")?;
        script_world.initialize_player_spatial(selected, base, [0; 3])?;
        script_world.initialize_camera_pose(CameraPose {
            position: base,
            angles: [0; 3],
        })?;
        let view = [base[0] >> 8, base[2] >> 8];
        script_world.ordinary_actors(view)?;
        ensure!(contains_terminal(&script_world)?, "{world}: near terminal");

        let mut placement = script_world.selected_player_placement()?;
        placement.camera_position[1] -= 4096 * 256;
        script_world.restore_selected_player_placement(&placement)?;
        script_world.ordinary_actors(view)?;
        ensure!(
            contains_terminal(&script_world)?,
            "{world}: same-cell movement retains the terminal"
        );

        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: Some(&scene),
            scripts: &scripts,
            animations: &data,
            collision: &scene.collision,
        };
        let snapshot =
            serde_json::from_slice(&serde_json::to_vec(&script_world.snapshot(&sources)?)?)?;
        let mut restored = ScriptWorld::from_snapshot(
            snapshot,
            &sources,
            WorldRestoreContext {
                generation: 2,
                globals: Default::default(),
                samples: &Default::default(),
            },
        )?;
        restored.ordinary_actors(view)?;
        ensure!(
            contains_terminal(&restored)?,
            "{world}: restored window retains the terminal"
        );

        placement.camera_position[0] += 512 * 256;
        restored.restore_selected_player_placement(&placement)?;
        restored.ordinary_actors([view[0] + 512, view[1]])?;
        ensure!(
            !contains_terminal(&restored)?,
            "{world}: next cell drops the terminal"
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
