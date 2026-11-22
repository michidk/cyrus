//! Checkpoint ownership across the preliminary native world entry.
use super::*;
use bevy::prelude::{Entity, NextState, State};
use cyrus_game::AppState;
use cyrus_world::lifecycle::{CurrentScene, SceneRequest};
use std::collections::HashSet;

pub(super) enum Checkpoint {
    Capturing,
    Session(Box<RuntimeSessionSnapshot>),
    Startup(Box<Startup>),
}

pub(super) struct Startup {
    entities: HashSet<Entity>,
    actors: cyrus_script::script::ScriptActors,
    hud: cyrus_script::script::ScriptHud,
    clock: Option<cyrus_script::clock::SimulationClock>,
    random: Option<u32>,
    unlocks: Option<cyrus_game::MovieUnlocks>,
    current: CurrentScene,
    request: Option<SceneRequest>,
}

impl Startup {
    pub(super) fn capture(world: &mut World) -> Self {
        let entities = world
            .query_filtered::<Entity, bevy::prelude::Without<bevy::ecs::resource::IsResource>>()
            .iter(world)
            .collect();
        let actors = world.remove_resource().unwrap_or_default();
        let hud = world.remove_resource().unwrap_or_default();
        world.init_resource::<cyrus_script::script::ScriptActors>();
        world.init_resource::<cyrus_script::script::ScriptHud>();
        Self {
            entities,
            actors,
            hud,
            clock: world
                .get_resource::<cyrus_script::clock::SimulationClock>()
                .cloned(),
            random: world
                .get_resource::<cyrus_game::random::EngineRandom>()
                .map(|rng| rng.state()),
            current: world
                .get_resource::<CurrentScene>()
                .cloned()
                .unwrap_or_default(),
            request: world.get_resource::<SceneRequest>().cloned(),
            unlocks: world.get_resource::<cyrus_game::MovieUnlocks>().copied(),
        }
    }

    pub(super) fn restore(self, world: &mut World) -> Result<()> {
        let added: Vec<_> = world
            .query_filtered::<Entity, bevy::prelude::Without<bevy::ecs::resource::IsResource>>()
            .iter(world)
            .filter(|entity| !self.entities.contains(entity))
            .collect();
        for entity in added {
            if world.get_entity(entity).is_ok() {
                world.despawn(entity);
            }
        }
        cyrus_render::persistence::clear_scene(world);
        let mut retired = world
            .remove_resource::<cyrus_script::script::ScriptActors>()
            .unwrap_or_default();
        retired.restore_empty_checkpoint(self.actors)?;
        world.insert_resource(retired);
        world.insert_resource(self.hud);
        world.insert_resource(self.current);
        if let Some(request) = self.request {
            world.insert_resource(request);
        } else {
            world.remove_resource::<SceneRequest>();
        }
        if let Some(clock) = self.clock {
            world.insert_resource(clock);
        } else {
            world.remove_resource::<cyrus_script::clock::SimulationClock>();
        }
        if let Some(unlocks) = self.unlocks {
            world.insert_resource(unlocks);
        } else {
            world.remove_resource::<cyrus_game::MovieUnlocks>();
        }
        if let Some(random) = self.random {
            world.insert_resource(cyrus_game::random::EngineRandom::from_state(random));
        } else {
            world.remove_resource::<cyrus_game::random::EngineRandom>();
        }
        world.remove_resource::<cyrus_world::lifecycle::PreparedWorld>();
        world.remove_resource::<cyrus_world::lifecycle::ReloadScene>();
        world.remove_resource::<cyrus_game::PauseBookEntry>();
        world.remove_resource::<cyrus_script::PausedGameplay>();
        world.remove_resource::<cyrus_game::RestoredSessionEntry>();
        world.remove_resource::<cyrus_world::lifecycle::RestoredWorldIdentity>();
        Ok(())
    }
}

pub(super) fn begin(world: &mut World) -> Result<Checkpoint> {
    if world
        .get_resource::<cyrus_script::script::ScriptActors>()
        .is_some_and(|actors| actors.current.is_some())
    {
        world.resource_mut::<SessionBoundary>().capture_live()?;
        Ok(Checkpoint::Capturing)
    } else {
        Ok(Checkpoint::Startup(Box::new(Startup::capture(world))))
    }
}

