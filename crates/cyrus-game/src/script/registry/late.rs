//! Automatic late collision composes retained candidates and logical actor response.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_collision::ground::SelectedSupport;

/// Prevalidation, selected support and the surface updater's (previous, returned) surfaces when it ran.
type PointSupport = (
    super::super::OrdinaryGroundPrevalidation,
    Option<SelectedSupport>,
    Option<(u8, u8)>,
);

#[derive(Clone, Copy)]
pub(super) struct OrdinaryLateInputs<'a> {
    pub script: super::ScriptInputs<'a>,
    pub ticks: u32,
    pub fine_ticks: Option<u32>,
    pub step_fps: Option<u32>,
    pub weapon: Option<super::weapon::WeaponInputs>,
}

impl ScriptWorld {
    /// The ordinary surface updater. Returns its result: zero for an excluded update, the resolved
    /// surface for an update that stores it or (falling, sliding, living states 27 to 29) returns
    /// it without storing.
    fn assign_ordinary_support_surface(
        &mut self,
        id: ScriptActorId,
        scene: &crate::player::data::PlayerScene,
        support: Option<&SelectedSupport>,
        selected: bool,
    ) -> Result<u8> {
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("support surface control unavailable")?;
        if control.queued_motion != 0
            || control.directional_state.iter().any(|flags| flags & 3 != 0)
        {
            return Ok(0);
        }
        let keep_stored = control.falling_state != 0
            || control.sliding_state != 0
            || (27..=29).contains(&control.living_state);
        if keep_stored && !selected {
            // Only the selected player's transition handler reads the returned value.
            return Ok(self.actors[id.index].surface.unwrap_or(0));
        }
        let surface = support
            .map(|support| {
                scene.support_surface(
                    support,
                    self.spatial(id)?.query_pose()?.position.map(i64::from),
                )
            })
            .transpose()?
            .unwrap_or(0);
        if !keep_stored {
            self.set_actor_surface(id, surface)?;
            self.surface_veto = Some(matches!(surface, 5 | 11));
        }
        Ok(surface)
    }

    /// The selected player's water and lava transition effects (preservation player.md): the
    /// swim entry and walk exit group requests, the splash effect object (identifiers 0x2500 water,
    /// 0x2580 lava) and the splash sound. `effect_height` is the effect's Q8 Y from the support
    /// query (`None` without support); a host without effect or camera services creates no effect.
    #[allow(clippy::too_many_arguments)] // the handler's inputs: surfaces, clock, random, support, sink
    pub fn selected_surface_transition(
        &mut self,
        id: ScriptActorId,
        previous: u8,
        returned: u8,
        ticks: u32,
        random: &mut crate::random::EngineRandom,
        effect_height: Option<i32>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        if returned == previous {
            return Ok(());
        }
        let stored = self.actors[id.index].surface.unwrap_or(0);
        let in_water = |surface: u8| (1..=4).contains(&surface);
        let deep = |surface: u8| matches!(surface, 2 | 4);
        if !in_water(stored) && !in_water(previous) {
            if stored == 5 || previous == 5 {
                self.spawn_transition_effect(id, 0x2580, effect_height)?;
            }
            return Ok(());
        }
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("surface transition control unavailable")?;
        // shortcut: the retained direction is normalised, so the raw forward motion bytes 8, 9 and
        // 24 read as forward here; upgrade when the raw motion byte is retained.
        let forward = control.motion_direction == 2;
        let torch = control.torch_burning != 0;
        if deep(stored) {
            self.request_ordinary_group(id, if forward { 142 } else { 143 }, false, ticks, sounds)?;
        } else if deep(previous) {
            let group = match (forward, torch) {
                (true, false) => 18,
                (true, true) => 30,
                (false, false) => 19,
                (false, true) => 23,
            };
            self.request_ordinary_group(id, group, false, ticks, sounds)?;
        }
        self.spawn_transition_effect(id, 0x2500, effect_height)?;
        let effect = if !in_water(stored) || (in_water(previous) && !deep(stored)) {
            70
        } else {
            71
        };
        let rate_offset_hz = random.rnd(64)? - 32;
        let position = self.sight_position(id.index)?;
        sounds(
            id,
            super::super::ScriptSound::Positional {
                effect,
                volume: 0,
                rate_offset_hz,
                position,
                start_index: None,
            },
        )
    }

