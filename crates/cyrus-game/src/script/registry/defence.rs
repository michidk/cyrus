//! Receiver-owned defence picks and synchronous animation, sound and timed-taunt execution.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::decision::{DefenceView, defence_step_with_animation};
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Read the current motion's zero-based counter-attack reference.
    /// Missing combat or action -1/nine returns zero without resolving move metadata.
    /// Active actions use the retained defence attack slot and current motion, not
    /// the action selector as a table index, following preservation's open-pick query.
    pub fn current_combat_counter_attack(&self, id: ScriptActorId) -> Result<i32> {
        let result = (|| -> Result<i32> {
            let index = self.resolve(id)?;
            let Some(combat) = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
            else {
                return Ok(0);
            };
            if matches!(combat.action, -1 | 9) {
                return Ok(0);
            }
            let slot = usize::try_from(combat.retained_action_indices[1])
                .context("negative current attack slot")?;
            let motion = usize::try_from(combat.decision.move_selector)
                .context("negative current motion selector")?;
            let attack = self
                .combat_attack(id, slot)?
                .with_context(|| format!("attack slot {slot}: unpopulated"))?;
            let motion_data = attack
                .motions
                .get(motion)
                .with_context(|| format!("attack slot {slot} motion {motion}: unavailable"))?;
            let counter = motion_data.defense.with_context(|| {
                format!("attack slot {slot} motion {motion}: counter-attack reference unavailable")
            })?;
            Ok(counter.wrapping_sub(1))
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} current counter-attack",
                self.world, id.index
            )
        })
    }

    /// Pick best/open defence using live receiver slots. Best never reads counter
    /// references; open reads the current counter only after an admitted slot.
    pub fn pick_combat_defence(
        &self,
        id: ScriptActorId,
        target: ScriptActorId,
        distance: i32,
        open: bool,
    ) -> Result<Option<i32>> {
        self.resolve(id)?;
        self.resolve(target)?;
        crate::script::decision::pick_defence_with_queries(
            &mut LivePacedQueries {
                world: self,
                id,
                target,
            },
            distance,
            open,
        )
        .with_context(|| format!("world {} instance {} defence pick", self.world, id.index))
    }

    /// Pick from this receiver's attack slots, querying only fields used by each branch.
    /// The caller supplies the shared draw, distance and stance sample at invocation time.
    /// The receiver's live pacing and counter state are read here; counter writes survive
    /// later query failures. This does not start a defence or enable ordinary traversal.
    pub fn pick_combat_paced_defence(
        &mut self,
        id: ScriptActorId,
        target: ScriptActorId,
        input: crate::script::decision::PacedDefenceInput,
    ) -> Result<i32> {
        let index = self.resolve(id)?;
        let mut decision = self.actors[index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .with_context(|| format!("world {} instance {index}: combat unavailable", self.world))?
            .decision;
        let result = crate::script::decision::paced_defence_with_queries(
            &mut decision,
            &mut LivePacedQueries {
                world: self,
                id,
                target,
            },
            input,
        );
        self.actors[index]
            .control
            .as_mut()
            .expect("admitted control")
            .combat
            .as_mut()
            .expect("admitted combat")
            .decision = decision;
        result.with_context(|| format!("world {} instance {index} paced defence", self.world))
    }

    /// Run one caller-admitted defence selection against live receiver state.
    /// Inputs are sampled by the scheduler; starts execute synchronously with the shared
    /// random stream. Every service retains earlier decision, animation and sound writes
    /// on failure. This explicit operation does not enable automatic combat traversal.
    #[allow(clippy::too_many_arguments)] // One invocation shares scheduler and executor services.
    pub fn select_combat_defence(
        &mut self,
        id: ScriptActorId,
        target: ScriptActorId,
        selection: crate::script::decision::DefenceSelectionInput,
        weapon: super::WeaponInputs,
        frame_value: i32,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        movement: &mut dyn FnMut(&mut Self, ScriptActorId, i32, i32) -> Result<()>,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        self.resolve(target)?;
        let combat = self.actors[index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .context("defence selection: receiver combat unavailable")?;
        let mut decision = combat.decision;
        let mut flags = combat.flags;
        let mut owner = LiveDefenceSelection {
            world: self,
            id,
            target,
            selection,
            weapon,
            frame_value,
            inputs,
            random,
            movement,
            sounds,
        };
        let result = crate::script::decision::defence_selection(
            &mut decision,
            &mut flags,
            &selection,
            &mut owner,
        );
        owner.store(decision, flags);
        result.with_context(|| {
            format!(
                "world {} instance {} defence selection",
                owner.world.world, id.index
            )
        })
    }

    /// Start this receiver's authored defence synchronously, including live executor services.
    /// Caller-established weapon gates use a generation-checked selected-player identity.
    /// Selected-player kind four invokes movement with Q8 distance and mode one before
    /// selector writes. A movement-service error leaves selectors untouched; executor errors
    /// retain the committed selectors and any earlier animation/combat writes.
    /// Admission follows preservation's SOUP defence-start rules.
    #[allow(clippy::too_many_arguments)] // One synchronous receiver operation with caller-owned services.
    pub fn start_combat_defence(
        &mut self,
        id: ScriptActorId,
        kind: i32,
        weapon: super::WeaponInputs,
        frame_value: i32,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        movement: impl FnOnce(&mut Self, ScriptActorId, i32, i32) -> Result<()>,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let result = (|| -> Result<bool> {
            let index = self.resolve(id)?;
            self.resolve(weapon.selected_player)?;
            if !matches!(kind, 0..=6 | 8) {
                return Ok(false);
            }
            if self.combat_attack(id, kind as usize)?.is_none() {
                return Ok(false);
            }
            let actor = &self.actors[index];
            let control = actor
                .control
                .as_ref()
                .context("receiver control unavailable")?;
            let combat = control
                .combat
                .as_ref()
                .context("receiver combat unavailable")?;
            let (_, drawing, sheathing) = crate::script::activation::weapon_predicates(
                control.living_state,
                actor
                    .animation
                    .as_ref()
                    .map(|animation| crate::script::ActivationAnimation {
                        group: animation.playback.current_group(),
                    }),
                true,
                control.weapon_drawn,
                actor.attributes[110],
                weapon.item_system_ready,
                weapon.shell_execution,
            );
            let admission = crate::script::decision::DefenceStartInput {
                slot_present: true,
                selected_player: id == weapon.selected_player,
                cooldown: combat.cooldown,
                drawing,
                sheathing,
                attribute_16: actor.attributes[16],
            };
            let mut action = combat.action;
            let mut selector = combat.retained_action_indices[1];
            if !crate::script::decision::start_defence(
                kind,
                admission,
                &mut action,
                &mut selector,
                |distance, mode| movement(self, id, distance, mode),
                |_, _, _| Ok(()),
            )? {
                return Ok(false);
            }
            let combat = self.actors[index]
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat");
            combat.retained_action_indices[1] = selector;
            combat.action = action;
            self.execute_combat_defence(id, 0, frame_value, inputs, random, sounds)?;
            Ok(true)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} defence starter kind {kind}",
                self.world, id.index
            )
        })
    }

    /// Execute an already selected defence synchronously. The defence selector addresses this
    /// receiver's nine attack slots; missing authored motions retain zero group/type defaults.
    /// Animation and sounds run before combat start writes, followed by the kind-one taunt.
    /// A failed service retains earlier writes. This does not select or start ordinary combat.
    /// Service order follows [preservation](https://github.com/michidk/redguard-preservation/pull/115).
    pub fn execute_combat_defence(
        &mut self,
        id: ScriptActorId,
        requested: i32,
        frame_value: i32,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let result = (|| -> Result<bool> {
            let index = self.resolve(id)?;
            let combat = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("receiver combat unavailable")?;
            let selector = combat.retained_action_indices[1];
            if combat.decision.attack_state == 0 && requested == -1
                || combat.decision.attack_state != 0 && selector == -1
            {
                return Ok(false);
            }
            let slot = usize::try_from(selector).context("negative defence selector")?;
            let attack = self
                .combat_attack(id, slot)?
                .context("selected attack slot is unpopulated")?;
            let groups: [i32; 3] = std::array::from_fn(|motion| {
                attack
                    .motions
                    .get(motion)
                    .map_or(0, |motion| motion.animation_group)
            });
            let kinds: [i32; 3] = std::array::from_fn(|motion| {
                attack.motions.get(motion).map_or(0, |motion| motion.kind)
            });
            let ticks = inputs.nominal_ticks.context("nominal ticks unavailable")?;
            let position = Some(self.sight_position(index)?);
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .context("receiver animation unavailable")?;
            let combat = actor
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat");
            let view = DefenceView {
                current_group: animation.playback.current_group().map(i32::from),
                pending_group: animation.playback.pending_group().map(i32::from),
                defence_group: &|motion| {
                    usize::try_from(motion)
                        .ok()
                        .and_then(|motion| groups.get(motion).copied())
                },
                entry_type_one: &|motion| {
                    usize::try_from(motion)
                        .ok()
                        .and_then(|motion| motion.checked_add(1))
                        .and_then(|next| kinds.get(next))
                        == Some(&1)
                },
            };
            let step = defence_step_with_animation(
                &mut combat.decision,
                &mut combat.flags,
                &mut combat.action,
                &mut combat.retained_action_indices[1],
                requested,
                &view,
                &mut |group| {
                    let group =
                        u16::try_from(group).context("animation group outside u16 range")?;
                    let frame =
                        animation
                            .playback
                            .request_with_sounds(group, 0, ticks, &mut |sound| {
                                sounds(
                                    id,
                                    crate::script::ScriptSound::Animation {
                                        sound,
                                        surface,
                                        position,
                                    },
                                )
                            })?;
                    animation.frame = frame.or(animation.frame);
                    animation.playback.set_loop_count(1)
                },
            )?;
            if step.effect.taunt_kind_one {
                self.request_timed_combat_taunt(id, 1, frame_value, inputs, random, sounds)?;
            }
            Ok(step.active)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} defence executor",
                self.world, id.index
            )
        })
    }
}

