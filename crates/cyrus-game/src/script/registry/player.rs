//! Explicit grounded player updates retain movement continuation with the logical actor.
use super::ScriptWorld;
use crate::player::{
    data::PlayerScene,
    grounded::{self, ForwardWalk, GroundedUpdate, GroundedWalk, StaticGroundQuery},
    placement::PlayerPose,
    runtime::GroundedTick,
    turn,
    walk::WalkingStartup,
};
use anyhow::{Context, Result, ensure};
use cyrus_assets::animation::{Animations, ModelFrame};
use cyrus_collision::ground::SupportHistory;
use cyrus_world::animation::AnimationSound;

pub(super) struct PlayerMovement {
    startup: WalkingStartup,
    pub(super) history: SupportHistory,
    veto: i32,
    accepted_forward: Option<bool>,
    pub(super) latches: super::attack_input::AttackLatches,
}

pub(crate) struct GroundedPresentation {
    pub pose: PlayerPose,
    pub frame: Option<ModelFrame>,
    pub surface: u8,
}

/// Held Faithful keyboard sample and the independently retained walk-mode toggle.
#[derive(Clone, Copy, Debug)]
pub struct PlayerControllerInput<'a> {
    pub held: &'a crate::player::input::PlayerInput,
    pub walk_mode: bool,
}

impl ScriptWorld {
    /// Run supported death or sheathed grounded control at the ordinary native hook.
    /// Animation/queued stages precede this; continuation and late response follow it.
    pub fn advance_player_controller(
        &mut self,
        id: super::ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        timing: crate::clock::ClockStep,
        inputs: super::ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(super::ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        use crate::player::controller::{
            SheathedAction, SheathedControllerState, select_sheathed_action,
        };
        self.run_player_native_hook(id, &mut |world| {
            let actor = &world.actors[id.index];
            let control = actor
                .control
                .as_ref()
                .context("player controller control unavailable")?;
            let sliding = control.sliding_state;
            world.globals.begin_player_controller(
                timing.nominal_ticks,
                control.queued_motion,
                control.falling_state,
            )?;
            if sliding == 0 && world.globals.player_pending_action() {
                world.run_pending_player_action(id, inputs, random, sounds)?;
            }
            if world
                .globals
                .finish_player_controller(timing.nominal_ticks)?
            {
                return world.advance_selected_death_controller(
                    id,
                    inputs,
                    timing.nominal_ticks,
                    random,
                    sounds,
                );
            }
            let input = inputs
                .player_controller
                .context("player controller input unavailable")?;
            if let (Some(tap_time), Some(scale)) =
                (query.scene.settings.tap_time, inputs.frame_scale)
            {
                world.update_attack_latches(id, input.held, tap_time, scale as i32);
            }
            let actor = &world.actors[id.index];
            let control = actor.control.as_ref().expect("validated control");
            let group = actor
                .animation
                .as_ref()
                .context("player controller animation unavailable")?
                .playback
                .current_group();
            let idle = world.globals.prepare_player_idle(
                control.rope_attached,
                group,
                actor.attributes[6],
                random,
            )?;
            if let Some(group) = idle {
                let position = world.spatial(id)?.local.position;
                let surface = world.actor_surface(id)?;
                let event_context = super::effects::ActorEventContext::capture(world, id);
                let event_camera = &mut world.camera;
                let event_effects = &mut world.effects;
                let animation = world.actors[id.index]
                    .animation
                    .as_mut()
                    .expect("validated animation");
                let frame = animation.playback.request_with_sounds(
                    group,
                    0,
                    timing.nominal_ticks,
                    &mut |sound| {
                        if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                            return event_context.dispatch(
                                flags,
                                event_camera.as_mut(),
                                event_effects,
                            );
                        }
                        sounds(
                            id,
                            super::super::ScriptSound::Animation {
                                sound,
                                surface: Some(surface),
                                position: Some(position),
                            },
                        )
                    },
                )?;
                animation.frame = frame.or(animation.frame);
            }
            {
                // Queued motion 3 ends once the actor is back on its default group.
                let actor = &mut world.actors[id.index];
                let at_default = actor.animation.as_ref().is_some_and(|animation| {
                    animation.playback.current_group() == Some(animation.playback.default_group())
                });
                if let Some(control) = actor.control.as_mut()
                    && control.queued_motion == 3
                    && at_default
                {
                    control.queued_motion = 0;
                }
            }
            let lockout = world
                .globals
                .lockout()
                .context("player controller lockout unavailable")?;
            if lockout != 0 {
                return Ok(());
            }
            let dialogue_state = world
                .globals
                .dialogue_state()
                .context("player controller dialogue mode unavailable")?;
            let menu_state = world.globals.menu_state();
            if dialogue_state == 1 || menu_state == 1 {
                return Ok(());
            }
            let control = world.actors[id.index]
                .control
                .as_ref()
                .expect("validated control");
            ensure!(
                !control.rope_attached
                    && control.sliding_state == 0
                    && (matches!(control.living_state, 0 | 20 | 21 | 33 | 34)
                        || (10..=19).contains(&control.living_state)),
                "player controller requires a grounded or airborne state without rope or slide"
            );

            let airborne = control.queued_motion != 0 || control.falling_state != 0;
            // Holding a weapon or the combat flag selects the armed movement dispatch.
            let holding = {
                let actor = &world.actors[id.index];
                let (holding, _, _) = super::super::activation::weapon_predicates(
                    control.living_state,
                    Some(super::super::ActivationAnimation { group }),
                    control.combat_death_default.is_some(),
                    control.weapon_drawn,
                    actor.attributes[110],
                    inputs
                        .hud
                        .map(|hud| hud.inventory_ready())
                        .transpose()?
                        .unwrap_or(false),
                    inputs.shell_execution.unwrap_or(false),
                );
                holding
            };
            let armed = holding
                || control
                    .combat
                    .as_ref()
                    .is_some_and(|combat| combat.flags & 4 != 0);
            ensure!(
                world.actors[id.index].attributes[110] == 0,
                "player controller swimmer actor unsupported"
            );
            let action = select_sheathed_action(
                input.held,
                SheathedControllerState {
                    lockout,
                    dialogue_state,
                    menu_state,
                    walk_mode: input.walk_mode,
                    armed,
                },
            )?;
            if matches!(action, SheathedAction::Returned(_)) {
                return Ok(());
            }
            if (10..=19).contains(
                &world.actors[id.index]
                    .control
                    .as_ref()
                    .expect("validated control")
                    .living_state,
            ) {
                return world.control_ledge_player(id, action, query, inputs, timing, sounds);
            }
            // The drawing group and the scabbard-grip idle group return before turning.
            let group = world.actors[id.index]
                .animation
                .as_ref()
                .and_then(|animation| animation.playback.current_group());
            if matches!(group, Some(129 | 91)) {
                return Ok(());
            }
            world.turn_player(id, input.held, timing, sounds)?;
            if airborne {
                world.control_airborne_player(id, input.held)?;
                return Ok(());
            }
            let moved = match action {
                SheathedAction::Returned(_) | SheathedAction::Idle => Ok(()),
                SheathedAction::Sidestep(strafe) => {
                    let function = match strafe {
                        super::super::locomotion::Strafe::Left => "MoveLeft",
                        super::super::locomotion::Strafe::Right => "MoveRight",
                    };
                    world.step_player(id, function, inputs, sounds)
                }
                SheathedAction::WalkForward => {
                    world.advance_player_walking(id, query, timing, false, sounds)?;
                    Ok(())
                }
                SheathedAction::RunForward => world.step_player(id, "MoveForward", inputs, sounds),
                SheathedAction::RunBackward => {
                    world.step_player(id, "MoveBackward", inputs, sounds)
                }
                SheathedAction::WalkBackward => {
                    world.step_player(id, "WalkBackward", inputs, sounds)
                }
            };
            moved?;
            world.jump_key_arbitration(id, input.held, query, inputs, timing, sounds)?;
            if holding {
                ensure!(
                    query.scene.settings.tap_time.is_some(),
                    "player attack tap_time absent; re-export world"
                );
                world.control_player_attack(id, input.held, inputs, sounds)?;
            }
            Ok(())
        })
    }