    /// The transition's effect object: the actor's X/Z moved eight map units toward the camera,
    /// at the support height plus the standing height and six units (preservation player.md).
    fn spawn_transition_effect(
        &mut self,
        id: ScriptActorId,
        descriptor: u32,
        height: Option<i32>,
    ) -> Result<()> {
        let (Some(height), Some(camera)) = (height, self.camera.as_ref()) else {
            return Ok(());
        };
        if self.effects.is_none() {
            return Ok(());
        }
        let position = self.spatial(id)?.local.position;
        let bearing = crate::script::spatial::query_bearing(
            position[0].wrapping_sub(camera.current.position[0]),
            position[2].wrapping_sub(camera.current.position[2]),
        );
        let heading = ((1024 + i32::from(bearing)) & 2047) as u16;
        let [x, _, z] = cyrus_world::transform::translate_forward(position, heading, 0x800)?;
        self.spawn_environment_effect(descriptor, [x, height, z])
    }

    /// Q8 Y of a transition effect for the selected support: the support height plus standing
    /// height, plus six units (a fallback support keeps its actor origin, so its extent is added).
    fn transition_effect_height(
        &self,
        id: ScriptActorId,
        support: Option<&SelectedSupport>,
        standing_height: i32,
    ) -> Result<Option<i32>> {
        let origin = match support {
            None => return Ok(None),
            Some(support @ SelectedSupport::Primary { .. }) => {
                support.actor_height(0, standing_height)
            }
            Some(support) => {
                let extent = self.ordinary_collision_shape(id)?.positive_extent[1];
                support.actor_height(0, standing_height) + i64::from(extent) * 256
            }
        };
        Ok(Some(
            i32::try_from(origin + 0x600).context("transition effect height outside Q8 range")?,
        ))
    }

