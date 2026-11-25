use super::*;

#[test]
fn actor_prelight_entry_retains_loader_pose_and_restores_current_eligibility() -> anyhow::Result<()>
{
    use cyrus_assets::{animation::Animations, install::GameInstall, native::Files};
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
        });
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (actor prelight checkpoint)");
        return Ok(());
    };
    let files = Files::open(&install)?;
    let scripts: Scripts = serde_json::from_slice(
        &cyrus_assets::game::prepare(&files, "TAVERN.scripts")?["TAVERN.scripts"],
    )?;
    let animations: Animations = serde_json::from_slice(
        &cyrus_assets::game::prepare(&files, "TAVERN.animations")?["TAVERN.animations"],
    )?;
    let scene = cyrus_script::player::data::PlayerScene::decode(
        &cyrus_assets::game::prepare(&files, "TAVERN.player")?["TAVERN.player"],
    )?;
    let models = cyrus_collision::actor_models::ActorModelCollision::new(
        scene.collision.actor_models.as_ref().unwrap(),
    )?;
    let mut world = ScriptWorld::prepare(&scripts, 50)?;
    let actor = world.id(15)?;
    assert!(world.actor_prelight(actor).is_err());
    world.prepare_model_resources(&animations, &models)?;
    world.prepare_actor_prelighting()?;
    let initial = world.actor_prelight(actor)?.unwrap().clone();
    assert_eq!(initial.model(), "TV_FLOGS");
    assert_eq!(initial.position(), [1815.0, -74.0, 2860.0]);
    assert_eq!(initial.matrix()[0][2], 1.0);
    assert_eq!(initial.matrix()[2][0], -1.0);
    assert!(world.actor_prelight(world.id(6)?)?.is_none());
    world.set_actor_attribute(15, 122, 0)?;
    world.prepare_actor_prelighting()?;
    assert_eq!(world.actor_prelight(actor)?, Some(&initial));
    let sources = WorldSnapshotSources {
        identity_memo: None,
        scripts: &scripts,
        animations: &animations,
        collision: &scene.collision,
        player: Some(&scene),
        effects: None,
    };
    let saved = world.snapshot(&sources)?;
    let mut restored = ScriptWorld::from_snapshot(
        saved,
        &sources,
        WorldRestoreContext {
            generation: 51,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )?;
    assert!(restored.actor_prelight(actor).is_err());
    assert!(restored.actor_prelight(restored.id(15)?)?.is_none());
    restored.set_actor_attribute(15, 122, 1)?;
    let saved = restored.snapshot(&sources)?;
    let mut malformed = serde_json::to_value(&saved)?;
    malformed["actor_prelighting"]["15"]["position"][0] = serde_json::json!(0);
    let error = ScriptWorld::from_snapshot(
        serde_json::from_value(malformed)?,
        &sources,
        WorldRestoreContext {
            generation: 52,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .err()
    .expect("mismatched prelight owner must be rejected");
    assert!(
        error
            .to_string()
            .contains("saved actor prelight checkpoint differs"),
        "{error:#}"
    );

    let restored = ScriptWorld::from_snapshot(
        saved,
        &sources,
        WorldRestoreContext {
            generation: 52,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )?;
    assert_eq!(restored.actor_prelight(restored.id(15)?)?, Some(&initial));
    Ok(())
}
