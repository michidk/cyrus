use super::*;

#[test]
fn positional_sound_uses_the_routed_receivers_pose_and_returns_zero() {
    use cyrus_script::script::{ScriptSound, ScriptStage};
    let mut scripts = random_scripts(
        Native::Sound,
        vec![
            Value::Integer { value: 7 },
            Value::Integer { value: -40 },
            Value::Integer { value: 2000 },
        ],
        Object::Named { string: 0 },
    );
    scripts.definition.functions.last_mut().unwrap().param_count = 3;
    scripts.instances[0].map_position = [3, 4, 5];
    scripts.instances[1].map_position = [10, 20, 30];
    scripts.content_id = scripts.content_identity().unwrap();
    for surface in [0, 1, 6, 7, 12, 255] {
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        world.refresh_families().unwrap();
        let caller = world.id(1).unwrap();
        let receiver = world.id(0).unwrap();
        world
            .set_actor_surface(receiver, Surface::from_byte(surface))
            .unwrap();
        let mut cues = Vec::new();
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                cyrus_script::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                Some(&mut |actor, sound| {
                    cues.push((actor, sound));
                    Ok(())
                }),
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            cues,
            [(
                receiver,
                ScriptSound::Positional {
                    effect: 7,
                    volume: -40,
                    rate_offset_hz: 2000,
                    position: [768, 1024, 1280],
                    start_index: None,
                }
            )]
        );
        assert_eq!(world.actors()[1].runtime().unwrap().variables(), [0]);
    }
}
