//! Current-group playback and reverse rules from preservation's engine/animation.md.
use super::Animations;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
enum Command {
    Frame(u32),
    End,
    Previous(usize),
    Future(usize),
    Breakpoint,
    Delay(u8),
    Sound {
        effect: Option<i64>,
        rate_shift: Option<i64>,
    },
    Event(u32),
    /// Trigger mask and the third operand (`target_group`), the command index a taken mask-4
    /// transition jumps to.
    Transition(u32, u32),
    Unsupported(u8),
}

/// Authored animation operations dispatched synchronously by the actor's caller, in command order.
/// The caller owns surface remapping, positional gains, channel allocation and its last handle.
/// `Event` and `Transition` are not sounds: sound consumers ignore them and the world host owns
/// their effects and decisions.
#[derive(Debug, PartialEq, Eq)]
pub enum AnimationSound {
    /// A forward `Transition` command's trigger mask, for masks other than 4 (the sequence
    /// decides mask 4 from the actor's falling byte). The host decides whether the transition is
    /// taken; returning `Ok` continues with the next command, and an error stops playback, which
    /// is how a host refuses a mask whose state it does not model. Reverse playback never asks.
    Transition { mask: u32 },
    /// The actor-event service with the command's operand: a bit set the host maps to surface
    /// effects and a screen-shake impulse.
    Event { flags: u32 },
    /// Start the signed effect index with a signed offset in hertz, before surface remapping.
    Play { effect: i16, rate_offset_hz: i32 },
    /// Stop the last remembered sound at EndAnimation, if its handle is valid.
    Stop,
    /// Clear the remembered handle on replacement without stopping its channel.
    Forget,
}

pub(super) type SoundDispatcher<'a> =
    Option<&'a mut dyn FnMut(AnimationSound) -> anyhow::Result<()>>;

/// A current group. Callers supply activation, whole nominal ticks, and exit eligibility.
/// Sound and actor-event commands require actor-side dispatch; remaining forward operations stay explicitly unsupported.
#[derive(Clone)]
pub struct FrameSequence {
    context: String,
    commands: Vec<Command>,
    command: i16,
    speed: u8,
    countdown: u8,
    loops: u8,
    exit_reached: bool,
    ended: bool,
    failure: Option<String>,
    translation_suppressed: bool,
    delay: u8,
    type_one: bool,
    type_two: bool,
    /// Transient: whether the pending request is the actor's default group, set per advance.
    pending_default: bool,
    /// Transient: whether the actor's falling byte is nonzero, set per advance by the owner.
    motion_active: bool,
    sound_requested: bool,
}

/// Mutable playback only; immutable commands and type come from matched installed groups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SequenceSnapshot {
    command: i16,
    speed: u8,
    countdown: u8,
    loops: u8,
    exit_reached: bool,
    ended: bool,
    failure: Option<String>,
    translation_suppressed: bool,
    delay: u8,
    sound_requested: bool,
}

impl FrameSequence {
    pub(super) fn snapshot(&self) -> SequenceSnapshot {
        SequenceSnapshot {
            command: self.command,
            speed: self.speed,
            countdown: self.countdown,
            loops: self.loops,
            exit_reached: self.exit_reached,
            ended: self.ended,
            failure: self.failure.clone(),
            translation_suppressed: self.translation_suppressed,
            delay: self.delay,
            sound_requested: self.sound_requested,
        }
    }

