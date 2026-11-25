use super::*;

#[test]
fn contact_death_prefers_122_then_121_and_retains_combat_default() {
    use cyrus_script::script::CombatControlState;
    for (groups, selector, expected) in [
        (vec![121, 122], 0, Some(124)),
        (vec![121], 0, Some(123)),
        (vec![121, 122], 1, Some(123)),
        (vec![121, 122], -1, Some(123)),
        (vec![122], 1, None),
    ] {
        let (world, scripts, mut data) = movement_fixture(
            18,
            false,
            true,
            |_| {},
            |saved| {
                saved["actors"][0]["control"]["death_selector"] = 0.into();
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(CombatControlState::default()).unwrap();
            },
        );
        for group in groups {
            data.metadata["actors"][0]["animation_groups"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "group_index":group,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]
                }));
        }
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["animation"] = serde_json::Value::Null;
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        world
            .bind_animation(id, &data, AnimationGroup::RUN)
            .unwrap();
        world
            .dispatch_contact_death(id, selector, Some(1), &mut |_, _| Ok(()))
            .unwrap();
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.ai_type, AiType::DEAD);
        assert_eq!(control.combat_death_default, expected);
        let deaths = world.take_death_events();
        assert_eq!(deaths.len(), 1);
        assert_eq!(deaths[0].snapshot.actor, id);
        assert!(deaths[0].snapshot.dead);
        assert!(deaths[0].source_position.is_none());
        world
            .dispatch_contact_death(id, selector, Some(1), &mut |_, _| Ok(()))
            .unwrap();
        assert!(world.take_death_events().is_empty());
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.navigation.current, control.navigation.next),
            (-1, -1)
        );
    }
}

#[test]
fn a_direct_damage_message_hurts_the_selected_player_through_shared_health() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    let (_, mut world, _, _) = player_health_fixture();
    let receiver = world.selected_player().unwrap();
    let sender = (0..world.actors().len())
        .map(|index| world.id(index).unwrap())
        .find(|id| {
            *id != receiver && !matches!(world.actors()[id.index].source, ScriptActorSource::Player)
        })
        .unwrap();
    world.ordinary_actors([0, 0]).unwrap();
    world.set_attribute_for_test(sender, Attribute::Damage, 12);
    let source = world
        .spatial(sender)
        .unwrap()
        .local
        .position
        .map(|q8| q8 as f32 / 256.0);
    let before = world.spatial(receiver).unwrap().local.position;
    world
        .send_combat_message(sender, receiver, CombatMessageCode::DIRECT_DAMAGE)
        .unwrap();
    let hud = ScriptHud::default();
    world
        .process_combat_message_with_services(
            receiver,
            None,
            ScriptInputs {
                hud: Some(&hud),
                death_suppressed: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut cyrus_game::random::EngineRandom::from_state(42)),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    let events = world.take_damage_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].actor, receiver);
    assert_eq!(events[0].source_position, Some(source));
    assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
    assert_ne!(world.spatial(receiver).unwrap().local.position, before);
    assert_eq!(world.globals().selected_health().unwrap().displayed(), 38);
    assert_eq!(
        world.actors()[receiver.index]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .attacker_selection_state,
        0,
        "direct damage does not hand coordination to the selected player"
    );
}

