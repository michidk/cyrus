//! World teardown retires the shared sample pool, independent of per-actor ownership.
use bevy::prelude::*;
use cyrus_audio::sound::{Effect, PlayingSound, SampleRepetitions, SoundEffects, SoundPlugin};
use cyrus_script::script::ScriptActors;
use cyrus_world::sound::SoundCue;

#[path = "support/sound_callers.rs"]
mod callers;

#[test]
fn world_release_stops_unowned_samples_and_allows_a_later_world_start() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, SoundPlugin))
        .init_resource::<ScriptActors>()
        .insert_resource(SoundEffects(vec![Some(Effect {
            audio: Handle::default(),
            bits_per_sample: 8,
            base_volume: 64,
            sample_rate: 22050,
            repetitions: SampleRepetitions::FOREVER,
        })]));
    app.world_mut().resource_mut::<ScriptActors>().current =
        Some(callers::separate_sound_world(false));
    app.update();
    let start = |app: &mut App| {
        app.world_mut().write_message(SoundCue::Flat {
            effect: 0,
            volume: 0,
            rate_offset_hz: 0,
        });
        app.update();
    };
    let count = |app: &mut App| {
        app.world_mut()
            .query_filtered::<Entity, With<PlayingSound>>()
            .iter(app.world())
            .count()
    };
    start(&mut app);
    assert_eq!(count(&mut app), 1);
    app.world_mut().resource_mut::<ScriptActors>().current = None;
    app.update();
    assert_eq!(
        count(&mut app),
        0,
        "released world left an unowned sample audible"
    );
    app.world_mut().resource_mut::<ScriptActors>().current =
        Some(callers::separate_sound_world(false));
    start(&mut app);
    assert_eq!(count(&mut app), 1, "new world starts must survive teardown");
    app.world_mut().resource_mut::<ScriptActors>().current =
        Some(callers::separate_sound_world_at_generation(false, 18));
    app.update();
    assert_eq!(
        count(&mut app),
        0,
        "direct replacement must also retire the previous pool"
    );
    start(&mut app);
    assert_eq!(count(&mut app), 1);
}
