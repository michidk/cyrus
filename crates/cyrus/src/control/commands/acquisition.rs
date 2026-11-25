//! Asynchronous capture, replay and input acquisition commands.
use super::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Acquisition<'w> {
    pub(super) keys: ResMut<'w, ButtonInput<KeyCode>>,
    pub(super) taps: ResMut<'w, TapRelease>,
    pub(super) frames: Res<'w, bevy::diagnostic::FrameCount>,
    pub(super) replay_step: Option<Res<'w, ReplayStep>>,
    pub(super) rendered_actor_pending: Option<Res<'w, rendered_actor::PendingRenderedActor>>,
    pub(super) effect_geometry_pending: Option<Res<'w, effect_geometry::PendingEffectGeometry>>,
    pub(super) input_scheduler: ResMut<'w, input_schedule::InputScheduler>,
    pub(super) frame_captures: ResMut<'w, frame_capture::FrameCaptures>,
    pub(super) player_trace_pending: Option<Res<'w, player_trace::PendingTrace>>,
}

impl Acquisition<'_> {
    pub(super) fn handle(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
        scene: &Scene,
        gameplay: &Gameplay,
    ) -> Outcome {
        let Self {
            keys,
            taps,
            replay_step,
            rendered_actor_pending,
            effect_geometry_pending,
            input_scheduler,
            frame_captures,
            ..
        } = self;
        let Reply { id, reply } = request;
        let Scene { clock, .. } = scene;
        let Gameplay { script_actors, .. } = gameplay;
        Outcome::Reply(match command {
            Command::RenderedActor {
                index,
                after_generation,
            } => {
                if rendered_actor_pending.is_some() {
                    Err("rendered actor observation already pending".into())
                } else {
                    commands.insert_resource(rendered_actor::PendingRenderedActor {
                        id,
                        index,
                        after_generation,
                        reply,
                        deadline: std::time::Instant::now() + std::time::Duration::from_secs(180),
                    });
                    return Outcome::Barrier;
                }
            }
            Command::EffectGeometry { after_generation } => {
                if effect_geometry_pending.is_some() {
                    Err("effect geometry observation already pending".into())
                } else {
                    commands.insert_resource(effect_geometry::PendingEffectGeometry {
                        id,
                        after_generation,
                        reply,
                        deadline: std::time::Instant::now() + std::time::Duration::from_secs(180),
                    });
                    return Outcome::Barrier;
                }
            }
            Command::BoundLightImage { name, primitive } => {
                commands.insert_resource(light_image::PendingLightImage {
                    id,
                    name,
                    primitive,
                    reply,
                });
                // Stop dispatch so later commands cannot change the selected world before capture.
                return Outcome::Barrier;
            }
            Command::CapturePresentationFrame { path } => {
                match frame_captures.arm(
                    id,
                    path.into(),
                    None,
                    reply.clone(),
                    None,
                    frame_capture::CaptureKind::Presentation,
                ) {
                    Ok(()) => return Outcome::Continue,
                    Err(error) => Err(error.to_string()),
                }
            }
            Command::CaptureGameplayFrame {
                path,
                pcm_path,
                boundary,
            } => {
                match frame_captures.arm(
                    id,
                    path.into(),
                    pcm_path.map(Into::into),
                    reply.clone(),
                    boundary,
                    frame_capture::CaptureKind::Gameplay,
                ) {
                    Ok(()) => return Outcome::Barrier,
                    Err(error) => Err(format!("{error:#}")),
                }
            }
            Command::TracePlayer {
                x,
                y,
                z,
                heading,
                updates,
            } => {
                commands.queue(move |world: &mut World| {
                    if let Err(error) =
                        player_trace::start(world, id, reply.clone(), [x, y, z], heading, updates)
                    {
                        let _ = reply.send(Response::err(id, format!("{error:#}")));
                    }
                });
                return Outcome::Barrier;
            }
            Command::ScheduleInput { events } => {
                let generation = script_actors
                    .as_ref()
                    .and_then(|actors| actors.current.as_ref())
                    .map(|world| world.generation());
                match input_scheduler.arm(id, reply.clone(), generation, events, &keys, None) {
                    Ok(()) => {
                        commands.remove_resource::<cyrus_script::script::ReplayFrameScale>();
                        return Outcome::Continue;
                    }
                    Err(error) => Err(error),
                }
            }
            Command::ReplayFrame {
                frame_scale,
                elapsed_bios_ticks,
                elapsed_host_nanos,
            } => {
                let prepared = (|| -> Result<_, String> {
                    if replay_step.is_none() {
                        return Err("recorded frame requires --replay-step-micros".into());
                    }
                    let world = script_actors
                        .as_ref()
                        .and_then(|actors| actors.current.as_ref())
                        .ok_or("recorded frame requires a staged world")?;
                    let mut candidate = clock
                        .as_deref()
                        .ok_or("recorded frame requires a clock")?
                        .clone();
                    let tick = elapsed_bios_ticks.map_or_else(
                        || candidate.simulated_bios_tick(),
                        |elapsed| candidate.bios_sample().wrapping_add(elapsed),
                    );
                    candidate
                        .advance_recorded(tick, frame_scale)
                        .map_err(|error| error.to_string())?;
                    Ok(cyrus_script::script::ReplayFrameScale {
                        generation: world.generation(),
                        previous_update: world.ordinary_updates(),
                        frame_scale,
                        elapsed_bios_ticks,
                    })
                })();
                match prepared {
                    Ok(recorded) => match input_scheduler.arm(
                        id,
                        reply.clone(),
                        Some(recorded.generation),
                        vec![cyrus_control::InputBoundary {
                            after_updates: 0,
                            key: None,
                            action: None,
                        }],
                        &keys,
                        elapsed_host_nanos.map(std::time::Duration::from_nanos),
                    ) {
                        Ok(()) => {
                            commands.insert_resource(recorded);
                            return Outcome::Continue;
                        }
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                }
            }
            Command::AcquireInput { acquisition } => {
                let generation = script_actors
                    .as_ref()
                    .and_then(|actors| actors.current.as_ref())
                    .map(|world| world.generation());
                match input_scheduler.arm_clock(id, reply.clone(), generation, acquisition, &keys) {
                    Ok(()) => {
                        commands.remove_resource::<cyrus_script::script::ReplayFrameScale>();
                        return Outcome::Continue;
                    }
                    Err(error) => Err(error),
                }
            }
            Command::Input { .. } if input_scheduler.pending() => {
                Err("input schedule owns control input until completion".into())
            }
            Command::Input { key, action } => match parse_key(&key) {
                Some(code) => {
                    match action {
                        InputAction::Press => keys.press(code),
                        InputAction::Release => keys.release(code),
                        InputAction::Tap => {
                            keys.press(code);
                            taps.0.push(code);
                        }
                    }
                    Ok(json!({"key": key, "action": action}))
                }
                None => Err(format!("unknown key {key}")),
            },
            _ => unreachable!("command routed to the wrong acquisition handler"),
        })
    }
}