pub(super) fn cancel_entry(world: &mut World) {
    crate::scene::cancel_preparation(world);
    world.remove_resource::<cyrus_startup::WorldLoadRequest>();
    world.remove_resource::<cyrus_startup::StartGame>();
    world.remove_resource::<cyrus_world::lifecycle::SceneLoadError>();
}

pub(super) fn reject(state: &mut NativeSessionLoad, world: &mut World) {
    if state.result.as_ref().is_none_or(Result::is_ok) {
        return;
    }
    let Some(checkpoint) = state.original_rollback.take() else {
        return;
    };
    state.original_entry = None;
    state.original = None;
    match checkpoint {
        Checkpoint::Capturing => {} // Entry was never submitted; the old world is untouched.
        Checkpoint::Startup(startup) => {
            cancel_entry(world);
            if let Err(error) = startup.restore(world) {
                let original = state
                    .result
                    .take()
                    .and_then(Result::err)
                    .expect("original rejection");
                state.result = Some(Err(error.context(format!(
                    "restoring startup menu after original rejection: {original:#}"
                ))));
                return;
            }
            // Re-enter Menu even when the failed candidate left an in-game book open.
            let menu = world
                .get_resource::<State<AppState>>()
                .is_some_and(|s| *s.get() == AppState::Menu);
            state.rollback_menu = menu;
            world.resource_mut::<NextState<AppState>>().set(if menu {
                AppState::Loading
            } else {
                AppState::Menu
            });
        }
        Checkpoint::Session(snapshot) => {
            cancel_entry(world);
            state.rollback_error = state.result.take().and_then(Result::err);
            if let Err(error) = start_original_snapshot(state, *snapshot, world) {
                state.result = Some(Err(
                    error.context("restoring previous world after original import rejection")
                ));
            }
            // Publication accepts gameplay or a prepared menu, not a failed Loading state.
            if world
                .get_resource::<State<AppState>>()
                .is_some_and(|s| *s.get() == AppState::Loading)
            {
                world
                    .resource_mut::<NextState<AppState>>()
                    .set(AppState::InGame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_rejection_does_not_cancel_the_previous_world_request() {
        let mut world = World::new();
        world.insert_resource(cyrus_startup::WorldLoadRequest("previous".into()));
        let mut state = NativeSessionLoad {
            original_rollback: Some(Checkpoint::Capturing),
            result: Some(Err(anyhow::anyhow!("checkpoint unavailable"))),
            ..Default::default()
        };
        reject(&mut state, &mut world);
        assert_eq!(
            world.resource::<cyrus_startup::WorldLoadRequest>().0,
            "previous"
        );
        assert!(state.result.unwrap().is_err());
    }

    #[test]
    fn startup_rejection_removes_candidate_world_and_restores_menu_owners() {
        let mut world = World::new();
        world.insert_resource(CurrentScene::default());
        world.insert_resource(SceneRequest("START".into()));
        world.insert_resource(cyrus_game::random::EngineRandom::from_state(73));
        let retained = world.spawn_empty().id();
        let checkpoint = Startup::capture(&mut world);
        let candidate = world.spawn_empty().id();
        world.insert_resource(CurrentScene {
            name: "ISLAND".into(),
            world: Some(1),
        });
        world.insert_resource(SceneRequest("ISLAND".into()));
        world.insert_resource(cyrus_game::random::EngineRandom::from_state(99));
        world.insert_resource(State::new(AppState::Loading));
        world.init_resource::<NextState<AppState>>();
        let mut state = NativeSessionLoad {
            original_rollback: Some(Checkpoint::Startup(Box::new(checkpoint))),
            result: Some(Err(anyhow::anyhow!("synthetic import rejection"))),
            ..Default::default()
        };
        reject(&mut state, &mut world);
        assert!(state.original_rollback.is_none());
        assert!(
            state
                .result
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("synthetic import rejection")
        );
        assert!(matches!(
            world.resource::<NextState<AppState>>(),
            NextState::Pending(AppState::Menu)
        ));
        assert!(world.get_entity(retained).is_ok());
        assert!(world.get_entity(candidate).is_err());
        assert_eq!(world.resource::<CurrentScene>().world, None);
        assert_eq!(world.resource::<SceneRequest>().0, "START");
        assert_eq!(
            world.resource::<cyrus_game::random::EngineRandom>().state(),
            73
        );
        assert!(
            world
                .resource::<cyrus_script::script::ScriptActors>()
                .current
                .is_none()
        );
        assert!(!world.contains_resource::<cyrus_render::ReadyScene>());
    }
}