    /// The grounded jump key, after movement dispatch. Running or walking jump groups with the
    /// step held set both directional-state bytes and wait for the jump window; any other state
    /// starts the selected jump entry.
    fn jump_key_arbitration(
        &mut self,
        id: super::ScriptActorId,
        held: &crate::player::input::PlayerInput,
        query: super::ground::OrdinaryGroundQuery<'_>,
        inputs: super::ScriptInputs<'_>,
        timing: crate::clock::ClockStep,
        sounds: &mut dyn FnMut(super::ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        if !held.jump {
            return Ok(());
        }
        let actor = &mut self.actors[id.index];
        let animation = actor
            .animation
            .as_ref()
            .context("jump arbitration animation unavailable")?;
        let (group, exit_reached) = (
            animation.playback.current_group(),
            animation.playback.exit_reached(),
        );
        let attribute_11 = actor.attributes[11];
        let control = actor
            .control
            .as_mut()
            .context("jump arbitration control unavailable")?;
        if control.directional_state.iter().any(|byte| byte & 1 != 0) {
            return Ok(());
        }
        if matches!(group, Some(65..=74))
            || control.queued_motion != 0
            || control.falling_state != 0
        {
            control
                .directional_state
                .iter_mut()
                .for_each(|byte| *byte &= !3);
            return Ok(());
        }
        let step_hold = attribute_11 == 0 || (control.movement_flags >> 4) & 3 == 3;
        if step_hold && matches!(group, Some(18 | 30)) && !exit_reached {
            control
                .directional_state
                .iter_mut()
                .for_each(|byte| *byte |= 3);
            control.jump_window = 0;
            return Ok(());
        }
        let environment = super::transition::ObstacleEnvironment {
            selected_player: Some(id),
            auto_grab: false,
            item_system_ready: inputs
                .hud
                .map(super::super::ScriptHud::inventory_ready)
                .transpose()?
                .unwrap_or(false),
            shell_execution: inputs.shell_execution.unwrap_or(false),
            ticks: timing.nominal_ticks,
        };
        let ledge_timing = super::ledge_states::LedgeTiming {
            fine_ticks: inputs
                .fine_ticks
                .context("jump arbitration needs the step's fine-tick increment")?,
            frame_scale: Some(timing.frame_scale),
        };
        self.begin_player_jump(id, query, environment, ledge_timing, 8, sounds)?;
        Ok(())
    }

    /// The controller in the ledge states. The jump key drops a hanging player through the hang
    /// release (refused during the jump cooldown and the action inhibition). A sidestep while
    /// hanging steps along the ledge; all other keys are ignored.
    fn control_ledge_player(
        &mut self,
        id: super::ScriptActorId,
        action: crate::player::controller::SheathedAction,
        query: super::ground::OrdinaryGroundQuery<'_>,
        inputs: super::ScriptInputs<'_>,
        timing: crate::clock::ClockStep,
        sounds: &mut dyn FnMut(super::ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        use crate::player::controller::SheathedAction;
        let held = inputs
            .player_controller
            .context("ledge controller input unavailable")?
            .held;
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("ledge controller control unavailable")?;
        if control.living_state == 15
            && let SheathedAction::Sidestep(strafe) = action
        {
            let function = match strafe {
                super::super::locomotion::Strafe::Left => "MoveLeft",
                super::super::locomotion::Strafe::Right => "MoveRight",
            };
            return self.step_player(id, function, inputs, sounds);
        }
        if !held.jump
            || control.living_state != 15
            || control.queued_motion != 0
            || control.falling_state != 0
            || self.globals.conditional_ticks()? != 0
        {
            return Ok(());
        }
        let environment = super::transition::ObstacleEnvironment {
            selected_player: Some(id),
            auto_grab: false,
            item_system_ready: inputs
                .hud
                .map(super::super::ScriptHud::inventory_ready)
                .transpose()?
                .unwrap_or(false),
            shell_execution: inputs.shell_execution.unwrap_or(false),
            ticks: timing.nominal_ticks,
        };
        let fine_ticks = inputs
            .fine_ticks
            .context("ledge controller needs the step's fine-tick increment")?;
        let timing = super::ledge_states::LedgeTiming {
            fine_ticks,
            frame_scale: Some(timing.frame_scale),
        };
        self.ledge_hang_release(id, query, environment, timing, sounds)?;
        Ok(())
    }

    /// The pending action (raised by a steep fatal landing): death animation override 74 and
    /// death-owning health -255 sourced from the player itself. The flag stays raised.
    fn run_pending_player_action(
        &mut self,
        id: super::ScriptActorId,
        inputs: super::ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(super::ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        self.globals.set_player_death_override_group(74)?;
        let source = super::super::activation::NativeSource::Actor {
            generation: id.generation,
            index: id.index,
        };
        self.apply_selected_death_health(id, -255, Some(source), inputs, Some(random), sounds)
    }

    /// The sheathed player's controller while its queued or falling byte is nonzero. Turning
    /// has been skipped and every movement helper refuses, so only these writes remain: forward
    /// steering of queued motion 1 in groups 32 and 33, the falling jump key's clearing of the
    /// companion motion byte, and the jump-key arbitration that clears the directional-state
    /// low bits.
    fn control_airborne_player(
        &mut self,
        id: super::ScriptActorId,
        held: &crate::player::input::PlayerInput,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let group = self.actors[index]
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        let control = self.actors[index]
            .control
            .as_mut()
            .context("airborne player control unavailable")?;
        steer_airborne(control, group, held);
        Ok(())
    }

    /// One of the controller's steppers (`MoveForward` for running, `MoveLeft` or `MoveRight` for a
    /// sidestep): the scripted native with duration 1, run synchronously on the selected player.
    /// The ordinary late stages resolve its translation.
    pub fn step_player(
        &mut self,
        id: super::ScriptActorId,
        function: &'static str,
        inputs: super::ScriptInputs<'_>,
        sounds: &mut dyn FnMut(super::ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let ordinary_actors = self
            .actor_lists
            .as_ref()
            .and_then(|lists| lists.retained_selected())
            .map(<[_]>::to_vec);
        let mut host = super::GameHost {
            native_source_program: None,
            world: self.world,
            animation_system: self.updates.animation_system,
            task_system: self.updates.task_system,
            index,
            generation: self.generation,
            ordinary_actors: ordinary_actors.as_deref(),
            walk_map_count: self.walk_nodes.as_ref().map(|nodes| nodes.map_count()),
            registry: self,
            sounds: Some(sounds),
            random: None,
            inputs,
        };
        host.native_task_call(
            (
                function,
                super::super::tasks::NativeTaskMode::SynchronousMovement,
            ),
            &[cyrus_script::Argument::Integer(1)],
            cyrus_script::Invocation {
                elapsed: Some(0),
                unpaused_tasks: 0,
                unpaused_blocking_tasks: 0,
            },
        )
        .with_context(|| format!("player {function} step"))?;
        Ok(())
    }

    /// Ordinary keyboard ground turn of the sheathed selected player, with its
    /// turn animation and roll, following preservation's
    /// [ground-turn rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ground-turn-eligibility-and-animation).
    /// The installed `mouse_turn` is 0; a SYSTEM.INI edited to enable mouse
    /// turning is not detected (the setting is not exported) and is ignored.
    fn turn_player(
        &mut self,
        id: super::ScriptActorId,
        held: &crate::player::input::PlayerInput,
        timing: crate::clock::ClockStep,
        sounds: &mut dyn FnMut(super::ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let control = self.actors[index]
            .control
            .as_ref()
            .context("player turn control unavailable")?;
        let torch = control.torch_burning != 0;
        // Actor bytes +0x1b2 and +0x1b3: the falling and queued-motion bytes.
        let eligible = control.falling_state == 0
            && control.queued_motion == 0
            && (held.walk || matches!(control.living_state, 0 | 27));
        let running = matches!(
            self.actors[index]
                .animation
                .as_ref()
                .and_then(|animation| animation.playback.current_group()),
            Some(18 | 30)
        );
        let angles = self.spatial(id)?.local.angles;
        let result = turn::ground_turn(
            angles[1],
            angles[2],
            turn::GroundTurnInput {
                delta: self.globals.turn.delta(timing.frame_scale),
                running,
                torch,
                walk_key: held.walk,
                forward_or_backward: held.forward || held.backward,
                eligible,
            },
        );
        let (heading, roll) = (result.heading, result.roll);
        if let Some(group) = result.group {
            let position = self.spatial(id)?.local.position;
            let surface = self.actor_surface(id)?;
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let animation = self.actors[index]
                .animation
                .as_mut()
                .context("player turn animation unavailable")?;
            let frame = animation.playback.request_with_sounds(
                group,
                0,
                timing.nominal_ticks,
                &mut |sound| {
                    if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                        return event_context.dispatch(flags, event_camera.as_mut(), event_effects);
                    }
                    sounds(
                        id,
                        super::super::ScriptSound::Animation {
                            sound,
                            surface: Some(surface),
                            position: Some(position),
                        },
                    )
                },
            )?;
            animation.frame = frame.or(animation.frame);
        }
        let spatial = self
            .spatial
            .as_mut()
            .context("player spatial registry unavailable")?
            .actor_mut(index)?;
        if spatial.local.angles[1] != heading || spatial.local.angles[2] != roll {
            spatial.local.angles[1] = heading;
            spatial.local.angles[2] = roll;
            spatial.refresh_transform()?;
        }
        Ok(())
    }

    /// Invoke the selected player's zero-argument native hook synchronously.
    /// Ordinary script/task suppression applies before the controller callback.
    /// The VM retains native failures; its runtime is restored even on failure.
    pub fn run_player_native_hook(
        &mut self,
        id: super::ScriptActorId,
        controller: &mut dyn FnMut(&mut ScriptWorld) -> Result<()>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        ensure!(
            matches!(self.actors[index].source, super::ScriptActorSource::Player),
            "player native hook requires selected player"
        );
        if let Some(error) = self.actors[index].failure() {
            anyhow::bail!("{error}");
        }
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("player native hook control unavailable")?;
        if control.script_control & 2 != 0
            || (self.updates.task_system
                && actor
                    .runtime
                    .as_ref()
                    .is_some_and(cyrus_script::Runtime::has_unpaused_task))
        {
            return Ok(false);
        }
        let call = actor
            .native_hook
            .clone()
            .context("player native hook absent")?;
        ensure!(
            call.function.eq_ignore_ascii_case("playermain")
                && call.arguments.is_empty()
                && call.kind == cyrus_script::program::CallKind::Function
                && call.target == cyrus_script::program::Object::Me,
            "player native hook requires direct zero-argument playermain"
        );
        let mut runtime = self.actors[index]
            .runtime
            .take()
            .context("player native runtime absent")?;
        let result = runtime.invoke_native_hook(
            &call,
            &mut PlayerHookHost {
                world: self,
                controller,
            },
            &mut [],
        );
        self.actors[index].runtime = Some(runtime);
        result.map(|_| true)
    }

    /// Begin one selected-player update before default selection and animation.
    /// This owns the support-history decrement, independently of controller timers.
    /// A matching finish is required before beginning another update.
    pub fn begin_player_movement_update(
        &mut self,
        id: super::ScriptActorId,
        nominal_ticks: u32,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        ensure!(
            matches!(self.actors[index].source, super::ScriptActorSource::Player),
            "player movement update requires selected player"
        );
        ensure!(
            self.actors[index].animation.is_some(),
            "player movement update requires explicit role initialization"
        );
        self.spatial(id)
            .context("player movement update requires logical entry pose")?;
        let step_index = self
            .attribute_names
            .iter()
            .position(|name| name.eq_ignore_ascii_case("attr_step_type"))
            .context("player movement: installed attr_step_type absent")?;
        let step_type = *self.actors[index]
            .attributes
            .get(step_index)
            .context("player movement: attr_step_type outside actor record")?;
        let startup =
            WalkingStartup::new(step_type).context("player movement: unsupported step type")?;
        let movement = self.actors[index]
            .player_movement
            .get_or_insert(PlayerMovement {
                startup,
                history: SupportHistory::new(0, 0),
                veto: 0,
                accepted_forward: None,
                latches: Default::default(),
            });
        ensure!(
            movement.accepted_forward.is_none(),
            "player movement update already active"
        );
        movement.history.begin_player_update(nominal_ticks);
        movement.accepted_forward = Some(false);
        Ok(())
    }

    /// Finish after continuation and late response. Rejected movement resets startup;
    /// accepted movement retains it even when the animation suppressed translation.
    pub fn finish_player_movement_update(&mut self, id: super::ScriptActorId) -> Result<()> {
        let index = self.resolve(id)?;
        let movement = self.actors[index]
            .player_movement
            .as_mut()
            .context("player movement update not begun")?;
        let accepted = movement
            .accepted_forward
            .take()
            .context("player movement update not active")?;
        movement.startup.finish_update(accepted);
        Ok(())
    }
    /// The selected player's scripted walking step check: the support query at the position one
    /// step ahead (`WalkForward`: attribute 12 plus the front body extent) or behind
    /// (`WalkBackward`: attribute 13 plus the rear extent), without moving the player. The
    /// admission is the forward walking admission applied to that probe, which is inferred.
    pub fn player_step_clear(
        &mut self,
        id: super::ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        forward: bool,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        let shape = self.ordinary_collision_shape(id)?;
        let (attribute, extent) = if forward {
            (12, shape.positive_extent[2])
        } else {
            (13, shape.negative_extent[2])
        };
        let step = i32::from(self.actors[index].attributes[attribute])
            .wrapping_add(extent)
            .wrapping_mul(256);
        let pose = self.spatial(id)?.local;
        let rotated =
            pose.matrix
                .transform([0, 0, if forward { step } else { step.wrapping_neg() }])?;
        let mut proposed = pose.position;
        proposed[0] = proposed[0].wrapping_add(rotated[0]);
        proposed[2] = proposed[2].wrapping_add(rotated[2]);
        let mut movement = self.actors[index]
            .player_movement
            .take()
            .context("player step check requires an active movement update")?;
        let result =
            self.query_player_forward_support(id, query, proposed, &mut movement.history, false);
        self.actors[index].player_movement = Some(movement);
        result
    }

    /// Probe an ordinary selected player's proposed forward position against the
    /// retained mixed candidates. This never moves the actor. Completed support
    /// queries commit history and surface even when final forward admission fails.
    pub fn query_player_forward_support(
        &mut self,
        id: super::ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        proposed: [i32; 3],
        history: &mut SupportHistory,
        alternate_collision: bool,
    ) -> Result<bool> {
        use bevy::math::{Mat3, Vec3};
        use cyrus_collision::{
            collision_mesh::BodyProbe,
            collision_world::{RuntimeForwardQuery, RuntimeForwardSupport},
            ground::{FallbackProbe, GroundProbe, SupportSource},
        };
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let actor = &self.actors[id.index];
        ensure!(
            matches!(actor.source, super::ScriptActorSource::Player),
            "world {} instance {}: forward query requires selected player",
            self.world,
            id.index
        );
        let control = actor
            .control
            .as_ref()
            .context("player forward control unavailable")?;
        ensure!(
            control.queued_motion == 0
                && control.falling_state == 0
                && control.sliding_state == 0
                && control.living_state == 0
                && control.directional_state.iter().all(|flags| flags & 3 == 0),
            "player forward query: ordinary grounded state required"
        );
        let collision = actor.attributes[1];
        let spatial = self.spatial(id)?;
        ensure!(
            spatial.parent.is_none(),
            "attached player forward query unsupported"
        );
        let rows = cyrus_world::transform::Matrix::collision_orientation(spatial.local.angles)
            .0
            .map(Vec3::from_array);
        let shape = self.ordinary_collision_shape(id)?;
        let spheres = shape
            .body
            .iter()
            .map(|sphere| {
                (
                    Vec3::from_array(sphere.center.map(|value| value as f32)),
                    sphere.radius as f32,
                )
            })
            .collect::<Vec<_>>();
        let position = proposed.map(i64::from);
        let ground = GroundProbe::new(position, shape.positive_extent[1], query.distance)?;
        let fallback = FallbackProbe::new(
            position,
            [0; 3],
            shape.positive_extent[1],
            history.ticks() != 0,
        )?;
        let terrain = query
            .scene
            .terrain
            .as_ref()
            .map(|terrain| {
                terrain
                    .sample(position)
                    .context("player forward query: terrain edge/water owner unavailable")
            })
            .transpose()?;
        let result = query.scene.static_collision.runtime_forward_support(
            &query.prepared.candidates,
            query
                .scene
                .actor_collision
                .as_ref()
                .context("player forward live geometry unavailable")?,
            &self.ordinary_collision_poses(query.prepared)?,
            RuntimeForwardQuery {
                body: BodyProbe {
                    position,
                    rotation: Mat3::from_cols(rows[0], rows[1], rows[2]).transpose(),
                    spheres: &spheres,
                    sidedness: query.sidedness,
                },
                ground: &ground,
                terrain,
                fallback: &fallback,
                check_slope: collision & 2 != 0 && !alternate_collision,
                excluded_candidate_type: None,
            },
        )?;
        let RuntimeForwardSupport::Queried(support) = result else {
            return Ok(false);
        };
        let metadata = support
            .as_ref()
            .map(|support| match support.source() {
                SupportSource::Actor { actor, .. } => self
                    .actors
                    .get(actor)
                    .and_then(|actor| actor.model.collision_flags)
                    .context("player support live metadata unavailable"),
                source => query.scene.static_collision.support_metadata(source),
            })
            .transpose()?;
        history.finish_query(metadata, true);
        if collision & 4 != 0 {
            let previous = self.actor_surface(id)?;
            let surface = support
                .as_ref()
                .map(|support| query.scene.support_surface(support, position))
                .transpose()?
                .unwrap_or(0);
            self.set_actor_surface(id, surface)?;
            self.surface_veto = Some(matches!(surface, 5 | 11));
            ensure!(
                previous == surface
                    || (!(1..=5).contains(&previous) && !(1..=5).contains(&surface)),
                "player forward query: water/lava transition effects unsupported"
            );
        }
        match support {
            None => Ok(false),
            Some(support) => Ok(crate::player::support::allows_forward(
                &support,
                collision,
                i32::from(
                    self.surface_veto
                        .context("player forward shared surface veto unavailable")?,
                ),
            )),
        }
    }

    /// Execute the admitted walking stage against mixed runtime candidates.
    /// `continuation` preserves pending idle selection by avoiding another group
    /// request. The caller invokes continuation only if the native stage did not
    /// already accept forward movement, and owns late collision/startup cleanup.
    pub fn advance_player_walking(
        &mut self,
        id: super::ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        timing: crate::clock::ClockStep,
        continuation: bool,
        sounds: &mut dyn FnMut(super::ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        use bevy::math::{Mat3, Vec3};
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        ensure!(
            matches!(
                self.actors[id.index].source,
                super::ScriptActorSource::Player
            ),
            "walking stage requires selected player"
        );
        ensure!(
            self.updates.animation_system,
            "walking stage requires enabled animation system"
        );
        let mut definition = query
            .scene
            .definition
            .as_ref()
            .context("walking definition unavailable")?
            .clone();
        let attribute = |name: &str| -> Result<u8> {
            let index = self
                .attribute_names
                .iter()
                .position(|installed| installed.eq_ignore_ascii_case(name))
                .with_context(|| {
                    format!("walking attribute {name}: installed SOUP definition absent")
                })?;
            self.actors[id.index]
                .attributes
                .get(index)
                .copied()
                .with_context(|| {
                    format!("walking attribute {name}: index {index} outside actor record")
                })
        };
        definition.forward_walk = attribute("attr_forward_walk")?;
        definition.forward_speed = attribute("attr_forward_speed")?;
        definition.step_type = attribute("attr_step_type")?;
        ensure!(
            definition.step_type == 1,
            "walking live step type unsupported"
        );
        let accepted = self.actors[id.index]
            .player_movement
            .as_ref()
            .and_then(|movement| movement.accepted_forward)
            .context("walking stage requires an active player movement update")?;
        if accepted {
            return Ok(true);
        }
        let mut movement = self.actors[id.index]
            .player_movement
            .take()
            .expect("prepared movement");
        let result = (|| -> Result<bool> {
            let animation = self.actors[id.index]
                .animation
                .as_ref()
                .context("walking animation unavailable")?;
            if continuation && animation.playback.current_group() != Some(20) {
                return Ok(false);
            }
            let pose = self.spatial(id)?.local;
            let rows = pose.matrix.0.map(Vec3::from_array);
            let rotation = Mat3::from_cols(rows[0], rows[1], rows[2]).transpose();
            let probe = definition.forward_probe(pose.position.map(i64::from), rotation)?;
            let mut proposed = [0; 3];
            for axis in 0..3 {
                proposed[axis] =
                    i32::try_from(probe[axis]).context("walking probe outside Q8 range")?;
            }
            if !self.query_player_forward_support(
                id,
                query,
                proposed,
                &mut movement.history,
                false,
            )? {
                return Ok(false);
            }
            self.actors[id.index]
                .control
                .as_mut()
                .context("walking control unavailable")?
                .motion_classification = 6;
            let surface = self.actor_surface(id)?;
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let animation = self.actors[id.index]
                .animation
                .as_mut()
                .expect("validated animation");
            let distance = if continuation {
                movement.startup.advance(
                    definition.forward_walk,
                    definition.forward_speed,
                    timing.frame_scale,
                    !animation.playback.translation_suppressed(),
                )
            } else {
                let Some(step) = movement.startup.request(
                    &mut animation.playback,
                    &definition,
                    timing.frame_scale,
                    timing.nominal_ticks,
                    &mut |sound| {
                        if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                            return event_context.dispatch(
                                flags,
                                event_camera.as_mut(),
                                event_effects,
                            );
                        }
                        sounds(
                            id,
                            super::super::ScriptSound::Animation {
                                sound,
                                surface: Some(surface),
                                position: Some(pose.position),
                            },
                        )
                    },
                )?
                else {
                    return Ok(false);
                };
                animation.frame = step.frame.or(animation.frame);
                step.distance
            };
            self.apply_ordinary_directional_motion(
                id,
                6,
                i32::try_from(distance).context("walking distance outside i32")?,
            )?;
            movement.accepted_forward = Some(true);
            Ok(true)
        })();
        self.actors[id.index].player_movement = Some(movement);
        result
    }

    /// Execute one caller-admitted update. Entry coordinates are supplied only for
    /// an explicit new presentation entry; subsequent updates use the logical pose.
    pub(crate) fn explicit_grounded_step(
        &mut self,
        scene: &PlayerScene,
        data: &Animations,
        tick: &GroundedTick,
        entry: Option<&PlayerPose>,
        sounds: &mut dyn FnMut(AnimationSound, u8, [i64; 3]) -> Result<()>,
    ) -> Result<GroundedPresentation> {
        let index = self.resolve(tick.actor)?;
        ensure!(
            matches!(self.actors[index].source, super::ScriptActorSource::Player),
            "world {} instance {index}: grounded update requires the logical player",
            self.world
        );
        ensure!(
            scene.world == self.world
                && data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} player update: scene/animation content mismatch",
            self.world
        );
        ensure!(
            tick.rotation.is_finite(),
            "player update: nonfinite rotation"
        );
        ensure!(
            self.actors[index]
                .player_movement
                .as_ref()
                .is_none_or(|movement| movement.accepted_forward.is_none()),
            "explicit grounded step conflicts with an active player movement update"
        );
        let definition = scene
            .definition
            .as_ref()
            .context("player definition unavailable")?;
        if self.spatial.is_none() {
            self.prepare_spatial()?;
        }
        if let Some(entry) = entry {
            let mut position = [0; 3];
            for (axis, value) in entry.position.into_iter().enumerate() {
                position[axis] = i32::try_from(value).with_context(|| {
                    format!("player entry axis {axis}: Q8 position outside i32")
                })?;
            }
            self.spatial
                .as_mut()
                .context("player spatial registry unavailable")?
                .place_player(
                    index,
                    &self.actors[index],
                    position,
                    [0, entry.heading, 0],
                    true,
                )?;
            self.actors[index].player_movement = None;
        }
        let initial_position = self.spatial(tick.actor)?.local.position.map(i64::from);
        let initial_surface = self.actor_surface(tick.actor)?;
        if self.actors[index].player_movement.is_none() {
            self.actors[index].player_movement = Some(PlayerMovement {
                startup: WalkingStartup::new(definition.step_type)
                    .context("player step: unsupported step type")?,
                history: SupportHistory::new(0, 0),
                veto: 0,
                accepted_forward: None,
                latches: Default::default(),
            });
        }
        self.player_animation(tick.actor, data, tick.nominal_ticks, &mut |sound| {
            sounds(sound, initial_surface, initial_position)
        })?;

        let spatial = self
            .spatial
            .as_mut()
            .context("player spatial registry unavailable")?
            .actor_mut(index)?;
        ensure!(
            spatial.parent.is_none(),
            "attached grounded player unsupported"
        );
        spatial.snapshot();
        let actor = &mut self.actors[index];
        let movement = actor
            .player_movement
            .as_mut()
            .context("player movement unavailable")?;
        let animation = actor
            .animation
            .as_mut()
            .context("player animation unavailable")?;
        let mut walk = GroundedWalk {
            position: initial_position,
            surface: initial_surface,
            startup: movement.startup,
        };
        let mut input = GroundedUpdate {
            motion: ForwardWalk {
                heading: spatial.local.angles[1],
                frame_scale: tick.frame_scale,
                nominal_ticks: tick.nominal_ticks,
            },
            walk: tick.walk,
        };
        let result = (|| -> Result<Option<ModelFrame>> {
            if let Some(frame) = grounded::advance_animation(
                scene,
                tick.nominal_ticks,
                &mut movement.history,
                &mut animation.playback,
                &mut |sound| sounds(sound, walk.surface, walk.position),
            )? {
                animation.frame = Some(frame);
            }
            let mut frame = animation
                .frame
                .map(|frame| data.resolve_frame("CYRUS", frame))
                .transpose()?;
            let candidates = scene.candidates(
                tick.view,
                walk.position,
                frame.as_ref().map(|frame| frame.model.as_str()),
            )?;
            let query = StaticGroundQuery {
                scene,
                candidates: &candidates,
                rotation: tick.rotation,
                collision_attribute: tick.collision_attribute,
                alternate_collision: tick.alternate_collision,
                sidedness: tick.sidedness,
                surface_update: tick.surface_update,
            };
            if let Some(reference) = walk.move_grounded(
                &query,
                &mut input,
                (&mut movement.history, &mut movement.veto),
                &mut animation.playback,
                sounds,
            )? {
                animation.frame = Some(reference);
                frame = Some(data.resolve_frame("CYRUS", reference)?);
            }
            Ok(frame)
        })();
        // Support and receiver mutations precede fallible transition effects.
        movement.startup = walk.startup;
        actor.surface = Some(walk.surface);
        let mut position = [0; 3];
        for (axis, value) in walk.position.into_iter().enumerate() {
            position[axis] = i32::try_from(value)
                .with_context(|| format!("player update axis {axis}: Q8 position outside i32"))?;
        }
        spatial.local.position = position;
        spatial.local.angles[1] = input.motion.heading;
        let frame = result?;
        spatial.refresh_transform()?;
        Ok(GroundedPresentation {
            pose: PlayerPose {
                position: walk.position,
                heading: input.motion.heading,
            },
            frame,
            surface: walk.surface,
        })
    }
}

struct PlayerHookHost<'a, 'b> {
    world: &'a mut ScriptWorld,
    controller: &'b mut dyn FnMut(&mut ScriptWorld) -> Result<()>,
}

impl cyrus_script::Host for PlayerHookHost<'_, '_> {
    fn call(
        &mut self,
        target: &cyrus_script::Target,
        kind: cyrus_script::program::CallKind,
        function: &str,
        arguments: &[cyrus_script::Argument],
        _: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> Result<cyrus_script::CallResult> {
        ensure!(
            target == &cyrus_script::Target::Me
                && kind == cyrus_script::program::CallKind::Function
                && function.eq_ignore_ascii_case("playermain")
                && arguments.is_empty(),
            "player native hook received an unexpected call"
        );
        (self.controller)(self.world)?;
        Ok(cyrus_script::CallResult::CompleteWithoutValue)
    }

    fn poll(
        &mut self,
        _: u64,
        _: &[cyrus_script::Argument],
        _: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> Result<bool> {
        anyhow::bail!("player native hook cannot poll tasks")
    }

    fn end(&mut self, _: cyrus_script::Thread) -> Result<()> {
        anyhow::bail!("player native hook cannot end a script body")
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct PlayerMovementSnapshot {
    phase: u8,
    support_ticks: u32,
    support_metadata: u8,
    veto: i32,
    accepted_forward: Option<bool>,
}
impl PlayerMovement {
    pub(super) fn snapshot(&self) -> PlayerMovementSnapshot {
        PlayerMovementSnapshot {
            phase: self.startup.snapshot_phase(),
            support_ticks: self.history.ticks(),
            support_metadata: self.history.metadata(),
            veto: self.veto,
            accepted_forward: self.accepted_forward,
        }
    }

    pub(super) fn from_snapshot(snapshot: PlayerMovementSnapshot) -> Result<Self> {
        Ok(Self {
            startup: WalkingStartup::from_snapshot_phase(snapshot.phase)?,
            history: SupportHistory::new(snapshot.support_ticks, snapshot.support_metadata),
            veto: snapshot.veto,
            accepted_forward: snapshot.accepted_forward,
            latches: Default::default(),
        })
    }
}

/// The airborne controller's writes: forward steering of queued motion 1 in groups 32 and 33,
/// the falling jump key's clearing of the companion motion byte, and the jump-key arbitration.
fn steer_airborne(
    control: &mut super::ActorControlState,
    group: Option<u16>,
    held: &crate::player::input::PlayerInput,
) {
    if held.forward && control.queued_motion & 127 == 1 && matches!(group, Some(32 | 33)) {
        control.queued_motion = 2;
    }
    if held.jump {
        // shortcut: the two retained rope-release flags are not modelled (ropes #21);
        // upgrade when the rope owner retains them.
        if control.falling_state != 0 {
            control.motion_companion = 0;
        }
        if control.directional_state.iter().all(|byte| byte & 1 == 0) {
            for byte in &mut control.directional_state {
                *byte &= !3;
            }
        }
    }
}

#[cfg(test)]
mod airborne_tests {
    use super::*;
    use crate::player::input::PlayerInput;

    #[test]
    fn forward_steers_queued_motion_in_the_fall_groups_only() {
        let held = PlayerInput {
            forward: true,
            ..Default::default()
        };
        let mut control = super::super::ActorControlState {
            queued_motion: 0x81,
            ..Default::default()
        };
        steer_airborne(&mut control, Some(30), &held);
        assert_eq!(control.queued_motion, 0x81);
        steer_airborne(&mut control, Some(32), &held);
        assert_eq!(control.queued_motion, 2);
    }

    #[test]
    fn the_jump_key_clears_companion_motion_and_the_low_directional_bits() {
        let held = PlayerInput {
            jump: true,
            ..Default::default()
        };
        let mut control = super::super::ActorControlState {
            falling_state: 1,
            motion_companion: 5,
            directional_state: [6, 0x82],
            ..Default::default()
        };
        steer_airborne(&mut control, None, &held);
        assert_eq!(
            (control.motion_companion, control.directional_state),
            (0, [4, 0x80])
        );
        control.directional_state = [1, 0];
        steer_airborne(&mut control, None, &held);
        assert_eq!(control.directional_state, [1, 0]);
    }
}
