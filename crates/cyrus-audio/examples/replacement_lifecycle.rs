//! Installed animation replacement through the public deferred scene-binding queue.
//! The caller supplies exported metadata; no installed content is embedded here.
use cyrus_script::script::{ScriptActorId, sound_dispatch::AnimationSoundQueue};
use cyrus_soup::AnimationGroup;
use cyrus_world::animation::{ActorAnimation, AnimationSound, Animations};
use cyrus_world::sound::{ScriptSound, SoundCue};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    animations: Animations,
    actor: String,
    source: u16,
    destination: u16,
}
#[derive(Clone, Copy, Debug)]
enum Replacement {
    Forced,
    Ordinary,
    SkippedFrames,
    Default,
}
impl Replacement {
    fn apply(
        self,
        actor: &mut ActorAnimation,
        destination: AnimationGroup,
        dispatch: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<u32>> {
        match self {
            Self::Forced => actor.force_request_with_sounds(destination, 0, 1, dispatch),
            Self::Ordinary => actor.request_with_sounds(destination, 0, 1, dispatch),
            Self::SkippedFrames => {
                actor.request_skipping_frames_with_sounds(destination, 0, 0, dispatch)
            }
            Self::Default => actor.select_default_with_sounds(destination, 1, dispatch),
        }
    }
}
fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    let text = std::fs::read_to_string(
        std::env::var("CYRUS_ANIMATION_FIXTURE").expect("explicit installed metadata required"),
    )
    .unwrap();
    #[cfg(target_arch = "wasm32")]
    let text = fixture_text();
    let fixture: Fixture = serde_json::from_str(&text).unwrap();
    let source = AnimationGroup::from_index(fixture.source);
    let destination = AnimationGroup::from_index(fixture.destination);
    assert_ne!(source, destination);
    let id = ScriptActorId {
        generation: 1,
        index: 0,
    };
    let mut rows = Vec::new();
    for branch in [
        Replacement::Forced,
        Replacement::Ordinary,
        Replacement::SkippedFrames,
        Replacement::Default,
    ] {
        for reject in [false, true] {
            let mut actor =
                ActorAnimation::from_asset(&fixture.animations, &fixture.actor, source).unwrap();
            let mut queue = AnimationSoundQueue::default();
            let mut played = false;
            for _ in 0..256 {
                actor
                    .advance_with_sounds(1, &mut |sound| {
                        played |= matches!(sound, AnimationSound::Play { .. });
                        send(&mut queue, id, sound)
                    })
                    .unwrap();
                if played {
                    break;
                }
            }
            assert!(
                played,
                "installed source must emit a sound before replacement"
            );
            // Ordinary zero-tick updates query the remembered sample while the actor
            // is still waiting for a presentation node. Fill the real bounded queue.
            if reject {
                for _ in 0..256 {
                    if queue.len() == 64 {
                        break;
                    }
                    actor
                        .advance_with_sounds(0, &mut |sound| send(&mut queue, id, sound))
                        .unwrap();
                }
                assert_eq!(queue.len(), 64);
            }
            let mut attempts = 0;
            let result = branch.apply(&mut actor, destination, &mut |sound| {
                attempts += 1;
                send(&mut queue, id, sound)
            });
            if reject {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("64 sounds already wait for a node"));
                assert_eq!(actor.failure(), Some(error.as_str()));
                assert_eq!(attempts, 1);
                let mut restored = ActorAnimation::from_snapshot(
                    &fixture.animations,
                    &fixture.actor,
                    actor.snapshot(),
                )
                .unwrap();
                for receiver in [&mut actor, &mut restored] {
                    assert_eq!(
                        receiver
                            .advance_with_sounds(1, &mut |_| panic!("failed owner replayed"))
                            .unwrap_err()
                            .to_string(),
                        error
                    );
                    assert_eq!(
                        branch
                            .apply(receiver, destination, &mut |_| panic!(
                                "replacement retried"
                            ))
                            .unwrap_err()
                            .to_string(),
                        error
                    );
                }
                assert_eq!(queue.len(), 64);
            } else {
                result.unwrap();
                assert_eq!(actor.current_group(), Some(destination));
                assert!(actor.failure().is_none());
            }
            let mut released = Vec::new();
            queue
                .release(
                    id.generation,
                    &mut |_| Ok(Some(bevy::prelude::Entity::PLACEHOLDER)),
                    &mut |cue| {
                        if let SoundCue::Animation { sound, .. } = cue {
                            released.push(sound);
                        }
                        Ok(())
                    },
                )
                .unwrap();
            assert!(queue.is_empty());
            assert_eq!(
                released
                    .iter()
                    .filter(|sound| matches!(sound, AnimationSound::Play { .. }))
                    .count(),
                1
            );
            assert_eq!(
                released
                    .iter()
                    .filter(|sound| matches!(sound, AnimationSound::Forget))
                    .count(),
                usize::from(!reject)
            );
            rows.push(serde_json::json!({
                "branch":format!("{branch:?}"), "rejected":reject,
                "released":released.len(), "plays":1, "replayed":false,
                "snapshot_failure_retained":reject,
            }));
        }
    }
    report(&serde_json::json!({"replacement_lifecycle":rows}).to_string());
}
fn send(
    queue: &mut AnimationSoundQueue,
    actor: ScriptActorId,
    sound: AnimationSound,
) -> anyhow::Result<()> {
    queue.send(
        actor,
        ScriptSound::Animation {
            sound,
            surface: None,
            position: None,
        },
        None,
        &mut |_| panic!("unbound actor emitted directly"),
    )
}
#[cfg(not(target_arch = "wasm32"))]
fn report(text: &str) {
    println!("{text}");
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=globalThis,js_name=captureFixture)]
    fn fixture_text() -> String;
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=globalThis,js_name=captureResult)]
    fn report(text: &str);
}
