use super::*;
use cyrus_assets::effects::{EffectImage, Effects};
use cyrus_game::{
    clock::ClockStep,
    random::EngineRandom,
    script::{
        ScriptActorSource, ScriptHud, ScriptInputs, ScriptSound, ScriptStage, WorldSnapshotSources,
        camera::CameraPose,
    },
};

fn source(capacity: u32, disabled: bool) -> Effects {
    Effects {
        capacity,
        disabled,
        images: BTreeMap::from([(
            0x2500,
            EffectImage {
                name: "synthetic-death".into(),
                size: [1, 4],
                texture_scale: 256,
                delay_ms: 71,
                flags: 1,
                alpha: 151,
                frames: vec![vec![255; 16]; 4],
            },
        )]),
    }
}

#[test]
fn routed_fatal_health_emits_retained_effects_and_rejects_lossy_capture() {
    for (capacity, disabled) in [(2, false), (1, false), (0, false), (2, true)] {
        let (scripts, mut world, data, scene) = player_walking_fixture_with(|scripts| {
            for (index, name, value) in [(3, "health", 40), (105, "skip_death", 1)] {
                scripts.definition.attributes[index] = name.into();
                scripts
                    .actors
                    .get_mut("CYRUS")
                    .unwrap()
                    .attributes
                    .insert(name.into(), value);
            }
            let function_id = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: "SetHealth".into(),
                param_count: 1,
                kind: CallKind::Function,
            });
            scripts
                .actors
                .get_mut("LOGIC")
                .unwrap()
                .program
                .steps
                .get_mut(&0)
                .unwrap()
                .operation = Operation::ObjectCall {
                call: Call {
                    function_id,
                    function: "SetHealth".into(),
                    kind: CallKind::Function,
                    target: Object::Player,
                    arguments: vec![Value::Integer { value: 0 }],
                },
            };
        });
        let player = world
            .id(world
                .actors()
                .iter()
                .position(|actor| matches!(actor.source, ScriptActorSource::Player))
                .unwrap())
            .unwrap();
        world
            .advance(
                player,
                ScriptStage::Entry,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap();
        world.set_actor_surface(player, 2).unwrap();
        world
            .initialize_camera_pose(CameraPose {
                position: [0; 3],
                angles: [0; 3],
            })
            .unwrap();
        let hud = ScriptHud::default();
        let mut random = EngineRandom::from_state(42);
        world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    hud: Some(&hud),
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    death_suppressed: Some(false),
                    ..Default::default()
                },
                None,
                Some(&mut random),
                100,
            )
            .unwrap();
        assert_eq!(
            world
                .globals()
                .selected_health()
                .unwrap()
                .death()
                .unwrap()
                .mode(),
            2
        );
        let prepared = world
            .prepare_ordinary_collision_query(
                player,
                &scene,
                &data,
                [10, 30],
                &[player, world.id(0).unwrap()],
            )
            .unwrap();
        let effects = source(capacity, disabled);
        world.prepare_environment_effects(&effects).unwrap();
        let mut sounds = Vec::new();
        for _ in 0..8 {
            world
                .advance_player_controller(
                    player,
                    cyrus_game::script::OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 4,
                        sidedness: false,
                    },
                    ClockStep {
                        nominal_ticks: 0,
                        frame_scale: 256,
                        fine_ticks: 0,
                    },
                    ScriptInputs {
                        hud: Some(&hud),
                        ..Default::default()
                    },
                    &mut random,
                    &mut |id, sound| {
                        assert_eq!(id, player);
                        sounds.push(sound);
                        Ok(())
                    },
                )
                .unwrap();
        }
        let active = world
            .environmental_effects()
            .unwrap()
            .active()
            .map(|(slot, effect)| (slot, effect.descriptor, effect.position, effect.frame))
            .collect::<Vec<_>>();
        if disabled || capacity == 0 {
            assert!(active.is_empty());
        } else {
            assert_eq!(active.len(), capacity as usize);
            assert_eq!(
                active,
                [(0, 0x2500, [8, 22, 23], 0), (1, 0x2500, [5, 22, 24], 0)][..capacity as usize]
            );
            let before = active.clone();
            world.prepare_environment_effects(&effects).unwrap();
            assert_eq!(
                world
                    .environmental_effects()
                    .unwrap()
                    .active()
                    .map(|(slot, effect)| (slot, effect.descriptor, effect.position, effect.frame))
                    .collect::<Vec<_>>(),
                before
            );
            let mut changed = effects.clone();
            changed.images.get_mut(&0x2500).unwrap().alpha = 152;
            assert!(world.prepare_environment_effects(&changed).is_err());
            let error = world
                .snapshot(&WorldSnapshotSources {
                    player: Some(&scene),
                    scripts: &scripts,
                    animations: &data,
                    collision: &scene.collision,
                })
                .err()
                .unwrap();
            assert!(
                format!("{error:#}").contains("saving active environmental effects is unavailable")
            );
            world.advance_environment_effects(3);
            assert!(
                world
                    .environmental_effects()
                    .unwrap()
                    .active()
                    .all(|(_, effect)| effect.frame == 3)
            );
            world.advance_environment_effects(1);
            assert!(world.environmental_effects().unwrap().is_empty());
        }
        assert_eq!(random.state(), 2681529084);
        assert_eq!(sounds.len(), 2);
        assert!(matches!(
            sounds[0],
            ScriptSound::Positional {
                rate_offset_hz: -28,
                ..
            }
        ));
        assert!(matches!(
            sounds[1],
            ScriptSound::Positional {
                rate_offset_hz: -12,
                ..
            }
        ));
        assert!(sounds.iter().all(|sound| matches!(
            sound,
            ScriptSound::Positional {
                effect: 71,
                volume: 0,
                position: [2560, -1536, 7680],
                ..
            }
        )));
    }
}
