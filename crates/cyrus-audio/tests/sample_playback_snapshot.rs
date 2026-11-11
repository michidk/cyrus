//! Production channel allocation and restored source-decoder continuation.
use anyhow::Result;
use bevy::{
    audio::{AudioPlayer, AudioSource, Decodable, PlaybackSettings},
    ecs::world::CommandQueue,
    prelude::*,
};
use cyrus_assets::surface::Surface;
use cyrus_assets::{audio::pcm_wav, sound::SoundSettings};
use cyrus_audio::{
    persistence::AudioRestoreBindings,
    sound::{
        Effect, Panned, PlayingSound, SampleRepetitions, SoundEffects, SoundPlugin, SoundRequest,
        persistence::SamplePlaybackSnapshot,
    },
};
use rgpre::import::sfx::AudioType;
use std::collections::BTreeMap;

fn fixture() -> Result<(App, AudioSource)> {
    fixture_with_repetitions(SampleRepetitions::FOREVER)
}

fn fixture_with_repetitions(repetitions: SampleRepetitions) -> Result<(App, AudioSource)> {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), SoundPlugin))
        .init_asset::<AudioSource>()
        .init_asset::<Panned>()
        .insert_resource(cyrus_audio::AudioListener(GlobalTransform::IDENTITY))
        .insert_resource(SoundSettings::parse("[system]\nsound_distance=64"));
    let source = AudioSource {
        bytes: pcm_wav(AudioType::Mono8, 22050, &[160, 180, 200, 220, 240])
            .map_err(anyhow::Error::msg)?
            .into(),
    };
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(source.clone());
    app.insert_resource(SoundEffects(vec![Some(Effect {
        bits_per_sample: 8,
        audio,
        repetitions,
        base_volume: 100,
        sample_rate: 22050,
    })]));
    app.world_mut().write_message(SoundRequest::Play {
        effect: 0,
        position: None,
    });
    app.update();
    Ok((app, source))
}

#[test]
fn sample_candidate_retains_decoder_progress_pause_and_live_pool_until_commit() -> Result<()> {
    check_sample_candidate(SampleRepetitions::FOREVER)
}

#[test]
fn finite_sample_candidate_retains_remaining_passes_after_wrap() -> Result<()> {
    check_sample_candidate(SampleRepetitions::finite(
        std::num::NonZeroU8::new(3).unwrap(),
    ))
}

fn check_sample_candidate(repetitions: SampleRepetitions) -> Result<()> {
    let (mut app, source) = fixture_with_repetitions(repetitions)?;
    let (old_entity, handle) = app
        .world_mut()
        .query::<(Entity, &AudioPlayer<Panned>)>()
        .single(app.world())
        .map(|(entity, player)| (entity, player.0.clone()))?;
    let mut original = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&handle)
        .unwrap()
        .decoder();
    // An odd stereo output count retains the pending right sample of mono input.
    for _ in 0..13 {
        original.next().unwrap();
    }
    app.world_mut()
        .get_mut::<PlaybackSettings>(old_entity)
        .unwrap()
        .paused = true;
    let snapshot = SamplePlaybackSnapshot::capture(app.world_mut())?;
    let before = serde_json::to_value(&snapshot)?;
    if !repetitions.is_infinite() {
        assert_eq!(before["players"][0]["transport"]["repetitions"], 2);
    }
    let replacement = app.world_mut().spawn_empty().id();
    let bindings = AudioRestoreBindings::new(&[], &[(old_entity.to_bits(), replacement)])?;
    let sources = BTreeMap::from([(
        snapshot
            .sources()
            .next()
            .unwrap()
            .source_identity
            .to_owned(),
        source,
    )]);
    let candidate = snapshot.restore(&sources, &bindings, app.world())?;
    assert_eq!(
        serde_json::to_value(SamplePlaybackSnapshot::capture(app.world_mut())?)?,
        before
    );
    assert!(
        app.world()
            .get::<AudioPlayer<Panned>>(replacement)
            .is_none()
    );
    let mut queue = CommandQueue::default();
    app.world_mut()
        .resource_scope(|world, mut assets: Mut<Assets<Panned>>| {
            let mut commands = Commands::new(&mut queue, world);
            candidate.queue(&mut commands, &mut assets);
            commands.entity(old_entity).despawn();
        });
    queue.apply(app.world_mut());
    assert!(app.world().get::<PlayingSound>(old_entity).is_none());
    assert!(
        app.world()
            .get::<PlaybackSettings>(replacement)
            .unwrap()
            .paused
    );
    let handle = &app
        .world()
        .get::<AudioPlayer<Panned>>(replacement)
        .unwrap()
        .0;
    let mut restored = app
        .world()
        .resource::<Assets<Panned>>()
        .get(handle)
        .unwrap()
        .decoder();
    for _ in 0..33 {
        assert_eq!(restored.next(), original.next());
    }
    let after = serde_json::to_value(SamplePlaybackSnapshot::capture(app.world_mut())?)?;
    assert!(
        after["players"][0]["token"].as_u64().unwrap()
            > before["players"][0]["token"].as_u64().unwrap()
    );
    Ok(())
}

