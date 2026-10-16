//! Public identity contracts for host-prepared audio replacements.
use bevy::prelude::World;
use cyrus_audio::persistence::AudioRestoreBindings;
use cyrus_game::script::ScriptActorId;

fn actor(generation: u64, index: usize) -> ScriptActorId {
    ScriptActorId { generation, index }
}

#[test]
fn audio_bindings_resolve_current_and_visited_owners_without_defaults() -> anyhow::Result<()> {
    let mut world = World::new();
    let first = world.spawn_empty().id();
    let second = world.spawn_empty().id();
    let entity_count = world.entities().len();
    let bindings = AudioRestoreBindings::new(
        &[
            (actor(3, 0), actor(11, 0)),
            (actor(3, 1), actor(11, 1)),
            (actor(7, 0), actor(12, 0)),
        ],
        &[(10, first), (20, second)],
    )?;
    assert_eq!(bindings.actor(actor(7, 0))?, actor(12, 0));
    assert_eq!(bindings.generation(3)?, 11);
    assert_eq!(bindings.entity(20)?, second);
    assert!(bindings.actor(actor(3, 2)).is_err());
    assert!(bindings.generation(8).is_err());
    assert!(bindings.entity(30).is_err());
    assert_eq!(world.entities().len(), entity_count);
    Ok(())
}

#[test]
fn audio_bindings_reject_aliases_duplicates_and_split_generations() {
    for actors in [
        vec![(actor(3, 0), actor(11, 0)), (actor(3, 0), actor(11, 1))],
        vec![(actor(3, 0), actor(11, 0)), (actor(3, 1), actor(11, 0))],
        vec![(actor(3, 0), actor(11, 0)), (actor(3, 1), actor(12, 1))],
        vec![(actor(3, 0), actor(11, 0)), (actor(7, 1), actor(11, 1))],
    ] {
        assert!(AudioRestoreBindings::new(&actors, &[]).is_err());
    }
    let mut world = World::new();
    let first = world.spawn_empty().id();
    let second = world.spawn_empty().id();
    let entity_count = world.entities().len();
    assert!(AudioRestoreBindings::new(&[], &[(10, first), (20, first)]).is_err());
    assert!(AudioRestoreBindings::new(&[], &[(10, first), (10, second)]).is_err());
    assert_eq!(world.entities().len(), entity_count);
}
