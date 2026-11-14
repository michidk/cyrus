//! Actor isolation must suspend the ordinary owner, not only its script body.
use super::*;
use cyrus_script::script::{OrdinaryInputs, ScriptInputs};

#[test]
fn actor_update_skip_retains_response_timers_until_ordinary_continuation() {
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 60000];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (world, data, _) = static_model_fixture_at(scripts.clone(), None, [10, 20, 60000]);
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["control"]["extra_time"] = 10.into();
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    assert!(world.ordinary_actors([0, 60000]).unwrap().contains(&id));
    let advance = |world: &mut ScriptWorld, skipped| {
        world
            .update_ordinary(
                &data,
                [0, 60000],
                OrdinaryInputs {
                    scene: None,
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        actor_update_skipped: Some(skipped),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
    };
    for _ in 0..2 {
        advance(&mut world, true);
        assert_eq!(world.actors()[0].control_state().unwrap().extra_time, 10);
        assert!(world.actors()[0].failure().is_none());
    }
    advance(&mut world, false);
    assert_eq!(world.actors()[0].control_state().unwrap().extra_time, 9);
}
