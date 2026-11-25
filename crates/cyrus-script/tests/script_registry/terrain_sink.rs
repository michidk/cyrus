use super::*;

#[test]
fn terrain_sink_check_restores_the_stable_position_only_below_the_margin() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    // The terrain is far below the body (fires) or far above it (does not).
    for (height, sinks) in [(0i16, true), (4000, false)] {
        let (mut world, data, mut scene, ids) = animated_bounce_pair_at([600, 20, 600]);
        scene.terrain = Some(
            cyrus_collision::ground::TerrainGround::new(
                vec![height; 256 * 256],
                vec![false; 256 * 256],
                None,
            )
            .unwrap(),
        );
        let before = world.spatial(ids[0]).unwrap().local.position;
        assert_ne!((before[0], before[2]), (0, 0));
        world.set_falling_for_test(ids[0], JumpMotion::STANDING, before[1]);
        // The stable record is allocated at zero and not valid; the restore ignores its validity.
        world
            .update_ordinary(
                &data,
                [10, 30],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        fine_ticks: Some(1),
                        step_fps: Some(30),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[0].failure().is_none(),
            "{:?}",
            world.actors()[0].failure()
        );
        let after = world.spatial(ids[0]).unwrap().local.position;
        let control = world.actors()[0].control_state().unwrap();
        if sinks {
            assert_eq!((after[0], after[2]), (0, 0), "sample {height}");
            assert_eq!(control.move_counter, 0);
        } else {
            assert_eq!(after[0], before[0], "sample {height}");
            assert_eq!(after[2], before[2], "sample {height}");
        }
    }
}
