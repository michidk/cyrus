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
#[derive(Debug, Default, serde::Serialize)]
pub struct SoundMaintenance {
    ticks: i32,
    inhibited: bool,
    reset: bool,
    failure: Option<String>,
}

impl SoundMaintenance {
    pub(super) fn initialize_player(&mut self) {
        self.inhibited = false;
    }

    /// Restart the counter on world-loop entry without changing inhibition.
    /// This does not recover a failed cleanup or reset gameplay ownership.
    pub fn enter_world(&mut self) {
        self.ticks = 0;
    }

    /// Restore retained actor state after clearing shared sound services.
    /// Unlike periodic maintenance, restoration preserves walk-node reservations
    /// and sets the reset flag only after actor restoration succeeds. The next
    /// maintenance sample clears it, including while accumulation is inhibited.
    /// Follows preservation's engine/sound.md saved-world restoration order.
    pub fn restore_world<T>(
        &mut self,
        cleanup: &mut dyn FnMut(SoundResetStage) -> Result<()>,
        restore_actors: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if let Some(error) = &self.failure {
            bail!("{error}");
        }
        let result = (|| {
            for stage in [
                SoundResetStage::ReleaseRtx,
                SoundResetStage::ClearDialogue,
                SoundResetStage::StopSamples,
            ] {
                cleanup(stage)
                    .map_err(|error| anyhow::anyhow!("world restoration {stage:?}: {error:#}"))?;
            }
            restore_actors()
        })();
        match result {
            Ok(value) => {
                self.reset = true;
                Ok(value)
            }
            Err(error) => {
                let message = format!("world sound restoration: {error:#}");
                self.failure = Some(message.clone());
                bail!("{message}")
            }
        }
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

    /// Apply menu/WaitOnDialog/LockoutPlayer inputs at the player-controller stage.
    /// The caller must establish controller admission; do not sample this once
    /// per ordinary map actor. See preservation engine/sound.md.
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

impl super::ScriptWorld {
    /// Lend the session sound timer and world to synchronous host cleanup.
    /// The timer, including partial-failure state, is restored on every Result path.
    pub fn with_sound_maintenance<R>(
        &mut self,
        update: impl FnOnce(&mut Self, &mut SoundMaintenance) -> R,
    ) -> R {
        let mut maintenance = std::mem::take(&mut self.globals_mut().sound_maintenance);
        let result = update(self, &mut maintenance);
        self.globals_mut().sound_maintenance = maintenance;
        result
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct SoundMaintenanceSnapshot {
    ticks: i32,
    inhibited: bool,
    reset: bool,
    failure: Option<String>,
}
impl SoundMaintenance {
    pub(super) fn snapshot(&self) -> SoundMaintenanceSnapshot {
        SoundMaintenanceSnapshot {
            ticks: self.ticks,
            inhibited: self.inhibited,
            reset: self.reset,
            failure: self.failure.clone(),
        }
    }

    pub(super) fn from_snapshot(snapshot: SoundMaintenanceSnapshot) -> Self {
        Self {
            ticks: snapshot.ticks,
            inhibited: snapshot.inhibited,
            reset: snapshot.reset,
            failure: snapshot.failure,
        }
    }
}
