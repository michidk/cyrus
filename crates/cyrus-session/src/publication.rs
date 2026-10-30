//! Exclusive publication of the prepared runtime owners.
use super::*;
use bevy::audio::AudioPlayer;
use bevy::ecs::world::CommandQueue;
use bevy::prelude::{Commands, Mut, Or};
use cyrus_audio::music::{MusicPlayer, MusicTransport};
use cyrus_audio::sound::{Panned, PlayingSound};
use cyrus_world::lifecycle::{PreparationProgress, SceneLoadError};

impl RuntimeSessionCandidate {
    /// Publish at an exclusive scheduler boundary after deferred commands and cues
    /// have been drained. Rejection leaves live owners untouched. This draft composes
    /// the represented owners; rendered preparation, revisit acceptance and host
    /// admission remain required before exposing a load action.
    pub fn commit(self, world: &mut World) -> Result<()> {
        self.publish(world, false)
    }

    pub(super) fn publish(mut self, world: &mut World, saved_game: bool) -> Result<()> {
        self.validate_publication(world)?;
        let same_world = saved_game
            && world
                .get_resource::<cyrus_world::lifecycle::CurrentScene>()
                .is_some_and(|current| current.world == self.lifecycle.current.world);
        if same_world {
            self.lifecycle.prepared.requested_name.clone_from(
                &world
                    .resource::<cyrus_world::lifecycle::CurrentScene>()
                    .name,
            );
        }
        if saved_game {
            self.dialogue.reset_saved_world(
                self.logical
                    .actors
                    .current
                    .as_mut()
                    .context("saved game current world absent")?,
                &mut self.samples,
            )?;
        }
        let entering = world.resource::<State<AppState>>().get() == &AppState::Menu;
        let old: Vec<_> = world
            .query_filtered::<Entity, Or<(
                With<Player>,
                With<ScriptBinding>,
                With<PlayingSound>,
                With<MusicPlayer>,
                With<cyrus_game::script::ScriptedPlayback>,
                With<cyrus_render::camera::ViewCamera>,
            )>>()
            .iter(world)
            .filter(|entity| {
                Some(*entity) != self.camera_entity
                    && !(saved_game && world.get::<MusicPlayer>(*entity).is_some())
            })
            .collect();
        let Self {
            sources,
            source_identities: _,
            lifecycle,
            scene,
            camera_entity,
            logical,
            player,
            entity_bindings,
            placement,
            runtime,
            samples,
            dialogue,
            music,
            cheats,
            movie_unlocks,
            hud,
            fidelity,
            presentation_clock,
            environment_edits,
            camera,
            sky,
            render_playback,
            player_model,
            actor_models,
            marker,
        } = self;
        let mut queue = CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, world);
            // Retire only after every preflight has succeeded. Ancestor validation
            // prevents these despawns from consuming prepared replacement entities.
            commands.queue(move |world: &mut World| {
                for entity in old {
                    if world.get_entity(entity).is_ok() {
                        world.despawn(entity);
                    }
                }
            });
            let sky_entity = scene.as_ref().map(|scene| scene.sky());
            if let Some(scene) = scene {
                commands.entity(scene.root()).remove::<(
                    crate::staging::StagedSessionEntity,
                    cyrus_render::persistence::StagedRenderRoot,
                )>();
                scene.queue(&mut commands);
            }
            if let (Some(camera), Some(entity)) = (camera, camera_entity) {
                commands.entity(entity).remove::<(
                    crate::staging::StagedSessionEntity,
                    cyrus_render::persistence::StagedRenderRoot,
                )>();
                camera.queue(&mut commands, entity);
            }
            if let (Some(sky), Some(entity)) = (sky, sky_entity) {
                commands
                    .entity(entity)
                    .remove::<crate::staging::StagedSessionEntity>()
                    .insert(bevy::prelude::Visibility::Inherited);
                sky.queue(&mut commands, entity);
            }
            if let Some(playback) = render_playback {
                playback.queue(&mut commands);
            }
            for model in player_model.into_iter().chain(actor_models) {
                commands.entity(model.root()).remove::<(
                    crate::staging::StagedSessionEntity,
                    cyrus_render::persistence::StagedRenderRoot,
                )>();
                model.queue(&mut commands);
            }
            for binding in entity_bindings {
                binding.queue(&mut commands, player.entity);
            }
            for entity in samples.entities() {
                commands
                    .entity(entity)
                    .remove::<crate::staging::StagedSessionEntity>();
            }
            commands.entity(player.entity).insert((Player, placement));
            PlayerSceneRequest(player.scene).queue_restored(&mut commands, &lifecycle.prepared);
            commands.insert_resource(AnimationRequest(player.animation));
            if same_world {
                lifecycle.prepared.queue_restored(&mut commands);
            } else {
                commands.insert_resource(lifecycle.prepared);
                commands.remove_resource::<cyrus_world::lifecycle::RestoredWorldIdentity>();
                commands.insert_resource(lifecycle.current);
                commands.insert_resource(lifecycle.request);
            }
            commands.remove_resource::<PreparationProgress>();
            if let Some(error) = lifecycle.error {
                commands.insert_resource(error);
            } else {
                commands.remove_resource::<SceneLoadError>();
            }
            commands.insert_resource(sources.into_catalog());
            commands.insert_resource(logical.actors);
            commands.insert_resource(logical.hud);
            commands.insert_resource(logical.clock);
            commands.insert_resource(logical.random);
            commands.insert_resource(logical.toggles);
            commands.insert_resource(runtime);
            commands.insert_resource(marker);
            commands.insert_resource(dialogue);
            if !saved_game {
                commands.insert_resource(music.music);
            }
            if let Some(cheats) = cheats {
                commands.queue(move |world: &mut World| {
                    cheats.publish(&mut world.resource_mut::<cyrus_game::cheats::Cheats>());
                });
            }
            if let Some(unlocks) = movie_unlocks {
                commands.insert_resource(unlocks);
            }
            if let Some(hud) = hud {
                commands.insert_resource(hud);
            }
            if let Some(fidelity) = fidelity {
                commands.insert_resource(fidelity);
            }
            if let Some(clock) = presentation_clock {
                commands.insert_resource(clock);
            } else {
                commands.remove_resource::<PresentationClock>();
            }
            if let Some(edits) = environment_edits {
                edits.queue(&mut commands);
            }
        }
        world.resource_scope(|world, mut assets: Mut<Assets<Panned>>| {
            samples.queue(&mut Commands::new(&mut queue, world), &mut assets);
        });
        if let Some((transport, settings)) = music.playback.filter(|_| !saved_game) {
            let handle = world
                .resource_mut::<Assets<MusicTransport>>()
                .add(transport);
            Commands::new(&mut queue, world).spawn((MusicPlayer, AudioPlayer(handle), settings));
        }
        queue.apply(world);
        if entering {
            world.insert_resource(cyrus_game::RestoredSessionEntry);
            world
                .resource_mut::<bevy::prelude::NextState<AppState>>()
                .set(AppState::InGame);
        }
        Ok(())
    }
}
