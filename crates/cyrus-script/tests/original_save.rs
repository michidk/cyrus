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
        assert_slot(&files, &hud_data, &path, &install)?;
    }
    Ok(())
}

fn assert_slot(
    files: &cyrus_assets::native::Files,
    hud_data: &HudAsset,
    path: &Path,
    install: &GameInstall,
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
    // The preceding wrong-world control deliberately changed only this identity.
    world.world = expected.0;
    assert_bound_pose_owners(&saved.game, &mut world, &animations, install)?;
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

// The allocation table comes from the installed map and exported geometry, never
// the saved handle values. This is component coverage, not session admission.
fn installed_model_allocations(
    game: &SaveGame,
    animations: &cyrus_assets::animation::Animations,
    install: &GameInstall,
) -> Result<Vec<rgpre::import::savegame::SaveModelAllocation>> {
    let worlds = cyrus_assets::scene::read_worlds(install)?;
    let entry = worlds
        .entries
        .iter()
        .find(|entry| entry.index == game.metadata.world_id)
        .context("saved world not installed")?;
    let filename = entry
        .map
        .rsplit(['/', '\\'])
        .next()
        .context("empty map name")?;
    let path = cyrus_assets::scene::find_file(&install.root.join("maps"), filename)
        .context("installed map absent")?;
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path)?)?;
    let allocations = rgpre::import::savegame::SaveModelAllocations::from_world(
        &map,
        &["CYRUS", "GREMLIN"],
        |source| {
            let file = source.rsplit(['/', '\\']).next().unwrap_or(source);
            let key = file
                .rsplit_once('.')
                .map_or(file, |(stem, _)| stem)
                .to_ascii_uppercase();
            let geometry = animations.models.get(&key).ok_or_else(|| {
                rgpre::error::Error::Parse(format!("installed geometry {key} missing"))
            })?;
            Ok((key, geometry.positions.len()))
        },
    )?;
    Ok(allocations.entries().to_vec())
}

fn assert_bound_pose_owners(
    game: &SaveGame,
    world: &mut ScriptWorld,
    animations: &cyrus_assets::animation::Animations,
    install: &GameInstall,
) -> Result<Vec<rgpre::import::savegame::SaveModelAllocation>> {
    use cyrus_script::script::ScriptActorSource;
    use rgpre::import::savegame::parse_save_objects;
    let allocations = installed_model_allocations(game, animations, install)?;
    let objects = parse_save_objects(
        &game
            .sections
            .iter()
            .find(|s| s.name == *b"SMOB")
            .context("SMOB missing")?
            .data,
    )?;
    let bindings = objects
        .iter()
        .filter(|object| object.animation_header.is_some())
        .map(|object| {
            let mut matches =
                world
                    .actors()
                    .iter()
                    .enumerate()
                    .filter(|(_, actor)| match actor.source {
                        ScriptActorSource::Player => object.object_id == -2,
                        ScriptActorSource::AlternatePlayer => object.object_id == -3,
                        ScriptActorSource::Map(_) => {
                            object.object_id >= 0
                                && actor.runtime_identifier.is_some_and(|identity| {
                                    identity as u16 == object.runtime_identifier()
                                })
                        }
                        _ => false,
                    });
            let (index, _) = matches.next().context("saved pose native owner absent")?;
            ensure!(
                matches.next().is_none(),
                "ambiguous saved pose native owner"
            );
            Ok((world.id(index)?, object))
        })
        .collect::<Result<Vec<_>>>()?;
    world.restore_original_bound_animations(&bindings, allocations.as_slice(), animations)?;
    for (id, object) in &bindings {
        let pose = object.pose();
        let selected = pose.current.bind(allocations.as_slice())?;
        let reference = world.animation_frame(*id)?;
        ensure!(
            selected.is_some() == reference.is_some(),
            "selected pose presence differs"
        );
        if let (Some(bound), Some(reference)) = (selected, reference) {
            let resolved = match world.actors()[id.index].source {
                ScriptActorSource::Map(_) => animations.resolve_map_frame(id.index, reference)?,
                _ => animations.resolve_frame(world.actors()[id.index].name(), reference)?,
            };
            let allocation = &allocations.as_slice()[bound.allocation];
            ensure!(
                resolved.model == allocation.source && resolved.frame == bound.frame,
                "bound native pose differs from saved installed allocation"
            );
        }
    }
    eprintln!(
        "restored {} installed animation owners with independently bound poses",
        bindings.len()
    );
    Ok(allocations.as_slice().to_vec())
}

