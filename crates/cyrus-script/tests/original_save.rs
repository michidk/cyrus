//! Installed original scalars restored into an initialized off-live logical world.
#![cfg(feature = "original-save-import")]
use anyhow::{Context, Result, ensure};
use cyrus_assets::{hud::HudAsset, install::GameInstall, script::Scripts};
use cyrus_game::preferences::{PreferenceKey, PreferenceStore, Preferences};
use cyrus_script::script::{ScriptHud, ScriptWorld, original_save::OriginalSaveSubset};
use cyrus_soup::Attribute;
use rgpre::import::savegame::{SaveGame, load_save_slot, parse_save_flags, parse_save_inventory};
use std::path::{Path, PathBuf};

struct NoWrites;
impl PreferenceStore for NoWrites {
    fn load(&self, _: PreferenceKey) -> Result<Option<String>> {
        Ok(None)
    }
    fn save(&self, _: PreferenceKey, _: &str) -> Result<()> {
        anyhow::bail!("original import test must not write preferences")
    }
}

fn installed_slots() -> Result<Option<(GameInstall, Vec<PathBuf>)>> {
    let Some(root) = std::env::var_os("REDGUARD_ORIGINAL_SAVE_INSTALL")
        .or_else(|| std::env::var_os("REDGUARD_DIR"))
    else {
        eprintln!("SKIP original subset: no install supplied");
        return Ok(None);
    };
    let install = GameInstall::locate(Some(Path::new(&root)))?;
    let saves = if let Some(path) = std::env::var_os("REDGUARD_ORIGINAL_SAVE_DIR") {
        PathBuf::from(path)
    } else {
        let saves = std::fs::read_dir(&install.root)?
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("SAVEGAME")
            });
        let Some(saves) = saves else {
            eprintln!("SKIP original subset: install has no SAVEGAME directory");
            return Ok(None);
        };
        saves.path()
    };
    let paths = std::fs::read_dir(saves)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("SAVEGAME.")
                && entry.path().is_dir()
        })
        .map(|entry| entry.path())
        .collect::<Vec<PathBuf>>();
    if paths.is_empty() {
        eprintln!("SKIP original subset: no original slots installed");
        return Ok(None);
    }
    Ok(Some((install, paths)))
}

#[test]
fn installed_original_subset_restores_both_retained_slots() -> Result<()> {
    let Some((install, paths)) = installed_slots()? else {
        return Ok(());
    };
    let files = cyrus_assets::native::Files::open(&install)?;
    let prepared = cyrus_assets::game::prepare(&files, "hud/overlay.hud")?;
    let hud_data: HudAsset = serde_json::from_slice(&prepared["hud/overlay.hud"])?;
    for path in paths {
        assert_slot(&files, &hud_data, &path)?;
    }
    Ok(())
}