struct LivePacedQueries<'a> {
    world: &'a ScriptWorld,
    id: ScriptActorId,
    target: ScriptActorId,
}

impl LivePacedQueries<'_> {
    fn motion(&self, slot: usize) -> Result<&cyrus_assets::script::combat::Motion> {
        self.world
            .combat_attack(self.id, slot)?
            .with_context(|| format!("attack slot {slot}: unpopulated"))?
            .motions
            .first()
            .with_context(|| format!("attack slot {slot}: first motion unavailable"))
    }
}

impl crate::script::decision::PacedDefenceQueries for LivePacedQueries<'_> {
    fn window(&mut self, slot: usize) -> Result<Option<(i32, i32)>> {
        let Some(attack) = self.world.combat_attack(self.id, slot)? else {
            return Ok(None);
        };
        let motion = attack
            .motions
            .first()
            .with_context(|| format!("attack slot {slot}: first motion unavailable"))?;
        Ok(Some((motion.min_range, motion.max_range)))
    }

    fn in_arc(&mut self, slot: usize) -> Result<bool> {
        let motion = self.motion(slot)?;
        let arc = motion
            .arc
            .with_context(|| format!("attack slot {slot}: arc unavailable"))?;
        let own = self
            .world
            .spatial(self.id)
            .with_context(|| format!("attack slot {slot}: receiver pose unavailable"))?
            .local;
        let other = self
            .world
            .spatial(self.target)
            .with_context(|| {
                format!(
                    "attack slot {slot} target instance {}: pose unavailable",
                    self.target.index
                )
            })?
            .local;
        Ok(crate::script::decision::defence_arc_from_positions(
            own.position,
            other.position,
            i32::from(own.angles[1]),
            arc,
            motion.arc_center.unwrap_or(0),
        ))
    }

    fn in_reach(&mut self, slot: usize) -> Result<bool> {
        use crate::script::decision::{combat_bounds_coordinate, defence_elevation};
        let elevation = self.motion(slot)?.elevation.unwrap_or(0);
        anyhow::ensure!(
            (0..=2).contains(&elevation),
            "attack slot {slot}: elevation {elevation} unsupported"
        );
        let own = self
            .world
            .spatial(self.id)
            .with_context(|| format!("attack slot {slot}: receiver pose unavailable"))?
            .local;
        let other = self
            .world
            .spatial(self.target)
            .with_context(|| {
                format!(
                    "attack slot {slot} target instance {}: pose unavailable",
                    self.target.index
                )
            })?
            .local;
        let own_shape = self
            .world
            .ordinary_collision_shape(self.id)
            .with_context(|| format!("attack slot {slot}: receiver bounds unavailable"))?;
        let target_shape = self
            .world
            .ordinary_collision_shape(self.target)
            .with_context(|| {
                format!(
                    "attack slot {slot} target instance {}: bounds unavailable",
                    self.target.index
                )
            })?;
        Ok(defence_elevation(
            elevation,
            combat_bounds_coordinate(
                own.position[1],
                own_shape.negative_extent[1],
                own_shape.positive_extent[1],
            ),
            combat_bounds_coordinate(
                other.position[1],
                target_shape.negative_extent[1],
                target_shape.positive_extent[1],
            ),
            own_shape.positive_extent[0],
            own_shape.negative_extent[0],
        ))
    }

    fn damage(&mut self, slot: usize) -> Result<i32> {
        self.world
            .combat_attack(self.id, slot)?
            .with_context(|| format!("attack slot {slot}: unpopulated"))?
            .motions
            .iter()
            .try_fold(0i32, |total, motion| {
                Ok(total.wrapping_add(motion.damage.with_context(|| {
                    format!(
                        "attack slot {slot} motion section {}: damage unavailable",
                        motion.section
                    )
                })?))
            })
    }

    fn near_timing(&mut self, slot: usize) -> Result<i32> {
        Ok(self.motion(slot)?.first_collide_frame)
    }
}

