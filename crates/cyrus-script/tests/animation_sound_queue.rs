//! Public pre-binding sound routing keeps owner/effect stops ahead of admission.
use bevy::prelude::*;
use cyrus_script::script::{ScriptActorId, ScriptSound, sound_dispatch::AnimationSoundQueue};
use cyrus_world::{animation::AnimationSound, sound::SoundCue};
fn actor(index: usize) -> ScriptActorId {
    ScriptActorId {
        generation: 1,
        index,
    }
}
fn play(effect: i16) -> ScriptSound {
    ScriptSound::Animation {
        sound: AnimationSound::Play {
            effect,
            rate_offset_hz: 0,
        },
        surface: None,
        position: Some([0; 3]),
    }
}
#[test]
fn stops_filter_waiting_starts_by_owner_and_requested_effect() {
    for (stop, expected) in [
        (ScriptSound::StopOwned, vec![3]),
        (
            ScriptSound::StopEffect {
                effect: 3,
                owned: true,
            },
            vec![4, 3],
        ),
        (
            ScriptSound::StopEffect {
                effect: 3,
                owned: false,
            },
            vec![4],
        ),
        (ScriptSound::StopAll, vec![]),
    ] {
        let mut queue = AnimationSoundQueue::default();
        let mut cues = Vec::new();
        let mut emit = |cue| {
            cues.push(cue);
            Ok(())
        };
        for (owner, effect) in [(0, 3), (0, 4), (1, 3)] {
            queue
                .send(actor(owner), play(effect), None, &mut emit)
                .unwrap();
        }
        queue.send(actor(0), stop, None, &mut emit).unwrap();
        let mut world = World::new();
        let node = world.spawn_empty().id();
        queue
            .release(1, &mut |_| Ok(Some(node)), &mut emit)
            .unwrap();
        let effects = cues
            .iter()
            .filter_map(|cue| match cue {
                SoundCue::Animation {
                    sound: AnimationSound::Play { effect, .. },
                    ..
                } => Some(*effect),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(effects, expected);
        assert!(queue.is_empty());
    }
}
#[test]
fn forget_and_latest_stop_keep_order_while_owner_stop_cancels_forgotten_starts() {
    let mut queue = AnimationSoundQueue::default();
    let mut cues = Vec::new();
    let mut emit = |cue| {
        cues.push(cue);
        Ok(())
    };
    queue.send(actor(0), play(3), None, &mut emit).unwrap();
    for sound in [AnimationSound::Forget, AnimationSound::Stop] {
        queue
            .send(
                actor(0),
                ScriptSound::Animation {
                    sound,
                    surface: None,
                    position: None,
                },
                None,
                &mut emit,
            )
            .unwrap();
    }
    queue
        .send(actor(0), ScriptSound::StopOwned, None, &mut emit)
        .unwrap();
    let mut world = World::new();
    let node = world.spawn_empty().id();
    queue
        .release(1, &mut |_| Ok(Some(node)), &mut emit)
        .unwrap();
    assert!(matches!(
        &cues[1],
        SoundCue::Animation {
            sound: AnimationSound::Forget,
            ..
        }
    ));
    assert!(matches!(
        &cues[2],
        SoundCue::Animation {
            sound: AnimationSound::Stop,
            ..
        }
    ));
    assert_eq!(cues.len(), 3);
}
#[test]
fn waiting_counts_only_the_requested_generation_and_release_empties_it() {
    let mut queue = AnimationSoundQueue::default();
    let mut emit = |_| Ok(());
    queue.send(actor(0), play(3), None, &mut emit).unwrap();
    queue
        .send(
            ScriptActorId {
                generation: 2,
                index: 1,
            },
            play(4),
            None,
            &mut emit,
        )
        .unwrap();
    assert_eq!(
        (queue.waiting(1), queue.waiting(2), queue.waiting(3)),
        (1, 1, 0)
    );
    let mut world = World::new();
    let node = world.spawn_empty().id();
    queue
        .release(1, &mut |_| Ok(Some(node)), &mut emit)
        .unwrap();
    assert_eq!((queue.waiting(1), queue.waiting(2)), (0, 0));
}
