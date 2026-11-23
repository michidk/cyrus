use super::*;
use cyrus_script::script::{CombatControlState, ScriptHud, ScriptInputs};

#[test]
fn hit_message_cancels_contact_even_when_attack_playback_continues() {
    // Synthetic playback isolates intent from animation selection. Collision timing
    // comes from the installed primary player motion, rather than a test clock.
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let path = root.join("COMBAT.INI");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!(
            "skipped: no install (attack interruption: {})",
            path.display()
        );
        return;
    };
    let ini = rgpre::import::combat_ini::CombatIni::parse(&text).unwrap();
    let attacks = combat::prepare(&ini).unwrap();
    let attack = attacks[72].clone();
    let motion = &attack.motions[0];
    assert_eq!(
        motion.animation_group,
        i32::from(AnimationGroup::ATTACK_1.index())
    );
    let start = motion.first_collide_frame;
    let end = start + motion.collide_duration;
    assert!(start > 0 && end > start);

    for interrupted in [false, true] {
        let (world, scripts, mut data) = movement_fixture(
            AnimationGroup::RUN.index(),
            false,
            true,
            |scripts| {
                scripts.combat_attacks = vec![attack.clone()];
                scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Health] = 100;
                for instance in &mut scripts.instances {
                    instance.active = 1;
                }
            },
            |saved| {
                for index in [0, 1] {
                    saved["actors"][index]["control"]["combat"] =
                        serde_json::to_value(CombatControlState {
                            action: 0,
                            flags: CombatFlags::MOVE_INTENT,
                            attack_indices: [Some(0); 9],
                            retained_action_indices: [-1, 0],
                            ..Default::default()
                        })
                        .unwrap();
                }
                saved["actors"][0]["control"]["death_selector"] = 0.into();
            },
        );
        // Each explicit animation update visits one command through and beyond
        // the collision interval. The reaction group is deliberately unavailable.
        let groups = data.metadata["actors"][0]["animation_groups"]
            .as_array_mut()
            .unwrap();
        let group = groups
            .iter_mut()
            .find(|group| group["group_index"] == AnimationGroup::RUN.index())
            .unwrap();
        group["group_index"] = AnimationGroup::ATTACK_1.index().into();
        group["commands"] = serde_json::Value::Array(
            (0..end + 3)
                .map(|_| serde_json::json!({"opcode":0,"frame_reference":18}))
                .collect(),
        );
        let mut saved = movement_snapshot(&world, &scripts, &data);
        let mut playback_world = ScriptWorld::prepare(&scripts, 1).unwrap();
        playback_world.prepare_spatial().unwrap();
        let playback_actor = playback_world.id(0).unwrap();
        playback_world
            .bind_animation(playback_actor, &data, AnimationGroup::ATTACK_1)
            .unwrap();
        let playback_saved = movement_snapshot(&playback_world, &scripts, &data);
        saved["actors"][0]["animation"] = playback_saved["actors"][0]["animation"].clone();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let attacker = world.id(0).unwrap();
        let opponent = world.id(1).unwrap();
        world.ordinary_actors([0, 0]).unwrap();
        let inputs = ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        };
        let opponent_health = world.actors()[opponent.index].attributes()[Attribute::Health];
        let mut observed_open = false;
        let mut reached_collision_start = false;
        for step in 0..end + 1 {
            world
                .advance(attacker, ScriptStage::Animation, inputs, None, None, 100)
                .unwrap();
            if interrupted && step == 0 {
                world
                    .send_combat_message(opponent, attacker, CombatMessageCode::HIT)
                    .unwrap();
                let hud = ScriptHud::default();
                world
                    .process_combat_message_with_services(
                        attacker,
                        None,
                        ScriptInputs {
                            hud: Some(&hud),
                            ..inputs
                        },
                        Some(&mut cyrus_game::random::EngineRandom::from_state(42)),
                        &mut |_, _| Ok(()),
                    )
                    .unwrap();
            }
            let snapshot = movement_snapshot(&world, &scripts, &data);
            let current = &snapshot["actors"][0]["animation"]["playback"]["current"];
            assert_eq!(current[0], AnimationGroup::ATTACK_1.index());
            let cursor = current[1]["command"].as_i64().unwrap() as i32;
            reached_collision_start |= cursor >= start;
            if interrupted {
                let combat = world.actors()[attacker.index]
                    .control_state()
                    .unwrap()
                    .combat
                    .as_ref()
                    .unwrap();
                assert_eq!(
                    combat.action, 0,
                    "intent must be tested independently of action cleanup"
                );
                assert!(!combat.flags.contains(CombatFlags::MOVE_INTENT));
                assert_eq!(
                    world.actors()[attacker.index].attributes()[Attribute::Health],
                    100 - motion.damage.unwrap() as u8
                );
            }
            let open = world.combat_attack_window(attacker).unwrap();
            observed_open |= open;
            if interrupted {
                assert!(!open, "cancelled swing admitted contact at update {step}");
                // Exercise production maintenance as well: without geometry it
                // would fail if the cancelled swing reached candidate dispatch.
                world
                    .maintain_map_combat(attacker, 1, false, inputs, None, &mut |_, _| Ok(()))
                    .unwrap();
                assert_eq!(
                    world.actors()[opponent.index].attributes()[Attribute::Health],
                    opponent_health
                );
            }
        }
        assert!(
            reached_collision_start,
            "playback must actually cross the contact boundary"
        );
        assert_eq!(observed_open, !interrupted);
    }
}