#[test]
fn retained_execution_population_restores_without_reissuing_callbacks() -> Result<()> {
    use cyrus_script::script::{DialogueState, OriginalExecutionOwners, WorldSnapshotSources};
    use rgpre::import::savegame::{
        parse_save_code_owners, parse_save_objects, parse_save_script_locals, parse_save_shells,
    };
    let Some((install, paths)) = installed_slots()? else {
        return Ok(());
    };
    let files = cyrus_assets::native::Files::open(&install)?;
    for path in paths {
        let saved = load_save_slot(&path)?;
        let game = &saved.game;
        let name = game.metadata.world_id.to_string();
        let key = format!("{name}.scripts");
        let prepared = cyrus_assets::game::prepare(&files, &key)?;
        let scripts: Scripts = serde_json::from_slice(&prepared[&key])?;
        let animations: cyrus_assets::animation::Animations =
            serde_json::from_slice(&prepared[&format!("{name}.animations")])?;
        let player: cyrus_assets::player::SceneData =
            serde_json::from_slice(&prepared[&format!("{name}.player")])?;
        let collision = player
            .collision
            .context("retained execution collision source absent")?;
        let mut world = initialized_import_world(&scripts, &prepared, &name)?;
        let allocations = assert_bound_pose_owners(game, &mut world, &animations, &install)?;
        *world
            .globals_mut()
            .dialogue_mut()
            .context("dialogue service absent")? =
            DialogueState::from_original_load(&scripts.dialogue)?;
        let section = |name| -> Result<&[u8]> {
            Ok(&game
                .sections
                .iter()
                .find(|s| s.name == name)
                .context("original execution section absent")?
                .data)
        };
        let objects = parse_save_objects(section(*b"SMOB")?)?;
        let codes = parse_save_code_owners(section(*b"SMCO")?)?;
        let shells = parse_save_shells(section(*b"SMSH")?)?;
        let locals = parse_save_script_locals(section(*b"SVRG")?)?;
        let maps = (0..animations.metadata["walk_node_maps"]
            .as_array()
            .context("walk maps absent")?
            .len())
            .map(u32::try_from)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let channels = (0u8..16).map(|slot| (slot, slot)).collect();
        let sources = WorldSnapshotSources {
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &animations,
            collision: &collision,
            identity_memo: None,
        };
        world.prepare_navigation(&animations)?;
        world.prepare_ropes(&animations)?;
        world.prepare_markers(&animations)?;
        let rope_ids = (1..=world.ropes().len())
            .map(u32::try_from)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        world = world.with_original_world(
            game,
            &sources,
            cyrus_script::script::OriginalWorldAllocations {
                models: &allocations,
                walk_maps: &maps,
                ropes: &rope_ids,
                ambient_channels: &channels,
            },
            section(*b"SVG2")?,
        )?;
        let before = serde_json::to_value(world.snapshot(&sources)?)?;
        let mut invalid_shells = shells.clone();
        // Shells are visited after every actor and map-code owner. Failure here
        // must preserve the complete world, including all earlier native tasks.
        invalid_shells[0].body[0x1a..0x1e].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(
            world
                .restore_original_execution(
                    OriginalExecutionOwners {
                        actors: &objects,
                        codes: &codes,
                        shells: &invalid_shells,
                        locals: &locals,
                        walk_map_ids: &maps,
                        ambient_channels: &channels
                    },
                    &scripts,
                    &animations
                )
                .is_err()
        );
        assert_eq!(serde_json::to_value(world.snapshot(&sources)?)?, before);
        world.restore_original_execution(
            OriginalExecutionOwners {
                actors: &objects,
                codes: &codes,
                shells: &shells,
                locals: &locals,
                walk_map_ids: &maps,
                ambient_channels: &channels,
            },
            &scripts,
            &animations,
        )?;
        let programs = scripts
            .actors
            .values()
            .map(|definition| {
                (
                    cyrus_soup::CodeOrigin(definition.code_origin),
                    std::sync::Arc::new(definition.program.clone()),
                )
            })
            .collect();
        let waiting = world
            .actors()
            .iter()
            .enumerate()
            .find_map(|(index, actor)| {
                let runtime = actor.runtime()?;
                let vm = runtime.snapshot(&programs).ok()?;
                let calls = vm.task_calls().ok()?;
                (calls.len() == 1
                    && calls[0].call.function == cyrus_soup::Native::Wait
                    && runtime.task_slots()[0].4 < u16::MAX)
                    .then_some(index)
            })
            .context("retained save has no isolated native Wait continuation")?;
        let before = world.actors()[waiting].runtime().unwrap().task_slots();
        let cursor = world.actors()[waiting]
            .runtime()
            .unwrap()
            .program_counter(cyrus_soup::Thread::Main);
        let id = world.id(waiting)?;
        world.advance(
            id,
            cyrus_script::script::ScriptStage::Tasks,
            cyrus_script::script::ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            100,
        )?;
        let after = world.actors()[waiting].runtime().unwrap().task_slots();
        assert_eq!(
            world.actors()[waiting]
                .runtime()
                .unwrap()
                .program_counter(cyrus_soup::Thread::Main),
            cursor,
            "explicit service update must not re-enter the script body"
        );
        if let Some(slot) = after.first() {
            assert_eq!(slot.4, before[0].4.wrapping_add(1));
        }
        eprintln!(
            "restored original execution {} actors/{} codes/{} shells and polled native Wait without callback reissue",
            objects.len(),
            codes.len(),
            shells.len()
        );
    }
    Ok(())
}

