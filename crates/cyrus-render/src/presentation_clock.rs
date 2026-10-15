//! Presentation time continuation rebased independently of the host real-time epoch.
use anyhow::{Result, ensure};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Effective presentation elapsed time at a completed render boundary.
/// This is not a checkpoint of the host clock or the simulation clock.
#[derive(Clone, Serialize, Deserialize)]
pub struct PresentationClockSnapshot {
    version: u32,
    elapsed: Duration,
}

/// Restored presentation epoch used by sky and water without advancing either.
#[derive(Resource)]
pub struct PresentationClock {
    elapsed: Duration,
    host_epoch: Duration,
}

impl PresentationClock {
    /// Capture the effective elapsed time, including an earlier restored epoch.
    pub fn snapshot(clock: Option<&Self>, time: &Time<Real>) -> PresentationClockSnapshot {
        PresentationClockSnapshot {
            version: 1,
            elapsed: Self::elapsed(clock, time),
        }
    }

    /// Rebase at the supplied host instant; construction does not advance time.
    pub fn from_snapshot(snapshot: PresentationClockSnapshot, time: &Time<Real>) -> Result<Self> {
        ensure!(
            snapshot.version == 1,
            "presentation clock snapshot version unsupported"
        );
        Ok(Self {
            elapsed: snapshot.elapsed,
            host_epoch: time.elapsed(),
        })
    }

    pub(crate) fn elapsed(clock: Option<&Self>, time: &Time<Real>) -> Duration {
        clock.map_or_else(
            || time.elapsed(),
            |clock| {
                clock
                    .elapsed
                    .saturating_add(time.elapsed().saturating_sub(clock.host_epoch))
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_presentation_epoch_retains_fraction_and_advances_only_new_time() {
        let mut original = Time::<Real>::default();
        original.advance_by(Duration::new(732, 123_456_789));
        let saved = PresentationClock::snapshot(None, &original);
        let mut replacement = Time::<Real>::default();
        replacement.advance_by(Duration::from_secs(9));
        let clock = PresentationClock::from_snapshot(saved, &replacement).unwrap();
        assert_eq!(
            PresentationClock::elapsed(Some(&clock), &replacement),
            original.elapsed()
        );
        let step = Duration::from_nanos(19_876_543);
        original.advance_by(step);
        replacement.advance_by(step);
        assert_eq!(
            PresentationClock::elapsed(Some(&clock), &replacement),
            original.elapsed()
        );
        let again = PresentationClock::snapshot(Some(&clock), &replacement);
        let fresh = Time::<Real>::default();
        let resumed = PresentationClock::from_snapshot(again, &fresh).unwrap();
        assert_eq!(
            PresentationClock::elapsed(Some(&resumed), &fresh),
            original.elapsed()
        );
    }
}