    pub(super) fn from_snapshot(
        context: String,
        group: &serde_json::Value,
        snapshot: SequenceSnapshot,
    ) -> anyhow::Result<Self> {
        let mut sequence = Self::from_group(context, group, 0, 0)?;
        let count = sequence.commands.len();
        let index = usize::try_from(snapshot.command).ok();
        // A final Delay advances past the table before waiting. Its next tick may
        // advance once more before retaining an out-of-range failure.
        anyhow::ensure!(
            index.is_some_and(|index| index < count
                || (index == count && snapshot.delay != 0)
                || (snapshot.failure.is_some() && index <= count.saturating_add(1)))
                || (snapshot.failure.is_some() && snapshot.command < 0),
            "{} command {}: saved cursor outside group",
            sequence.context,
            snapshot.command
        );
        anyhow::ensure!(
            !snapshot.ended
                || matches!(
                    index.and_then(|index| sequence.commands.get(index)),
                    Some(Command::End)
                ),
            "{} command {}: saved End state mismatch",
            sequence.context,
            snapshot.command
        );
        sequence.command = snapshot.command;
        sequence.speed = snapshot.speed;
        sequence.countdown = snapshot.countdown;
        sequence.loops = snapshot.loops;
        sequence.exit_reached = snapshot.exit_reached;
        sequence.ended = snapshot.ended;
        sequence.failure = snapshot.failure;
        sequence.translation_suppressed = snapshot.translation_suppressed;
        sequence.delay = snapshot.delay;
        sequence.sound_requested = snapshot.sound_requested;
        Ok(sequence)
    }

    /// Selects an explicitly requested forward group and its finite loop countdown.
    /// This does not run the actor script or choose a default animation.
    pub fn from_asset(
        data: &Animations,
        actor: &str,
        group: u16,
        loops: u8,
    ) -> anyhow::Result<FrameSequence> {
        data.require_version()?;
        let context = format!("world {} actor {actor} group {group}", data.world);
        let actor = data.actor(actor)?;
        let selected = actor["animation_groups"]
            .as_array()
            .with_context(|| format!("{context}: groups absent"))?
            .iter()
            .find(|candidate| candidate["group_index"].as_u64() == Some(u64::from(group)))
            .with_context(|| format!("{context}: absent"))?;
        FrameSequence::from_group(context, selected, loops, 0)
    }
}

impl FrameSequence {
    pub(super) fn from_group(
        context: String,
        selected: &serde_json::Value,
        loops: u8,
        modifier: u8,
    ) -> anyhow::Result<Self> {
        let group_type = u16::try_from(
            selected["anim_type"]
                .as_u64()
                .with_context(|| format!("{context}: group type absent"))?,
        )
        .with_context(|| format!("{context}: group type outside u16 range"))?;
        anyhow::ensure!(
            group_type.to_le_bytes()[0] <= 2,
            "{context}: group type unsupported"
        );
        let speed = u16::try_from(
            selected["anim_id"]
                .as_u64()
                .with_context(|| format!("{context}: frame countdown absent"))?,
        )
        .with_context(|| format!("{context}: frame countdown outside u16 range"))?
        .to_le_bytes()[0]
            .wrapping_add(modifier);
        let mut commands = Vec::new();
        for (index, command) in selected["commands"]
            .as_array()
            .with_context(|| format!("{context}: command table absent"))?
            .iter()
            .enumerate()
        {
            let opcode = u8::try_from(
                command["opcode"]
                    .as_u64()
                    .with_context(|| format!("{context} command {index}: opcode absent"))?,
            )?;
            let decoded = match opcode {
                0 => Command::Frame(u32::try_from(command["frame_reference"].as_u64().with_context(|| format!("{context} command {index}: file frame reference absent; re-export the world"))?)?),
                1 => Command::End,
                5 => Command::Breakpoint,
                4 => Command::Sound {
                    effect: command["sound_param"].as_i64(),
                    // Preservation's legacy field name stores rate steps, not volume.
                    rate_shift: command["volume_shift"].as_i64(),
                },
                11 => {
                    let value = command["value"].as_u64().with_context(|| {
                        format!("{context} command {index}: event operand absent")
                    })?;
                    Command::Event(u32::try_from(value).with_context(|| {
                        format!("{context} command {index}: event operand outside u32 range")
                    })?)
                }
                15 => match (
                    command["trigger_mask"].as_u64().map(u32::try_from),
                    command["target_group"].as_u64().map(u32::try_from),
                ) {
                    (Some(mask), Some(target)) => Command::Transition(
                        mask.with_context(|| {
                            format!("{context} command {index}: trigger mask outside u32 range")
                        })?,
                        target.with_context(|| {
                            format!("{context} command {index}: transition target outside u32 range")
                        })?,
                    ),
                    _ => Command::Unsupported(15),
                },
                12 => {
                    let value = command["value"].as_i64().with_context(|| {
                        format!("{context} command {index}: delay operand absent")
                    })?;
                    let value = i32::try_from(value).with_context(|| {
                        format!("{context} command {index}: delay operand outside i32 range")
                    })?;
                    Command::Delay(value as u8)
                }
                2 | 3 => {
                    let target = usize::try_from(command["value"].as_u64().with_context(|| format!("{context} command {index}: invalid target"))?)?;
                    if opcode == 2 { Command::Previous(target) } else { Command::Future(target) }
                }
                _ => Command::Unsupported(opcode),
            };
            commands.push(decoded);
        }
        for (index, command) in commands.iter().enumerate() {
            if let Command::Previous(target) | Command::Future(target) = command {
                anyhow::ensure!(
                    *target < commands.len(),
                    "{context} command {index}: target {target} outside group"
                );
            }
        }
        Ok(FrameSequence {
            context,
            commands,
            command: 0,
            speed,
            countdown: speed,
            loops,
            exit_reached: false,
            ended: false,
            failure: None,
            translation_suppressed: false,
            delay: 0,
            type_one: group_type.to_le_bytes()[0] == 1,
            type_two: group_type.to_le_bytes()[0] == 2,
            pending_default: false,
            motion_active: false,
            sound_requested: false,
        })
    }

