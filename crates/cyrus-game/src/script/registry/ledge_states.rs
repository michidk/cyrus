//! The selected player's ledge transition (living states 10 through 19), the hang release and the
//! climb request, from preservation's player.md. Entry through the ground-level classes 1 and 2
//! (states 12 and 10) stays unsupported: only falls and queued motion, which produce class 3
//! records, reach the ledge here.
use super::{ScriptActorId, ScriptWorld, ground::OrdinaryGroundQuery, ledge::LedgeScan};
use anyhow::{Context, Result, bail};

/// Per-update clock inputs of the ledge services: the fine-tick increment that sets the reach
/// of the scan and the frame scale that sets the turn rate of a ledge turn.
#[derive(Clone, Copy, Debug)]
pub(super) struct LedgeTiming {
    pub fine_ticks: u32,
    pub frame_scale: Option<u32>,
}

/// Climb tables in map units, indexed by the animation's `real_frame` without a bound check in
/// the original; frames beyond a table are an explicit failure here.
const LOW_HEIGHT: [i32; 22] = [
    35, 35, 35, 35, 38, 29, 15, 0, -7, -21, -42, -56, -58, -60, -59, -61, -61, -61, -61, -61, -61,
    -61,
];
const LOW_FORWARD: [i32; 22] = [
    28, 28, 27, 27, 28, 24, 21, 16, 8, 7, 6, 5, 4, 4, 4, 5, 4, 3, 2, 2, 2, 2,
];
const HIGH_HEIGHT: [i32; 26] = [
    81, 67, 45, 24, 11, -1, -19, -28, -29, -29, -29, -29, -45, -53, -58, -60, -61, -61, -60, -60,
    -60, -60, -60, -60, -60, -60,
];
const HIGH_FORWARD: [i32; 26] = [
    30, 30, 30, 26, 20, 16, 8, 3, -6, -6, -4, 0, -6, -3, 1, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
];

impl ScriptWorld {
    /// Run the ledge transition once; returns its result (1 while a ledge action holds the actor).
    /// Anything but the selected player in states 10 through 19 returns zero without changes.
    pub(super) fn ledge_transition(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        timing: LedgeTiming,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<u8> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ledge transition control unavailable")?;
        if environment.selected_player != Some(id)
            || actor.attributes[105] == 1
            || !(10..=19).contains(&control.living_state)
        {
            return Ok(0);
        }
        // The hang-model reference the original clears and sets has no reader, so it is not kept.
        if control.torch_burning != 0 || self.ledge_blocked(index, environment) {
            self.ledge_hang_release(id, query, environment, timing, sounds)?;
            return Ok(0);
        }
        let mut ctx = Transition {
            id,
            index,
            query,
            environment,
            timing,
            table: None,
        };
        if self.living(index) == 15 {
            return self.ledge_hanging(&mut ctx, sounds);
        }
        if self.living(index) == 19 && !self.ledge_enter(&mut ctx)? {
            return Ok(0);
        }
        if self.living(index) == 12
            && let Some(result) = self.ledge_turn_low(&ctx, sounds)?
        {
            return Ok(result);
        }
        if self.living(index) == 10 {
            // The high-ledge start needs a group started partway through its frames.
            bail!(
                "world {} instance {index}: ledge turn state 10 unsupported",
                self.world
            );
        }
        if self.living(index) == 14 {
            return self.ledge_catch(&mut ctx, sounds);
        }
        match self.living(index) {
            13 => self.ledge_climb(&ctx, 90, &LOW_FORWARD, &LOW_HEIGHT, 6),
            16 => {
                self.force_ledge_group(&ctx, 86, sounds)?;
                self.set_living(index, 11);
                self.actors[index]
                    .animation
                    .as_mut()
                    .context("ledge climb animation unavailable")?
                    .playback
                    .reset_frame_progress();
                self.ledge_climb(&ctx, 86, &HIGH_FORWARD, &HIGH_HEIGHT, -1)
            }
            11 => self.ledge_climb(&ctx, 86, &HIGH_FORWARD, &HIGH_HEIGHT, -1),
            17 => {
                self.force_ledge_group(&ctx, 56, sounds)?;
                let control = self.actors[index]
                    .control
                    .as_mut()
                    .expect("validated ledge control");
                control.action_transition_counter = 6;
                control.living_state = 0;
                self.actors[index].model.ledge_support = None;
                Ok(0)
            }
            _ => Ok(0),
        }
    }