fn assert_slot(
    files: &cyrus_assets::native::Files,
    hud_data: &HudAsset,
    path: &Path,
) -> Result<()> {
    let before = std::fs::read(path.join("SAVEGAME.SAV"))?;
    let saved = load_save_slot(path)?;
    let name = saved.game.metadata.world_id.to_string();
    let key = format!("{name}.scripts");
    let prepared = cyrus_assets::game::prepare(files, &key)?;
    let scripts: Scripts = serde_json::from_slice(&prepared[&key])?;
    let subset = OriginalSaveSubset::decode(&saved.game, hud_data, &scripts.definition)
        .with_context(|| format!("slot {}", path.display()))?;
    let expected = (
        subset.world,
        subset.position_q8,
        subset.heading,
        subset.health,
    );
    let mut world = initialized_import_world(&scripts, &prepared, &name)?;
    let animations: cyrus_assets::animation::Animations =
        serde_json::from_slice(&prepared[&format!("{name}.animations")])?;
    let hud = ScriptHud::default();
    hud.configure(hud_data, &Preferences::new(NoWrites));
    let message = subset.apply(&mut world, &hud, &animations)?;
    ensure!(
        message.contains("unsupported") && message.contains("continuations"),
        "limited import must report unsupported state"
    );
    let player = world.selected_player().context("restored player absent")?;
    assert_eq!(world.world, expected.0);
    assert_eq!(world.spatial(player)?.local.position, expected.1);
    assert_eq!(
        world.spatial(player)?.local.angles[1],
        i32::from(expected.2)
    );
    assert_eq!(
        world.actors()[player.index].attributes()[Attribute::Health],
        expected.3
    );
    assert_eq!(
        world.globals().selected_health().unwrap().displayed(),
        i32::from(expected.3)
    );
    assert_flags_and_inventory(&saved.game, hud_data, &world, &hud)?;
    assert_support_owner(&saved.game, &world, player)?;
    assert_saved_idle_playback(&saved.game, &mut world)?;
    assert_eq!(std::fs::read(path.join("SAVEGAME.SAV"))?, before);
    // Wrong-world candidates reject before changing initialized state.
    let subset = OriginalSaveSubset::decode(&saved.game, hud_data, &scripts.definition)?;
    world.world = world.world.wrapping_add(1);
    let before = world.flags().to_vec();
    assert!(
        subset
            .apply(&mut world, &hud, &animations)
            .unwrap_err()
            .to_string()
            .contains("candidate world")
    );
    assert_eq!(world.flags(), before);
    assert_saved_fields(&saved.game, hud_data, &scripts)?;
    Ok(())
}

fn assert_saved_idle_playback(game: &SaveGame, world: &mut ScriptWorld) -> Result<()> {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    use rgpre::import::savegame::{SaveAnimationPlayback, parse_save_objects};
    let section = game
        .sections
        .iter()
        .find(|section| section.name == *b"SMOB")
        .unwrap();
    for object in parse_save_objects(&section.data)? {
        let (Some(header), Some(attributes)) = (object.animation_header, object.attributes) else {
            continue;
        };
        let playback = SaveAnimationPlayback::from(&header);
        if object.object_id < 0
            || playback.current < 0
            || i16::from(attributes[Attribute::PanicGroup.index()]) != playback.current
        {
            continue;
        }
        let Some(index) = world
            .actors()
            .iter()
            .position(|actor| actor.runtime_identifier == Some(object.runtime_identifier() as i16))
        else {
            continue;
        };
        let actor = world.id(index)?;
        let before = world.inspect(index, 1)["actors"][0]["animation"]["current_group"].clone();
        if before.is_null() {
            continue;
        }
        assert_eq!(
            before, playback.current,
            "saved idle actor {}",
            object.object_id
        );
        world.select_map_animation_default(actor, false, 1, &mut |_, _| Ok(()))?;
        world.advance(
            actor,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            100,
        )?;
        assert_eq!(
            world.inspect(index, 1)["actors"][0]["animation"]["current_group"],
            playback.current,
            "saved idle actor {} after first update",
            object.object_id
        );
    }
    Ok(())
}

fn assert_support_owner(
    game: &SaveGame,
    world: &ScriptWorld,
    player: cyrus_script::script::ScriptActorId,
) -> Result<()> {
    let section = game
        .sections
        .iter()
        .find(|section| section.name == *b"SMOB")
        .unwrap();
    let objects = rgpre::import::savegame::parse_save_objects(&section.data)?;
    let saved_player = objects
        .iter()
        .find(|object| object.object_id == -2)
        .unwrap();
    let identity = i32::from_le_bytes(saved_player.body[468..472].try_into()?);
    let support = world.model_support(player)?.actor;
    if identity == -1 {
        assert!(support.is_none());
        return Ok(());
    }
    let support = support.context("imported player support missing")?;
    let saved = objects
        .iter()
        .find(|object| object.object_id == identity)
        .unwrap();
    assert_eq!(
        world.actors()[support.index]
            .runtime_identifier
            .map(i32::from),
        Some(identity)
    );
    assert_eq!(world.spatial(support)?.local.position, saved.position_q8());
    let angles = std::array::from_fn(|axis| {
        i32::from_le_bytes(
            saved.body[288 + axis * 4..292 + axis * 4]
                .try_into()
                .unwrap(),
        )
    });
    assert_eq!(world.spatial(support)?.local.angles, angles);
    assert_eq!(
        world.model_presentation(support)?.unwrap().pose.position,
        saved.position_q8()
    );
    Ok(())
}