    pub(super) fn set_loop_count(&mut self, count: u8) {
        self.loops = count;
    }

    /// Writes the wait byte: zero runs, 255 holds until released, other values count ticks.
    pub(super) fn set_wait(&mut self, wait: u8) {
        self.delay = wait;
    }

    /// Clears the wait byte only when it holds the animation (255).
    pub(super) fn release_hold(&mut self) {
        if self.delay == u8::MAX {
            self.delay = 0;
        }
    }

    pub(super) fn clear_exit_marker(&mut self) {
        self.exit_reached = false;
    }

    pub(super) fn exit_reached(&self) -> bool {
        self.exit_reached
    }

    pub(super) fn replace_after_exit(&self) -> bool {
        (self.type_one || self.type_two) && self.exit_reached
    }

    pub(super) fn set_pending_default(&mut self, pending_default: bool) {
        self.pending_default = pending_default;
    }

    pub(super) fn set_motion_active(&mut self, motion_active: bool) {
        self.motion_active = motion_active;
    }

    pub(super) fn forget_sound(&mut self, sounds: &mut SoundDispatcher<'_>) -> anyhow::Result<()> {
        if self.sound_requested {
            self.dispatch_sound(sounds, AnimationSound::Forget)?;
            self.sound_requested = false;
        }
        Ok(())
    }

