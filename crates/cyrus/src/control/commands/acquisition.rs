//! Asynchronous capture, replay and input acquisition commands.
use super::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Acquisition<'w> {
    pub(super) keys: ResMut<'w, ButtonInput<KeyCode>>,
    pub(super) taps: ResMut<'w, TapRelease>,
    pub(super) frames: Res<'w, bevy::diagnostic::FrameCount>,
    pub(super) replay_step: Option<Res<'w, ReplayStep>>,
    pub(super) replay_hold: Option<Res<'w, cyrus_script::script::ReplayHold>>,
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
        scene: &mut Scene,
        gameplay: &Gameplay,
    ) -> Outcome {
        match command {
            command @ Command::RenderedActor { .. } => {
                self.rendered_actor(command, request, commands)
            }
            command @ Command::EffectGeometry { .. } => {
                self.effect_geometry(command, request, commands)
            }
            command @ Command::BoundLightImage { .. } => {
                self.bound_light_image(command, request, commands)
            }
            command @ Command::CapturePresentationFrame { .. } => {
                self.capture_presentation_frame(command, request)
            }
            command @ Command::CaptureGameplayFrame { .. } => {
                self.capture_gameplay_frame(command, request)
            }
            command @ Command::TracePlayer { .. } => self.trace_player(command, request, commands),
            command @ Command::ScheduleInput { .. } => {
                self.schedule_input(command, request, commands, gameplay)
            }
            command @ Command::SetReplayClockOrigin { .. } => {
                self.set_replay_clock_origin(command, scene, gameplay)
            }
            command @ Command::ReplayFrame { .. } => {
                self.replay_frame(command, request, commands, scene, gameplay)
            }
            command @ Command::AcquireInput { .. } => {
                self.acquire_input(command, request, commands, gameplay)
            }
            command @ Command::Input { .. } => self.input(command, gameplay),
            _ => unreachable!("command routed to the wrong acquisition handler"),
        }
    }

    fn rendered_actor(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
    ) -> Outcome {
        let Self {
            rendered_actor_pending,
            ..
        } = self;
        let Reply { id, reply, .. } = request;
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
            _ => unreachable!("command routed to the wrong rendered_actor handler"),
        })
    }

    fn effect_geometry(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
    ) -> Outcome {
        let Self {
            effect_geometry_pending,
            ..
        } = self;
        let Reply { id, reply, .. } = request;
        Outcome::Reply(match command {
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
            _ => unreachable!("command routed to the wrong effect_geometry handler"),
        })
    }

    fn bound_light_image(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
    ) -> Outcome {
        let Reply { id, reply, .. } = request;
        match command {
            Command::BoundLightImage { name, primitive } => {
                commands.insert_resource(light_image::PendingLightImage {
                    id,
                    name,
                    primitive,
                    reply,
                });
                // Stop dispatch so later commands cannot change the selected world before capture.
                Outcome::Barrier
            }
            _ => unreachable!("command routed to the wrong bound_light_image handler"),
        }
    }

    fn capture_presentation_frame(&mut self, command: Command, request: Reply) -> Outcome {
        let Self { frame_captures, .. } = self;
        let Reply { id, reply, .. } = request;
        Outcome::Reply(match command {
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
            _ => unreachable!("command routed to the wrong capture_presentation_frame handler"),
        })
    }

    fn capture_gameplay_frame(&mut self, command: Command, request: Reply) -> Outcome {
        let Self { frame_captures, .. } = self;
        let Reply { id, reply, .. } = request;
        Outcome::Reply(match command {
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
            _ => unreachable!("command routed to the wrong capture_gameplay_frame handler"),
        })
    }

    fn trace_player(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
    ) -> Outcome {
        let Reply { id, reply, .. } = request;
        match command {
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
                Outcome::Barrier
            }
            _ => unreachable!("command routed to the wrong trace_player handler"),
        }
    }

    fn schedule_input(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
        gameplay: &Gameplay,
    ) -> Outcome {
        let Self {
            keys,
            input_scheduler,
            ..
        } = self;
        let Reply { id, reply, .. } = request;
        let Gameplay { script_actors, .. } = gameplay;
        Outcome::Reply(match command {
            Command::ScheduleInput { events } => {
                let generation = script_actors
                    .as_ref()
                    .and_then(|actors| actors.current.as_ref())
                    .map(|world| world.generation());
                match input_scheduler.arm(id, reply.clone(), generation, events, keys, None) {
                    Ok(()) => {
                        commands.remove_resource::<cyrus_script::script::ReplayFrameScale>();
                        return Outcome::Continue;
                    }
                    Err(error) => Err(error),
                }
            }
            _ => unreachable!("command routed to the wrong schedule_input handler"),
        })
    }

    fn set_replay_clock_origin(
        &mut self,
        command: Command,
        scene: &mut Scene,
        gameplay: &Gameplay,
    ) -> Outcome {
        let Command::SetReplayClockOrigin {
            generation,
            expected_update,
            expected_bios_sample,
            bios_sample,
        } = command
        else {
            unreachable!("origin command routing");
        };
        Outcome::Reply((|| {
            if self.replay_step.is_none() || !self.replay_hold.as_deref().is_some_and(|hold| hold.0)
            {
                return Err("replay clock origin requires a held replay world".into());
            }
            let world = gameplay
                .script_actors
                .as_deref()
                .and_then(|actors| actors.current.as_ref())
                .ok_or("replay clock origin requires a current world")?;
            if world.generation() != generation || world.ordinary_updates() != expected_update {
                return Err(format!(
                    "replay clock origin stale world/update: expected {generation}/{expected_update}, current {}/{}",
                    world.generation(),
                    world.ordinary_updates()
                ));
            }
            if expected_update != 0
                || !self.input_scheduler.origin_available(generation)
                || self.keys.get_pressed().next().is_some()
                || !self.taps.0.is_empty()
            {
                return Err("replay clock origin requires update0 before input, schedule, frame or prior setup".into());
            }
            let ready = *scene.state.get() == cyrus_game::AppState::InGame
                && scene.current.world == Some(world.world)
                && scene.prepared.as_deref().is_some_and(|prepared| {
                    prepared.world == Some(world.world)
                        && scene
                            .ready_scene
                            .as_deref()
                            .is_some_and(|ready| ready.0 == prepared.animations)
                });
            if !ready {
                return Err(format!(
                    "world {} replay clock origin requires completed scene readiness",
                    world.world
                ));
            }
            let clock = scene
                .clock
                .as_deref_mut()
                .ok_or("replay clock origin requires a clock")?;
            if clock.bios_sample() != expected_bios_sample {
                return Err(format!(
                    "replay clock origin stale BIOS sample: expected {expected_bios_sample}, current {}",
                    clock.bios_sample()
                ));
            }
            clock.rebase(bios_sample);
            self.input_scheduler.note_origin_setup(generation);
            Ok(
                json!({"generation": generation, "ordinary_update": expected_update,
                "previous_bios_sample": expected_bios_sample, "bios_sample": clock.bios_sample(),
                "continuation": clock.snapshot()}),
            )
        })())
    }

    fn replay_frame(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
        scene: &Scene,
        gameplay: &Gameplay,
    ) -> Outcome {
        let Self {
            keys,
            replay_step,
            input_scheduler,
            ..
        } = self;
        let Reply { id, reply, .. } = request;
        let Scene { clock, .. } = scene;
        let Gameplay { script_actors, .. } = gameplay;
        Outcome::Reply(match command {
            Command::ReplayFrame {
                frame_scale,
                elapsed_bios_ticks,
                elapsed_host_nanos,
            } => {
                if elapsed_bios_ticks.is_none()
                    && script_actors
                        .as_deref()
                        .and_then(|actors| actors.current.as_ref())
                        .is_some_and(|world| input_scheduler.has_origin(world.generation()))
                {
                    return Outcome::Reply(Err(
                        "an aligned replay frame requires observed elapsed_bios_ticks".into(),
                    ));
                }
                let prepared = recorded_frame(
                    replay_step.is_some(),
                    script_actors.as_deref(),
                    clock.as_deref(),
                    frame_scale,
                    elapsed_bios_ticks,
                );
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
                        keys,
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
            _ => unreachable!("command routed to the wrong replay_frame handler"),
        })
    }

    fn acquire_input(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
        gameplay: &Gameplay,
    ) -> Outcome {
        let Self {
            keys,
            input_scheduler,
            ..
        } = self;
        let Reply { id, reply, .. } = request;
        let Gameplay { script_actors, .. } = gameplay;
        Outcome::Reply(match command {
            Command::AcquireInput { acquisition } => {
                let generation = script_actors
                    .as_ref()
                    .and_then(|actors| actors.current.as_ref())
                    .map(|world| world.generation());
                match input_scheduler.arm_clock(id, reply.clone(), generation, acquisition, keys) {
                    Ok(()) => {
                        commands.remove_resource::<cyrus_script::script::ReplayFrameScale>();
                        return Outcome::Continue;
                    }
                    Err(error) => Err(error),
                }
            }
            _ => unreachable!("command routed to the wrong acquire_input handler"),
        })
    }

    fn input(&mut self, command: Command, gameplay: &Gameplay) -> Outcome {
        let Self {
            keys,
            taps,
            input_scheduler,
            ..
        } = self;
        Outcome::Reply(match command {
            Command::Input { .. } if input_scheduler.pending() => {
                Err("input schedule owns control input until completion".into())
            }
            Command::Input { key, action } => match parse_key(&key) {
                Some(code) => {
                    input_scheduler.note_control_input(
                        gameplay
                            .script_actors
                            .as_deref()
                            .and_then(|actors| actors.current.as_ref())
                            .map(|world| world.generation()),
                    );
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
            _ => unreachable!("command routed to the wrong input handler"),
        })
    }
}

fn recorded_frame(
    replay_mode: bool,
    script_actors: Option<&cyrus_script::script::ScriptActors>,
    clock: Option<&cyrus_script::clock::SimulationClock>,
    frame_scale: u32,
    elapsed_bios_ticks: Option<u32>,
) -> Result<cyrus_script::script::ReplayFrameScale, String> {
    if !replay_mode {
        return Err("recorded frame requires --replay-step-micros".into());
    }
    let world = script_actors
        .and_then(|actors| actors.current.as_ref())
        .ok_or("recorded frame requires a staged world")?;
    let mut candidate = clock.ok_or("recorded frame requires a clock")?.clone();
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
}
