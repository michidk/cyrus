//! The selected player on a rope: the attach test, the attached controls, the release and the
//! placement on the node. Rules are preservation's engine/SOUP.md "Rope attachment", "Rope control
//! while attached" and "Rope attachment removal"; the rope model itself is [`crate::rope`].
//!
//! Shortcuts, each recorded in docs/DIFFERENCES.md: the climb helpers' animation-record byte checks
//! are not modelled, and a host without collision data skips the hand-height link choice's floor clamp.
use super::{ScriptActorId, ScriptWorld};
use crate::player::input::PlayerInput;
use crate::rope::{Attachment, PumpKeys};
use anyhow::{Context, Result};
use cyrus_assets::animation::Animations;

/// Group requests the rope controls make.
const GROUP_IDLE: u16 = 6;
const GROUP_CLIMB_UP: u16 = 82;
const GROUP_CLIMB_DOWN: u16 = 83;
const GROUP_GRAB: u16 = 84;
const GROUP_JUMP: u16 = 85;
/// The animation group whose first frame supplies the hand point of the grab.
const GROUP_HAND: u64 = 79;
/// Links searched below the requested one start this many before it.
const HAND_SEARCH_BACK: i32 = 8;
/// The ordinary jump entry's queued motion after a rope release.
const QUEUED_ROPE_JUMP: u8 = 24;
/// The live world data the hand-height link choice reads for its floor probe. Absent when the
/// host has no collision scene; the clamp is then skipped.
#[derive(Clone, Copy)]
pub(super) struct RopeProbe<'a> {
    pub scene: &'a crate::player::data::PlayerScene,
    pub view: [i32; 2],
    pub actors: &'a [ScriptActorId],
}

/// Retained state of the selected player's rope handling. The rope model itself is saved with the
/// world snapshot; these transient input latches are rebuilt when the selected player is rebound.
#[derive(Clone, Debug, Default)]
pub(super) struct PlayerRope {
    /// The rope the player last held; skipped while the 24-tick countdown (the runtime jump
    /// selector) runs.
    last_rope: Option<usize>,
    /// The jump key was held when the rope was grabbed; the release waits for it to be up.
    latch: bool,
    turn_rate: i32,
    turn_total: i32,
    /// The node position of the previous update, engine map units, for the node's measured speed.
    previous_node: Option<[f64; 3]>,
}

fn angle_difference(a: i32, b: i32) -> i32 {
    let d = (a & 0x7ff) - (b & 0x7ff);
    if d > 0x400 {
        d - 0x800
    } else if d < -0x400 {
        d + 0x800
    } else {
        d
    }
}

/// The shorter heading difference of an actor to a rope heading or to that heading plus 0x400.
fn rope_heading_difference(rope_heading: i32, actor_heading: i32) -> i32 {
    let first = angle_difference(rope_heading, actor_heading);
    let second = angle_difference((rope_heading + 0x400) & 0x7ff, actor_heading);
    if first.abs() < second.abs() {
        first
    } else {
        second
    }
}

