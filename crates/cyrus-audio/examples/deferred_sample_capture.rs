//! Browser bank-admission checkpoint acceptance with explicitly supplied installed PCM.
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("deferred_sample_capture requires the browser target and an explicit fixture");
}
#[cfg(target_arch = "wasm32")]
fn main() {
    browser::run();
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use bevy::{audio::AudioPlugin, ecs::world::CommandQueue, prelude::*};
    use cyrus_audio::{
        SoundBankHandle,
        persistence::AudioRestoreBindings,
        sound::{
            Effect, Panned, PlayingSound, SampleRepetitions, SoundEffects, SoundPlugin,
            SoundRequest, persistence::SamplePlaybackSnapshot,
        },
    };
    use cyrus_script::script::ScriptActorId;
    use cyrus_world::sound::{ScriptSound, SoundCue};
    use std::{collections::BTreeMap, time::Duration};
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = globalThis)]
        fn captureFixture() -> String;
        #[wasm_bindgen(js_namespace = globalThis)]
        fn captureAction() -> String;
        #[wasm_bindgen(js_namespace = console, js_name = log)]
        fn report(text: &str);
    }
    #[derive(serde::Deserialize)]
    struct Fixture {
        wav: Vec<u8>,
        bits: u16,
        sample_rate: u32,
        loop_flag: i8,
    }
    #[derive(Clone, Copy, serde::Deserialize, serde::Serialize)]
    #[serde(rename_all = "snake_case")]
    enum Action {
        Defer,
        Admit,
        Start,
        Stop,
        Capture,
        Reject,
        Restore,
    }
    #[derive(Resource)]
    struct Probe {
        source: AudioSource,
        saved: Option<SamplePlaybackSnapshot>,
        action: Option<Action>,
    }
    pub fn run() {
        let fixture: Fixture = serde_json::from_str(&captureFixture()).unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins.set(bevy::app::ScheduleRunnerPlugin::run_loop(
                Duration::from_millis(10),
            )),
            AssetPlugin::default(),
            AudioPlugin::default(),
            SoundPlugin,
        ))
        .insert_resource(cyrus_audio::AudioReadiness { prepare: false })
        .insert_resource(cyrus_assets::sound::SoundSettings::parse(""));
        let source = AudioSource {
            bytes: fixture.wav.into(),
        };
        let audio = app
            .world_mut()
            .resource_mut::<Assets<AudioSource>>()
            .add(source.clone());
        app.insert_resource(SoundEffects(vec![Some(Effect {
            audio,
            bits_per_sample: fixture.bits,
            sample_rate: fixture.sample_rate,
            base_volume: 64,
            repetitions: SampleRepetitions::from_sfx_flag(fixture.loop_flag),
        })]))
        .insert_resource(Probe {
            source,
            saved: None,
            action: None,
        })
        .add_systems(First, command)
        .add_systems(Last, observe);
        app.run();
    }
    fn command(world: &mut World) {
        let request = captureAction();
        if request.is_empty() {
            return;
        }
        let action: Action = serde_json::from_str(&request).expect("invalid fixture action");
        match action {
            Action::Defer => {
                // The fixture holds only the browser bank-admission gate. Cue enqueue,
                // cancellation, dispatch and checkpoint code remain production owners.
                world.insert_resource(SoundBankHandle(Handle::default()));
                start(world);
            }
            Action::Admit => {
                world.remove_resource::<SoundBankHandle>();
            }
            Action::Start => start(world),
            Action::Stop => {
                world.write_message(SoundRequest::StopAll);
            }
            Action::Capture | Action::Reject | Action::Restore => {}
        }
        world.resource_mut::<Probe>().action = Some(action);
    }
    fn start(world: &mut World) {
        world.write_message(SoundCue::Script {
            generation: 17,
            index: 0,
            sound: ScriptSound::Flat {
                effect: 0,
                volume: 0,
                rate_offset_hz: 0,
            },
        });
    }
    fn observe(world: &mut World) {
        let Some(action) = world.resource_mut::<Probe>().action.take() else {
            return;
        };
        let result = match action {
            Action::Capture => SamplePlaybackSnapshot::capture(world).map(|snapshot| {
                world.resource_mut::<Probe>().saved = Some(snapshot);
            }),
            Action::Reject | Action::Restore => restore(world, action),
            _ => Ok(()),
        };
        let count = world
            .query_filtered::<Entity, With<PlayingSound>>()
            .iter(world)
            .count();
        report(
            &serde_json::json!({"deferred_capture":true,"action":action,
            "error":result.err().map(|error|format!("{error:#}")),"playing":count})
            .to_string(),
        );
    }
    fn restore(world: &mut World, action: Action) -> anyhow::Result<()> {
        let snapshot = world
            .resource::<Probe>()
            .saved
            .clone()
            .expect("capture first");
        let source = world.resource::<Probe>().source.clone();
        let sources = if matches!(action, Action::Reject) {
            BTreeMap::new()
        } else {
            BTreeMap::from([(blake3::hash(&source.bytes).to_hex().to_string(), source)])
        };
        let mut entities = Vec::new();
        for source in snapshot.sources() {
            entities.push((source.saved_entity, world.spawn_empty().id()));
        }
        let actor = ScriptActorId {
            generation: 17,
            index: 0,
        };
        let bindings = AudioRestoreBindings::new(&[(actor, actor)], &entities)?;
        let candidate = match snapshot.restore(&sources, &bindings, world) {
            Ok(candidate) => candidate,
            Err(error) => {
                for (_, entity) in entities {
                    world.despawn(entity);
                }
                return Err(error);
            }
        };
        let old = world
            .query_filtered::<Entity, With<PlayingSound>>()
            .iter(world)
            .collect::<Vec<_>>();
        let mut queue = CommandQueue::default();
        world.resource_scope(|world, mut assets: Mut<Assets<Panned>>| {
            let mut commands = Commands::new(&mut queue, world);
            for entity in old {
                commands.entity(entity).despawn();
            }
            candidate.queue(&mut commands, &mut assets);
        });
        queue.apply(world);
        Ok(())
    }
}