impl crate::script::decision::DefenceQueries for LivePacedQueries<'_> {
    fn window(&mut self, slot: usize) -> Result<Option<(i32, i32)>> {
        crate::script::decision::PacedDefenceQueries::window(self, slot)
    }
    fn near_timing(&mut self, slot: usize) -> Result<i32> {
        crate::script::decision::PacedDefenceQueries::near_timing(self, slot)
    }
    fn counter_attack(&mut self, slot: usize) -> Result<i32> {
        self.motion(slot)?
            .defense
            .with_context(|| format!("attack slot {slot}: counter-attack reference unavailable"))
    }
    fn current_counter_attack(&mut self) -> Result<i32> {
        self.world.current_combat_counter_attack(self.id)
    }
}

struct LiveDefenceSelection<'a, 'inputs> {
    world: &'a mut ScriptWorld,
    id: ScriptActorId,
    target: ScriptActorId,
    selection: crate::script::decision::DefenceSelectionInput,
    weapon: super::WeaponInputs,
    frame_value: i32,
    inputs: ScriptInputs<'inputs>,
    random: &'a mut crate::random::EngineRandom,
    movement: &'a mut dyn FnMut(&mut ScriptWorld, ScriptActorId, i32, i32) -> Result<()>,
    sounds: &'a mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
}