#[test]
fn sample_candidate_rejects_missing_source_owner_and_transport_without_live_changes() -> Result<()>
{
    let (mut app, source) = fixture()?;
    let saved = SamplePlaybackSnapshot::capture(app.world_mut())?;
    let before = serde_json::to_value(&saved)?;
    let hint = saved.sources().next().unwrap();
    let replacement = app.world_mut().spawn_empty().id();
    let bindings = AudioRestoreBindings::new(&[], &[(hint.saved_entity, replacement)])?;
    let sources = BTreeMap::from([(hint.source_identity.to_owned(), source)]);
    assert!(
        saved
            .clone()
            .restore(&BTreeMap::new(), &bindings, app.world())
            .is_err()
    );
    assert!(
        saved
            .clone()
            .restore(&sources, &AudioRestoreBindings::new(&[], &[])?, app.world())
            .is_err()
    );
    for field in ["version", "width", "token"] {
        let mut malformed = before.clone();
        if field == "version" {
            malformed[field] = 99.into();
        } else {
            malformed["players"][0][field] = 99.into();
        }
        let malformed: SamplePlaybackSnapshot = serde_json::from_value(malformed)?;
        assert!(malformed.restore(&sources, &bindings, app.world()).is_err());
    }
    assert_eq!(
        serde_json::to_value(SamplePlaybackSnapshot::capture(app.world_mut())?)?,
        before
    );
    let extra = app.world_mut().spawn(PlayingSound).id();
    assert!(SamplePlaybackSnapshot::capture(app.world_mut()).is_err());
    app.world_mut().despawn(extra);
    assert_eq!(
        serde_json::to_value(SamplePlaybackSnapshot::capture(app.world_mut())?)?,
        before
    );
    Ok(())
}

#[test]
fn silent_pool_retains_animation_owner_bindings_after_stop_all() -> Result<()> {
    use cyrus_world::{animation::AnimationSound, sound::SoundCue};
    let (mut app, _) = fixture()?;
    let actor = app.world_mut().spawn(GlobalTransform::IDENTITY).id();
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        surface: Some(Surface::UNKNOWN),
        sound: AnimationSound::Play {
            effect: 0,
            rate_offset_hz: 0,
        },
    });
    app.update();
    app.world_mut().write_message(SoundRequest::StopAll);
    app.update();
    let snapshot = SamplePlaybackSnapshot::capture(app.world_mut())?;
    assert_eq!(snapshot.sources().len(), 0);
    assert_eq!(
        snapshot.entity_owners().into_iter().collect::<Vec<_>>(),
        [actor.to_bits()]
    );
    assert!(
        snapshot
            .clone()
            .restore(
                &BTreeMap::new(),
                &AudioRestoreBindings::new(&[], &[])?,
                app.world()
            )
            .is_err()
    );
    let replacement = app.world_mut().spawn(GlobalTransform::IDENTITY).id();
    let bindings = AudioRestoreBindings::new(&[], &[(actor.to_bits(), replacement)])?;
    let candidate = snapshot.restore(&BTreeMap::new(), &bindings, app.world())?;
    let mut queue = CommandQueue::default();
    app.world_mut()
        .resource_scope(|world, mut assets: Mut<Assets<Panned>>| {
            candidate.queue(&mut Commands::new(&mut queue, world), &mut assets);
        });
    queue.apply(app.world_mut());
    assert_eq!(
        SamplePlaybackSnapshot::capture(app.world_mut())?
            .entity_owners()
            .into_iter()
            .collect::<Vec<_>>(),
        [replacement.to_bits()]
    );
    Ok(())
}

#[test]
fn finished_playback_releases_its_channel_before_capture() -> Result<()> {
    let (mut app, _) = fixture()?;
    let before = serde_json::to_value(SamplePlaybackSnapshot::capture(app.world_mut())?)?;
    // The backend despawns a finished effect after the frame's channel reclamation ran.
    let finished = app
        .world_mut()
        .query_filtered::<Entity, With<PlayingSound>>()
        .single(app.world())?;
    app.world_mut().despawn(finished);
    assert!(SamplePlaybackSnapshot::capture(app.world_mut()).is_err());
    SamplePlaybackSnapshot::reclaim_finished(app.world_mut());
    let after = serde_json::to_value(SamplePlaybackSnapshot::capture(app.world_mut())?)?;
    assert_ne!(after, before);
    assert!(after["players"].as_array().is_some_and(Vec::is_empty));
    Ok(())
}

