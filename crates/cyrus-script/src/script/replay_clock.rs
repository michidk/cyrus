//! Recorded cache refreshes at independently observed nonplayer decision boundaries.
use super::ScriptActorId;
use crate::clock::SimulationClock;
use anyhow::{Context, Result, ensure};
use std::cell::RefCell;

/// Synchronous decision-entry sample provider. Each invocation copies its sample
/// before receiver-state gates and retains it throughout that decision.
pub trait DecisionClock: std::fmt::Debug {
    fn sample(&self, actor: ScriptActorId) -> Result<u32>;
}

/// One observed calculation and the complete ordered decision dispatch for a body.
/// Refreshes change the same raw cache/baseline without advancing timing. The host
/// validates semantic receiver identity before creating these generation-bound IDs.
/// The supported refresh positions follow preservation's decision-cache lifetime;
/// other clock consumers require independent evidence and are not represented here.
#[derive(Clone, Debug)]
pub struct RecordedClockTimeline {
    pub stored_sample: u32,
    pub decisions: Vec<ScriptActorId>,
    pub before_decisions: Vec<u32>,
    pub after_decisions: Vec<u32>,
}

#[derive(Debug)]
struct State<'a> {
    clock: &'a mut SimulationClock,
    next: usize,
    failure: Option<String>,
}

/// A borrowed clock for one traversal. Final refreshes apply only after the full
/// expected dispatch was consumed. Rejections retain actual partial execution;
/// they never roll back gameplay, fabricate an invocation or redirect an actor.
#[derive(Debug)]
pub struct RecordedDecisionClock<'a> {
    timeline: &'a RecordedClockTimeline,
    state: RefCell<State<'a>>,
}

impl<'a> RecordedDecisionClock<'a> {
    pub fn new(
        clock: &'a mut SimulationClock,
        timeline: &'a RecordedClockTimeline,
    ) -> Result<Self> {
        let first = timeline
            .decisions
            .first()
            .context("recorded clock requires decision entries")?;
        ensure!(
            timeline
                .decisions
                .iter()
                .all(|actor| actor.generation == first.generation),
            "recorded clock decision generations differ"
        );
        ensure!(
            clock.bios_sample() == timeline.stored_sample,
            "recorded clock measurement sample mismatch: expected {}, current {}",
            timeline.stored_sample,
            clock.bios_sample()
        );
        Ok(Self {
            timeline,
            state: RefCell::new(State {
                clock,
                next: 0,
                failure: None,
            }),
        })
    }

    /// Complete the native traversal before its stopped-body acknowledgement.
    /// Actual per-entry observations establish equivalence to recorded decisions;
    /// this hook does not assert ordering of unobserved later engine operations.
    pub fn finish(self) -> Result<u32> {
        let state = self.state.into_inner();
        if let Some(error) = state.failure {
            anyhow::bail!(error);
        }
        ensure!(
            state.next == self.timeline.decisions.len(),
            "recorded clock consumed {} of {} decision entries",
            state.next,
            self.timeline.decisions.len()
        );
        for sample in &self.timeline.after_decisions {
            state.clock.rebase(*sample);
        }
        Ok(state.clock.bios_sample())
    }
}

impl DecisionClock for RecordedDecisionClock<'_> {
    fn sample(&self, actor: ScriptActorId) -> Result<u32> {
        let mut state = self.state.borrow_mut();
        if let Some(error) = &state.failure {
            anyhow::bail!(error.clone());
        }
        if self.timeline.decisions.get(state.next) != Some(&actor) {
            let error = format!(
                "recorded clock decision {} expected {:?}, measured {:?}",
                state.next + 1,
                self.timeline.decisions.get(state.next),
                actor
            );
            state.failure = Some(error.clone());
            anyhow::bail!(error);
        }
        if state.next == 0 {
            for sample in &self.timeline.before_decisions {
                state.clock.rebase(*sample);
            }
        }
        state.next += 1;
        Ok(state.clock.bios_sample())
    }
}