impl LiveDefenceSelection<'_, '_> {
    fn store(&mut self, decision: crate::script::combat::CombatDecision, flags: u8) {
        let combat = self.world.actors[self.id.index]
            .control
            .as_mut()
            .expect("admitted control")
            .combat
            .as_mut()
            .expect("admitted combat");
        combat.decision = decision;
        combat.flags = flags;
    }
    fn read(&self) -> (crate::script::combat::CombatDecision, u8) {
        let combat = self.world.actors[self.id.index]
            .control
            .as_ref()
            .expect("admitted control")
            .combat
            .as_ref()
            .expect("admitted combat");
        (combat.decision, combat.flags)
    }
}

impl crate::script::decision::DefenceSelectionServices for LiveDefenceSelection<'_, '_> {
    fn best(
        &mut self,
        decision: &mut crate::script::combat::CombatDecision,
        flags: u8,
    ) -> Result<Option<i32>> {
        self.store(*decision, flags);
        self.world
            .pick_combat_defence(self.id, self.target, self.selection.distance, false)
    }
    fn open(
        &mut self,
        decision: &mut crate::script::combat::CombatDecision,
        flags: u8,
    ) -> Result<Option<i32>> {
        self.store(*decision, flags);
        self.world
            .pick_combat_defence(self.id, self.target, self.selection.distance, true)
    }
    fn paced(
        &mut self,
        decision: &mut crate::script::combat::CombatDecision,
        flags: u8,
    ) -> Result<Option<i32>> {
        self.store(*decision, flags);
        let result = self.world.pick_combat_paced_defence(
            self.id,
            self.target,
            crate::script::decision::PacedDefenceInput {
                distance: self.selection.distance,
                random_draw: self.random.draw() as u16,
                pacing_sum: self.selection.pacing_sum,
                facing_alike: self.selection.facing_alike,
                target_in_combat_stance: self.selection.target_in_combat_stance,
                attribute_58: self.selection.attribute_58,
            },
        );
        *decision = self.read().0;
        result.map(Some)
    }
    fn in_reach(&mut self, kind: i32) -> Result<bool> {
        let slot = usize::try_from(kind).context("negative defence kind")?;
        crate::script::decision::PacedDefenceQueries::in_reach(
            &mut LivePacedQueries {
                world: self.world,
                id: self.id,
                target: self.target,
            },
            slot,
        )
    }
    fn start(
        &mut self,
        decision: &mut crate::script::combat::CombatDecision,
        flags: &mut u8,
        kind: i32,
    ) -> Result<()> {
        self.store(*decision, *flags);
        let result = self.world.start_combat_defence(
            self.id,
            kind,
            self.weapon,
            self.frame_value,
            self.inputs,
            self.random,
            &mut *self.movement,
            self.sounds,
        );
        (*decision, *flags) = self.read();
        result.map(|_| ())
    }
}
