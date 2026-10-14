//! Ordered sound maintenance follows preservation's engine/sound.md.
use anyhow::{Result, bail};

/// Required cleanup order before ambient tasks observe a sample reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundResetStage {
    /// Release loaded RTX text/audio payloads and clear their occupied state.
    ReleaseRtx,
    /// Clear active dialogue records/text and reset their remembered samples.
    ClearDialogue,
    /// Stop every sample channel before ambient tasks replace remembered handles.
    StopSamples,
    /// Clear every loaded walk-node ownership byte, regardless of owner.
    ClearNodeReservations,
}

/// Shared world-loop timer and inhibition, distinct from book-menu presentation.
/// Initial inhibition is cleared by engine initialization. World-loop entry resets
/// only the signed nominal-tick counter. Cleanup failures stop further sampling.
#[derive(Debug, Default)]
pub struct SoundMaintenance {
    ticks: i32,
    inhibited: bool,
    reset: bool,
    failure: Option<String>,
}

impl SoundMaintenance {
    /// Restart the counter on world-loop entry without changing inhibition.
    /// This does not recover a failed cleanup or reset gameplay ownership.
    pub fn enter_world(&mut self) {
        self.ticks = 0;
    }

    /// Sample once after clock sampling and before ordinary actor updates.
    /// The host must execute each requested cleanup stage synchronously. Missing
    /// ownership is an error. A partial reset remains failed rather than being
    /// retried or reported as complete on the next update.
    pub fn advance(
        &mut self,
        nominal_ticks: u32,
        cleanup: &mut dyn FnMut(SoundResetStage) -> Result<()>,
    ) -> Result<bool> {
        if let Some(error) = &self.failure {
            bail!("{error}");
        }
        self.reset = false;
        if !self.inhibited {
            self.ticks = self.ticks.wrapping_add(nominal_ticks as i32);
        }
        if self.ticks <= 1400 {
            return Ok(false);
        }
        self.ticks = 0;
        for stage in [
            SoundResetStage::ReleaseRtx,
            SoundResetStage::ClearDialogue,
            SoundResetStage::StopSamples,
            SoundResetStage::ClearNodeReservations,
        ] {
            if let Err(error) = cleanup(stage) {
                let message = format!("sound maintenance {stage:?}: {error:#}");
                self.failure = Some(message.clone());
                bail!("{message}");
            }
            if stage == SoundResetStage::StopSamples {
                self.reset = true;
            }
        }
        Ok(true)
    }

    /// Apply the ordinary actor's menu/WaitOnDialog/LockoutPlayer inputs.
    /// An active menu or dialogue inhibits maintenance. With both inactive,
    /// nonzero lockout retains the prior inhibition rather than enabling it.
    pub fn ordinary_actor(
        &mut self,
        menu_state: u16,
        waiting_on_dialogue: bool,
        player_lockout: u16,
    ) {
        if menu_state != 0 || waiting_on_dialogue {
            self.inhibited = true;
        } else if player_lockout == 0 {
            self.inhibited = false;
        }
    }

    /// Successful current-update reset flag. A failed cleanup never supplies it.
    pub fn reset(&self) -> Result<bool> {
        if let Some(error) = &self.failure {
            bail!("{error}");
        }
        Ok(self.reset)
    }
}