#[test]
fn retained_logical_session_import_preserves_rejection_and_reaches_first_service_update()
-> Result<()> {
    use cyrus_script::script::{
        LogicalSessionCandidate, OriginalSessionImport, OriginalWorldAllocations, ScriptActors,
        WorldSnapshotSources,
    };
    use std::collections::BTreeMap;
    let Some((install, paths)) = installed_slots()? else {
        return Ok(());
    };
    let files = cyrus_assets::native::Files::open(&install)?;
    let exported_hud = cyrus_assets::game::prepare(&files, "hud/overlay.hud")?;
    let hud_data: HudAsset = serde_json::from_slice(&exported_hud["hud/overlay.hud"])?;
    let preferences = Preferences::new(NoWrites);
    for path in paths {
        let slot = load_save_slot(&path)?;
        let saves = std::iter::once(&slot.game)
            .chain(slot.maps.values())
            .collect::<Vec<_>>();
        let mut scripts = BTreeMap::new();
        let mut animations = BTreeMap::new();
        let mut players = BTreeMap::new();
        let mut models = BTreeMap::new();
        let mut walk_maps = BTreeMap::new();
        let mut ropes = BTreeMap::new();
        let mut prior_world = None;
        for game in saves {
            let id = game.metadata.world_id;
            let name = id.to_string();
            let key = format!("{name}.scripts");
            let exported = cyrus_assets::game::prepare(&files, &key)?;
            let script: Scripts = serde_json::from_slice(&exported[&key])?;
            let animation: cyrus_assets::animation::Animations =
                serde_json::from_slice(&exported[&format!("{name}.animations")])?;
            let player = cyrus_script::player::data::PlayerScene::decode(
                &exported[&format!("{name}.player")],
            )?;
            models.insert(id, installed_model_allocations(game, &animation, &install)?);
            walk_maps.insert(
                id,
                (0..animation.metadata["walk_node_maps"]
                    .as_array()
                    .context("walk-map allocation absent")?
                    .len())
                    .map(u32::try_from)
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            );
            let rope_count = animation.metadata["ropes"].as_array().map_or(0, Vec::len);
            ropes.insert(
                id,
                (1..=rope_count)
                    .map(u32::try_from)
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            );
            if id == slot.game.metadata.world_id {
                // This is an explicit prior native session, not original parity
                // evidence. Import must retain its unsaved owners and never invoke
                // this standalone-player initializer a second time.
                let mut prior = initialized_import_world(&script, &exported, &name)?;
                let selected = prior
                    .selected_player()
                    .context("prior selected player absent")?;
                prior.initialize_player_animation(selected, &animation, 0, &mut |_| Ok(()))?;
                prior_world = Some(prior);
            }
            scripts.insert(id, script);
            animations.insert(id, animation);
            players.insert(id, player);
        }
        let sources = scripts
            .iter()
            .map(|(&id, script)| {
                (
                    id,
                    WorldSnapshotSources {
                        scripts: script,
                        animations: &animations[&id],
                        collision: &players[&id].collision,
                        player: Some(&players[&id]),
                        effects: None,
                        identity_memo: None,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let channels = (0u8..16).map(|slot| (slot, slot)).collect();
        let allocations = scripts
            .keys()
            .map(|&id| {
                (
                    id,
                    OriginalWorldAllocations {
                        models: &models[&id],
                        walk_maps: &walk_maps[&id],
                        ropes: &ropes[&id],
                        ambient_channels: &channels,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let hud = ScriptHud::default();
        hud.configure(&hud_data, &preferences);
        let mut actors = ScriptActors::default();
        actors.current = prior_world;
        let clock =
            cyrus_script::clock::SimulationClock::new(scripts[&slot.game.metadata.world_id].clock)?;
        let mut live = LogicalSessionCandidate {
            actors,
            hud,
            clock,
            random: cyrus_game::random::EngineRandom::from_state(12345),
            toggles: cyrus_script::player::input::PlayerToggles {
                walk_mode: false,
                auto_defend: true,
            },
        };
        let snapshot = |live: &LogicalSessionCandidate| -> Result<serde_json::Value> {
            Ok(serde_json::to_value(live.actors.snapshot_session(
                &sources,
                &live.hud,
                &live.clock,
                &live.random,
                live.toggles,
            )?)?)
        };
        let before = snapshot(&live)?;
        let mut invalid = slot.clone();
        let svg2 = &mut invalid
            .game
            .sections
            .iter_mut()
            .find(|section| section.name == *b"SVG2")
            .context("SVG2 missing")?
            .data;
        // HUD is validated after every world, interpreter and native service. A
        // countdown beyond the supported display range rejects that whole session.
        svg2[1662..1666].copy_from_slice(&51i32.to_le_bytes());
        let error = live
            .import_original(OriginalSessionImport {
                slot: &invalid,
                sources: &sources,
                allocations: &allocations,
                hud: &hud_data,
                preferences: &preferences,
            })
            .err()
            .context("late invalid HUD admitted")?;
        ensure!(
            format!("{error:#}").contains("health feedback"),
            "failed before late HUD validation: {error:#}"
        );
        assert_eq!(
            snapshot(&live)?,
            before,
            "whole logical session changed after rejected import"
        );
        live.import_original(OriginalSessionImport {
            slot: &slot,
            sources: &sources,
            allocations: &allocations,
            hud: &hud_data,
            preferences: &preferences,
        })?;
        // A second late rejection protects already-restored active native tasks,
        // allocations and visited worlds, rather than only a fresh prior session.
        let admitted = snapshot(&live)?;
        let error = live
            .import_original(OriginalSessionImport {
                slot: &invalid,
                sources: &sources,
                allocations: &allocations,
                hud: &hud_data,
                preferences: &preferences,
            })
            .err()
            .context("late invalid HUD replaced an active restored session")?;
        ensure!(
            format!("{error:#}").contains("health feedback"),
            "active-session rejection occurred before late HUD: {error:#}"
        );
        assert_eq!(
            snapshot(&live)?,
            admitted,
            "rejected import changed active interpreter/native/service/world state"
        );
        for world in live
            .actors
            .current
            .iter()
            .chain(live.actors.visited_worlds().values())
        {
            let saved = std::iter::once(&slot.game)
                .chain(slot.maps.values())
                .find(|saved| saved.metadata.world_id == world.world)
                .context("imported world document absent")?;
            let object_bytes = &saved
                .sections
                .iter()
                .find(|section| section.name == *b"SMOB")
                .context("saved model census absent")?
                .data;
            for object in rgpre::import::savegame::parse_save_objects(object_bytes)? {
                let receiver = world
                    .actors()
                    .iter()
                    .enumerate()
                    .find(|(_, actor)| match actor.source {
                        cyrus_script::script::ScriptActorSource::Player => object.object_id == -2,
                        cyrus_script::script::ScriptActorSource::AlternatePlayer => {
                            object.object_id == -3
                        }
                        cyrus_script::script::ScriptActorSource::Camera => object.object_id == 2,
                        cyrus_script::script::ScriptActorSource::Map(_) => actor
                            .runtime_identifier
                            .is_some_and(|id| i32::from(id as u16) == object.object_id),
                        _ => false,
                    })
                    .context("restored model receiver absent")?
                    .0;
                if let Some(presentation) = world.model_presentation(world.id(receiver)?)? {
                    let saved_transform = object.actor_state()?.model_transform;
                    assert_eq!(presentation.position, saved_transform.translation());
                    assert_eq!(
                        presentation.pose.matrix.0,
                        saved_transform.matrix(),
                        "stored model matrix must survive independently of logical actor angles"
                    );
                }
            }
            let bytes = &saved
                .sections
                .iter()
                .find(|section| section.name == *b"SMSH")
                .context("saved shell census absent")?
                .data;
            let mut expected = rgpre::import::savegame::parse_save_shells(bytes)?
                .iter()
                .map(|shell| {
                    std::str::from_utf8(shell.name())
                        .map(|name| name.trim_end_matches('\0').to_owned())
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut actual = world
                .actors()
                .iter()
                .filter_map(|actor| match &actor.source {
                    cyrus_script::script::ScriptActorSource::Shell(name) => Some(name.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            actual.sort();
            expected.sort();
            assert_eq!(
                actual, expected,
                "import must reconstruct saved shells without fresh-entry shells"
            );
        }
        assert_eq!(
            live.random.state(),
            12345,
            "load must not reseed or consume the retained random stream"
        );
        let world = live
            .actors
            .current
            .as_mut()
            .context("imported current world absent")?;
        let programs = scripts[&world.world]
            .actors
            .values()
            .map(|definition| {
                (
                    cyrus_soup::CodeOrigin(definition.code_origin),
                    std::sync::Arc::new(definition.program.clone()),
                )
            })
            .collect();
        let waiting = world
            .actors()
            .iter()
            .enumerate()
            .find_map(|(index, actor)| {
                let runtime = actor.runtime()?;
                let snapshot = runtime.snapshot(&programs).ok()?;
                let calls = snapshot.task_calls().ok()?;
                (calls.len() == 1 && calls[0].call.function == cyrus_soup::Native::Wait)
                    .then_some(index)
            })
            .context("imported native Wait continuation absent")?;
        let cursor = world.actors()[waiting]
            .runtime()
            .unwrap()
            .program_counter(cyrus_soup::Thread::Main);
        world.advance(
            world.id(waiting)?,
            cyrus_script::script::ScriptStage::Tasks,
            cyrus_script::script::ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            100,
        )?;
        assert_eq!(
            world.actors()[waiting]
                .runtime()
                .unwrap()
                .program_counter(cyrus_soup::Thread::Main),
            cursor
        );
        eprintln!(
            "atomic original logical session world {}: late rejection preserved all owners; first explicit native task update passed",
            world.world
        );
    }
    Ok(())
}