    fn query_ordinary_point_support(
        &mut self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<PointSupport> {
        let prevalidation = self.query_ordinary_ground_prevalidation(id, query, false)?;
        self.apply_ordinary_ground_prevalidation(
            id,
            prevalidation,
            query.animations,
            environment,
            sounds,
        )?;
        // An allocator-zeroed actor has surface zero before its first update.
        let previous = self.actors[id.index].surface.unwrap_or(0);
        let support = self.query_ordinary_ground(id, query)?;
        let updated = if self.actors[id.index].attributes[1] & 4 != 0 {
            let selected = environment.selected_player == Some(id);
            let returned =
                self.assign_ordinary_support_surface(id, query.scene, support.as_ref(), selected)?;
            Some((previous, returned))
        } else {
            None
        };
        Ok((prevalidation, support, updated))
    }

    /// Check nonplayer support at a proposed local Q8 position, restoring XYZ
    /// while retaining collision writes, including rollback and surface state.
    /// Linked actors and selected-player feedback remain unavailable.
    pub fn admit_ordinary_destination(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        ensure!(
            self.selected_player_index() != Some(index) && environment.selected_player != Some(id),
            "world {} instance {index}: selected-player destination collision unavailable",
            self.world,
        );
        let spatial = self.spatial(id)?;
        ensure!(
            spatial.parent.is_none() && spatial.linked_position.is_none(),
            "world {} instance {index}: linked destination collision unavailable",
            self.world,
        );
        let saved = spatial.local.position;
        self.spatial
            .as_mut()
            .context("destination spatial owner unavailable")?
            .actor_mut(index)?
            .local
            .position = destination;
        let result = self.query_ordinary_point_support(id, query, environment, sounds);
        self.spatial
            .as_mut()
            .context("destination spatial owner unavailable")?
            .actor_mut(index)?
            .local
            .position = saved;
        let (_, support, _) = result?;
        let Some(support) = support else {
            return Ok(false);
        };
        if self.actors[index].attributes[1] & 2 != 0
            && matches!(support, SelectedSupport::Primary { support, .. } if support.classification() == 2)
        {
            return Ok(false);
        }
        Ok(!self.surface_veto.with_context(|| {
            format!(
                "world {} instance {index}: destination shared surface veto unavailable",
                self.world,
            )
        })?)
    }

    /// Support regained while falling: threshold response, then the landing request.
    fn land_ordinary_fall(
        &mut self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        weapon: Option<super::weapon::WeaponInputs>,
        selected: Option<super::falling::SelectedLanding<'_, '_>>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let landing = self.updates.fall_landing.with_context(|| {
            format!(
                "world {} instance {}: fall landing thresholds missing; re-export scripts",
                self.world, id.index
            )
        })?;
        let settings = super::super::OrdinaryLandingSettings {
            death_height: landing.death_height,
            hurt_height: landing.hurt_height,
            hurt_zap: landing.hurt_zap,
        };
        let combat_mode = self.actors[id.index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .is_some_and(|combat| combat.flags & 4 != 0);
        let selected_holding = weapon
            .and_then(|input| self.resolve(input.selected_player).ok().map(|i| (i, input)))
            .and_then(|(index, input)| {
                let actor = &self.actors[index];
                let control = actor.control.as_ref()?;
                let animation = actor.animation.as_ref()?;
                let (holding, _, _) = super::super::activation::weapon_predicates(
                    control.living_state,
                    Some(super::super::ActivationAnimation {
                        group: animation.playback.current_group(),
                    }),
                    control.combat_death_default.is_some(),
                    control.weapon_drawn,
                    actor.attributes[110],
                    input.item_system_ready,
                    input.shell_execution,
                );
                Some(holding)
            });
        let decision = self.land_with_damage(
            id,
            data,
            environment,
            settings,
            Some(combat_mode),
            selected,
            sounds,
        )?;
        if decision == super::super::OrdinaryLandingDecision::Ordinary {
            self.complete_ordinary_landing(id, data, environment, selected_holding, sounds)?;
        }
        Ok(())
    }

    /// Walking off an edge: restore the stable position, enter the ordinary jump with
    /// motion 8, force its group and run the queued arbitration once.
    fn recover_ordinary_fall_entry(
        &mut self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        fine_ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let actor = &mut self.actors[id.index];
        let control = actor
            .control
            .as_mut()
            .context("fall recovery control unavailable")?;
        for byte in &mut control.directional_state {
            *byte &= !3;
        }
        let stable = control.stable_local_position;
        let torch = control.torch_burning != 0;
        let task_active = actor.tasks.is_active();
        self.spatial
            .as_mut()
            .context("fall recovery spatial state unavailable")?
            .actor_mut(id.index)?
            .local
            .position = stable;
        self.begin_ordinary_jump(id, query.animations, 8, task_active, environment, sounds)?;
        if self.updates.animation_system {
            self.request_ordinary_group(
                id,
                if torch { 35 } else { 34 },
                true,
                environment.ticks,
                sounds,
            )?;
        }
        self.resolve_ordinary_queued_motion(id, query, environment, fine_ticks, 0, sounds)?;
        Ok(())
    }

    /// Clear the falling byte after the edge service took over the update's fall handling.
    fn end_ordinary_fall(&mut self, id: ScriptActorId) -> Result<()> {
        self.actors[id.index]
            .control
            .as_mut()
            .context("edge service fall control unavailable")?
            .falling_state = 0;
        self.actors[id.index].sync_falling_motion();
        Ok(())
    }

    /// Support missed: start or continue an animated fall. The selector
    /// and static-model entry services are not ported and refuse explicitly.
    fn miss_ordinary_support(
        &mut self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        fine_ticks: u32,
        frame_scale: Option<u32>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let timing = super::ledge_states::LedgeTiming {
            fine_ticks,
            frame_scale,
        };
        let post = query.scene.settings.post_collide_height;
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("support miss control unavailable")?;
        if control.falling_state != 0 {
            if self.selected_edge_service(id, query, environment, timing, sounds)? {
                self.end_ordinary_fall(id)?;
                return Ok(());
            }
            self.continue_ordinary_falling(
                id,
                query.animations,
                environment,
                fine_ticks,
                0,
                post,
                sounds,
            )?;
            return Ok(());
        }
        // A jump selector within three of 24 turns the support miss into the scripted jump entry.
        if control.jump_selector != 0 && control.jump_selector.abs_diff(24) < 4 {
            if environment.selected_player == Some(id) {
                self.begin_player_jump(id, query, environment, timing, 8, sounds)?;
            } else {
                let task_active = self.actors[id.index].tasks.is_active();
                self.begin_ordinary_jump(
                    id,
                    query.animations,
                    8,
                    task_active,
                    environment,
                    sounds,
                )?;
            }
            return Ok(());
        }
        if control.directional_state.iter().any(|byte| byte & 3 != 0) {
            return self.recover_ordinary_fall_entry(id, query, environment, fine_ticks, sounds);
        }
        let group = self.actors[id.index]
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        // The jump groups' last displacement: the retained direction at Q8 distance 4096.
        if matches!(group, Some(41..=43)) {
            let direction = self.actors[id.index]
                .control
                .as_ref()
                .context("fall entry control unavailable")?
                .motion_direction;
            self.apply_ordinary_directional_motion(id, direction, 4096)?;
        }
        let path = query.animations.actor(self.actors[id.index].name())?["model_load_path"]
            .as_str()
            .context("fall entry model path unavailable")?;
        if path == "animation_table" {
            if self.complete_ordinary_animated_fall_entry(
                id,
                query.animations,
                environment,
                sounds,
            )? {
                if self.selected_edge_service(id, query, environment, timing, sounds)? {
                    return self.end_ordinary_fall(id);
                }
                self.run_ordinary_falling_writer(id, true, fine_ticks, 0, post)?;
            }
        } else {
            ensure!(
                matches!(path, "static" | "none"),
                "world {} instance {}: fall entry model path {path} unsupported",
                self.world,
                id.index
            );
            if self.begin_static_ordinary_fall(id, query.animations)? {
                self.run_ordinary_falling_writer(id, false, fine_ticks, 0, post)?;
            }
        }
        Ok(())
    }

    /// A slide that ends on support: past the installed slide hurt distance the actor is pushed
    /// on, plays the hurt animation (group 73) and loses the installed zap of health. Sliding
    /// state clears either way. The selected player hurts whenever its pending action flag is
    /// raised too, stores death override 74 instead of requesting the animation, and takes the
    /// zap through its death-owning health (`selected` supplies that service's inputs).
    fn end_ordinary_slide(
        &mut self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        selected: Option<(
            crate::script::ScriptInputs<'_>,
            &mut crate::random::EngineRandom,
        )>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("slide termination control unavailable")?;
        if control.sliding_state == 0 {
            return Ok(());
        }
        let distance = control.sliding_distance;
        let (height, zap) = query
            .scene
            .settings
            .slide_hurt
            .context("slide termination settings unavailable; re-export world")?;
        let is_selected = environment.selected_player == Some(id);
        if (is_selected && self.globals.player_pending_action())
            || i64::from(distance) > i64::from(height) << 8
        {
            let group = self.actors[id.index]
                .animation
                .as_ref()
                .and_then(|animation| animation.playback.current_group());
            self.apply_ordinary_directional_motion(
                id,
                if matches!(group, Some(75 | 77)) { 2 } else { 3 },
                0x800,
            )?;
            if is_selected {
                self.globals.set_player_death_override_group(74)?;
            } else if self.updates.animation_system {
                self.request_ordinary_group(id, 73, false, environment.ticks, sounds)?;
            }
            let (_, combat_active) = self.slope_stance(id, environment, None)?;
            self.change_health(
                id,
                -zap,
                query.animations,
                environment,
                Some(combat_active),
                selected,
                sounds,
            )?;
        }
        let control = self.actors[id.index]
            .control
            .as_mut()
            .context("slide termination control unavailable")?;
        control.sliding_state = 0;
        Ok(())
    }

    /// The held-weapon predicate and combat mode bit 2 that the slope turn rate and groups read.
    fn slope_stance(
        &self,
        id: ScriptActorId,
        environment: super::transition::ObstacleEnvironment,
        weapon: Option<super::weapon::WeaponInputs>,
    ) -> Result<(bool, bool)> {
        let actor = &self.actors[id.index];
        let control = actor
            .control
            .as_ref()
            .context("slope stance control unavailable")?;
        let (holding, _, _) = super::super::activation::weapon_predicates(
            control.living_state,
            Some(super::super::ActivationAnimation {
                group: actor
                    .animation
                    .as_ref()
                    .and_then(|animation| animation.playback.current_group()),
            }),
            control.combat_death_default.is_some(),
            control.weapon_drawn,
            actor.attributes[110],
            environment.item_system_ready || weapon.is_some_and(|input| input.item_system_ready),
            environment.shell_execution,
        );
        let combat_active = control
            .combat
            .as_ref()
            .is_some_and(|combat| combat.flags & 4 != 0);
        Ok((holding, combat_active))
    }

    /// The ground response: the support query, queued motion and the support, landing, slope and
    /// miss handling that follow it. Returns whether the support query found support.
    #[allow(clippy::too_many_arguments)] // one stage of the late wrapper with its caller's services
    fn ground_response_stage(
        &mut self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        inputs: OrdinaryLateInputs<'_>,
        (mask, special, falling): (u8, bool, bool),
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let OrdinaryLateInputs {
            script,
            ticks,
            fine_ticks,
            step_fps,
            weapon,
        } = inputs;
        // An animated actor that is falling probes a shorter column once the measured FPS
        // reaches ten; static models always use the full distance.
        let support_query = if falling {
            let fps = step_fps.context("falling support probe needs the step's measured FPS")?;
            super::ground::OrdinaryGroundQuery {
                distance: if fps >= 10 {
                    (query.distance >> 2) * 3
                } else {
                    query.distance
                },
                ..query
            }
        } else {
            query
        };
        let (prevalidation, support, updated) =
            self.query_ordinary_point_support(id, support_query, environment, sounds)?;
        if let Some((previous, returned)) = updated
            && environment.selected_player == Some(id)
        {
            let effect_height = self.transition_effect_height(
                id,
                support.as_ref(),
                query.scene.settings.standing_height,
            )?;
            self.selected_surface_transition(
                id,
                previous,
                returned,
                ticks,
                random,
                effect_height,
                sounds,
            )?;
        }
        let support_found = support.is_some();
        let mut queued_handled = false;
        if mask & 4 != 0 && !special {
            let control = self.actors[id.index]
                .control
                .as_ref()
                .expect("admitted ground control");
            // The `BounceObject` task owns living states 30 and 31 (entry and rising): the ground
            // response is skipped for them; state 32 (falling) takes the ordinary response.
            if matches!(control.living_state, 30 | 31) {
                return Ok(support_found);
            }
            if control.queued_motion != 0 && !matches!(control.living_state, 20 | 21) {
                queued_handled = self.resolve_ordinary_queued_motion(
                    id,
                    query,
                    environment,
                    fine_ticks.context("queued motion needs the step's fine-tick increment")?,
                    0,
                    sounds,
                )?;
            }
        }
        if mask & 4 != 0 {
            let living_state = self.actors[id.index]
                .control
                .as_ref()
                .expect("admitted ground control")
                .living_state;
            if matches!(living_state, 20 | 21) {
                self.admit_actor_weapon_draw(
                    id,
                    weapon.context("combat ground weapon admission unavailable")?,
                    ticks,
                    sounds,
                )?;
            } else if special {
                // The selected player's ledge states run their transition in place of the
                // ordinary motion and ground response; other actors change nothing.
                self.ledge_transition(
                    id,
                    query,
                    environment,
                    super::ledge_states::LedgeTiming {
                        fine_ticks: fine_ticks
                            .context("ledge transition needs the step's fine-tick increment")?,
                        frame_scale: inputs.script.frame_scale,
                    },
                    sounds,
                )?;
            } else if !queued_handled {
                let actor = &self.actors[id.index];
                let control = actor.control.as_ref().expect("admitted late control");
                ensure!(
                    !matches!(control.living_state, 27..=29),
                    "state-specific ground response owner unavailable"
                );
                if let Some(support) = support.as_ref() {
                    if falling {
                        let selected = (environment.selected_player == Some(id)).then(|| {
                            super::falling::SelectedLanding {
                                inputs: script,
                                random: &mut *random,
                                steep: crate::player::support::classification(support, mask) == 2,
                            }
                        });
                        self.land_ordinary_fall(
                            id,
                            query.animations,
                            environment,
                            weapon,
                            selected,
                            sounds,
                        )?;
                    }
                    let extent = self.ordinary_collision_shape(id)?.positive_extent[1];
                    let height = i32::try_from(
                        support.actor_height(extent, query.scene.settings.standing_height),
                    )
                    .context("ordinary support origin outside Q8 range")?;
                    self.spatial
                        .as_mut()
                        .context("ground response spatial state unavailable")?
                        .actor_mut(id.index)?
                        .local
                        .position[1] = height;
                    // A steep-surface result with a zero native return state ends the ground
                    // response in the slope response; a nonzero state ignores the result.
                    if prevalidation.sliding && self.globals.native_command_return == 0 {
                        let control = self.actors[id.index]
                            .control
                            .as_mut()
                            .expect("admitted ground control");
                        control.falling_state = 0;
                        let (holding, combat_active) =
                            self.slope_stance(id, environment, weapon)?;
                        self.actors[id.index].sync_falling_motion();
                        self.resolve_ordinary_slope(
                            id,
                            Some(support),
                            query,
                            super::slope::OrdinarySlopeInputs {
                                selected_player: environment.selected_player,
                                holding_weapon: holding,
                                combat_active,
                                frame_scale: script
                                    .frame_scale
                                    .context("slope response needs the step's frame scale")?,
                                ticks,
                            },
                            sounds,
                        )?;
                    } else {
                        let selected = (environment.selected_player == Some(id))
                            .then_some((script, &mut *random));
                        self.end_ordinary_slide(id, query, environment, selected, sounds)?;
                    }
                } else if self.actors[id.index].attributes[107] != 0 {
                    self.actors[id.index]
                        .control
                        .as_mut()
                        .expect("admitted ground control")
                        .falling_state = 0;
                } else {
                    self.miss_ordinary_support(
                        id,
                        query,
                        environment,
                        fine_ticks.context("falling needs the step's fine-tick increment")?,
                        inputs.script.frame_scale,
                        sounds,
                    )?;
                }
            }
            self.retain_ordinary_stable_position(id)?;
        }
        Ok(support_found)
    }

    pub(super) fn resolve_ordinary_late_collision(
        &mut self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        inputs: OrdinaryLateInputs<'_>,
        rollback: &super::recovery::OrdinaryRollbackSnapshot,
        recovery: super::recovery::OrdinaryStableRecoveryInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let OrdinaryLateInputs {
            script,
            ticks,
            fine_ticks,
            step_fps,
            weapon,
        } = inputs;
        let environment = super::transition::ObstacleEnvironment {
            selected_player: weapon.map(|input| input.selected_player),
            auto_grab: false,
            item_system_ready: weapon.is_some_and(|input| input.item_system_ready),
            shell_execution: weapon.is_some_and(|input| input.shell_execution),
            ticks,
        };
        self.run_jump_window(
            id,
            query,
            environment,
            fine_ticks,
            script.frame_scale,
            sounds,
        )?;
        let obstacle_query = || super::obstacle::OrdinaryObstacleQuery {
            scene: query.scene,
            animations: query.animations,
            prepared: query.prepared,
            vertical_offset: 0,
            respond: true,
        };
        let mask = self.actors[id.index].attributes[1];
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("late collision control unavailable")?;
        let special = (10..=19).contains(&control.living_state);
        let falling = control.falling_state != 0;
        ensure!(
            !falling || !special,
            "world {} instance {}: falling in a special state is unsupported",
            self.world,
            id.index
        );
        if !special {
            self.resolve_ordinary_obstacle_wrapper(id, obstacle_query(), environment, sounds)?;
        }
        if mask & 8 != 0 {
            self.resolve_ordinary_overlap_stage(id, obstacle_query(), environment, sounds)?;
        }
        self.ground_response_stage(
            id,
            query,
            environment,
            OrdinaryLateInputs {
                script,
                ticks,
                fine_ticks,
                step_fps,
                weapon,
            },
            (mask, special, falling),
            &mut *recovery.random,
            sounds,
        )?;
        let control = self.actors[id.index]
            .control
            .as_ref()
            .expect("admitted body control");
        if mask & 1 != 0
            && (matches!(control.living_state, 0 | 15) || (27..=32).contains(&control.living_state))
        {
            // Ordinary entry clears the extra-contact append flag. These admitted
            // stages do not produce an appended normal; heightmap availability
            // is independent of that flag and still feeds the support queries.
            let sliding = control.sliding_state != 0;
            let contacts = self.query_ordinary_body(
                id,
                super::body::OrdinaryBodyQuery {
                    scene: query.scene,
                    animations: query.animations,
                    prepared: query.prepared,
                    sidedness: true,
                    excluded_candidate_type: sliding.then_some(64),
                    terrain: None,
                },
            )?;
            self.globals.primary_body_contacts = contacts.len() as u32;
            if self.selected_player_index() == Some(id.index) {
                for contact in &contacts {
                    if let cyrus_collision::ground::SupportSource::Actor { actor, .. } =
                        contact.source
                    {
                        self.globals.record_player_contact(actor);
                    }
                }
            }
            self.resolve_ordinary_primary_contacts(
                id,
                &contacts,
                &query.scene.settings,
                &mut |world| {
                    world.apply_ordinary_obstacle_transition(
                        id,
                        query.animations,
                        environment,
                        sounds,
                    )
                },
            )?;
            let contacts = self.query_ordinary_residual_body(
                id,
                super::body::OrdinaryResidualQuery {
                    scene: query.scene,
                    animations: query.animations,
                    prepared: query.prepared,
                    sidedness: true,
                },
            )?;
            self.resolve_ordinary_residual_contacts(id, &contacts, &query.scene.settings)?;
        }
        // A sliding actor's slide ends after a body response that found contacts; the retained
        // count is tested even when this actor's own response did not run.
        if self.globals.primary_body_contacts != 0 {
            let control = self.actors[id.index]
                .control
                .as_mut()
                .expect("admitted body control");
            control.sliding_state = 0;
        }
        self.actors[id.index].attributes[6] = self.actors[id.index].attributes[115];
        let control = self.actors[id.index]
            .control
            .as_ref()
            .expect("admitted recovery control");
        let moving = control.falling_state != 0 || control.sliding_state != 0;
        let recover = if self.actors[id.index].attributes[89] != 0 && moving {
            true
        } else if rollback.landing_recovery {
            moving
                || self
                    .surface_veto
                    .context("landing recovery shared surface veto owner unavailable")?
        } else {
            false
        };
        if recover && environment.selected_player == Some(id) {
            if let Some((attempted, falling_before)) =
                self.begin_selected_stable_recovery(id, &recovery, rollback, sounds)?
            {
                let found = self.ground_response_stage(
                    id,
                    query,
                    environment,
                    OrdinaryLateInputs {
                        script,
                        ticks,
                        fine_ticks,
                        step_fps,
                        weapon,
                    },
                    (mask, special, false),
                    &mut *recovery.random,
                    sounds,
                )?;
                if !found {
                    self.undo_selected_stable_recovery(id, attempted, falling_before)?;
                }
            }
        } else if recover {
            self.recover_ordinary_stable(id, recovery, rollback, sounds)?;
        }
        self.rollback_ordinary_water(id, environment.selected_player, rollback, sounds)?;
        Ok(())
    }
}