use crate::rope::engine_position as to_engine;

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl ScriptWorld {
    /// Whether the selected player hangs on a rope; the movement stages skip while it does.
    pub fn selected_player_on_rope(&self) -> bool {
        self.selected_player_index()
            .and_then(|index| self.actors[index].control.as_ref())
            .is_some_and(|control| control.rope_attached)
    }

    /// One selected-player rope update before the movement stages: while attached the controls run
    /// and the player is placed on its node; otherwise the attach test runs. Returns whether the
    /// player is on a rope, which skips the ordinary movement and combat stages.
    #[allow(clippy::too_many_arguments)] // The update reads the keys, clock, probe data and sound sink.
    pub(super) fn update_player_rope(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        held: Option<&PlayerInput>,
        frame_scale: i32,
        ticks: u32,
        probe: Option<RopeProbe<'_>>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        let Some(settings) = self.updates.rope else {
            return Ok(false);
        };
        let attached = self.actors[index]
            .control
            .as_ref()
            .is_some_and(|control| control.rope_attached);
        let holder = self
            .ropes
            .iter()
            .position(|rope| rope.attachments.iter().any(|a| a.actor == index));
        if attached && holder.is_none() {
            // A restored session keeps the flag but not the rope: let go.
            if let Some(control) = self.actors[index].control.as_mut() {
                control.rope_attached = false;
            }
            return Ok(false);
        }
        if let Some(rope) = holder {
            self.run_rope_controls(
                id,
                rope,
                data,
                held,
                frame_scale,
                ticks,
                settings,
                probe,
                sounds,
            )?;
            return Ok(true);
        }
        if self.ropes.is_empty() {
            return Ok(false);
        }
        let Some((rope, link)) = self.rope_attach_candidate(id, data, settings, ticks)? else {
            return Ok(false);
        };
        let link = self.rope_hand_link(id, rope, link, data, probe)?;
        self.attach_player(id, data, rope, link, held, frame_scale, ticks, sounds)?;
        Ok(true)
    }

    fn holding_weapon(&self, index: usize) -> bool {
        let actor = &self.actors[index];
        actor.attributes[110] != 0
            || actor
                .control
                .as_ref()
                .is_some_and(|control| control.weapon_drawn)
    }

    /// The attach test of preservation "Rope attachment": a rope and link when the airborne,
    /// weaponless player touches one.
    fn rope_attach_candidate(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        settings: cyrus_assets::script::RopeSettings,
        ticks: u32,
    ) -> Result<Option<(usize, i32)>> {
        let index = self.resolve(id)?;
        if self.holding_weapon(index) || self.actors[index].animation.is_none() {
            return Ok(None);
        }
        let control = self.actors[index]
            .control
            .as_mut()
            .context("rope attach control unavailable")?;
        if control.falling_state == 0 && control.queued_motion == 0 {
            return Ok(None);
        }
        if control.jump_selector != 0 {
            let ticks = u8::try_from(ticks).unwrap_or(u8::MAX);
            if control.jump_selector > ticks {
                control.jump_selector -= ticks;
            } else {
                control.jump_selector = 0;
                self.player_rope.last_rope = None;
            }
        }
        let skipped = self.actors[index]
            .control
            .as_ref()
            .is_some_and(|control| control.jump_selector != 0)
            .then_some(self.player_rope.last_rope)
            .flatten();
        let Ok(spheres) = self.ordinary_overlap_spheres(id, data) else {
            return Ok(None);
        };
        let heading = i32::from(self.spatial(id)?.local.angles[1]);
        // The engine tests the retained rope list head first.  Keep that order here: when
        // several ropes overlap the player's contact spheres, the first authored rope owns the
        // grab (the selected link is resolved below).
        for (rope_index, rope) in self.ropes.iter().enumerate() {
            if rope.links <= 1 || skipped == Some(rope_index) {
                continue;
            }
            if rope.kind & 1 != 0
                && rope_heading_difference(rope.heading, heading).abs() > settings.attach_angle
            {
                continue;
            }
            let radius = f64::from(rope.link_length) * f64::from(crate::ENGINE_UNIT_SCALE);
            let mut lowest: Option<usize> = None;
            let links: Vec<[f64; 3]> = rope
                .chain()
                .iter()
                .map(|pose| to_engine(pose.position))
                .collect();
            for sphere in &spheres {
                let center = sphere.center.map(f64::from);
                for (link, position) in links.iter().enumerate() {
                    let reach = f64::from(sphere.radius) + radius;
                    let measured = (distance(center, *position) * 256.0).round();
                    if measured < (reach * 256.0).trunc() && lowest.is_none_or(|l| link < l) {
                        lowest = Some(link);
                    }
                }
            }
            if let Some(link) = lowest {
                let last = i32::try_from(rope.links).unwrap_or(i32::MAX) - 1;
                return Ok(Some((rope_index, i32::try_from(link)?.clamp(0, last))));
            }
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)] // The grab reads the keys, clock and sound sink of one update.
    fn attach_player(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        rope_index: usize,
        link: i32,
        held: Option<&PlayerInput>,
        frame_scale: i32,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        self.request_ordinary_group(id, GROUP_GRAB, false, ticks, sounds)?;
        let pose = self.spatial(id)?.local;
        let heading = i32::from(pose.angles[1]);
        let step = {
            let previous = self.spatial(id)?.previous_local_position;
            let dx = f64::from(pose.position[0] - previous[0]);
            let dz = f64::from(pose.position[2] - previous[2]);
            dx.hypot(dz).round() as i64
        };
        let control = self.actors[index]
            .control
            .as_mut()
            .context("rope attach control unavailable")?;
        let airborne = control.queued_motion > 1 || control.falling_state > 1;
        let backward = matches!(control.motion_classification, 3 | 7);
        control.rope_attached = true;
        control.falling_state = 0;
        control.queued_motion = 0;
        control.sliding_state = 0;
        control.living_state = 0;
        control.directional_state = [0; 2];
        control.jump_selector = 0;
        let rope = &mut self.ropes[rope_index];
        // The engine links a newly attached node at the rope-list head.  Chain
        // updates and owner scans are list-order sensitive, so appending here
        // would make a second attachment run after older nodes.
        rope.attachments.insert(
            0,
            Attachment {
                actor: index,
                node: link,
                motion: 0,
            },
        );
        if rope.kind & 1 == 0 {
            rope.heading = heading;
        } else {
            let shifted = (heading + rope_heading_difference(rope.heading, heading)) & 0x7ff;
            self.spatial
                .as_mut()
                .context("rope attach spatial state unavailable")?
                .actor_mut(index)?
                .local
                .angles[1] = shifted as u16;
        }
        let rope = &mut self.ropes[rope_index];
        if airborne {
            rope.phase = if backward { 0 } else { 0x40000 };
            let links = i64::try_from(rope.links).unwrap_or(i64::MAX);
            let divisor = i64::from(link.max(1));
            rope.amplitude = i32::try_from((step / 1000) * links / divisor).unwrap_or(i32::MAX);
        }
        rope.push = 0;
        self.player_rope = PlayerRope {
            last_rope: Some(rope_index),
            latch: held.is_some_and(|keys| keys.jump),
            turn_rate: 0,
            turn_total: 0,
            previous_node: None,
        };
        let _ = frame_scale;
        self.place_player_on_rope(id, rope_index, data)
    }

    #[allow(clippy::too_many_arguments)] // The controls read the keys, clock, sound sink and installed values.
    fn run_rope_controls(
        &mut self,
        id: ScriptActorId,
        rope_index: usize,
        data: &Animations,
        held: Option<&PlayerInput>,
        frame_scale: i32,
        ticks: u32,
        settings: cyrus_assets::script::RopeSettings,
        probe: Option<RopeProbe<'_>>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let keys = held;
        let pressed = |select: fn(&PlayerInput) -> bool| keys.is_some_and(select);
        let (up, down) = (pressed(|k| k.forward), pressed(|k| k.backward));
        let (left, right) = (pressed(|k| k.left), pressed(|k| k.right));
        let (walk, jump, look) = (
            pressed(|k| k.walk),
            pressed(|k| k.jump),
            pressed(|k| k.look),
        );
        let weapon = self.holding_weapon(index);
        if weapon || (!up && !down) {
            self.request_ordinary_group(id, GROUP_IDLE, false, ticks, sounds)?;
        }
        if jump {
            if !self.player_rope.latch {
                self.release_player(
                    id,
                    rope_index,
                    up,
                    down,
                    frame_scale,
                    ticks,
                    settings,
                    sounds,
                )?;
                return Ok(());
            }
        } else {
            self.player_rope.latch = false;
        }
        if !weapon && !look {
            if up {
                self.climb(id, rope_index, data, true, ticks, probe, sounds)?;
            } else if down {
                self.climb(id, rope_index, data, false, ticks, probe, sounds)?;
            }
        }
        let heading = i32::from(self.spatial(id)?.local.angles[1]);
        let mut turned = heading;
        if self.player_rope.turn_rate != 0 {
            let rate = self.player_rope.turn_rate;
            let step = ((rate >> 4) * frame_scale) >> 8;
            turned = (heading + step) & 0x7ff;
            self.player_rope.turn_total += step;
            if self.player_rope.turn_total.abs() > rate.abs() {
                self.player_rope.turn_rate = 0;
            }
        } else if walk {
            let rope = &self.ropes[rope_index];
            if rope.amplitude < 5 {
                if rope.kind & 1 != 0 {
                    if left {
                        self.player_rope.turn_rate = 0x400;
                        self.player_rope.turn_total = 0;
                    } else if right {
                        self.player_rope.turn_rate = -0x400;
                        self.player_rope.turn_total = 0;
                    }
                } else {
                    let step = ((i32::from(right) * frame_scale) >> 8)
                        - ((i32::from(left) * frame_scale) >> 8);
                    turned = (heading + step) & 0x7ff;
                }
            }
        } else if !look {
            let difference = angle_difference(self.ropes[rope_index].heading, heading).abs();
            self.ropes[rope_index].pump(PumpKeys { left, right }, difference);
        }
        if turned != heading {
            self.spatial
                .as_mut()
                .context("rope turn spatial state unavailable")?
                .actor_mut(index)?
                .local
                .angles[1] = turned as u16;
        }
        self.place_player_on_rope(id, rope_index, data)
    }

    /// The link nearest below the player's hand, searched from eight links before `start`: the
    /// hand point is the grip vertex of the first frame of group 79 plus the player's height and
    /// two link lengths, and the winner is the link at or above it that is closest. Without that
    /// frame `start` stands. Near the ground the hand height is clamped a margin (the grip's height
    /// plus the actor's feet extent) above the floor probe.
    fn rope_hand_link(
        &mut self,
        id: ScriptActorId,
        rope_index: usize,
        start: i32,
        data: &Animations,
        probe: Option<RopeProbe<'_>>,
    ) -> Result<i32> {
        let Some(grip) = self.group_hand_vertex(id, data, GROUP_HAND)? else {
            return Ok(start);
        };
        let rope = &self.ropes[rope_index];
        let length = (f64::from(rope.link_length) * f64::from(crate::ENGINE_UNIT_SCALE) * 256.0)
            .round() as i32;
        let base = self.spatial(id)?.local.position[1]
            .wrapping_add(length.wrapping_mul(2))
            .wrapping_sub(1);
        let mut target = grip[1].wrapping_add(base) >> 8;
        if let Some(probe) = probe {
            let floor = self.rope_floor_height(id, probe, data)? >> 8;
            let margin = (grip[1].wrapping_abs() >> 8)
                .wrapping_add(self.ordinary_collision_shape(id)?.positive_extent[1]);
            if (floor.wrapping_sub(target)).wrapping_abs() < margin {
                target = floor.wrapping_sub(margin);
            }
        }
        let target = f64::from(target);
        let rope = &self.ropes[rope_index];
        let mut best = (f64::from(999_999), None);
        for (link, pose) in rope
            .chain()
            .iter()
            .enumerate()
            .skip((start - HAND_SEARCH_BACK).max(0) as usize)
        {
            let height = to_engine(pose.position)[1];
            let gap = (height - target).round().abs();
            if gap < best.0 && height <= target {
                best = (gap, Some(link));
            }
        }
        Ok(best.1.map_or(start, |link| link as i32))
    }

    /// The actor floor probe the hand-height link choice clamps against, in Q8 engine units: the
    /// lowest class 2 or 4 face crossing the vertical column from the actor's head extent down to
    /// twice its feet extent, replaced by the terrain sample when that lies strictly inside the
    /// column and above the face; 0x3e800 below the actor when nothing is found. The terrain
    /// height truncates like the queued clearance's (Inferred: the same conversion service).
    fn rope_floor_height(
        &mut self,
        id: ScriptActorId,
        probe: RopeProbe<'_>,
        data: &Animations,
    ) -> Result<i32> {
        let prepared =
            self.prepare_ordinary_collision_query(id, probe.scene, data, probe.view, probe.actors)?;
        let current = self.spatial(id)?.query_pose()?.position;
        let (positive, negative) = {
            let shape = self.ordinary_collision_shape(id)?;
            (shape.positive_extent[1], shape.negative_extent[1])
        };
        let column = current.map(|value| value >> 8);
        let top = column[1].wrapping_sub(negative);
        let bottom = column[1].wrapping_add(positive.wrapping_mul(2));
        let poses = self.ordinary_collision_poses(&prepared)?;
        let models = probe
            .scene
            .actor_collision
            .as_ref()
            .context("rope floor probe live geometry unavailable; re-export world")?;
        let contacts = probe.scene.static_collision.runtime_segment_contacts(
            &prepared.candidates,
            models,
            &poses,
            [column[0], top, column[2]],
            [column[0], bottom, column[2]],
            false,
            None,
        )?;
        let mut floor = 999_999;
        for (_, contact) in contacts {
            let contact = contact.integer()?;
            let support = cyrus_collision::ground::GroundSupport {
                height: f64::from(contact.point[1]),
                normal: contact.normal,
            };
            if matches!(support.classification(), 2 | 4) && contact.point[1] < floor {
                floor = contact.point[1];
            }
        }
        if let Some(terrain) = &probe.scene.terrain {
            let sample = terrain
                .height(current.map(i64::from))
                .with_context(|| {
                    format!(
                        "world {} instance {} rope floor probe terrain height-grid edge unsupported",
                        self.world, id.index
                    )
                })?
                .trunc();
            anyhow::ensure!(
                sample.is_finite()
                    && sample >= f64::from(i32::MIN)
                    && sample <= f64::from(i32::MAX),
                "rope floor probe terrain height outside integer range"
            );
            let sample = sample as i32;
            if top < sample && sample < bottom && sample < floor {
                floor = sample;
            }
        }
        Ok(if floor == 999_999 {
            current[1].wrapping_add(0x3e800)
        } else {
            floor.wrapping_shl(8)
        })
    }

    /// The climb helpers of preservation "Rope control while attached".
    #[allow(clippy::too_many_arguments)] // The climb reads the keys' direction, clock, probe data and sound sink.
    fn climb(
        &mut self,
        id: ScriptActorId,
        rope_index: usize,
        data: &Animations,
        up: bool,
        ticks: u32,
        probe: Option<RopeProbe<'_>>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let node = self.ropes[rope_index]
            .attachments
            .iter()
            .find(|a| a.actor == index)
            .map(|a| a.node)
            .context("rope climb: attachment unavailable")?;
        if up && node - 1 <= 0
            || !up && self.rope_hand_link(id, rope_index, node + 1, data, probe)? <= node
        {
            return Ok(());
        }
        let group = if up { GROUP_CLIMB_UP } else { GROUP_CLIMB_DOWN };
        self.request_ordinary_group(id, group, false, ticks, sounds)
    }

    /// The original's detach routine: removes the actor's attachment from the rope, applies the
    /// detach motion (speed from the node motion, amplitude, the rope direction and the held
    /// vertical keys), clears the actor's attachment flag and pitch and starts its 24-tick rope
    /// countdown.
    fn detach_attachment(
        &mut self,
        id: ScriptActorId,
        rope_index: usize,
        up: bool,
        down: bool,
        frame_scale: i32,
        settings: cyrus_assets::script::RopeSettings,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let heading = i32::from(self.spatial(id)?.local.angles[1]);
        let rope = &mut self.ropes[rope_index];
        let position = rope
            .attachments
            .iter()
            .position(|a| a.actor == index)
            .context("rope release: attachment unavailable")?;
        let attachment = rope.attachments.remove(position);
        let motion = i64::from(attachment.motion);
        let scale = i64::from(frame_scale.max(1));
        let speed = if rope.amplitude < 6 && motion.abs() < (scale * 0x1800) >> 8 {
            if up {
                settings.jump_add
            } else if down {
                -settings.jump_add
            } else {
                0
            }
        } else {
            let mut direction = rope.direction;
            if rope.kind & 1 != 0 && angle_difference(heading, rope.heading).abs() > 0x10 {
                direction = (direction + 1) & 1;
            }
            let extra = i32::try_from((motion.abs() << 8) / scale).unwrap_or(i32::MAX);
            if direction == 1 {
                settings.jump_add.saturating_add(extra)
            } else {
                -settings.jump_add.saturating_sub(extra)
            }
        };
        self.spatial
            .as_mut()
            .context("rope detach spatial state unavailable")?
            .actor_mut(index)?
            .local
            .angles[0] = 0;
        let control = self.actors[index]
            .control
            .as_mut()
            .context("rope detach control unavailable")?;
        control.rope_attached = false;
        control.rope_speed = speed;
        control.jump_selector = 24;
        Ok(())
    }

    /// The health services' rope detach: the selected player lets go with no vertical key held.
    /// Unknown: whether the original reads the held keys here; neutral keys are assumed.
    pub(super) fn detach_selected_from_rope(
        &mut self,
        id: ScriptActorId,
        inputs: super::ScriptInputs<'_>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let settings = self
            .updates
            .rope
            .context("rope detach settings unavailable")?;
        let frame_scale = i32::try_from(
            inputs
                .frame_scale
                .context("rope detach frame scale unavailable")?,
        )?;
        let rope_index = self
            .ropes
            .iter()
            .position(|rope| rope.attachments.iter().any(|a| a.actor == index))
            .context("rope detach: attachment unavailable")?;
        self.detach_attachment(id, rope_index, false, false, frame_scale, settings)
    }

    /// The jump-key release: the removal's detach motion, then the ordinary jump entry with the
    /// 24-tick countdown running, which queues motion 24 and forces the rope-jump group.
    #[allow(clippy::too_many_arguments)] // The release reads the keys, clock, installed values and sound sink.
    fn release_player(
        &mut self,
        id: ScriptActorId,
        rope_index: usize,
        up: bool,
        down: bool,
        frame_scale: i32,
        ticks: u32,
        settings: cyrus_assets::script::RopeSettings,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let origin_y = self.spatial(id)?.local.position[1];
        self.detach_attachment(id, rope_index, up, down, frame_scale, settings)?;
        let control = self.actors[index]
            .control
            .as_mut()
            .context("rope release control unavailable")?;
        control.stable_position_valid = false;
        control.fall_origin_y = origin_y;
        control.fall_timer = 0;
        control.sliding_distance = 0;
        control.queued_motion = QUEUED_ROPE_JUMP;
        control.motion_classification = 2;
        self.player_rope.latch = true;
        self.player_rope.turn_rate = 0;
        self.player_rope.previous_node = None;
        self.request_ordinary_group(id, GROUP_JUMP, true, ticks, sounds)
    }

    /// Puts the player on its link, measures the node's speed and sets the actor's pitch from the
    /// accumulated step angle, as the chain update does for each attachment.
    fn place_player_on_rope(
        &mut self,
        id: ScriptActorId,
        rope_index: usize,
        data: &Animations,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let rope = &mut self.ropes[rope_index];
        rope.clamp_nodes();
        let node = rope
            .attachments
            .iter()
            .find(|a| a.actor == index)
            .map(|a| a.node)
            .context("rope placement: attachment unavailable")?;
        let chain = rope.chain();
        let pose = chain
            .get(usize::try_from(node)?)
            .context("rope placement: node outside the chain")?;
        let engine = to_engine(pose.position);
        let speed = self
            .player_rope
            .previous_node
            .map_or(0.0, |previous| (distance(engine, previous) * 256.0).round());
        self.player_rope.previous_node = Some(engine);
        let rope = &mut self.ropes[rope_index];
        if let Some(attachment) = rope.attachments.iter_mut().find(|a| a.actor == index) {
            let signed = speed as i32;
            attachment.motion = if rope.direction == 0 { -signed } else { signed };
            attachment.node = node;
        }
        let accumulated = rope.step * (node - 1).max(0);
        let heading = i32::from(self.spatial(id)?.local.angles[1]);
        let close = angle_difference(heading, self.ropes[rope_index].heading).abs() < 0x10;
        let pitch = if close {
            (accumulated >> 8) & 0x7ff
        } else {
            (-(accumulated >> 8)) & 0x7ff
        };
        let mut position = engine.map(|value| (value * 256.0).round() as i32);
        // The hand sits on the node: the player moves by the grip vertex of the group 79 pose,
        // negated and rotated by its own orientation (heading, rope pitch, no roll).
        if let Some(grip) = self.group_hand_vertex(id, data, GROUP_HAND)? {
            let offset =
                cyrus_world::transform::Matrix::orientation([pitch as u16, heading as u16, 0])
                    .transform([0, grip[1].wrapping_neg(), grip[2].wrapping_neg()])?;
            for axis in 0..3 {
                position[axis] = position[axis].wrapping_add(offset[axis]);
            }
        }
        let spatial = self
            .spatial
            .as_mut()
            .context("rope placement spatial state unavailable")?
            .actor_mut(index)?;
        spatial.local.position = position;
        spatial.previous_local_position = position;
        spatial.local.angles[0] = pitch as u16;
        spatial.local.angles[2] = 0;
        self.refresh_transform(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_differences_take_the_short_way_round() {
        assert_eq!(angle_difference(10, 2040), 18);
        assert_eq!(angle_difference(2040, 10), -18);
        assert_eq!(angle_difference(0x400, 0), 0x400);
        assert_eq!(angle_difference(0x401, 0), -0x3ff);
    }

    #[test]
    fn a_rope_heading_matches_either_end_of_its_line() {
        assert_eq!(rope_heading_difference(0, 5), -5);
        assert_eq!(rope_heading_difference(0, 0x400 + 5), -5);
        assert!(rope_heading_difference(0, 0x200).abs() == 0x200);
    }

    #[test]
    fn exported_positions_map_back_to_the_engine_frame() {
        // World 24's player stands at engine map units (2174, -64, 2067).
        let engine = to_engine([-108.7, 3.2, -3173.45]);
        assert!((engine[0] - 2174.0).abs() < 1e-2);
        assert!((engine[1] + 64.0).abs() < 1e-2);
        assert!((engine[2] - 2067.0).abs() < 1e-1);
    }
}