#[test]
fn a_fatal_direct_damage_message_starts_the_selected_players_death_through_the_combat_death_service()
 {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    let (_, mut world, _, _) = player_health_fixture();
    let receiver = world.selected_player().unwrap();
    let sender = (0..world.actors().len())
        .map(|index| world.id(index).unwrap())
        .find(|id| {
            *id != receiver && !matches!(world.actors()[id.index].source, ScriptActorSource::Player)
        })
        .unwrap();
    world.ordinary_actors([0, 0]).unwrap();
    world.set_attribute_for_test(sender, Attribute::Damage, 255);
    world
        .send_combat_message(sender, receiver, CombatMessageCode::DIRECT_DAMAGE)
        .unwrap();
    let hud = ScriptHud::default();
    world
        .process_combat_message_with_services(
            receiver,
            None,
            ScriptInputs {
                hud: Some(&hud),
                death_suppressed: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut cyrus_game::random::EngineRandom::from_state(42)),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    let health = world.globals().selected_health().unwrap();
    assert!(health.hit_inhibited(), "the death-owning health ran");
    assert_eq!(health.displayed(), 0);
    assert_eq!(
        world.globals().player_death_override_group(),
        Some(121),
        "the fixture table has no `anim_death_fight_hard`, so a zero-selector death falls back to `anim_death_fight_stab`"
    );
    let control = world.actors()[receiver.index].control_state().unwrap();
    assert_eq!(control.ai_type, AiType::DEAD);
    assert_eq!(control.combat.as_ref().unwrap().action, -1);
    let deaths = world.take_death_events();
    assert_eq!(deaths.len(), 1);
    assert_eq!(deaths[0].snapshot.actor, receiver);
    assert!(deaths[0].snapshot.dead);
    assert!(deaths[0].source_position.is_some());
    assert!(world.take_death_events().is_empty());
}

#[test]
fn a_direct_damage_message_displaces_hurts_and_hands_coordination_off_without_clearing_intent() {
    use cyrus_script::script::ScriptInputs;
    let (mut world, _, _) = coordination_message_fixture(|saved| {
        saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = 0.into();
        saved["actors"][1]["control"]["combat"]["flags"] = 3.into();
        saved["actors"][0]["control"]["combat"]["flags"] = 3.into();
        saved["actors"][1]["attributes"][96] = 12.into();
        saved["actors"][0]["attributes"][3] = 40.into();
    });
    world.ordinary_actors([0, 0]).unwrap();
    let receiver = world.id(0).unwrap();
    let sender = world.id(1).unwrap();
    let before = world.spatial(receiver).unwrap().local.position;
    world
        .send_combat_message(sender, receiver, CombatMessageCode::DIRECT_DAMAGE)
        .unwrap();
    let mut sounds = Vec::new();
    world
        .process_combat_message_with_services(
            receiver,
            None,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut cyrus_game::random::EngineRandom::default()),
            &mut |actor, sound| {
                sounds.push((actor, format!("{sound:?}")));
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        world.consume_combat_message(receiver).unwrap(),
        None,
        "consumed"
    );
    assert_ne!(
        world.spatial(receiver).unwrap().local.position,
        before,
        "displaced"
    );
    assert_eq!(
        world.actors()[receiver.index].attributes()[Attribute::Health],
        28,
        "40 - attribute 96"
    );
    for actor in [receiver, sender] {
        let flags = world.actors()[actor.index]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .flags
            .bits();
        assert_eq!(flags, 3, "intent bits are not cleared");
    }
    assert!(sounds.iter().any(|(_, text)| text.contains("Positional")));
}

#[test]
fn a_fatal_direct_damage_message_runs_the_combat_death_service_for_a_nonplayer() {
    use cyrus_script::script::ScriptInputs;
    let (mut world, _, _) = coordination_message_fixture(|saved| {
        saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = 1.into();
        saved["actors"][0]["control"]["combat"]["close_distance_passes"] = 7.into();
        saved["actors"][0]["control"]["combat"]["action"] = 3.into();
        saved["actors"][0]["control"]["combat"]["flags"] = 7.into();
        saved["actors"][1]["attributes"][96] = 12.into();
        saved["actors"][0]["attributes"][3] = 5.into();
    });
    world.ordinary_actors([0, 0]).unwrap();
    let receiver = world.id(0).unwrap();
    let sender = world.id(1).unwrap();
    world
        .send_combat_message(sender, receiver, CombatMessageCode::DIRECT_DAMAGE)
        .unwrap();
    let hud = cyrus_script::script::ScriptHud::default();
    world
        .process_combat_message_with_services(
            receiver,
            None,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut cyrus_game::random::EngineRandom::default()),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    let control = world.actors()[receiver.index].control_state().unwrap();
    let combat = control.combat.as_ref().unwrap();
    assert_eq!(
        world.actors()[receiver.index].attributes()[Attribute::Health],
        0
    );
    assert_eq!(control.ai_type, AiType::DEAD);
    assert_eq!(combat.action, -1);
    assert_eq!(combat.flags.bits() & 4, 0, "combat flag 0x04 cleared");
    assert_eq!(
        (
            combat.close_distance_passes,
            combat.attacker_selection_state
        ),
        (0, 0)
    );
    let deaths = world.take_death_events();
    assert_eq!(deaths.len(), 1);
    assert_eq!(deaths[0].snapshot.actor, receiver);
    assert!(deaths[0].snapshot.dead);
    assert!(deaths[0].source_position.is_some());
    assert!(world.take_death_events().is_empty());
}

#[test]
fn direct_damage_uses_generic_nonplayer_health_admission_gates() {
    use cyrus_script::script::ScriptInputs;

    for gate in [120usize, 97] {
        let (mut world, _, _) = coordination_message_fixture(|saved| {
            saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = 1.into();
            saved["actors"][0]["attributes"][3] = 40.into();
            saved["actors"][0]["attributes"][gate] = 1.into();
            saved["actors"][0]["control"]["combat"]["flags"] = 0.into();
            saved["actors"][1]["attributes"][96] = 12.into();
        });
        world.ordinary_actors([0, 0]).unwrap();
        let receiver = world.id(0).unwrap();
        let sender = world.id(1).unwrap();
        world
            .send_combat_message(sender, receiver, CombatMessageCode::DIRECT_DAMAGE)
            .unwrap();
        world
            .process_combat_message_with_services(
                receiver,
                None,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut cyrus_game::random::EngineRandom::default()),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            world.actors()[receiver.index].attributes()[Attribute::Health],
            40,
            "attr-{gate} gate"
        );
    }
}

