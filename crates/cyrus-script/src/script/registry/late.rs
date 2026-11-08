//! Automatic late collision composes retained candidates and logical actor response.
use super::{ScriptActorId, ScriptWorld};
use crate::script::CombatFlags;
use crate::script::JumpMotion;
use crate::script::LivingState;
use crate::script::MotionDirection;
use crate::script::registry::ActionWords;
use anyhow::{Context, Result, ensure};
use cyrus_assets::surface::Surface;
use cyrus_collision::ground::SelectedSupport;
use cyrus_collision::ground::SurfaceClass;
use cyrus_soup::{AnimationGroup, Attribute};
use cyrus_world::heading::wrap;

/// Prevalidation, selected support and the surface updater's (previous, returned) surfaces when it ran.
type PointSupport = (
    super::super::OrdinaryGroundPrevalidation,
    Option<SelectedSupport>,
    Option<(Surface, Surface)>,
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
    /// surface for an update that stores it or (falling, sliding, the actor-bounce living states) returns
    /// it without storing.
    fn assign_ordinary_support_surface(
        &mut self,
        id: ScriptActorId,
        scene: &crate::player::data::PlayerScene,
        support: Option<&SelectedSupport>,
        selected: bool,
    ) -> Result<Surface> {
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("support surface control unavailable")?;
        if control.queued_motion != JumpMotion::NONE
            || control
                .actions
                .iter()
                .any(|flags| flags & ActionWords::JUMPS != 0)
        {
            return Ok(Surface::UNKNOWN);
        }
        let keep_stored = control.falling_state != JumpMotion::NONE
            || control.sliding_state != 0
            || control.living_state.is_actor_bounce();
        if keep_stored && !selected {
            // Only the selected player's transition handler reads the returned value.
            return Ok(self.actors[id.index].surface.unwrap_or_default());
        }
        let surface = support
            .map(|support| scene.support_surface(support, self.query_position(id)?.map(i64::from)))
            .transpose()?
            .unwrap_or_default();
        if !keep_stored {
            self.set_actor_surface(id, surface)?;
            self.surface_veto = Some(matches!(surface, Surface::LAVA | Surface::GLOOP));
        }
        Ok(surface)
    }

    /// Run the transitions the selected player's probes queued, oldest first. The caller owns the
    /// random stream and sound sink and calls this right after the probe.
    pub fn flush_surface_transitions(
        &mut self,
        id: ScriptActorId,
        ticks: u32,
        random: &mut cyrus_game::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        for pending in std::mem::take(&mut self.pending_surface_transitions) {
            self.selected_surface_transition(
                id,
                pending.previous,
                pending.returned,
                ticks,
                random,
                pending.effect_height,
                sounds,
            )?;
        }
        Ok(())
    }

    /// The selected player's water and lava transition effects (preservation player.md): the
    /// swim entry and walk exit group requests, the splash effect object (identifiers 0x2500 water,
    /// 0x2580 lava) and the splash sound. `effect_height` is the effect's Q8 Y from the support
    /// query (`None` without support); a host without effect or camera services creates no effect.
    #[allow(clippy::too_many_arguments)] // the handler's inputs: surfaces, clock, random, support, sink
    pub fn selected_surface_transition(
        &mut self,
        id: ScriptActorId,
        previous: Surface,
        returned: Surface,
        ticks: u32,
        random: &mut cyrus_game::random::EngineRandom,
        effect_height: Option<i32>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        if returned == previous {
            return Ok(());
        }
        let stored = self.actors[id.index].surface.unwrap_or_default();
        let in_water = |surface: Surface| surface.is_water();
        let deep = |surface: Surface| surface.is_deep_water();
        if !in_water(stored) && !in_water(previous) {
            if stored == Surface::LAVA || previous == Surface::LAVA {
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
        let forward = control.motion_direction == MotionDirection::FORWARD;
        let torch = control.torch_burning != 0;
        if deep(stored) {
            self.request_ordinary_group(
                id,
                if forward {
                    AnimationGroup::SWIM_FORWARD
                } else {
                    AnimationGroup::SWIM_BACKWARD
                },
                false,
                ticks,
                sounds,
            )?;
        } else if deep(previous) {
            let group = match (forward, torch) {
                (true, false) => AnimationGroup::RUN,
                (true, true) => AnimationGroup::RUN_TORCH,
                (false, false) => AnimationGroup::RUN_BACKWARD,
                (false, true) => AnimationGroup::WALK_BACKWARD_TORCH,
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
        let heading = wrap(1024 + i32::from(bearing)) as u16;
        let [x, _, z] = cyrus_world::transform::translate_forward(position, heading, 0x800)?;
        self.spawn_environment_effect(descriptor, [x, height, z])
    }

    /// Q8 Y of a transition effect for the selected support: the support height plus standing
    /// height, plus six units (a fallback support keeps its actor origin, so its extent is added).
    pub(super) fn transition_effect_height(
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
        Ok(Some((origin + 0x600) as i32))
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
        let previous = self.actors[id.index].surface.unwrap_or_default();
        let support = self.query_ordinary_ground(id, query)?;
        let updated = if self.actors[id.index].attributes[Attribute::Collide] & 4 != 0 {
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
    /// Attached actors and selected-player feedback remain unavailable.
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
            spatial.parent.is_none(),
            "world {} instance {index}: attached destination collision unavailable",
            self.world,
        );
        // A linked actor's probe reads its link position, so the temporary local destination
        // changes nothing it queries, as in the original.
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
        if self.actors[index].attributes[Attribute::Collide] & 2 != 0
            && matches!(support, SelectedSupport::Primary { support, .. } if support.classification() == SurfaceClass::Steep)
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
            .is_some_and(|combat| combat.flags.contains(CombatFlags::STANCE));
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
                    actor.attributes[Attribute::HoldingWeapon],
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
        frame_scale: Option<u32>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let actor = &mut self.actors[id.index];
        let control = actor
            .control
            .as_mut()
            .context("fall recovery control unavailable")?;
        for byte in control.actions.iter_mut() {
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
        if environment.selected_player == Some(id) {
            self.begin_player_jump(
                id,
                query,
                environment,
                super::ledge_states::LedgeTiming {
                    fine_ticks,
                    frame_scale,
                },
                8,
                sounds,
            )?;
        } else {
            self.begin_ordinary_jump(
                id,
                query.animations,
                8,
                task_active,
                environment,
                None,
                sounds,
            )?;
        }
        if self.updates.animation_system {
            self.request_ordinary_group(
                id,
                if torch {
                    AnimationGroup::JUMP_RIGHT_START_TORCH
                } else {
                    AnimationGroup::JUMP_RUN_START
                },
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
            .falling_state = JumpMotion::NONE;
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
        if control.falling_state != JumpMotion::NONE {
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
                    None,
                    sounds,
                )?;
            }
            return Ok(());
        }
        if control
            .actions
            .iter()
            .any(|byte| byte & ActionWords::JUMPS != 0)
        {
            return self.recover_ordinary_fall_entry(
                id,
                query,
                environment,
                fine_ticks,
                frame_scale,
                sounds,
            );
        }
        let group = self.actors[id.index]
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        // The jump groups' last displacement: the retained direction at Q8 distance 4096.
        if group.is_some_and(|group| {
            (AnimationGroup::JUMP_FORWARD_TORCH..=AnimationGroup::JUMP_FORWARD_ARMS)
                .contains(&group)
        }) {
            let direction = self.actors[id.index]
                .control
                .as_ref()
                .context("fall entry control unavailable")?
                .motion_direction;
            self.apply_ordinary_directional_motion(id, direction.byte(), 4096)?;
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
    /// on, plays the hurt animation (`anim_land_hurt`) and loses the installed zap of health. Sliding
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
            &mut cyrus_game::random::EngineRandom,
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
                if matches!(
                    group,
                    Some(AnimationGroup::SLIDE | AnimationGroup::SLIDE_FORWARD_TORCH)
                ) {
                    2
                } else {
                    3
                },
                0x800,
            )?;
            if is_selected {
                self.globals.set_player_death_override_group(74)?;
            } else if self.updates.animation_system {
                self.request_ordinary_group(
                    id,
                    AnimationGroup::LAND_HURT,
                    false,
                    environment.ticks,
                    sounds,
                )?;
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
            actor.attributes[Attribute::HoldingWeapon],
            environment.item_system_ready || weapon.is_some_and(|input| input.item_system_ready),
            environment.shell_execution,
        );
        let combat_active = control
            .combat
            .as_ref()
            .is_some_and(|combat| combat.flags.contains(CombatFlags::STANCE));
        Ok((holding, combat_active))
    }

    /// The `yeahbaby` ground bypass of the selected player's probe: while Page Up or Page Down is
    /// held or the Home toggle is set, the actor's living state, sliding, queued-motion and
    /// falling bytes and its motion classification clear and the probe ends before any support
    /// query. The original also clears the actor's rope node reference; the probe does not run
    /// for an actor attached to a rope here, so there is no reference to clear. Unknown keys
    /// refuse the update instead of reading as released. Returns whether the bypass was taken.
    fn ground_bypass_taken(
        &mut self,
        id: ScriptActorId,
        script: super::ScriptInputs<'_>,
    ) -> Result<bool> {
        let Some(keys) = self.ground_bypass_keys(script)? else {
            return Ok(false);
        };
        if !(keys.page_up || keys.page_down || self.ground_bypass_toggle) {
            return Ok(false);
        }
        let control = self.actors[id.index]
            .control
            .as_mut()
            .expect("admitted ground control");
        ensure!(
            !control.rope_attached,
            "yeahbaby ground bypass while attached to a rope is unsupported"
        );
        control.living_state = LivingState::ORDINARY;
        control.sliding_state = 0;
        control.queued_motion = JumpMotion::NONE;
        control.falling_state = JumpMotion::NONE;
        control.motion_classification = MotionDirection::NONE;
        self.actors[id.index].sync_falling_motion();
        Ok(true)
    }

    /// The keys the ground bypass reads, when it is active: the original enables the handler and
    /// the probe test for a nonzero `yeahbaby` or when `[debug] final_version` is clear
    /// (developer mode). Unknown key state refuses the update under the cheat; developer mode
    /// without key state leaves the bypass inactive instead of guessing.
    fn ground_bypass_keys(
        &self,
        script: super::ScriptInputs<'_>,
    ) -> Result<Option<super::super::ScriptKeys>> {
        if script.ground_bypass == Some(true) {
            return script
                .script_keys
                .map(Some)
                .context("yeahbaby ground bypass needs the debug navigation keys");
        }
        Ok(if self.updates.final_version {
            None
        } else {
            script.script_keys
        })
    }

    /// The key part of the selected player's per-update handler. While `yeahbaby` is on or in
    /// developer mode: Page Up moves the actor's local Y up by 0x1000 (Q8, 16 map units) and Page
    /// Down down by the same amount, each held key once per update and in that order, so both held
    /// cancel; a Home press flips the ground bypass state, which persists also while the handler
    /// is off; Insert held suspends the camera's movement limiting (bit 0 of the selected camera
    /// record's flags, set on the first held update) and moves the actor forward along its heading
    /// by the frame scale times 128 (Q8) each update. Insert not held, or the handler off, restores
    /// the limiting once, on the record selected at that moment. Unknown keys refuse the update
    /// under the cheat; in developer mode they leave the state unchanged.
    fn run_yeahbaby_keys(
        &mut self,
        id: ScriptActorId,
        script: super::ScriptInputs<'_>,
        selected_player: Option<ScriptActorId>,
    ) -> Result<()> {
        if selected_player != Some(id) {
            return Ok(());
        }
        let cheat = script.ground_bypass == Some(true);
        let keys = if cheat || !self.updates.final_version {
            match script.script_keys {
                Some(keys) => Some(keys),
                None if cheat => {
                    anyhow::bail!("yeahbaby ground bypass needs the debug navigation keys")
                }
                None => return Ok(()),
            }
        } else {
            None
        };
        if let Some(keys) = keys {
            // Both held cancel: the two sequential wrapping steps sum to zero.
            let delta = (i32::from(keys.page_down) - i32::from(keys.page_up)) * 0x1000;
            if delta != 0 {
                let spatial = self
                    .spatial
                    .as_mut()
                    .context("yeahbaby vertical keys need the prepared spatial state")?
                    .actor_mut(id.index)?;
                spatial.local.position[1] = spatial.local.position[1].wrapping_add(delta);
                self.map_updates.register(id.index)?;
            }
            if keys.home_pressed {
                self.ground_bypass_toggle = !self.ground_bypass_toggle;
            }
        }
        if keys.is_some_and(|keys| keys.insert) {
            if !self.insert_camera_flag {
                let camera = self
                    .camera
                    .as_mut()
                    .context("yeahbaby Insert needs the camera session")?;
                let selected = camera.selected.index();
                camera.records[selected]
                    .flags
                    .insert(crate::script::camera::CameraFlags::UNLIMITED_MOVEMENT);
                self.insert_camera_flag = true;
            }
            let frame_scale = script
                .frame_scale
                .context("yeahbaby Insert step requires frame_scale")?;
            let distance = (frame_scale as i32).wrapping_shl(15) >> 8;
            let spatial = self
                .spatial
                .as_mut()
                .context("yeahbaby Insert step needs the prepared spatial state")?
                .actor_mut(id.index)?;
            spatial.local.position = cyrus_world::transform::translate_forward(
                spatial.local.position,
                spatial.local.wrapped_angles()[1],
                distance,
            )?;
            self.map_updates.register(id.index)?;
        } else if self.insert_camera_flag {
            self.insert_camera_flag = false;
            if let Some(camera) = self.camera.as_mut() {
                let selected = camera.selected.index();
                camera.records[selected]
                    .flags
                    .remove(crate::script::camera::CameraFlags::UNLIMITED_MOVEMENT);
            }
        }
        Ok(())
    }

    /// End the probe at a handled bounce or queued-motion answer. The probe's callers ignore its
    /// answer and retain the stable position afterwards whenever the queued, falling and sliding
    /// bytes are zero, so an early exit retains it too. The answer is reported as found support;
    /// how the selected player's stable recovery treats it is not recovered.
    fn end_probe_early(&mut self, id: ScriptActorId) -> Result<bool> {
        self.retain_ordinary_stable_position(id)?;
        Ok(true)
    }

    /// The part of the original's bounce handler that precedes the support query and that the
    /// published rules settle (preservation player.md, actor bounce). `BOUNCE_ENTRY` and
    /// `BOUNCE_RISING` belong to `BounceObject`: the handler reports handled and the probe ends
    /// before the support query. An actor in one of the actor-bounce states whose support
    /// reference is empty, has no animation or `MISC1` group, or has no bounce power loses the
    /// reference and its living state. A valid support is left to `run_actor_bounce`. The support
    /// reference is the retained support actor, which must persist between updates for these
    /// states to last. Returns whether the probe ends.
    pub(super) fn bounce_handler_prefix(&mut self, id: ScriptActorId) -> Result<bool> {
        let living = self.actors[id.index]
            .control
            .as_ref()
            .context("bounce handler control unavailable")?
            .living_state;
        if matches!(
            living,
            LivingState::BOUNCE_ENTRY | LivingState::BOUNCE_RISING
        ) {
            return Ok(true);
        }
        if !living.is_actor_bounce() {
            return Ok(false);
        }
        let valid = self.actors[id.index]
            .model
            .support
            .actor
            .and_then(|support| self.resolve(support).ok())
            .is_some_and(|support| {
                let support = &self.actors[support];
                support.attributes[Attribute::BouncePower] != 0
                    && support.animation.as_ref().is_some_and(|animation| {
                        animation.playback.has_group(AnimationGroup::MISC1)
                    })
            });
        if !valid {
            self.actors[id.index].model.support.actor = None;
            self.actors[id.index]
                .control
                .as_mut()
                .expect("checked above")
                .living_state = LivingState::ORDINARY;
        }
        Ok(false)
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
        random: &mut cyrus_game::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let OrdinaryLateInputs {
            script,
            ticks,
            fine_ticks,
            step_fps,
            weapon,
        } = inputs;
        // The probe opens with the terrain sink check, then the bounce handler and the queued
        // motion; any fall they start shortens the support probe below like any other fall.
        self.recover_terrain_sink(id, query, environment, sounds)?;
        if mask & 4 != 0 && !special {
            if self.bounce_handler_prefix(id)? {
                return self.end_probe_early(id);
            }
            let bouncing = self.actors[id.index]
                .control
                .as_ref()
                .is_some_and(|control| control.living_state.is_actor_bounce());
            if bouncing
                && self.run_actor_bounce(id, script.frame_scale, ticks, environment, sounds)?
            {
                return self.end_probe_early(id);
            }
            // Queued motion resolves before the support query, and a handled resolution ends
            // the probe without it.
            let control = self.actors[id.index]
                .control
                .as_ref()
                .expect("admitted ground control");
            // The bounce states (27 to 32) run the bounce handler in place of this resolution.
            if control.queued_motion != JumpMotion::NONE
                && !control.living_state.is_bounce()
                && !matches!(
                    control.living_state,
                    LivingState::DRAWING_SWORD | LivingState::SHEATHING_SWORD
                )
            {
                let handled = self.resolve_ordinary_queued_motion(
                    id,
                    query,
                    environment,
                    fine_ticks.context("queued motion needs the step's fine-tick increment")?,
                    0,
                    sounds,
                )?;
                if handled {
                    return self.end_probe_early(id);
                }
            }
        }
        if mask & 4 != 0 && environment.selected_player == Some(id) {
            if self.ground_bypass_taken(id, script)? {
                return self.end_probe_early(id);
            }
            if script.magic_carpet == Some(true) {
                // The magiccarpet exit clears the falling byte only.
                self.actors[id.index]
                    .control
                    .as_mut()
                    .expect("admitted ground control")
                    .falling_state = JumpMotion::NONE;
                self.actors[id.index].sync_falling_motion();
                return self.end_probe_early(id);
            }
        }
        let falling = falling
            || self.actors[id.index]
                .control
                .as_ref()
                .is_some_and(|control| control.falling_state != JumpMotion::NONE);
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
        if mask & 4 != 0 {
            let living_state = self.actors[id.index]
                .control
                .as_ref()
                .expect("admitted ground control")
                .living_state;
            if matches!(
                living_state,
                LivingState::DRAWING_SWORD | LivingState::SHEATHING_SWORD
            ) {
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
            } else {
                let actor = &self.actors[id.index];
                let control = actor.control.as_ref().expect("admitted late control");
                ensure!(
                    !control.living_state.is_actor_bounce(),
                    "state-specific ground response owner unavailable"
                );
                if let Some(support) = support.as_ref() {
                    if falling {
                        let surface = query
                            .scene
                            .support_surface(support, self.query_position(id)?.map(i64::from))?;
                        self.set_actor_surface(id, surface)?;
                        let selected = (environment.selected_player == Some(id)).then(|| {
                            super::falling::SelectedLanding {
                                inputs: script,
                                random: &mut *random,
                                steep: crate::player::support::classification(support, mask)
                                    == SurfaceClass::Steep,
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
                    let height =
                        support.actor_height(extent, query.scene.settings.standing_height) as i32;
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
                        control.falling_state = JumpMotion::NONE;
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
                } else if self.actors[id.index].attributes[Attribute::FallHold] != 0 {
                    self.actors[id.index]
                        .control
                        .as_mut()
                        .expect("admitted ground control")
                        .falling_state = JumpMotion::NONE;
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
        self.run_yeahbaby_keys(id, script, environment.selected_player)?;
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
        let mask = self.actors[id.index].attributes[Attribute::Collide];
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("late collision control unavailable")?;
        let special = control.living_state.is_ledge();
        let falling = control.falling_state != JumpMotion::NONE;
        ensure!(
            !falling || !special,
            "world {} instance {}: falling in a special state is unsupported",
            self.world,
            id.index
        );
        // The moonraker cheat takes the selected player out of both collision passes.
        let uncollided = skips_collision(environment.selected_player, id, script);
        if !special && !uncollided {
            self.resolve_ordinary_obstacle_wrapper(id, obstacle_query(), environment, sounds)?;
        }
        if mask & 8 != 0 && !uncollided {
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
            && (matches!(
                control.living_state,
                LivingState::ORDINARY | LivingState::HANGING
            ) || control.living_state.is_bounce())
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
        self.actors[id.index].attributes[Attribute::Weight] =
            self.actors[id.index].attributes[Attribute::ReservedWeight];
        let control = self.actors[id.index]
            .control
            .as_ref()
            .expect("admitted recovery control");
        let moving = control.falling_state != JumpMotion::NONE || control.sliding_state != 0;
        let recover = if self.actors[id.index].attributes[Attribute::StickToSurface] != 0 && moving
        {
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

/// Whether the moonraker cheat takes `id` out of the late collision passes: only the selected player
/// is exempt, and an unknown cheat state leaves the passes on.
fn skips_collision(
    selected_player: Option<ScriptActorId>,
    id: ScriptActorId,
    script: super::ScriptInputs<'_>,
) -> bool {
    selected_player == Some(id) && script.player_collision_skipped == Some(true)
}

#[cfg(test)]
mod moonraker_tests {
    use super::*;

    #[test]
    fn only_the_selected_player_under_a_known_active_cheat_skips_collision() {
        let player = ScriptActorId {
            generation: 1,
            index: 0,
        };
        let other = ScriptActorId {
            generation: 1,
            index: 1,
        };
        for (selected, id, state, expected) in [
            (Some(player), player, Some(true), true),
            (Some(player), player, Some(false), false),
            (Some(player), player, None, false),
            (Some(player), other, Some(true), false),
            (None, player, Some(true), false),
        ] {
            let inputs = super::super::ScriptInputs {
                player_collision_skipped: state,
                ..Default::default()
            };
            assert_eq!(skips_collision(selected, id, inputs), expected);
        }
    }
}
