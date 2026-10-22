//! Request arbitration and default fallback from preservation's engine/animation.md.
use super::sequence::{SequenceSnapshot, SoundDispatcher};
use super::{AnimationSound, Animations, FrameSequence};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One actor's current, pending and explicitly supplied default group.
/// Type-zero, type-one and type-two groups retain forward/reverse playback state. The caller owns eligibility,
/// default selection and each advancement's original nominal-tick input.
pub struct ActorAnimation {
    context: String,
    groups: BTreeMap<u16, serde_json::Value>,
    default: u16,
    current: Option<(u16, FrameSequence)>,
    pending: Option<(u16, u8)>,
    frame_progress: u8,
    reverse: bool,
    failure: Option<String>,
    /// Transient mirror of the actor's falling byte (nonzero), kept by the owner. It is not
    /// saved: the owner sets it before the next advance.
    motion_active: bool,
}

/// Saved current receiver and pending request for ordinary collision rollback.
/// The caller saves its selected model frame and remembered sound slot separately.
/// This excludes the independently retained default and sticky host failure.
pub struct ActorAnimationCheckpoint {
    context: String,
    current: Option<(u16, FrameSequence)>,
    pending: Option<(u16, u8)>,
    frame_progress: u8,
    reverse: bool,
}

/// Complete, versioned playback state with exact installed group definitions.
/// The game host saves actor/world identity, displayed frame and native sound slot
/// alongside this snapshot, then validates and binds them before committing a restore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorAnimationSnapshot {
    version: u32,
    context: String,
    groups: BTreeMap<u16, serde_json::Value>,
    default: u16,
    current: Option<(u16, SequenceSnapshot)>,
    pending: Option<(u16, u8)>,
    frame_progress: u8,
    reverse: bool,
    failure: Option<String>,
}

impl ActorAnimation {
    /// Captures the entire receiver, including default, delays, sound latch and failures.
    /// This does not advance commands or dispatch sounds, and is distinct from collision rollback.
    pub fn snapshot(&self) -> ActorAnimationSnapshot {
        ActorAnimationSnapshot {
            version: 3,
            context: self.context.clone(),
            groups: self.groups.clone(),
            default: self.default,
            current: self
                .current
                .as_ref()
                .map(|(group, sequence)| (*group, sequence.snapshot())),
            pending: self.pending,
            frame_progress: self.frame_progress,
            reverse: self.reverse,
            failure: self.failure.clone(),
        }
    }

    /// Builds a complete replacement after matching actor context and exact installed groups.
    /// No initialization requests, animation commands or sounds run. The caller restores the
    /// displayed frame and sound binding with the same validated actor/world generation.
    pub fn from_snapshot(
        data: &Animations,
        actor: &str,
        snapshot: ActorAnimationSnapshot,
    ) -> anyhow::Result<Self> {
        let context = format!("world {} actor {actor} animation restore", data.world);
        let restore = || -> anyhow::Result<Self> {
            anyhow::ensure!(
                snapshot.version == 3,
                "unsupported snapshot version {}",
                snapshot.version
            );
            let mut animation = Self::from_asset(data, actor, snapshot.default)?;
            anyhow::ensure!(
                animation.context == snapshot.context,
                "actor context mismatch"
            );
            anyhow::ensure!(
                animation.groups == snapshot.groups,
                "installed group content mismatch"
            );
            if let Some((group, _)) = snapshot.pending {
                anyhow::ensure!(
                    animation.groups.contains_key(&group),
                    "pending group {group}: absent"
                );
            }
            if let Some((group, state)) = snapshot.current {
                let metadata = animation
                    .groups
                    .get(&group)
                    .with_context(|| format!("current group {group}: absent"))?;
                animation.current = Some((
                    group,
                    FrameSequence::from_snapshot(
                        format!("{} group {group}", animation.context),
                        metadata,
                        state,
                    )?,
                ));
            }
            animation.pending = snapshot.pending;
            animation.frame_progress = snapshot.frame_progress;
            animation.reverse = snapshot.reverse;
            animation.failure = snapshot.failure;
            Ok(animation)
        };
        restore().with_context(|| context)
    }