#[test]
fn combat_hit_restores_a_charged_sender_weapon_after_damage() {
    use cyrus_script::script::{CombatControlState, ScriptHud, ScriptInputs};

    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            let mut instance = scripts.instances[0].clone();
            instance.object_id = 2;
            scripts.instances.push(instance);
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 3;
            actor.instance_variables = vec![vec![4]; 3];
            for instance in &mut scripts.instances {
                instance.active = 1;
            }
            scripts.combat_attacks = vec![cyrus_assets::script::combat::Attack {
                is_combo: false,
                motions: vec![
                    serde_json::from_value(serde_json::json!({
                        "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                        "damage":1,"first_collide_frame":0,"collide_duration":1
                    }))
                    .unwrap(),
                ],
                min_range: 0,
                max_range: 100,
            }];
        },
        |saved| {
            for index in [1, 2] {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(CombatControlState {
                        attacker_selection_state: 1,
                        action: 0,
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
            }
            saved["actors"][1]["attributes"][3] = 40.into();
            saved["actors"][2]["attributes"][23] = 75.into();
        },
    );
    let receiver = world.id(1).unwrap();
    let sender = world.id(2).unwrap();
    let selected_weapon_before = world.selected_player().map(|selected| {
        (
            selected,
            world.actors()[selected.index].attributes()[Attribute::Weapon],
        )
    });
    let hud = ScriptHud::default();
    hud.configure(
        &inventory_asset(&[(1, 0), (75, 1)]),
        &cyrus_game::preferences::Preferences::default(),
    );
    world
        .send_combat_message(sender, receiver, CombatMessageCode::HIT)
        .unwrap();
    world
        .process_combat_message_with_services(
            receiver,
            None,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut cyrus_game::random::EngineRandom::default()),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.actors()[sender.index].attributes()[Attribute::Weapon],
        1
    );
    if let Some((selected_player, selected_weapon_before)) = selected_weapon_before {
        assert_eq!(
            world.actors()[selected_player.index].attributes()[Attribute::Weapon],
            selected_weapon_before,
            "restoring a nonselected sender must not rewrite the selected actor's weapon"
        );
    }
    let inventory = hud.inventory(None).unwrap();
    assert_eq!((inventory.counts[&1], inventory.counts[&75]), (1, 0));
    assert_eq!(inventory.selected, Some(1));
    assert_eq!(
        world.actors()[receiver.index].attributes()[Attribute::Health],
        39
    );
}