    /// For the selected player hanging with a zero action inhibition, drop: state 17 without a
    /// support, then the transition. A nonzero state afterwards clears sliding, falling and
    /// queued motion.
    pub(super) fn ledge_hang_release(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryGroundQuery<'_>,
        environment: super::transition::ObstacleEnvironment,
        timing: LedgeTiming,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<u8> {
        let index = self.resolve(id)?;
        if environment.selected_player != Some(id) || self.living(index) != 15 {
            return Ok(0);
        }
        let inhibition = self
            .globals
            .selected_action_inhibition
            .context("ledge hang release inhibition unavailable")?;
        if inhibition != 0 {
            return Ok(0);
        }
        self.set_living(index, 17);
        self.actors[index].model.ledge_support = None;
        let result = self.ledge_transition(id, query, environment, timing, sounds)?;
        if self.living(index) != 0 {
            self.clear_ledge_motion(index);
        }
        Ok(result)
    }

    /// The climb request: a class 3 record with room above (record byte 2) starts the climb.
    fn ledge_climb_request(
        &mut self,
        ctx: &Transition<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<u8> {
        let Transition {
            id,
            index,
            query,
            environment,
            timing,
            ..
        } = *ctx;
        if environment.selected_player != Some(id)
            || self.actors[index].attributes[105] == 1
            || self.living(index) != 15
        {
            return Ok(0);
        }
        let scan = self.ledge_scan(id, query, environment, timing.fine_ticks, 15)?;
        let local_y = self.spatial(id)?.local.position[1];
        let Some(record) = scan
            .hand_nearest(local_y)
            .filter(|record| record.clear != 0)
        else {
            return Ok(1);
        };
        let support = record.support;
        self.actors[index].model.ledge_support = self.ledge_support_id(support)?;
        self.set_living(index, 16);
        let result = self.ledge_transition(id, query, environment, timing, sounds)?;
        if self.living(index) != 0 {
            self.clear_ledge_motion(index);
        }
        Ok(result)
    }

    /// State 19: the scan selects the record that starts the ledge action. A scan without a
    /// usable class 3 record ends the attempt in state zero.
    fn ledge_enter(&mut self, ctx: &mut Transition<'_>) -> Result<bool> {
        let Transition {
            id,
            index,
            query,
            environment,
            timing,
            ..
        } = *ctx;
        let scan = self.ledge_scan(id, query, environment, timing.fine_ticks, 19)?;
        let local_y = self.spatial(id)?.local.position[1];
        let (record, state) = match scan.result {
            1 | 2 => (
                scan.first(scan.result),
                if scan.result == 1 { 12 } else { 10 },
            ),
            _ => (scan.hand_nearest(local_y), 14),
        };
        let Some(record) = record.cloned() else {
            self.set_living(index, 0);
            return Ok(false);
        };
        self.set_living(index, state);
        self.globals.selected_ledge_anchor = Some((record.point, record.heading));
        self.actors[index].model.ledge_support = self.ledge_support_id(record.support)?;
        ctx.table = Some(scan);
        Ok(true)
    }

    /// State 12: turn to the low ledge, then rescan and start the climb in state 13. Returns the
    /// transition's result while turning or when the ledge is gone; `None` continues into the
    /// climb state.
    fn ledge_turn_low(
        &mut self,
        ctx: &Transition<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<Option<u8>> {
        let (id, index) = (ctx.id, ctx.index);
        let (point, heading) = self.ledge_anchor()?;
        let frame_scale = ctx
            .timing
            .frame_scale
            .context("ledge turn needs the step's frame scale")?;
        let turn = self.turn_ordinary_toward(
            id,
            heading,
            super::slope::OrdinarySlopeInputs {
                selected_player: ctx.environment.selected_player,
                holding_weapon: false,
                combat_active: false,
                frame_scale,
                ticks: ctx.environment.ticks,
            },
            sounds,
        )?;
        if turn != 0 {
            if turn.abs() <= 256 {
                return Ok(Some(1));
            }
            self.set_living(index, 0);
            return Ok(Some(0));
        }
        let scan = self.ledge_scan(id, ctx.query, ctx.environment, ctx.timing.fine_ticks, 12)?;
        if scan.result != 1 {
            self.set_living(index, 0);
            return Ok(Some(0));
        }
        let [dx, dz] =
            super::ledge::heading_offset(LOW_FORWARD[0] * 256, heading.wrapping_add(1024));
        self.write_ledge_pose(
            index,
            Some(point[0].wrapping_add(dx)),
            None,
            Some(point[2].wrapping_add(dz)),
        )?;
        self.force_ledge_group(ctx, 90, sounds)?;
        self.set_living(index, 13);
        Ok(None)
    }

    /// State 14: the hang is caught when the actor still faces the ledge.
    fn ledge_catch(
        &mut self,
        ctx: &mut Transition<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<u8> {
        let (index, id) = (ctx.index, ctx.id);
        let (point, heading) = self.ledge_anchor()?;
        let own = self.spatial(id)?.local;
        if super::super::decision::heading_difference(i32::from(own.angles[1]), i32::from(heading))
            .abs()
            > 256
        {
            self.set_living(index, 0);
            return Ok(0);
        }
        let record = ctx
            .table
            .as_ref()
            .and_then(|table| table.hand_nearest(own.position[1]))
            .cloned();
        let Some(record) = record else {
            self.set_living(index, 0);
            return Ok(0);
        };
        let [dx, dz] =
            super::ledge::heading_offset(HIGH_FORWARD[0] * 256, heading.wrapping_add(1024));
        self.write_ledge_pose(
            index,
            Some(point[0].wrapping_add(dx)),
            None,
            Some(point[2].wrapping_add(dz)),
        )?;
        self.force_ledge_group(ctx, if record.flag == 0 { 88 } else { 89 }, sounds)?;
        self.set_living(index, 15);
        self.spatial
            .as_mut()
            .context("ledge spatial state unavailable")?
            .actor_mut(index)?
            .local
            .angles[1] = record.heading;
        self.actors[index].model.ledge_support = self.ledge_support_id(record.support)?;
        Ok(1)
    }

    /// State 15: follow the nearest class 3 record, shimmy along it, then hang or climb.
    fn ledge_hanging(
        &mut self,
        ctx: &mut Transition<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<u8> {
        let Transition {
            id,
            index,
            query,
            environment,
            timing,
            ..
        } = *ctx;
        let scan = self.ledge_scan(id, query, environment, timing.fine_ticks, 15)?;
        let local_y = self.spatial(id)?.local.position[1];
        let Some(record) = scan.hand_nearest(local_y).cloned() else {
            self.ledge_hang_release(id, query, environment, timing, sounds)?;
            return Ok(0);
        };
        let mut point = record.point;
        // A sidestep this update moves the ledge point a table step along the wall.
        if self.globals.locomotion_called & 3 != 0 {
            let [dx, dz] = super::ledge::heading_offset(256, 1792);
            point[0] = point[0].wrapping_add(dx);
            point[2] = point[2].wrapping_add(dz);
        }
        let suppressed = self.actors[index]
            .animation
            .as_ref()
            .context("ledge hang animation unavailable")?
            .playback
            .translation_suppressed();
        if !suppressed {
            self.globals.selected_ledge_anchor = Some((point, record.heading));
            self.spatial
                .as_mut()
                .context("ledge spatial state unavailable")?
                .actor_mut(index)?
                .local
                .angles[1] = record.heading;
        }
        let distance = self.query_ordinary_queued_clearance(id, query)?;
        let extent = self.ordinary_collision_shape(id)?.positive_extent[1];
        if i64::from(distance) < (i64::from(extent) + 24) * 256 {
            return self.ledge_climb_request(ctx, sounds);
        }
        self.apply_ledge_hang_pose(ctx, &scan)?;
        self.actors[index].model.ledge_support = self.ledge_support_id(record.support)?;
        Ok(1)
    }

    /// The hang pose: against the wall at the grip's height. Without a hand frame nothing moves.
    fn apply_ledge_hang_pose(&mut self, ctx: &Transition<'_>, scan: &LedgeScan) -> Result<()> {
        let Some(grip) = scan.grip else {
            return Ok(());
        };
        let (point, heading) = self.ledge_anchor()?;
        let standing = ctx.query.scene.settings.standing_height;
        let [dx, dz] =
            super::ledge::heading_offset(HIGH_FORWARD[0] * 256, heading.wrapping_add(1024));
        let y = point[1]
            .wrapping_add(standing.wrapping_mul(256))
            .wrapping_add(grip[1].wrapping_abs())
            .wrapping_add(512);
        self.write_ledge_pose(
            ctx.index,
            Some(point[0].wrapping_add(dx)),
            Some(y),
            Some(point[2].wrapping_add(dz)),
        )
    }

    /// States 13 and 11: follow the climb tables while the climb group plays; leave state 18
    /// once it ends. `from_frame` is the first frame whose height is written (-1: every frame).
    fn ledge_climb(
        &mut self,
        ctx: &Transition<'_>,
        group: u16,
        forward: &[i32],
        height: &[i32],
        from_frame: i32,
    ) -> Result<u8> {
        let index = ctx.index;
        let playback = &self.actors[index]
            .animation
            .as_ref()
            .context("ledge climb animation unavailable")?
            .playback;
        if playback.current_group() != Some(group) {
            self.set_living(index, 18);
            self.clear_ledge_motion(index);
            return Ok(1);
        }
        let (_, _, frame) = playback.frame_wait_state()?;
        let frame = usize::from(frame);
        let (&step, &rise) = forward.get(frame).zip(height.get(frame)).with_context(|| {
            format!(
                "world {} instance {index}: ledge climb frame {frame} outside the climb table",
                self.world
            )
        })?;
        // Attached to the animation's command position, the climb lags the table after halfway.
        let lag = match (
            playback.current_command_cursor()?,
            playback.current_command_count()?,
        ) {
            (Some(cursor), Some(count)) if self.updates.animation_system => {
                (cursor - (count / 2) as i32).max(0).wrapping_mul(256)
            }
            _ => 0,
        };
        let (point, heading) = self.ledge_anchor()?;
        let [dx, dz] = super::ledge::heading_offset(
            step.wrapping_mul(256).wrapping_sub(lag),
            heading.wrapping_add(1024),
        );
        let standing = ctx.query.scene.settings.standing_height;
        let y = (i32::try_from(frame)? > from_frame)
            .then(|| point[1].wrapping_add(rise.wrapping_add(standing).wrapping_mul(256)));
        self.write_ledge_pose(
            index,
            Some(point[0].wrapping_add(dx)),
            y,
            Some(point[2].wrapping_add(dz)),
        )?;
        Ok(1)
    }

    /// State 18, at the start of the ordinary update: a climb group still playing keeps it and
    /// drops its pending replacement; otherwise the default group is forced and the state clears.
    pub(super) fn finish_ledge_climb(
        &mut self,
        id: ScriptActorId,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        if self.living(index) != 18 {
            return Ok(());
        }
        let Some(animation) = self.actors[index].animation.as_mut() else {
            return Ok(());
        };
        if matches!(animation.playback.current_group(), Some(86 | 90)) {
            animation.playback.clear_pending_request();
            return Ok(());
        }
        let default = animation.playback.default_group();
        if self.updates.animation_system {
            self.request_ordinary_group(id, default, true, ticks, sounds)?;
        }
        self.set_living(index, 0);
        Ok(())
    }

    fn living(&self, index: usize) -> u8 {
        self.actors[index]
            .control
            .as_ref()
            .map_or(0, |control| control.living_state)
    }

    fn set_living(&mut self, index: usize, state: u8) {
        if let Some(control) = self.actors[index].control.as_mut() {
            control.living_state = state;
        }
    }

    fn clear_ledge_motion(&mut self, index: usize) {
        if let Some(control) = self.actors[index].control.as_mut() {
            control.sliding_state = 0;
            control.falling_state = 0;
            control.queued_motion = 0;
        }
        self.actors[index].sync_falling_motion();
    }

    fn ledge_anchor(&self) -> Result<([i32; 3], u16)> {
        self.globals
            .selected_ledge_anchor
            .context("ledge point unavailable")
    }

    /// Resolve a record's live-model owner to its actor; a static wall has no owner.
    fn ledge_support_id(&self, support: Option<usize>) -> Result<Option<ScriptActorId>> {
        support.map(|actor| self.id(actor)).transpose()
    }

    fn write_ledge_pose(
        &mut self,
        index: usize,
        x: Option<i32>,
        y: Option<i32>,
        z: Option<i32>,
    ) -> Result<()> {
        let position = &mut self
            .spatial
            .as_mut()
            .context("ledge spatial state unavailable")?
            .actor_mut(index)?
            .local
            .position;
        for (axis, value) in [x, y, z].into_iter().enumerate() {
            if let Some(value) = value {
                position[axis] = value;
            }
        }
        if !self.map_updates.contains(&index) {
            self.map_updates.push(index);
        }
        Ok(())
    }

    /// A forced group request; static models and disabled animation skip it.
    fn force_ledge_group(
        &mut self,
        ctx: &Transition<'_>,
        group: u16,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        if self.updates.animation_system {
            self.request_ordinary_group(ctx.id, group, true, ctx.environment.ticks, sounds)?;
        }
        Ok(())
    }
}

/// The inputs one transition call shares across its states, and the scan table of its entry.
struct Transition<'a> {
    id: ScriptActorId,
    index: usize,
    query: OrdinaryGroundQuery<'a>,
    environment: super::transition::ObstacleEnvironment,
    timing: LedgeTiming,
    table: Option<LedgeScan>,
}
