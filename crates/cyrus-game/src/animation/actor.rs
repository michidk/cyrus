//! Request arbitration and default fallback from preservation's engine/animation.md.
use super::sequence::SoundDispatcher;
use super::{AnimationSound, Animations, FrameSequence};
use anyhow::Context;
use std::collections::{BTreeMap, BTreeSet};

/// One actor's current, pending and explicitly supplied default group.
/// Only forward type-zero and type-one groups are supported. The caller owns eligibility,
/// default selection and each advancement's original nominal-tick input.
pub struct ActorAnimation {
    context: String,
    groups: BTreeMap<u16, serde_json::Value>,
    default: u16,
    current: Option<(u16, FrameSequence)>,
    pending: Option<(u16, u8)>,
    failure: Option<String>,
}

impl Animations {
    /// Creates inactive request state with a caller-selected default group.
    /// This does not activate a placement or infer the default from its attributes.
    pub fn actor_animation(&self, actor: &str, default: u16) -> anyhow::Result<ActorAnimation> {
        self.require_version()?;
        let context = format!("world {} actor {actor}", self.world);
        let mut groups = BTreeMap::new();
        for group in self.actor(actor)?["animation_groups"]
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
            failure: None,
        })
    }
}

impl ActorAnimation {
    /// Current group number, independent of whether its mesh frame changed.
    pub fn current_group(&self) -> Option<u16> {
        self.current.as_ref().map(|(group, _)| *group)
    }

    /// The single pending group; ordinary requests do not form a queue.
    pub fn pending_group(&self) -> Option<u16> {
        self.pending.map(|(group, _)| group)
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

    fn advance_dispatch(
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
                if let Some(reference) = sequence.advance_with_sounds(ticks, exiting, sounds)? {
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
