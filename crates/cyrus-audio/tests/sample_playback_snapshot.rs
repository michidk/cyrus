//! Production channel allocation and restored source-decoder continuation.
use anyhow::Result;
use bevy::{
    audio::{AudioPlayer, AudioSource, Decodable, PlaybackSettings},
    ecs::world::CommandQueue,
    prelude::*,
};
use cyrus_assets::{audio::pcm_wav, sound::SoundSettings};
use cyrus_audio::{
    persistence::AudioRestoreBindings,
    sound::{
        Effect, Panned, PlayingSound, SoundEffects, SoundPlugin, SoundRequest,
        persistence::SamplePlaybackSnapshot,
    },
};
use rgpre::import::sfx::AudioType;
use std::collections::BTreeMap;

fn fixture() -> Result<(App, AudioSource)> {
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
        looping: true,
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
    let (mut app, source) = fixture()?;
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
    for _ in 0..7 {
        original.next().unwrap();
    }
    app.world_mut()
        .get_mut::<PlaybackSettings>(old_entity)
        .unwrap()
        .paused = true;
    let snapshot = SamplePlaybackSnapshot::capture(app.world_mut())?;
    let before = serde_json::to_value(&snapshot)?;
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
        surface: Some(0),
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