    /// Save the post-advancement receiver without executing commands or audio.
    pub fn checkpoint(&self) -> anyhow::Result<ActorAnimationCheckpoint> {
        self.check_failure()?;
        Ok(ActorAnimationCheckpoint {
            context: self.context.clone(),
            current: self.current.clone(),
            pending: self.pending,
            frame_progress: self.frame_progress,
            reverse: self.reverse,
        })
    }

    /// Restore a checkpoint from this actor's receiver. The game host must also
    /// validate world generation and restore its frame and remembered sound slot.
    /// The current default and any sticky failure remain owned by their services.
    pub fn restore_checkpoint(
        &mut self,
        checkpoint: &ActorAnimationCheckpoint,
    ) -> anyhow::Result<()> {
        self.check_failure()?;
        anyhow::ensure!(
            self.context == checkpoint.context,
            "{}: animation checkpoint belongs to another actor",
            self.context
        );
        self.current = checkpoint.current.clone();
        self.pending = checkpoint.pending;
        self.frame_progress = checkpoint.frame_progress;
        self.reverse = checkpoint.reverse;
        Ok(())
    }

    /// Initializes an animation-table model with the two ordinary group-zero requests
    /// specified by preservation's engine/animation.md. The caller establishes the
    /// model loading path and supplies the shared nominal ticks and sound dispatcher.
    pub fn initialize(
        data: &Animations,
        actor: &str,
        ticks: u32,
        sounds: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<(Self, Option<u32>)> {
        let mut animation = Self::from_asset(data, actor, 0)?;
        let mut frame = animation.request_with_sounds(0, 0, ticks, sounds)?;
        frame = animation
            .request_with_sounds(0, 0, ticks, sounds)?
            .or(frame);
        Ok((animation, frame))
    }

    /// Creates inactive request state with a caller-selected default group.
    /// This does not activate a placement or infer the default from its attributes.
    pub fn from_asset(
        data: &Animations,
        actor: &str,
        default: u16,
    ) -> anyhow::Result<ActorAnimation> {
        data.require_version()?;
        let context = format!("world {} actor {actor}", data.world);
        let mut groups = BTreeMap::new();
        for group in data.actor(actor)?["animation_groups"]
            .as_array()
            .with_context(|| format!("{context}: groups absent"))?
        {
            let index = u16::try_from(
                group["group_index"]
                    .as_u64()
                    .with_context(|| format!("{context}: group index absent"))?,
            )
            .with_context(|| format!("{context}: group index outside u16 range"))?;
            anyhow::ensure!(
                groups.insert(index, group.clone()).is_none(),
                "{context} group {index}: duplicate group"
            );
        }
        Ok(ActorAnimation {
            context,
            groups,
            default,
            current: None,
            pending: None,
            frame_progress: 0,
            reverse: false,
            failure: None,
            motion_active: false,
        })
    }
}

impl ActorAnimation {
    /// Mirror whether the actor's falling byte is nonzero. A forward mask-4 `Transition`
    /// command reads it; the owner refreshes it whenever the byte changes and before stepping.
    pub fn set_motion_active(&mut self, active: bool) {
        self.motion_active = active;
    }

