//! Logical actor inspection and explicit script-stage commands.
use super::script_stage::{
    binding_entity, execute_stage, player_query, prepare_stage, script_inputs,
};
use super::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Gameplay<'w, 's> {
    pub(super) script_hud: Option<Res<'w, cyrus_script::script::ScriptHud>>,
    pub(super) script_actors: Option<ResMut<'w, cyrus_script::script::ScriptActors>>,
    pub(super) random: ResMut<'w, cyrus_game::random::EngineRandom>,
    pub(super) cheats: ResMut<'w, cyrus_game::cheats::Cheats>,
    pub(super) activation: Option<Res<'w, cyrus_script::script::ActivationInputs>>,
    pub(super) animations: Res<'w, Assets<cyrus_assets::animation::Animations>>,
    pub(super) script_nodes: Query<'w, 's, (Entity, &'static Name)>,
    pub(super) binding_nodes: Query<'w, 's, (Entity, &'static cyrus_script::script::ScriptBinding)>,
    pub(super) script_sounds: Option<ResMut<'w, Messages<cyrus_world::sound::SoundCue>>>,
    pub(super) player_poses:
        Query<'w, 's, &'static cyrus_script::player::placement::PlayerPose, With<Player>>,
}

impl Gameplay<'_, '_> {
    pub(super) fn handle(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
        scene: &Scene,
        acquisition: &Acquisition,
        script_bindings: &mut std::collections::BTreeMap<
            Entity,
            cyrus_script::script::ScriptActorId,
        >,
    ) -> Outcome {
        match command {
            command @ Command::Cheat { .. } => self.cheat(command),
            command @ Command::Console { .. } => self.console(command, request, commands, scene),
            command @ Command::SetAttribute { .. } => self.set_attribute(command),
            command @ Command::MarkDead { .. } => self.mark_dead(command),
            command @ Command::SetTorch { .. } => self.set_torch(command),
            command @ Command::ShowItem { .. } => self.show_item(command),
            command @ Command::ResetCamera => self.reset_camera(command),
            command @ Command::SelectItem { .. } => self.select_item(command),
            command @ Command::PlacePlayer { .. } => self.place_player(command, scene, acquisition),
            command @ Command::Scripts { .. } => self.scripts(command),
            command @ Command::QueueMovie { .. } => self.queue_movie(command),
            command @ Command::ScriptLoadWorld { .. } => self.script_load_world(command),
            command @ Command::ScriptSummary { .. } => self.script_summary(command),
            command @ Command::BindScriptAnimation { .. } => {
                self.bind_script_animation(command, commands, scene, script_bindings)
            }
            command @ Command::StepScript { .. } => self.step_script(command, script_bindings),
            _ => unreachable!("command routed to the wrong gameplay handler"),
        }
    }

    fn cheat(&mut self, command: Command) -> Outcome {
        let Self { cheats, .. } = self;
        Outcome::Reply(match command {
            Command::Cheat { name, value } => cheats
                .execute(&name, value)
                .map(|effect| json!({"effect":effect,"cheats":cheats.inspect()}))
                .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong cheat handler"),
        })
    }

    fn console(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
        scene: &Scene,
    ) -> Outcome {
        let Reply { id, reply, .. } = request;
        let Scene { state, .. } = scene;
        Outcome::Reply(match command {
            Command::Console { open, line } => {
                if *state.get() != cyrus_game::AppState::InGame {
                    Err("console requires an active world".into())
                } else {
                    commands.queue(move |world: &mut World| {
                        if let Some(open) = open { world.resource_mut::<cyrus_console::Console>().open = open; }
                        if let Some(line) = line { let _ = cyrus_console::submit(world, &line, cyrus_console::Source::Interactive); }
                        let result = json!({"console": world.resource::<cyrus_console::Console>(), "cheats": world.resource::<cyrus_game::cheats::Cheats>().inspect()});
                        let _ = reply.send(Response::ok(id, result));
                    });
                    // Later requests must observe this command after its world mutation.
                    return Outcome::Barrier;
                }
            }
            _ => unreachable!("command routed to the wrong console handler"),
        })
    }

    fn set_attribute(&mut self, command: Command) -> Outcome {
        let Self { script_actors, .. } = self;
        Outcome::Reply(match command {
            Command::SetAttribute {
                index,
                attribute,
                value,
            } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.set_actor_attribute(index, attribute, value)?;
                Ok(json!({"index": index, "attribute": attribute, "value": value}))
            })()
            .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong set_attribute handler"),
        })
    }

    fn mark_dead(&mut self, command: Command) -> Outcome {
        let Self { script_actors, .. } = self;
        Outcome::Reply(match command {
            Command::MarkDead { index } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.mark_actor_dead(index)?;
                Ok(json!({"index": index, "dead": true}))
            })()
            .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong mark_dead handler"),
        })
    }

    fn set_torch(&mut self, command: Command) -> Outcome {
        let Self { script_actors, .. } = self;
        Outcome::Reply(match command {
            Command::SetTorch { burning } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.set_selected_torch(burning)?;
                Ok(json!({"burning": burning}))
            })()
            .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong set_torch handler"),
        })
    }

    fn show_item(&mut self, command: Command) -> Outcome {
        let Self { script_actors, .. } = self;
        Outcome::Reply(match command {
            Command::ShowItem { item, no_text } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world
                    .globals_mut()
                    .request_item_viewer(item, [0, 0], no_text);
                Ok(json!({"item": item, "no_text": no_text}))
            })()
            .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong show_item handler"),
        })
    }

    fn reset_camera(&mut self, command: Command) -> Outcome {
        let Self { script_actors, .. } = self;
        Outcome::Reply(match command {
            Command::ResetCamera => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.globals_mut().request_camera_reset();
                Ok(json!({"requested": true}))
            })()
            .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong reset_camera handler"),
        })
    }

    fn select_item(&mut self, command: Command) -> Outcome {
        let Self { script_hud, .. } = self;
        Outcome::Reply(match command {
            Command::SelectItem { id } => (|| -> anyhow::Result<_> {
                let hud = script_hud
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("inventory is not prepared"))?;
                hud.select_item(id)?;
                Ok(json!({"selected": id}))
            })()
            .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong select_item handler"),
        })
    }

    fn place_player(
        &mut self,
        command: Command,
        scene: &Scene,
        acquisition: &Acquisition,
    ) -> Outcome {
        let Self { script_actors, .. } = self;
        let Scene {
            prepared, clock, ..
        } = scene;
        let Acquisition {
            player_trace_pending,
            ..
        } = acquisition;
        Outcome::Reply(match command {
            Command::PlacePlayer { .. } if player_trace_pending.is_some() => {
                Err("player trace owns placement until completion".into())
            }
            Command::PlacePlayer { x, y, z, heading } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.place_selected_player(
                    [x, y, z].map(|value| value.wrapping_mul(256)),
                    heading,
                )?;
                // handle runs in PreUpdate: placement is applied before the next ordinary update.
                // An absent clock cannot acknowledge an exact simulation event.
                let player = world
                    .selected_player()
                    .ok_or_else(|| anyhow::anyhow!("placement: selected player unavailable"))?;
                let boundary = clock
                    .as_ref()
                    .zip(prepared.as_ref())
                    .map(|(clock, prepared)| {
                        json!({
                            "ready": true, "scene": prepared.requested_name,
                            "frame": world.ordinary_updates(), "generation": world.generation(),
                            "elapsed_scale": clock.elapsed_frame_scale(),
                            "normal_frame_rate": clock.settings().normal_frame_rate,
                            "game_seconds": clock.game_seconds(), "execution": "running",
                            "event": "placement_applied", "phase": "before_ordinary_update",
                            "player": world.inspect(player.index, 1)["actors"][0],
                        })
                    });
                Ok(json!({"position": [x, y, z], "heading": wrap(heading),
                    "acknowledged": boundary.is_some(), "boundary": boundary}))
            })()
            .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong place_player handler"),
        })
    }

    fn scripts(&mut self, command: Command) -> Outcome {
        let Self { script_actors, .. } = self;
        Outcome::Reply(match command {
            Command::Scripts { offset, limit } => {
                match script_actors
                    .as_ref()
                    .and_then(|actors| actors.current.as_ref())
                {
                    Some(world) => {
                        let mut observation = world.inspect(offset, limit);
                        observation["flags"] = json!(world.snapshot_flags());
                        observation["player_position_q8"] = json!(
                            world
                                .selected_player()
                                .and_then(|id| world.spatial(id).ok())
                                .and_then(|spatial| spatial.effective.as_ref())
                                .map(|pose| pose.position)
                        );
                        Ok(observation)
                    }
                    None => Err("script world is not prepared".into()),
                }
            }
            _ => unreachable!("command routed to the wrong scripts handler"),
        })
    }

    fn queue_movie(&mut self, command: Command) -> Outcome {
        let Self { script_actors, .. } = self;
        Outcome::Reply(match command {
            Command::QueueMovie { movie } => match script_actors
                .as_mut()
                .and_then(|actors| actors.current.as_mut())
            {
                Some(world) => {
                    world.globals_mut().queue_movie(movie);
                    Ok(json!({"queued": movie}))
                }
                None => Err("script world is not prepared".into()),
            },
            _ => unreachable!("command routed to the wrong queue_movie handler"),
        })
    }

    fn script_load_world(&mut self, command: Command) -> Outcome {
        let Self { script_actors, .. } = self;
        Outcome::Reply(match command {
            Command::ScriptLoadWorld {
                world,
                marker,
                heading,
            } => match script_actors
                .as_mut()
                .and_then(|actors| actors.current.as_mut())
            {
                Some(current) => {
                    current
                        .globals_mut()
                        .request_load_world([world, marker, heading]);
                    Ok(json!({"requested": [world, marker, heading]}))
                }
                None => Err("script world is not prepared".into()),
            },
            _ => unreachable!("command routed to the wrong script_load_world handler"),
        })
    }

    fn script_summary(&mut self, command: Command) -> Outcome {
        let Self { script_actors, .. } = self;
        Outcome::Reply(match command {
            Command::ScriptSummary { offset, limit } => match script_actors
                .as_ref()
                .and_then(|actors| actors.current.as_ref())
            {
                Some(world) => Ok(world.inspect_summary(offset, limit)),
                None => Err("script world is not prepared".into()),
            },
            _ => unreachable!("command routed to the wrong script_summary handler"),
        })
    }

    fn bind_script_animation(
        &mut self,
        command: Command,
        commands: &mut Commands,
        scene: &Scene,
        script_bindings: &mut std::collections::BTreeMap<
            Entity,
            cyrus_script::script::ScriptActorId,
        >,
    ) -> Outcome {
        let Self {
            script_actors,
            animations,
            script_nodes,
            ..
        } = self;
        let Scene { prepared, .. } = scene;
        Outcome::Reply(match command {
            Command::BindScriptAnimation {
                generation,
                index,
                default_group,
                node,
            } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                let source = &prepared
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("world is not prepared"))?
                    .animations;
                let data = animations
                    .get(source)
                    .ok_or_else(|| anyhow::anyhow!("animation data is not loaded"))?;
                let entity = binding_entity(script_nodes, script_bindings, generation, node)?;
                let actor = cyrus_script::script::ScriptActorId { generation, index };
                world.bind_animation(
                    actor,
                    data,
                    cyrus_soup::AnimationGroup::from_index(default_group),
                )?;
                if let Some(entity) = entity {
                    script_bindings.insert(entity, actor);
                    commands
                        .entity(entity)
                        .remove::<(
                            cyrus_script::script::ScriptedPlayback,
                            cyrus_world::animation::AnimationPlayback,
                            cyrus_world::animation::AnimationPose,
                        )>()
                        .insert(cyrus_script::script::ScriptBinding {
                            actor,
                            source: source.clone(),
                        });
                }
                Ok(world.inspect(index, 1))
            })()
            .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong bind_script_animation handler"),
        })
    }

    fn step_script(
        &mut self,
        command: Command,
        script_bindings: &mut std::collections::BTreeMap<
            Entity,
            cyrus_script::script::ScriptActorId,
        >,
    ) -> Outcome {
        let Self {
            script_hud,
            script_actors,
            random,
            cheats,
            activation,
            script_sounds,
            player_poses,
            ..
        } = self;
        Outcome::Reply(match command {
            Command::StepScript {
                generation,
                index,
                stage,
                frame_scale,
                nominal_ticks,
                distance_override,
                fallback_position,
            } => (|| -> anyhow::Result<_> {
                use cyrus_script::script::ScriptActorId;
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                let id = ScriptActorId { generation, index };
                anyhow::ensure!(world.id(index)? == id, "stale script actor generation");
                let Some(stage) = prepare_stage(world, id, stage)? else {
                    return Ok(world.inspect(index, 1));
                };
                let player = player_query(player_poses)?;
                let inputs = script_inputs(
                    script_hud.as_deref(),
                    activation.as_deref(),
                    cheats,
                    cyrus_script::script::QueryInputs {
                        player,
                        distance_override,
                        fallback_position,
                        linked_position: None,
                    },
                    frame_scale,
                    nominal_ticks,
                );
                execute_stage(
                    world,
                    id,
                    stage,
                    inputs,
                    random,
                    script_sounds.as_deref_mut(),
                    script_bindings,
                )
            })()
            .map_err(|error| format!("{error:#}")),
            _ => unreachable!("command routed to the wrong step_script handler"),
        })
    }
}