#[test]
fn a_combat_hit_on_the_suppressed_selected_player_takes_no_damage() {
    use cyrus_script::script::{
        CombatControlState, ScriptHud, ScriptInputs, WorldRestoreContext, WorldSnapshotSources,
    };
    // One damage point from a sender with an attack slot: the unsuppressed player loses one
    // health, the suppressed one keeps all of it.
    for (suppressed, expected) in [(false, 49), (true, 50)] {
        let (scripts, mut world, mut data, _) = player_health_fixture_with(|scripts| {
            scripts.combat_attacks = vec![cyrus_assets::script::combat::Attack {
                is_combo: false,
                motions: vec![
                    serde_json::from_value(serde_json::json!({
                        "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                        "damage":1,"first_collide_frame":0,"collide_duration":1
                    }))
                    .unwrap(),
                ],
                min_range: 0,
                max_range: 100,
            }];
        });
        data.metadata["markers"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        let collision = Default::default();
        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        saved["actors"][0]["control"]["combat"] = serde_json::to_value(CombatControlState {
            attacker_selection_state: 1,
            action: 0,
            attack_indices: [Some(0); 9],
            retained_action_indices: [-1, 0],
            ..Default::default()
        })
        .unwrap();
        let generation = world.generation() + 1;
        let mut world = ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            WorldRestoreContext {
                generation,
                globals: std::mem::take(world.globals_mut()),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let receiver = world.selected_player().unwrap();
        let sender = world.id(0).unwrap();
        world.ordinary_actors([0, 0]).unwrap();
        world
            .send_combat_message(sender, receiver, CombatMessageCode::HIT)
            .unwrap();
        let hud = ScriptHud::default();
        world
            .process_combat_message_with_services(
                receiver,
                None,
                ScriptInputs {
                    hud: Some(&hud),
                    death_suppressed: Some(suppressed),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut cyrus_game::random::EngineRandom::from_state(42)),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            world.globals().selected_health().unwrap().displayed(),
            expected,
            "suppressed {suppressed}"
        );
    }
}

#[test]
fn combat_hit_from_a_sender_without_an_attack_slot_deals_no_damage() {
    use cyrus_script::script::{CombatControlState, ScriptHud, ScriptInputs};

    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            let mut instance = scripts.instances[0].clone();
            instance.object_id = 2;
            scripts.instances.push(instance);
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 3;
            actor.instance_variables = vec![vec![4]; 3];
            for instance in &mut scripts.instances {
                instance.active = 1;
            }
            scripts.combat_attacks = vec![cyrus_assets::script::combat::Attack {
                is_combo: false,
                motions: vec![
                    serde_json::from_value(serde_json::json!({
                        "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                        "damage":1,"first_collide_frame":0,"collide_duration":1
                    }))
                    .unwrap(),
                ],
                min_range: 0,
                max_range: 100,
            }];
        },
        |saved| {
            for (index, slot) in [(1, 0), (2, -1)] {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(CombatControlState {
                        attacker_selection_state: 1,
                        action: 0,
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, slot],
                        ..Default::default()
                    })
                    .unwrap();
            }
            saved["actors"][1]["attributes"][3] = 40.into();
        },
    );
    let receiver = world.id(1).unwrap();
    let sender = world.id(2).unwrap();
    let hud = ScriptHud::default();
    hud.configure(
        &inventory_asset(&[(1, 0)]),
        &cyrus_game::preferences::Preferences::default(),
    );
    world
        .send_combat_message(sender, receiver, CombatMessageCode::HIT)
        .unwrap();
    world
        .process_combat_message_with_services(
            receiver,
            None,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut cyrus_game::random::EngineRandom::default()),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.actors()[receiver.index].attributes()[Attribute::Health],
        40
    );
}

#[test]
fn direct_damage_restores_a_charged_sender_weapon_after_damage() {
    use cyrus_script::script::{CombatControlState, ScriptHud, ScriptInputs};

    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            let mut instance = scripts.instances[0].clone();
            instance.object_id = 2;
            scripts.instances.push(instance);
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 3;
            actor.instance_variables = vec![vec![4]; 3];
            for instance in &mut scripts.instances {
                instance.active = 1;
            }
            scripts.combat_attacks = vec![cyrus_assets::script::combat::Attack {
                is_combo: false,
                motions: vec![
                    serde_json::from_value(serde_json::json!({
                        "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                        "damage":1,"first_collide_frame":0,"collide_duration":1
                    }))
                    .unwrap(),
                ],
                min_range: 0,
                max_range: 100,
            }];
        },
        |saved| {
            for index in [1, 2] {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(CombatControlState {
                        attacker_selection_state: 1,
                        action: 0,
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
            }
            saved["actors"][1]["attributes"][3] = 40.into();
            saved["actors"][2]["attributes"][23] = 75.into();
            saved["actors"][2]["attributes"][96] = 1.into();
        },
    );
    let receiver = world.id(1).unwrap();
    let sender = world.id(2).unwrap();
    let selected_weapon_before = world.selected_player().map(|selected| {
        (
            selected,
            world.actors()[selected.index].attributes()[Attribute::Weapon],
        )
    });
    let hud = ScriptHud::default();
    hud.configure(
        &inventory_asset(&[(1, 0), (75, 1)]),
        &cyrus_game::preferences::Preferences::default(),
    );
    world
        .send_combat_message(sender, receiver, CombatMessageCode::DIRECT_DAMAGE)
        .unwrap();
    world
        .process_combat_message_with_services(
            receiver,
            None,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut cyrus_game::random::EngineRandom::default()),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.actors()[sender.index].attributes()[Attribute::Weapon],
        1
    );
    if let Some((selected_player, selected_weapon_before)) = selected_weapon_before {
        assert_eq!(
            world.actors()[selected_player.index].attributes()[Attribute::Weapon],
            selected_weapon_before,
            "restoring a nonselected sender must not rewrite the selected actor's weapon"
        );
    }
    let inventory = hud.inventory(None).unwrap();
    assert_eq!((inventory.counts[&1], inventory.counts[&75]), (1, 0));
    assert_eq!(inventory.selected, Some(1));
    assert_eq!(
        world.actors()[receiver.index].attributes()[Attribute::Health],
        39
    );
}