    /// Apply a caller-selected ordinary default outside shell execution.
    /// Replaces the old current default before its exit marker and separately
    /// retargets an old pending default, reusing this update's nominal ticks.
    /// Follows preservation's engine/animation.md living default selection.
    pub fn select_default_with_sounds(
        &mut self,
        group: u16,
        ticks: u32,
        sounds: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<u32>> {
        self.check_failure()?;
        let old = self.default;
        self.default = group;
        if group == old {
            return Ok(None);
        }
        let mut frame = None;
        if self
            .current
            .as_ref()
            .is_some_and(|(current, sequence)| *current == old && !sequence.exit_reached())
        {
            frame = self.force_request_with_sounds(group, 0, ticks, sounds)?;
        }
        if self.pending_group() == Some(old) {
            frame = self.request_with_sounds(group, 0, ticks, sounds)?.or(frame);
        }
        Ok(frame)
    }

    /// Apply the caller-admitted selected death override, bypassing ordinary defaults.
    /// A matching group keeps its cursor/frame/delays, cancels pending playback,
    /// clears the exit marker and replaces the finite loop count with 32.
    /// Otherwise force-starts with modifier zero and this update's nominal ticks.
    /// Follows preservation's SOUP selected-player death animation override.
    pub fn select_death_override_with_sounds(
        &mut self,
        group: u16,
        ticks: u32,
        sounds: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<u32>> {
        self.check_failure()?;
        self.default = group;
        if let Some((current, sequence)) = self.current.as_mut()
            && *current == group
        {
            self.pending = None;
            sequence.clear_exit_marker();
            sequence.set_loop_count(32);
            return Ok(None);
        }
        self.force_request_with_sounds(group, 0, ticks, sounds)
    }

    /// Set playback direction without advancing, requesting a group or changing countdowns.
    /// New group selection resets direction to forward. Follows preservation's reverse rules.
    pub fn set_reverse(&mut self, reverse: bool) -> anyhow::Result<()> {
        self.check_failure()?;
        self.reverse = reverse;
        Ok(())
    }

    /// Whether the receiver currently rejects forward-only attack admission.
    pub fn is_reversed(&self) -> bool {
        self.reverse
    }

    /// Current and pending signed groups plus the wrapping frame-progress byte.
    /// This is independent of the command cursor and displayed model frame.
    /// Follows preservation's engine/animation.md frame-wait contract.
    pub fn frame_wait_state(&self) -> anyhow::Result<(i32, i32, u8)> {
        self.check_failure()?;
        Ok((
            self.current_group()
                .map_or(-1, |group| i32::from(group as i16)),
            self.pending_group()
                .map_or(-1, |group| i32::from(group as i16)),
            self.frame_progress,
        ))
    }

    /// Signed sixteen-bit command index used by combat timing, widened to an integer.
    /// This differs from the wrapping progress byte and displayed model frame.
    /// An actor without a selected group has no cursor. Reading never advances playback.
    /// Follows preservation's engine/animation.md command-cursor contract.
    pub fn current_command_cursor(&self) -> anyhow::Result<Option<i32>> {
        self.check_failure()?;
        Ok(self
            .current
            .as_ref()
            .map(|(_, sequence)| sequence.command_cursor()))
    }

    /// Start `group` at once, skipping its first `frames` ShowFrames, unless it is already the
    /// current group before its exit marker with at least that many frames shown, in which case
    /// only the pending request clears. Returns the displayed frame reference.
    pub fn request_skipping_frames(
        &mut self,
        group: u16,
        modifier: u8,
        frames: u8,
    ) -> anyhow::Result<Option<u32>> {
        self.check_failure()?;
        if self.current.as_ref().is_some_and(|(current, sequence)| {
            *current == group && !sequence.exit_reached() && self.frame_progress >= frames
        }) {
            self.pending = None;
            return Ok(None);
        }
        let metadata = self
            .groups
            .get(&group)
            .with_context(|| format!("{} group {group}: absent", self.context))?;
        let mut sequence = FrameSequence::from_group(
            format!("{} group {group}", self.context),
            metadata,
            0,
            modifier,
        )?;
        self.clear_current(&mut None)?;
        self.pending = None;
        self.frame_progress = 0;
        self.reverse = false;
        let reference = sequence.skip_frames(frames, &mut self.frame_progress)?;
        self.current = Some((group, sequence));
        Ok(reference)
    }

    /// Number of commands in the current group's packed command table.
    pub fn current_command_count(&self) -> anyhow::Result<Option<usize>> {
        self.check_failure()?;
        Ok(self
            .current
            .as_ref()
            .map(|(_, sequence)| sequence.command_count()))
    }

    /// Assign the wrapping frame-progress byte zero, as a ledge climb does when it restarts.
    pub fn reset_frame_progress(&mut self) {
        self.frame_progress = 0;
    }

    /// Writes the animation wait byte (HoldAnimation 255, PauseAnimation value). The byte lives
    /// on the current group's sequence, so an actor without one has nothing to write.
    pub fn set_wait(&mut self, wait: u8) -> anyhow::Result<()> {
        self.check_failure()?;
        if let Some((_, sequence)) = self.current.as_mut() {
            sequence.set_wait(wait);
        }
        Ok(())
    }

    /// ReleaseAnimation: clears the wait byte only when it holds the animation.
    pub fn release_hold(&mut self) -> anyhow::Result<()> {
        self.check_failure()?;
        if let Some((_, sequence)) = self.current.as_mut() {
            sequence.release_hold();
        }
        Ok(())
    }

    /// Cancel only a matching pending group, preserving current playback and progress.
    /// Equipped-weapon sheath completion cancels group 129 this way.
    pub fn cancel_pending_group(&mut self, group: u16) -> anyhow::Result<()> {
        self.check_failure()?;
        if self.pending_group() == Some(group) {
            self.pending = None;
        }
        Ok(())
    }

    /// The caller-selected playback default, distinct from the model's attribute default.
    pub fn default_group(&self) -> u16 {
        self.default
    }

    /// Whether the current sequence reached its authored exit marker. This
    /// remains distinct from completion of the sequence's exit tail.
    pub fn exit_reached(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|(_, sequence)| sequence.exit_reached())
    }

    /// Sticky advancement or sound-dispatch failure.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// Whether an authored group exists, before request fallback rules are applied.
    pub fn has_group(&self, group: u16) -> bool {
        self.groups.contains_key(&group)
    }

    /// Current group number, independent of whether its mesh frame changed.
    pub fn current_group(&self) -> Option<u16> {
        self.current.as_ref().map(|(group, _)| *group)
    }

    /// The single pending group; ordinary requests do not form a queue.
    pub fn pending_group(&self) -> Option<u16> {
        self.pending.map(|(group, _)| group)
    }

    /// Cancel the queued group without changing current playback, its frame or sound.
    /// Combat attack starts clear this after the forced request and finite-loop assignment.
    pub fn clear_pending_request(&mut self) {
        self.pending = None;
    }

    /// The current group's BreakPoint marker for ordinary walking translation.
    pub fn translation_suppressed(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|(_, sequence)| sequence.translation_suppressed())
    }

