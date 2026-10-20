//! Retained 60 Hz effect presentation phase, independent of host rendering frequency.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The original Glide swap waits for a 60 Hz retrace; SOUP adds two map units
/// per scene draw as well as nominal ticks. Normalize that visible cadence as
/// the HUD does, without limiting host rendering. Absolute time comes from the
/// session's rebased PresentationClock, so loading time is not a host epoch.
#[derive(Clone, Default, Serialize, Deserialize)]
pub(in crate::fog) struct EffectDrawClock {
    // Fractional frames in billionths: elapsed nanoseconds multiplied by 60.
    phase: u32,
    previous: Option<Duration>,
}

impl EffectDrawClock {
    pub(in crate::fog) fn validate(&self) -> Result<()> {
        ensure!(
            self.phase < 1_000_000_000,
            "effect presentation phase exceeds one frame"
        );
        Ok(())
    }

    /// Stop measuring a paused world without discarding its fractional phase.
    pub(in crate::fog) fn pause(&mut self) {
        self.previous = None;
    }

    /// Count normalized scene draws since the preceding host update.
    /// Nominal ticks belong to that update's existing slots and are never retained
    /// here. Hosts apply both movement terms before object updates, even when no
    /// image submission is due. The first draw starts immediately.
    pub(in crate::fog) fn advance(&mut self, elapsed: Duration) -> Result<u32> {
        self.validate()?;
        let frames = if let Some(previous) = self.previous {
            ensure!(
                elapsed >= previous,
                "effect presentation clock moved backwards"
            );
            let units = u128::from(self.phase) + (elapsed - previous).as_nanos() * 60;
            self.phase = (units % 1_000_000_000) as u32;
            units / 1_000_000_000
        } else {
            1
        };
        self.previous = Some(elapsed);
        Ok(frames as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draws(rate: u64) -> Result<u32> {
        let mut clock = EffectDrawClock::default();
        let mut count = 0;
        for frame in 0..=rate {
            count += clock.advance(Duration::from_nanos(frame * 1_000_000_000 / rate))?;
        }
        Ok(count)
    }

    #[test]
    fn normalized_draw_count_does_not_depend_on_host_frequency() -> Result<()> {
        for rate in [15, 30, 60, 120, 240] {
            assert_eq!(draws(rate)?, 61);
        }
        Ok(())
    }

    #[test]
    fn restore_retains_fraction_without_advancing_at_capture_time() -> Result<()> {
        let mut live = EffectDrawClock::default();
        assert_eq!(live.advance(Duration::from_secs(7))?, 1);
        let partial = Duration::from_millis(7_010);
        assert_eq!(live.advance(partial)?, 0);
        let saved = serde_json::to_value(&live)?;
        let mut restored: EffectDrawClock = serde_json::from_value(saved.clone())?;
        assert_eq!(restored.advance(partial)?, 0);
        assert_eq!(serde_json::to_value(&restored)?, saved);
        let next = Duration::from_millis(7_017);
        assert_eq!(live.advance(next)?, 1);
        assert_eq!(restored.advance(next)?, 1);
        assert_eq!(serde_json::to_value(live)?, serde_json::to_value(restored)?);
        Ok(())
    }

    #[test]
    fn pause_excludes_menu_time_and_rejections_leave_the_clock_unchanged() -> Result<()> {
        let mut clock = EffectDrawClock::default();
        clock.advance(Duration::from_secs(1))?;
        clock.advance(Duration::from_millis(1_010))?;
        let before = serde_json::to_value(&clock)?;
        assert!(clock.advance(Duration::ZERO).is_err());
        assert_eq!(serde_json::to_value(&clock)?, before);
        clock.pause();
        assert_eq!(clock.advance(Duration::from_secs(900))?, 1);
        let mut malformed = serde_json::to_value(&clock)?;
        malformed["phase"] = 1_000_000_000_u32.into();
        assert!(
            serde_json::from_value::<EffectDrawClock>(malformed)?
                .validate()
                .is_err()
        );
        Ok(())
    }
}
