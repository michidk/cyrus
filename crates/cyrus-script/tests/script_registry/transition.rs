use super::*;

/// Run one surface transition on a fresh selected-player fixture: the group the actor ends up
/// requesting (or `None` when no group changed) and the sounds played.
fn surface_transition(
    (previous, stored, returned): (u8, u8, u8),
    forward: bool,
) -> (Option<u64>, Vec<cyrus_script::script::ScriptSound>) {
    use cyrus_game::random::EngineRandom;
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Weight] = 7;
    let (mut world, mut data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = serde_json::json!("animation_table");
    let group = |index: u32| {
        serde_json::json!({
            "group_index": index, "anim_id": 1, "anim_type": 0,
            "commands": [{"opcode": 0, "frame_reference": 10}, {"opcode": 1}]
        })
    };
    definition["animation_groups"] =
        serde_json::json!([group(0), group(18), group(19), group(142), group(143)]);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    if forward {
        world.apply_ordinary_directional_motion(id, 2, 512).unwrap();
    }
    world
        .set_actor_surface(id, Surface::from_byte(stored))
        .unwrap();
    let mut sounds = Vec::new();
    world
        .selected_surface_transition(
            id,
            Surface::from_byte(previous),
            Surface::from_byte(returned),
            1,
            &mut EngineRandom::default(),
            None,
            &mut |_, sound| {
                sounds.push(sound);
                Ok(())
            },
        )
        .unwrap();
    let animation = &world.inspect(0, 1)["actors"][0]["animation"];
    let group = animation["pending_group"]
        .as_u64()
        .or_else(|| animation["current_group"].as_u64())
        .filter(|group| *group != 0);
    (group, sounds)
}

#[test]
fn water_transition_rejects_retired_world_owner_before_effects() {
    let (mut world, _, _) = static_model_fixture(cache());
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, Surface::from_byte(1)).unwrap();
    let retired = cyrus_script::script::ScriptActorId {
        generation: id.generation.wrapping_add(1),
        index: id.index,
    };
    for returned in [Surface::UNKNOWN, Surface::from_byte(1)] {
        let mut sounds = Vec::new();
        let error = world
            .selected_surface_transition(
                retired,
                Surface::UNKNOWN,
                returned,
                1,
                &mut cyrus_game::random::EngineRandom::default(),
                None,
                &mut |_, sound| {
                    sounds.push(sound);
                    Ok(())
                },
            )
            .expect_err("retired world must not publish a water transition");
        assert!(error.to_string().contains("stale"), "{error:#}");
        assert!(sounds.is_empty());
        assert_eq!(world.actor_surface(id).unwrap(), Surface::from_byte(1));
    }
}

#[test]
fn selected_surface_transition_requests_swim_groups_and_plays_the_splash_sound() {
    use cyrus_script::script::ScriptSound;
    // An unchanged returned surface does nothing, and neither does dry to dry.
    assert_eq!(surface_transition((4, 4, 4), false), (None, vec![]));
    assert_eq!(surface_transition((0, 10, 10), true), (None, vec![]));
    // Dry land to deep water: `anim_swim_backward`, or `anim_swim_forward` moving forward, and the entry sound 71.
    for (forward, expected) in [(false, 143), (true, 142)] {
        let (group, sounds) = surface_transition((0, 4, 4), forward);
        assert_eq!(group, Some(expected));
        assert!(matches!(
            sounds[..],
            [ScriptSound::Positional {
                effect: 71,
                volume: 0,
                rate_offset_hz: -32..=31,
                ..
            }]
        ));
    }
    // Deep water to dry land: the exit sound 70 and `anim_run_backward`, or `anim_run` moving forward.
    for (forward, expected) in [(false, 19), (true, 18)] {
        let (group, sounds) = surface_transition((4, 10, 10), forward);
        assert_eq!(group, Some(expected));
        assert!(matches!(
            sounds[..],
            [ScriptSound::Positional { effect: 70, .. }]
        ));
    }
    // Shallow to shallow water requests no group but plays the sound; the transition fires on
    // the returned value differing from the previous surface.
    let (group, sounds) = surface_transition((1, 3, 3), true);
    assert_eq!(group, None);
    assert!(matches!(
        sounds[..],
        [ScriptSound::Positional { effect: 70, .. }]
    ));
}

#[test]
fn walking_continuation_enters_and_leaves_deep_water_facing_forward() {
    use cyrus_script::script::{ScriptHud, ScriptInputs, ScriptSound};
    for (previous, stored, expected, effect) in [
        (
            Surface::UNKNOWN,
            Surface::from_byte(4),
            AnimationGroup::SWIM_FORWARD,
            71,
        ),
        (
            Surface::from_byte(4),
            Surface::from_byte(10),
            AnimationGroup::RUN,
            70,
        ),
    ] {
        let (mut world, _, _) =
            movement_fixture(AnimationGroup::WALK.index(), false, true, |_| {}, |_| {});
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        world
            .continue_ordinary_motion(
                id,
                ScriptInputs {
                    hud: Some(&hud),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].control_state().unwrap().motion_direction,
            MotionDirection::WALK_FORWARD
        );
        world.set_actor_surface(id, stored).unwrap();
        let mut sounds = Vec::new();
        world
            .selected_surface_transition(
                id,
                previous,
                stored,
                1,
                &mut cyrus_game::random::EngineRandom::default(),
                None,
                &mut |_, sound| {
                    sounds.push(sound);
                    Ok(())
                },
            )
            .unwrap();
        let actor = world.inspect(0, 1);
        let animation = &actor["actors"][0]["animation"];
        assert_eq!(
            animation["pending_group"]
                .as_u64()
                .or_else(|| animation["current_group"].as_u64()),
            Some(u64::from(expected.index()))
        );
        assert!(matches!(sounds.as_slice(), [ScriptSound::Positional {
            effect: actual, volume: 0, rate_offset_hz: -32..=31, ..
        }] if *actual == effect));
    }
}