    /// Supplies the byte loop countdown when a PushAnimation task becomes current.
    /// This changes neither speed nor the displayed frame and does not restart playback.
    pub fn set_loop_count(&mut self, count: u8) -> anyhow::Result<()> {
        self.check_failure()?;
        let (_, sequence) = self
            .current
            .as_mut()
            .with_context(|| format!("{}: no current group for loop count", self.context))?;
        sequence.set_loop_count(count);
        Ok(())
    }

    /// Makes an ordinary request, advancing immediately only when no group is current.
    /// `ticks` is the existing shared increment, reused without adding a synthetic tick.
    /// A returned frame reference must be resolved through the source Animations asset.
    pub fn request(&mut self, group: u16, modifier: u8, ticks: u32) -> anyhow::Result<Option<u32>> {
        self.request_dispatch(group, modifier, ticks, &mut None)
    }

    /// Makes an ordinary request with synchronous authored sound dispatch.
    /// Use the same handler for subsequent advances and requests; it retains the last handle.
    pub fn request_with_sounds(
        &mut self,
        group: u16,
        modifier: u8,
        ticks: u32,
        sounds: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<u32>> {
        self.request_dispatch(group, modifier, ticks, &mut Some(sounds))
    }

    pub(super) fn request_dispatch(
        &mut self,
        group: u16,
        modifier: u8,
        ticks: u32,
        sounds: &mut SoundDispatcher<'_>,
    ) -> anyhow::Result<Option<u32>> {
        self.check_failure()?;
        if self.current_group() == Some(self.default) {
            if group == self.default {
                return Ok(None);
            }
            self.clear_current(sounds)?;
        }
        if self.current.is_none()
            || self.pending.is_none()
            || self
                .pending_group()
                .is_some_and(|pending| pending <= group || Some(pending) == self.current_group())
        {
            self.select_pending(group, modifier);
            if self.current.is_none() && self.pending.is_some() {
                return self.advance_dispatch(ticks, sounds);
            }
        }
        Ok(None)
    }

    /// Forces a replacement without ordinary numeric priority. A matching current group
    /// before its exit marker only cancels the pending request, preserving its cursor.
    pub fn force_request(
        &mut self,
        group: u16,
        modifier: u8,
        ticks: u32,
    ) -> anyhow::Result<Option<u32>> {
        self.force_request_dispatch(group, modifier, ticks, &mut None)
    }

    /// Forces a replacement with the actor's sound handler. Replacement forgets the
    /// prior handle without stopping its channel; immediate cues reuse `ticks`.
    pub fn force_request_with_sounds(
        &mut self,
        group: u16,
        modifier: u8,
        ticks: u32,
        sounds: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<u32>> {
        self.force_request_dispatch(group, modifier, ticks, &mut Some(sounds))
    }

    fn force_request_dispatch(
        &mut self,
        group: u16,
        modifier: u8,
        ticks: u32,
        sounds: &mut SoundDispatcher<'_>,
    ) -> anyhow::Result<Option<u32>> {
        self.check_failure()?;
        if self
            .current
            .as_ref()
            .is_some_and(|(current, sequence)| *current == group && !sequence.exit_reached())
        {
            self.pending = None;
            return Ok(None);
        }
        self.clear_current(sounds)?;
        self.select_pending(group, modifier);
        if self.pending.is_some() {
            self.advance_dispatch(ticks, sounds)
        } else {
            Ok(None)
        }
    }

    fn select_pending(&mut self, requested: u16, modifier: u8) {
        self.pending = if self.groups.contains_key(&requested) {
            Some((requested, modifier))
        } else if self.groups.contains_key(&self.default) {
            Some((self.default, modifier))
        } else {
            None
        };
    }

    fn check_failure(&self) -> anyhow::Result<()> {
        if let Some(message) = &self.failure {
            anyhow::bail!("{message}");
        }
        Ok(())
    }

    fn clear_current(&mut self, sounds: &mut SoundDispatcher<'_>) -> anyhow::Result<()> {
        if let Some((_, sequence)) = &mut self.current {
            sequence.forget_sound(sounds)?;
        }
        self.current = None;
        Ok(())
    }

    /// Advances an eligible actor, selecting pending/default groups after End in the same
    /// call. A replacement receives the same tick input. Unsupported state stops with an
    /// error; `None` keeps the prior mesh pose while waiting or without an available group.
    pub fn advance(&mut self, ticks: u32) -> anyhow::Result<Option<u32>> {
        self.advance_dispatch(ticks, &mut None)
    }

    /// Advances with synchronous PlaySound/End/replacement handling from preservation.
    /// The handler owns its last channel handle and must apply source surface and positional rules.
    pub fn advance_with_sounds(
        &mut self,
        ticks: u32,
        sounds: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<u32>> {
        self.advance_dispatch(ticks, &mut Some(sounds))
    }

    pub(super) fn advance_dispatch(
        &mut self,
        ticks: u32,
        sounds: &mut SoundDispatcher<'_>,
    ) -> anyhow::Result<Option<u32>> {
        self.check_failure()?;
        let result = self.advance_current(ticks, sounds);
        if let Err(error) = &result {
            self.failure = Some(error.to_string());
        }
        result
    }

    fn advance_current(
        &mut self,
        ticks: u32,
        sounds: &mut SoundDispatcher<'_>,
    ) -> anyhow::Result<Option<u32>> {
        let mut visited = BTreeSet::new();
        loop {
            if let Some((_, sequence)) = &mut self.current {
                if sequence.waiting_for_delay(ticks) {
                    return Ok(None);
                }
                if sequence.replace_after_exit()
                    && self
                        .pending
                        .is_some_and(|(pending, _)| pending != self.default)
                {
                    self.clear_current(sounds)?;
                }
            }
            if self.current.is_none() {
                if self.pending.is_none() {
                    self.select_pending(self.default, 0);
                }
                let Some((group, modifier)) = self.pending.take() else {
                    return Ok(None);
                };
                anyhow::ensure!(
                    visited.insert(group),
                    "{} group {group}: replacement cycle has no displayed frame",
                    self.context
                );
                let metadata = self
                    .groups
                    .get(&group)
                    .with_context(|| format!("{} group {group}: absent", self.context))?;
                self.frame_progress = 0;
                self.reverse = false;
                self.current = Some((
                    group,
                    FrameSequence::from_group(
                        format!("{} group {group}", self.context),
                        metadata,
                        0,
                        modifier,
                    )?,
                ));
            }
            if let Some((group, sequence)) = &mut self.current {
                let exiting = self.pending.is_some_and(|(pending, _)| pending != *group);
                sequence.set_pending_default(
                    self.pending
                        .is_some_and(|(pending, _)| pending == self.default),
                );
                sequence.set_motion_active(self.motion_active);
                if let Some(reference) = sequence.advance_with_progress(
                    ticks,
                    exiting,
                    sounds,
                    &mut self.frame_progress,
                    self.reverse,
                )? {
                    return Ok(Some(reference));
                }
                if !sequence.is_finished() {
                    return Ok(None);
                }
            }
            self.current = None;
        }
    }
}
