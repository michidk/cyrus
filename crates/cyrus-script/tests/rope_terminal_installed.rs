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

        // Restore a controlled nonzero swing outside the window, then advance through
        // the public world loop. This checks simulation lifetime, not only candidates.
        let mut saved = serde_json::to_value(restored.snapshot(&sources)?)?;
        saved["ropes"][rope]["amplitude"] = 1024.into();
        saved["ropes"][rope]["step"] = 200.into();
        let mut resumed = ScriptWorld::from_snapshot(
            serde_json::from_value(saved)?,
            &sources,
            WorldRestoreContext {
                generation: 3,
                globals: Default::default(),
                samples: &Default::default(),
            },
        )?;
        let frozen = resumed.ropes()[rope].state();
        let advance = |world: &mut ScriptWorld, view| -> Result<()> {
            world.update_ordinary(
                &data,
                view,
                cyrus_script::script::OrdinaryInputs {
                    scene: Some(&scene),
                    scripts: cyrus_script::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
        };
        advance(&mut resumed, [view[0] + 512, view[1]])?;
        ensure!(
            resumed.ropes()[rope].state() == frozen,
            "{world}: excluded swing frozen"
        );
        placement.camera_position = base;
        resumed.restore_selected_player_placement(&placement)?;
        advance(&mut resumed, [view[0] + 512, view[1]])?;
        ensure!(
            resumed.ropes()[rope].state() == frozen,
            "{world}: same-cell swing frozen"
        );
        advance(&mut resumed, view)?;
        ensure!(
            resumed.ropes()[rope].state() != frozen,
            "{world}: rebuilt window resumes swing"
        );
        ensure!(
            resumed.ropes()[rope].amplitude <= resumed.ropes()[rope].maximum,
            "{world}: resumed amplitude clamped to installed maximum"
        );

        // Installed terminals publish their incoming swing pose before oscillator advancement.
        let mut saved = serde_json::to_value(resumed.snapshot(&sources)?)?;
        saved["ropes"][rope]["amplitude"] = 12.into();
        saved["ropes"][rope]["step"] = 200.into();
        saved["ropes"][rope]["phase"] = 0.into();
        saved["ropes"][rope]["geometry"] = serde_json::Value::Null;
        let mut published = ScriptWorld::from_snapshot(
            serde_json::from_value(saved)?,
            &sources,
            WorldRestoreContext {
                generation: 4,
                globals: Default::default(),
                samples: &Default::default(),
            },
        )?;
        let expected = published.ropes()[rope].end_collision_pose();
        advance(&mut published, view)?;
        ensure!(
            published.ropes()[rope].step != 200,
            "{world}: oscillator advanced"
        );
        ensure!(
            published.ropes()[rope].end_collision_pose() == expected,
            "{world}: terminal collision retains incoming geometry"
        );
        let record = published
            .transient_collision_models(&scene)?
            .into_iter()
            .find(|record| record.model.is_some_and(|model| model.instance == key))
            .context("published terminal missing")?;
        ensure!(
            Some(record.model.context("terminal bounds missing")?.position)
                == expected.map(|pose| pose.0),
            "{world}: production candidate uses published bounds"
        );
        let snapshot = published.snapshot(&sources)?;
        let rebound = ScriptWorld::from_snapshot(
            snapshot,
            &sources,
            WorldRestoreContext {
                generation: 5,
                globals: Default::default(),
                samples: &Default::default(),
            },
        )?;
        ensure!(
            rebound.ropes()[rope].end_collision_pose() == expected,
            "{world}: restored geometry must not advance to the saved oscillator"
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