    fn dispatch_sound(
        &self,
        sounds: &mut SoundDispatcher<'_>,
        event: AnimationSound,
    ) -> anyhow::Result<()> {
        sounds
            .as_mut()
            .with_context(|| format!("{}: animation sound dispatch unsupported", self.context))?(
            event,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "{} command {}: animation sound dispatch failed: {error:#}",
                self.context,
                self.command
            )
        })
    }

    /// The current group's BreakPoint marker, checked by ordinary forward walking.
    /// End does not select a replacement group or establish other movement eligibility.
    pub fn translation_suppressed(&self) -> bool {
        self.translation_suppressed
    }

    /// True only after EndAnimation; waiting for a delay does not finish the group.
    pub fn is_finished(&self) -> bool {
        self.ended
    }

    pub(super) fn command_cursor(&self) -> i32 {
        i32::from(self.command)
    }

    /// Skip the first `frames` ShowFrames of a freshly selected group without running any other
    /// command, counting each in `progress`; returns the frame displayed next.
    pub(super) fn skip_frames(
        &mut self,
        frames: u8,
        progress: &mut u8,
    ) -> anyhow::Result<Option<u32>> {
        let mut remaining = frames;
        loop {
            let command = usize::try_from(self.command)
                .ok()
                .and_then(|index| self.commands.get(index))
                .with_context(|| {
                    format!("{} command {}: outside group", self.context, self.command)
                })?;
            match *command {
                Command::Frame(reference) if remaining == 0 => return Ok(Some(reference)),
                Command::Frame(_) => {
                    remaining -= 1;
                    *progress = progress.wrapping_add(1);
                }
                Command::End => {
                    self.ended = true;
                    return Ok(None);
                }
                Command::Previous(_) | Command::Future(_) | Command::Unsupported(_) => {
                    anyhow::bail!(
                        "{} command {}: frame skipping over a jump or unsupported command",
                        self.context,
                        self.command
                    )
                }
                _ => {}
            }
            self.command += 1;
        }
    }

    pub(super) fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// Advances using the original whole nominal-tick input; returns a file frame reference.
    /// `None` keeps the prior pose during a delay or after EndAnimation; `is_finished`
    /// distinguishes those states. Unknown events fail without being skipped.
    pub fn advance(&mut self, ticks: u32, exit_requested: bool) -> anyhow::Result<Option<u32>> {
        self.advance_with_sounds(ticks, exit_requested, &mut None)
    }

    pub(super) fn advance_with_sounds(
        &mut self,
        ticks: u32,
        exit_requested: bool,
        sounds: &mut SoundDispatcher<'_>,
    ) -> anyhow::Result<Option<u32>> {
        self.advance_with_progress(ticks, exit_requested, sounds, &mut 0, false)
    }

    pub(super) fn advance_with_progress(
        &mut self,
        ticks: u32,
        exit_requested: bool,
        sounds: &mut SoundDispatcher<'_>,
        progress: &mut u8,
        reverse: bool,
    ) -> anyhow::Result<Option<u32>> {
        if let Some(message) = &self.failure {
            anyhow::bail!("{message}");
        }
        let result = self.advance_current(ticks, exit_requested, sounds, progress, reverse);
        if let Err(error) = &result {
            self.failure = Some(error.to_string());
        }
        result
    }

    pub(super) fn waiting_for_delay(&mut self, ticks: u32) -> bool {
        if self.delay != 0 {
            if self.delay == u8::MAX {
                return true;
            }
            let remaining = i32::from(self.delay).wrapping_sub(ticks as i32);
            if remaining > 0 {
                self.delay = remaining as u8;
                return true;
            }
            self.delay = 0;
        }
        false
    }

    fn advance_current(
        &mut self,
        ticks: u32,
        exit_requested: bool,
        sounds: &mut SoundDispatcher<'_>,
        progress: &mut u8,
        reverse: bool,
    ) -> anyhow::Result<Option<u32>> {
        if self.ended || self.waiting_for_delay(ticks) {
            return Ok(None);
        }
        let step = if reverse { -1 } else { 1 };
        let mut skip = 0;
        if ticks != 0 {
            if self.countdown == 0 {
                self.command = self.command.wrapping_add(step);
                *progress = progress.wrapping_add_signed(step as i8);
                self.translation_suppressed = false;
                self.countdown = self.speed;
                skip = ticks - 1;
            }
            self.countdown = self.countdown.wrapping_sub(1);
        }
        let mut visited = BTreeSet::new();
        loop {
            if reverse {
                self.exit_reached = false;
                self.translation_suppressed = false;
                if self.command < 1 {
                    if self.sound_requested {
                        self.dispatch_sound(sounds, AnimationSound::Stop)?;
                        self.sound_requested = false;
                    }
                    self.ended = true;
                    return Ok(None);
                }
            }
            anyhow::ensure!(
                visited.insert((self.command, self.loops)),
                "{} command {}: control cycle has no displayed frame",
                self.context,
                self.command
            );
            let command = usize::try_from(self.command)
                .ok()
                .and_then(|index| self.commands.get(index))
                .with_context(|| {
                    format!("{} command {}: outside group", self.context, self.command)
                })?;
            match *command {
                Command::Frame(reference) => {
                    if skip == 0 {
                        return Ok(Some(reference));
                    }
                    skip -= 1;
                    *progress = progress.wrapping_add_signed(step as i8);
                    self.command = self.command.wrapping_add(step);
                    self.translation_suppressed = false;
                    visited.clear();
                }
                Command::End => {
                    if self.sound_requested {
                        self.dispatch_sound(sounds, AnimationSound::Stop)?;
                        self.sound_requested = false;
                    }
                    self.ended = true;
                    return Ok(None);
                }
                Command::Previous(target) => {
                    if self.loops != 0 {
                        self.loops -= 1;
                        if self.loops == 0 {
                            self.command = self.command.wrapping_add(1);
                            continue;
                        }
                    }
                    if reverse && self.loops == 0 {
                        self.command = self.command.wrapping_sub(1);
                    } else {
                        self.command = target as i16;
                        self.translation_suppressed = false;
                    }
                }
                Command::Future(target) => {
                    // A type-two group ignores a pending default, even after its exit marker is set.
                    let exits = (exit_requested || self.exit_reached)
                        && !(self.type_two && self.pending_default);
                    if reverse && self.loops == 0 {
                        self.command = self.command.wrapping_sub(1);
                    } else if self.loops == 0 && exits {
                        self.exit_reached = true;
                        self.command = target as i16;
                        self.translation_suppressed = false;
                    } else {
                        self.command = self.command.wrapping_add(1);
                    }
                }
                Command::Breakpoint => {
                    self.translation_suppressed = true;
                    self.command = self.command.wrapping_add(step);
                }
                Command::Delay(delay) => {
                    self.delay = delay;
                    self.command = self.command.wrapping_add(step);
                    if delay != 0 {
                        return Ok(None);
                    }
                }
                Command::Sound { effect, rate_shift } => {
                    anyhow::ensure!(
                        sounds.is_some(),
                        "{} command {}: opcode 4 playback unsupported",
                        self.context,
                        self.command
                    );
                    let effect = effect
                        .filter(|value| (-512..=511).contains(value))
                        .with_context(|| {
                            format!(
                                "{} command {}: sound effect outside signed ten-bit range",
                                self.context, self.command
                            )
                        })? as i16;
                    let rate_shift = rate_shift
                        .filter(|value| (-512..=511).contains(value))
                        .with_context(|| {
                            format!(
                                "{} command {}: sound rate outside signed ten-bit range",
                                self.context, self.command
                            )
                        })? as i32;
                    self.dispatch_sound(
                        sounds,
                        AnimationSound::Play {
                            effect,
                            rate_offset_hz: rate_shift * 64,
                        },
                    )?;
                    self.sound_requested = true;
                    self.command = self.command.wrapping_add(step);
                }
                Command::Event(flags) => {
                    anyhow::ensure!(
                        sounds.is_some(),
                        "{} command {}: opcode 11 playback unsupported",
                        self.context,
                        self.command
                    );
                    self.dispatch_sound(sounds, AnimationSound::Event { flags })?;
                    self.command = self.command.wrapping_add(step);
                }
                Command::Transition(..) if reverse => {
                    self.command = self.command.wrapping_sub(1);
                }
                // Mask 4 reads the actor's falling byte, which the caller supplies; taking it
                // jumps to the third operand like a taken exit jump.
                Command::Transition(4, target) => {
                    if self.motion_active {
                        self.exit_reached = true;
                        self.command = target as i16;
                        self.translation_suppressed = false;
                    } else {
                        self.command = self.command.wrapping_add(1);
                    }
                }
                Command::Transition(mask, _) => {
                    anyhow::ensure!(
                        sounds.is_some(),
                        "{} command {}: opcode 15 playback unsupported",
                        self.context,
                        self.command
                    );
                    self.dispatch_sound(sounds, AnimationSound::Transition { mask })?;
                    self.command = self.command.wrapping_add(1);
                }
                Command::Unsupported(opcode) if reverse && matches!(opcode, 6..=10 | 13..=15) => {
                    self.command = self.command.wrapping_sub(1);
                }
                Command::Unsupported(opcode) => anyhow::bail!(
                    "{} command {}: opcode {opcode} playback unsupported",
                    self.context,
                    self.command
                ),
            }
        }
    }
}
