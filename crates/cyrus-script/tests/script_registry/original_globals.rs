//! Saved globals replace their fields while explicit original-load owners retain state.
use super::*;
use cyrus_script::script::{GameplayGlobals, ScriptActorId};
use rgpre::import::savegame::{SaveGameplayGlobals, SaveSessionGlobals};

fn saved_globals() -> anyhow::Result<(SaveGameplayGlobals, SaveSessionGlobals)> {
    let mut game = vec![0u8; 1542];
    for (offset, value) in [
        (0x10, 3i32),
        (0x130, 12),
        (0x30, 1i32),
        (0x34, 74),
        (0x38, 39),
        (0x40, 2),
        (0x48, -512),
        (0x4c, 768),
        (0x50, 1024),
        (0x5c, 145),
        (0x60, 19),
        (0x64, 9),
        (0x90, 87),
        (0x10c, -1),
    ] {
        game[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    game[0x224..0x226].copy_from_slice(&37u16.to_le_bytes());
    let mut session = vec![0u8; 1666];
    session[..4].copy_from_slice(&74i32.to_le_bytes());
    session[1622..1626].copy_from_slice(&7i32.to_le_bytes());
    Ok((
        SaveGameplayGlobals::parse(&game)?,
        SaveSessionGlobals::parse(&session)?,
    ))
}

#[test]
fn original_saved_globals_replace_saved_fields_and_retain_session_owners() -> anyhow::Result<()> {
    let (scripts, mut world, _, _) = player_walking_fixture();
    let (saved, session) = saved_globals()?;
    world.globals_mut().begin_topic_menu();
    world.globals_mut().queue_movie(-2);
    world.globals_mut().request_item_viewer(7, [31, 41], true);
    let before = serde_json::to_value(world.globals())?;
    world
        .globals_mut()
        .restore_original_saved_fields(&saved, &session, &scripts.dialogue)?;
    let after = serde_json::to_value(world.globals())?;
    for field in [
        "menu_state",
        "menu",
        "potions",
        "queued_movie",
        "item_viewer",
        "selected_ledge_result",
    ] {
        assert_eq!(after[field], before[field], "retained {field}");
    }
    for field in ["conditional_ticks", "secondary_ticks"] {
        assert_eq!(
            after["controller_prelude"][field],
            before["controller_prelude"][field]
        );
    }
    assert_eq!(after["controller_prelude"]["action_ticks"], 19);
    assert_eq!(after["controller_prelude"]["override_group"], 0);
    assert_eq!(after["controller_prelude"]["pending_action"], false);
    assert_eq!(after["activation_consumed_use"], true);
    assert_eq!(after["primary_body_contacts"], 12);
    assert_eq!(after["lineup_result"], 9);
    assert_eq!(after["selected_combat_inhibition"], 9);
    assert_eq!(after["lockout"], 37);
    assert_eq!(after["player_idle_counter"], -1);
    assert_eq!(after["world_loop_frame"], 0);
    assert_eq!(after["dragon_breath_cooldown"], 0);
    assert!(world.globals().player_swapped());
    let health = world.globals().selected_health().unwrap();
    assert_eq!(health.maximum(), 100);
    assert_eq!(health.displayed(), 74);
    assert!(!health.hit_inhibited());
    assert_eq!(health.falling_elapsed(), Some(145));
    assert_eq!(health.death().unwrap().mode(), 2);
    assert_eq!(health.death().unwrap().progress(), 0);
    assert_eq!(
        health.death().unwrap().local_position(),
        Some([-512, 768, 1024])
    );
    assert!(!world.globals().dialogue().unwrap().waiting());
    world
        .globals_mut()
        .begin_player_controller(4, JumpMotion::NONE, JumpMotion::NONE)?;
    assert!(!world.globals_mut().finish_player_controller(4)?);
    assert_eq!(
        serde_json::to_value(world.globals())?["controller_prelude"]["action_ticks"],
        15
    );
    Ok(())
}

#[test]
fn original_saved_globals_reject_invalid_health_without_mutating_owner() -> anyhow::Result<()> {
    let (scripts, mut world, _, _) = player_walking_fixture();
    let (saved, mut session) = saved_globals()?;
    session.player_health = 256;
    let before = serde_json::to_value(world.globals())?;
    assert!(
        world
            .globals_mut()
            .restore_original_saved_fields(&saved, &session, &scripts.dialogue)
            .is_err()
    );
    assert_eq!(serde_json::to_value(world.globals())?, before);
    Ok(())
}

#[test]
fn original_globals_candidate_retains_viewer_and_leaves_prior_untouched() -> anyhow::Result<()> {
    let (scripts, mut world, _, _) = player_walking_fixture();
    let (saved, session) = saved_globals()?;
    world.globals_mut().request_item_viewer(7, [31, 41], true);
    let before = serde_json::to_value(world.globals())?;
    let bindings = (0..world.actors().len())
        .map(|index| {
            let old = ScriptActorId {
                generation: world.generation(),
                index,
            };
            (
                old,
                ScriptActorId {
                    generation: old.generation + 1,
                    index,
                },
            )
        })
        .collect::<Vec<_>>();
    let replacement = GameplayGlobals::from_original_load(
        world.globals(),
        &saved,
        &session,
        &scripts.dialogue,
        &bindings,
        &BTreeMap::new(),
    )?;
    let after = serde_json::to_value(&replacement)?;
    assert_eq!(after["item_viewer"], before["item_viewer"]);
    assert_eq!(after["selected_health"]["displayed"], 74);
    assert_eq!(serde_json::to_value(world.globals())?, before);
    Ok(())
}

#[test]
fn original_globals_candidate_rejects_unclassified_contact_owner_atomically() -> anyhow::Result<()>
{
    let (scripts, mut world, _, _) = player_walking_fixture();
    let (saved, session) = saved_globals()?;
    world.globals_mut().record_player_contact(0);
    let before = serde_json::to_value(world.globals())?;
    let error = GameplayGlobals::from_original_load(
        world.globals(),
        &saved,
        &session,
        &scripts.dialogue,
        &[],
        &BTreeMap::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("player contact ownership"));
    assert_eq!(serde_json::to_value(world.globals())?, before);
    Ok(())
}