#[test]
fn restored_control_voice_retains_source_hint_and_stop_slot_with_looping_effect() -> Result<()> {
    use cyrus_assets::sound::VoiceClip;
    use cyrus_audio::sound::Speech;
    let (mut app, effect_source) = fixture()?;
    let voice_source = AudioSource {
        bytes: pcm_wav(
            AudioType::Mono8,
            22050,
            &(0..128).map(|i| 64 + i).collect::<Vec<u8>>(),
        )
        .map_err(anyhow::Error::msg)?
        .into(),
    };
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(voice_source.clone());
    app.world_mut().resource_mut::<Speech>().0.insert(
        "test".into(),
        VoiceClip {
            audio,
            bits_per_sample: 8,
            base_volume: 100,
            seconds: 1.0,
            text: String::new(),
        },
    );
    app.world_mut()
        .write_message(SoundRequest::Voice("test".into()));
    app.update();
    let voice_handle = app
        .world_mut()
        .query::<&AudioPlayer<Panned>>()
        .iter(app.world())
        .find(|player| {
            std::sync::Arc::ptr_eq(
                &app.world()
                    .resource::<Assets<Panned>>()
                    .get(&player.0)
                    .unwrap()
                    .source()
                    .bytes,
                &voice_source.bytes,
            )
        })
        .unwrap()
        .0
        .clone();
    let mut old_voice = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&voice_handle)
        .unwrap()
        .decoder();
    for _ in 0..17 {
        old_voice.next();
    }
    let saved = SamplePlaybackSnapshot::capture(app.world())?;
    let value = serde_json::to_value(&saved)?;
    assert_eq!(
        value["ownership"]["speech"], 1,
        "the voice stop owner must be checkpointed"
    );
    assert!(
        value["players"]
            .as_array()
            .unwrap()
            .iter()
            .any(|player| player["dialogue_label"] == serde_json::json!(b"test"))
    );
    let mut entities = Vec::new();
    for hint in saved.sources() {
        entities.push((hint.saved_entity, app.world_mut().spawn_empty().id()));
    }
    let sources = BTreeMap::from([
        (
            blake3::hash(&effect_source.bytes).to_hex().to_string(),
            effect_source,
        ),
        (
            blake3::hash(&voice_source.bytes).to_hex().to_string(),
            voice_source,
        ),
    ]);
    let voice_saved_entity = value["players"]
        .as_array()
        .unwrap()
        .iter()
        .find(|player| player["dialogue_label"] == serde_json::json!(b"test"))
        .unwrap()["entity"]
        .as_u64()
        .unwrap();
    let voice_replacement = entities
        .iter()
        .find(|(old, _)| *old == voice_saved_entity)
        .unwrap()
        .1;
    let candidate = saved.clone().restore(
        &sources,
        &AudioRestoreBindings::new(&[], &entities)?,
        app.world(),
    )?;
    candidate.validate_publication(app.world())?;
    let old: Vec<_> = app
        .world_mut()
        .query_filtered::<Entity, With<PlayingSound>>()
        .iter(app.world())
        .collect();
    let mut queue = CommandQueue::default();
    app.world_mut()
        .resource_scope(|world, mut assets: Mut<Assets<Panned>>| {
            let mut commands = Commands::new(&mut queue, world);
            candidate.queue(&mut commands, &mut assets);
            for entity in old {
                commands.entity(entity).despawn();
            }
        });
    queue.apply(app.world_mut());
    let restored_handle = app
        .world()
        .get::<AudioPlayer<Panned>>(voice_replacement)
        .unwrap()
        .0
        .clone();
    let mut restored_decoder = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&restored_handle)
        .unwrap()
        .decoder();
    for _ in 0..33 {
        assert_eq!(
            old_voice.next(),
            restored_decoder.next(),
            "voice decoder must resume at the saved cursor"
        );
    }
    app.world_mut().resource_mut::<Speech>().0.clear();
    let restored = serde_json::to_value(SamplePlaybackSnapshot::capture(app.world())?)?;
    assert_eq!(restored["ownership"]["speech"], 1);
    assert!(
        restored["players"]
            .as_array()
            .unwrap()
            .iter()
            .any(|player| player["dialogue_label"] == serde_json::json!(b"test"))
    );
    assert_eq!(restored["players"].as_array().unwrap().len(), 2);
    let mut invalid = value;
    invalid["ownership"]["speech"] = 16.into();
    let invalid: SamplePlaybackSnapshot = serde_json::from_value(invalid)?;
    assert!(
        invalid
            .restore(
                &sources,
                &AudioRestoreBindings::new(&[], &entities)?,
                app.world()
            )
            .is_err()
    );
    app.init_resource::<cyrus_script::item_viewer::ExamineScreen>();
    app.world_mut()
        .resource_mut::<cyrus_script::item_viewer::ExamineScreen>()
        .speech_stop = true;
    app.update();
    let stopped = serde_json::to_value(SamplePlaybackSnapshot::capture(app.world())?)?;
    assert!(stopped["ownership"]["speech"].is_null());
    assert_eq!(
        stopped["players"].as_array().unwrap().len(),
        1,
        "closing speech must retain the looping effect"
    );
    Ok(())
}