fn initialized_import_world(
    scripts: &Scripts,
    prepared: &std::collections::BTreeMap<String, Vec<u8>>,
    name: &str,
) -> Result<ScriptWorld> {
    let animations: cyrus_assets::animation::Animations =
        serde_json::from_slice(&prepared[&format!("{name}.animations")])?;
    let player: cyrus_assets::player::SceneData =
        serde_json::from_slice(&prepared[&format!("{name}.player")])?;
    let collision = player
        .collision
        .context("installed original import: collision missing")?;
    let models = cyrus_collision::actor_models::ActorModelCollision::new(
        collision
            .actor_models
            .as_ref()
            .context("installed original import: actor models missing")?,
    )?;
    let mut world = ScriptWorld::prepare(scripts, 1)?;
    world.prepare_spatial()?;
    let selected = world
        .selected_player()
        .context("installed import: player missing")?;
    world.initialize_player_spatial(selected, [0; 3], [0; 3])?;
    world.prepare_model_resources(&animations, &models)?;
    Ok(world)
}

fn assert_saved_fields(game: &SaveGame, hud_data: &HudAsset, scripts: &Scripts) -> Result<()> {
    // A deliberately edited heading must decode from the saved object,
    // independently of the initialized placement or camera.
    let mut edited = game.clone();
    let object_section = edited
        .sections
        .iter_mut()
        .find(|s| s.name == *b"SMOB")
        .unwrap();
    let mut objects = rgpre::import::savegame::parse_save_objects(&object_section.data)?;
    let player = objects
        .iter_mut()
        .find(|object| object.object_id == -2)
        .unwrap();
    player.body[292..296].copy_from_slice(&(-1i32).to_le_bytes());
    object_section.data = rgpre::import::savegame::encode_save_objects(&objects)?;
    assert_eq!(
        OriginalSaveSubset::decode(&edited, hud_data, &scripts.definition)?.heading,
        2047
    );
    let flag_section = edited
        .sections
        .iter_mut()
        .find(|s| s.name == *b"SVGF")
        .unwrap();
    let mut flags = parse_save_flags(&flag_section.data)?;
    flags.states[0] = 255;
    flag_section.data = rgpre::import::savegame::encode_save_flags(&flags)?;
    assert!(
        OriginalSaveSubset::decode(&edited, hud_data, &scripts.definition)
            .err()
            .context("unsupported flag state was accepted")?
            .to_string()
            .contains("state 255 unsupported")
    );
    // A transformed player is explicitly outside this first subset.
    let mut unsupported = game.clone();
    let globals = &mut unsupported
        .sections
        .iter_mut()
        .find(|s| s.name == *b"SVG2")
        .unwrap()
        .data;
    globals[1622..1626].copy_from_slice(&1i32.to_le_bytes());
    assert!(
        OriginalSaveSubset::decode(&unsupported, hud_data, &scripts.definition)
            .err()
            .context("transformed player was accepted")?
            .to_string()
            .contains("transformed player unsupported")
    );
    Ok(())
}

fn assert_flags_and_inventory(
    game: &SaveGame,
    hud_data: &HudAsset,
    world: &ScriptWorld,
    hud: &ScriptHud,
) -> Result<()> {
    let section = |name| &game.sections.iter().find(|s| s.name == name).unwrap().data;
    let flags = parse_save_flags(section(*b"SVGF"))?;
    assert_eq!(world.flags(), flags.values);
    let inventory = parse_save_inventory(section(*b"SVIT"), hud_data.inventory.len() - 1)?;
    let inspect = hud.inspect();
    for (&id, item) in hud_data.inventory.keys().zip(&inventory.items) {
        assert_eq!(
            inspect["inventory"]["counts"][id.to_string()],
            item.player_total
        );
    }
    Ok(())
}
