//! Logical actor inspection and explicit script-stage commands.
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
        let Self {
            script_hud,
            script_actors,
            random,
            cheats,
            activation,
            animations,
            script_nodes,
            script_sounds,
            player_poses,
            ..
        } = self;
        let Reply { id, reply } = request;
        let Scene {
            prepared,
            clock,
            state,
            ..
        } = scene;
        let Acquisition {
            player_trace_pending,
            ..
        } = acquisition;
        Outcome::Reply(match command {
            Command::Cheat { name, value } => cheats.execute(&name, value)
                .map(|effect| json!({"effect":effect,"cheats":cheats.inspect()}))
                .map_err(|error| format!("{error:#}")),
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
            Command::MarkDead { index } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.mark_actor_dead(index)?;
                Ok(json!({"index": index, "dead": true}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::SetTorch { burning } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.set_selected_torch(burning)?;
                Ok(json!({"burning": burning}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::ShowItem { item, no_text } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.globals_mut().request_item_viewer(item, [0, 0], no_text);
                Ok(json!({"item": item, "no_text": no_text}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::ResetCamera => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.globals_mut().request_camera_reset();
                Ok(json!({"requested": true}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::SelectItem { id } => (|| -> anyhow::Result<_> {
                let hud = script_hud
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("inventory is not prepared"))?;
                hud.select_item(id)?;
                Ok(json!({"selected": id}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::PlacePlayer { .. } if player_trace_pending.is_some() => Err("player trace owns placement until completion".into()),
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
                let player = world.selected_player()
                    .ok_or_else(|| anyhow::anyhow!("placement: selected player unavailable"))?;
                let boundary = clock.as_ref().zip(prepared.as_ref()).map(|(clock, prepared)| {
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
            Command::Scripts { offset, limit } => {
                match script_actors
                    .as_ref()
                    .and_then(|actors| actors.current.as_ref())
                {
                    Some(world) => {
                        let mut observation = world.inspect(offset, limit);
                        observation["flags"] = json!(world.snapshot_flags());
                        observation["player_position_q8"] = json!(world
                            .selected_player()
                            .and_then(|id| world.spatial(id).ok())
                            .and_then(|spatial| spatial.effective.as_ref())
                            .map(|pose| pose.position));
                        Ok(observation)
                    }
                    None => Err("script world is not prepared".into()),
                }
            }
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
            Command::ScriptLoadWorld {
                world,
                marker,
                heading,
            } => match script_actors
                .as_mut()
                .and_then(|actors| actors.current.as_mut())
            {
                Some(current) => {
                    current.globals_mut().request_load_world([world, marker, heading]);
                    Ok(json!({"requested": [world, marker, heading]}))
                }
                None => Err("script world is not prepared".into()),
            },
            Command::ScriptSummary { offset, limit } => match script_actors
                .as_ref()
                .and_then(|actors| actors.current.as_ref())
            {
                Some(world) => Ok(world.inspect_summary(offset, limit)),
                None => Err("script world is not prepared".into()),
            },
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
                let entity = if let Some(name) = node {
                    let matches: Vec<_> = script_nodes
                        .iter()
                        .filter(|(_, candidate)| candidate.as_str() == name)
                        .collect();
                    anyhow::ensure!(
                        matches.len() == 1,
                        "scene node {name:?}: expected one match, found {}",
                        matches.len()
                    );
                    anyhow::ensure!(
                        script_bindings.get(&matches[0].0).is_none_or(|bound| bound.generation != generation),
                        "scene node {name:?} already has a script binding"
                    );
                    Some(matches[0].0)
                } else {
                    None
                };
                let actor = cyrus_script::script::ScriptActorId { generation, index };
                world.bind_animation(actor, data, cyrus_soup::AnimationGroup::from_index(default_group))?;
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
            Command::StepScript {
                generation,
                index,
                stage,
                frame_scale,
                nominal_ticks,
                distance_override,
                fallback_position,
            } => (|| -> anyhow::Result<_> {
                use cyrus_script::script::{ScriptActorId, ScriptStage, Thread};
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                let id = ScriptActorId { generation, index };
                anyhow::ensure!(world.id(index)? == id, "stale script actor generation");
                match stage {
                    cyrus_control::ScriptStage::PrepareSpatial => world.prepare_spatial()?,
                    cyrus_control::ScriptStage::Snapshot => world.snapshot_spatial(id)?,
                    cyrus_control::ScriptStage::AxisLocks => world.restore_axis_locks(id)?,
                    cyrus_control::ScriptStage::Transform => world.refresh_transform(id)?,
                    cyrus_control::ScriptStage::Families => world.refresh_families()?,
                    _ => {},
                }
                let stage = match stage {
                    cyrus_control::ScriptStage::PrepareSpatial | cyrus_control::ScriptStage::Snapshot
                    | cyrus_control::ScriptStage::AxisLocks | cyrus_control::ScriptStage::Transform | cyrus_control::ScriptStage::Families => return Ok(world.inspect(index, 1)),
                    cyrus_control::ScriptStage::Animation => ScriptStage::Animation,
                    cyrus_control::ScriptStage::OrdinaryMain => ScriptStage::OrdinaryMain,
                    cyrus_control::ScriptStage::OrdinaryHook => ScriptStage::OrdinaryHook,
                    cyrus_control::ScriptStage::NativeHook => ScriptStage::NativeHook,
                    cyrus_control::ScriptStage::Entry => ScriptStage::Entry,
                    cyrus_control::ScriptStage::Tasks => ScriptStage::Tasks,
                    cyrus_control::ScriptStage::Main => ScriptStage::Body(Thread::Main),
                    cyrus_control::ScriptStage::Interrupt => ScriptStage::Body(Thread::Interrupt),
                };
                let mut captured_sounds = Vec::new();
                let mut emit = |actor: ScriptActorId, sound| -> anyhow::Result<()> {
                    use cyrus_script::script::ScriptSound;
                    let (sound, surface, position) = match sound {
                        ScriptSound::Animation { sound, surface, position } => (sound, surface, position),
                        sound => {
                            let event = match &sound {
                                ScriptSound::Flat { effect, volume, rate_offset_hz } => json!({"type":"flat", "effect":effect, "volume":volume, "rate_offset_hz":rate_offset_hz}),
                                ScriptSound::Positional { effect, volume, rate_offset_hz, position, start_index } => json!({"type":"positional", "effect":effect, "volume":volume, "rate_offset_hz":rate_offset_hz, "position":position, "start_index":start_index}),
                                ScriptSound::StopEffect { effect, owned } => json!({"type":"stop_effect", "effect":effect, "owned":owned}),
                                ScriptSound::StopOwned => json!({"type":"stop_owned"}),
                                ScriptSound::StopAll => json!({"type":"stop_all"}),
                                ScriptSound::AnimationState(state) => json!({"type":"animation_state", "state":format!("{state:?}")}),
                                ScriptSound::Animation { .. } => unreachable!(),
                            };
                            captured_sounds.push(json!({"actor":actor, "sound":event}));
                            if let Some(sounds) = script_sounds.as_mut() {
                                sounds.write(cyrus_world::sound::SoundCue::Script {
                                    generation: actor.generation, index: actor.index, sound,
                                });
                            }
                            return Ok(());
                        }
                    };
                    anyhow::ensure!(
                        !matches!(
                            sound,
                            cyrus_world::animation::AnimationSound::Event { .. }
                                | cyrus_world::animation::AnimationSound::Transition { .. }
                        ),
                        "animation actor events and transitions are unsupported in the control script stage"
                    );
                    let mut nodes = script_bindings.iter().filter(|(_, bound)| **bound == actor);
                    let Some((entity, _)) = nodes.next() else {
                        use cyrus_world::animation::AnimationSound;
                        let event = match sound {
                            AnimationSound::Play { effect, rate_offset_hz } =>
                                json!({"type":"play", "effect":effect, "rate_offset_hz":rate_offset_hz}),
                            AnimationSound::CheckSample => json!({"type":"check_sample"}),
                            AnimationSound::Stop => json!({"type":"stop"}),
                            AnimationSound::Forget => json!({"type":"forget"}),
                            AnimationSound::Event { .. } | AnimationSound::Transition { .. } => {
                                unreachable!()
                            }
                        };
                        captured_sounds.push(json!({"actor":actor, "sound":event}));
                        return Ok(());
                    };
                    let entity = *entity;
                    anyhow::ensure!(
                        nodes.next().is_none(),
                        "script sound receiver has multiple scene bindings"
                    );
                    script_sounds
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("script sound consumer unavailable"))?
                        .write(cyrus_world::sound::SoundCue::Animation {
                            actor: entity,
                            sound,
                            surface,
                            position,
                        });
                    Ok(())
                };
                let mut poses = player_poses.iter();
                let selected = poses.next();
                anyhow::ensure!(poses.next().is_none(), "selected-player query has multiple player poses");
                let player = selected.map(|pose| -> anyhow::Result<_> {
                    let mut position = [0; 3];
                    for (axis, coordinate) in pose.position.into_iter().enumerate() {
                        position[axis] = i32::try_from(coordinate).map_err(|_| anyhow::anyhow!("player query coordinate axis {axis} outside Q8 i32 range"))?;
                    }
                    Ok(cyrus_script::script::PlayerQuery { position, heading: pose.heading })
                }).transpose()?;
                let inputs = cyrus_script::script::ScriptInputs {
                    hud: script_hud.as_deref(),
                    activation: activation.as_deref(),
                    death_suppressed: cheats.death_suppressed().ok(),
                    torch_retained: cheats.torch_retained().ok(),
                    actor_update_skipped: cheats.actor_update_skipped().ok(),
                    ground_bypass: cheats.ground_bypass().ok(),
                    magic_carpet: cheats.magic_carpet().ok(),
                    player_collision_skipped: cheats.player_collision_skipped().ok(),
                    nominal_ticks, frame_scale,
                    player_query: Some(cyrus_script::script::QueryInputs {
                        player, distance_override: distance_override.or_else(|| cheats.distance_override().ok()), fallback_position, linked_position: None,
                    }),
                    ..Default::default()
                };
                world.advance(id, stage, inputs, Some(&mut emit), Some(&mut **random), 100_000)?;
                let mut result = world.inspect(index, 1);
                result["captured_sounds"] = json!(captured_sounds);
                Ok(result)
            })()
            .map_err(|error| format!("{error:#}")),
        _ => unreachable!("command routed to the wrong gameplay handler"),
        })
    }
}
