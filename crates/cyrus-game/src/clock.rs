//! Shared nominal clock arithmetic; callers supply original-clock samples and invocation boundaries.
use bevy::prelude::*;
use cyrus_assets::script::ClockSettings;
use serde::{Deserialize, Serialize};

/// Convert a host monotonic duration to the standard PC BIOS timer units.
/// Uses the same timer frequency as preservation's `docs/engine/water.md`.
pub fn bios_tick(elapsed: std::time::Duration) -> u32 {
    (elapsed.as_nanos() * 1_193_182 / (65_536 * 1_000_000_000u128)) as u32
}

/// Timing consumed by one complete actor update, including zero-tick updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockStep {
    /// Movement scale in units of 1/256 nominal tick.
    pub frame_scale: u32,
    /// Whole animation and script ticks for this invocation.
    pub nominal_ticks: u32,
    /// Whole increments of the finer counter, eight units per nominal tick.
    pub fine_ticks: u32,
}

/// Timing restored after the book menu or a blocking resource operation.
///
/// Measurement remainder, smoothed FPS and warmup continue outside this snapshot.
/// Follows preservation's `docs/engine/player.md#fps-derived-frame-scale`.
#[derive(Debug)]
pub struct ClockCheckpoint {
    frames: u32,
    measured: u32,
    next_scale: u32,
    nominal_fraction: u32,
    fine_fraction: u32,
}

/// Complete Cyrus clock continuation, distinct from a partial menu checkpoint.
///
/// Retains settings, exposed timing, measurement and fractional state following
/// preservation's `docs/formats/SAVEGAME.md#general-state-and-clock`.
/// The host epoch is deliberately excluded; restoration requires a fresh BIOS
/// baseline. This is not an original SAV record or its accumulator history.
/// Deserialized values must pass `SimulationClock::from_snapshot` before use.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClockSnapshot {
    version: u32,
    settings: ClockSettings,
    step: ClockStep,
    measurement_intervals: u32,
    remainder: i32,
    frames: u32,
    measured: u32,
    smoothed: u32,
    next_scale: u32,
    nominal_fraction: u32,
    fine_fraction: u32,
    warmup: u8,
}

/// FPS-derived state, independent of rendering and platform clock acquisition.
///
/// Follows preservation's `docs/engine/player.md#fps-derived-frame-scale`.
/// Construct from validated installed settings. `advance` takes the BIOS-clock
/// counter sampled for this invocation, not elapsed milliseconds or render frames.
/// Reset starts the documented twelve-invocation warmup while measurement continues.
#[derive(Clone, Debug, Resource)]
pub struct SimulationClock {
    settings: ClockSettings,
    step: ClockStep,
    measurement_intervals: u32,
    baseline: u32,
    remainder: i32,
    frames: u32,
    measured: u32,
    smoothed: u32,
    next_scale: u32,
    nominal_fraction: u32,
    fine_fraction: u32,
    warmup: u8,
}

impl SimulationClock {
    /// Capture all retained simulation timing without advancing or rebasing it.
    pub fn snapshot(&self) -> ClockSnapshot {
        ClockSnapshot {
            version: 2,
            settings: self.settings,
            step: self.step,
            measurement_intervals: self.measurement_intervals,
            remainder: self.remainder,
            frames: self.frames,
            measured: self.measured,
            smoothed: self.smoothed,
            next_scale: self.next_scale,
            nominal_fraction: self.nominal_fraction,
            fine_fraction: self.fine_fraction,
            warmup: self.warmup,
        }
    }

    /// Validate a complete continuation and return a replacement clock.
    ///
    /// Saved settings take precedence over installation defaults. `bios_tick` is
    /// the caller's current host baseline; time spent loading contributes no
    /// synthetic update. Exposed timing remains unchanged until `advance`.
    /// This does not establish a safe whole-game save/restore boundary.
    pub fn from_snapshot(snapshot: ClockSnapshot, bios_tick: u32) -> anyhow::Result<Self> {
        use anyhow::ensure;
        ensure!(
            snapshot.version == 2,
            "clock snapshot version {} is unsupported",
            snapshot.version
        );
        anyhow::Context::context(snapshot.settings.validate(), "clock snapshot settings")?;
        ensure!(
            (0..18).contains(&snapshot.remainder),
            "clock snapshot remainder {} is outside 0..18",
            snapshot.remainder
        );
        ensure!(
            snapshot.measurement_intervals <= i32::MAX as u32 / 18,
            "clock snapshot measurement_intervals exceeds signed remainder bound"
        );
        ensure!(
            snapshot.warmup <= 12,
            "clock snapshot warmup {} exceeds 12",
            snapshot.warmup
        );
        ensure!(
            snapshot.nominal_fraction < 256,
            "clock snapshot nominal_fraction {} exceeds 255",
            snapshot.nominal_fraction
        );
        ensure!(
            snapshot.fine_fraction < 32,
            "clock snapshot fine_fraction {} exceeds 31",
            snapshot.fine_fraction
        );
        // Construction starts at 256; every later target is bounded by these settings.
        let max_scale =
            (snapshot.settings.normal_frame_rate * 256 / snapshot.settings.min_frame_rate).max(256);
        ensure!(
            snapshot.next_scale <= max_scale,
            "clock snapshot next_scale {} exceeds {max_scale}",
            snapshot.next_scale
        );
        ensure!(
            snapshot.step.frame_scale <= max_scale,
            "clock snapshot step.frame_scale {} exceeds {max_scale}",
            snapshot.step.frame_scale
        );
        ensure!(
            snapshot.step.nominal_ticks <= (max_scale + 255) >> 8,
            "clock snapshot step.nominal_ticks {} exceeds scale bound",
            snapshot.step.nominal_ticks
        );
        ensure!(
            snapshot.step.fine_ticks <= (max_scale + 31) >> 5,
            "clock snapshot step.fine_ticks {} exceeds scale bound",
            snapshot.step.fine_ticks
        );
        Ok(Self {
            settings: snapshot.settings,
            step: snapshot.step,
            measurement_intervals: snapshot.measurement_intervals,
            baseline: bios_tick,
            remainder: snapshot.remainder,
            frames: snapshot.frames,
            measured: snapshot.measured,
            smoothed: snapshot.smoothed,
            next_scale: snapshot.next_scale,
            nominal_fraction: snapshot.nominal_fraction,
            fine_fraction: snapshot.fine_fraction,
            warmup: snapshot.warmup,
        })
    }

    /// Installed inputs remain fixed while one game's timing state is retained.
    pub fn settings(&self) -> ClockSettings {
        self.settings
    }

    /// Last exposed timing, retained across checkpoint restoration until advance.
    pub fn step(&self) -> ClockStep {
        self.step
    }

    /// Completed 18-BIOS-tick measurement intervals from the last advance.
    /// Independent of nominal ticks, including during warmup. The session owner
    /// applies these to idle timing only in the admitted normal clock mode.
    pub fn measurement_intervals(&self) -> u32 {
        self.measurement_intervals
    }

    pub fn new(settings: ClockSettings) -> anyhow::Result<Self> {
        settings.validate()?;
        Ok(Self {
            settings,
            step: ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            measurement_intervals: 0,
            baseline: 0,
            remainder: 0,
            frames: 0,
            measured: 12,
            smoothed: 12,
            next_scale: 256,
            nominal_fraction: 0,
            fine_fraction: 0,
            warmup: 12,
        })
    }

    /// Excludes a suspension during which this clock received no invocations.
    /// Frame counts, fractional accumulators and the tick remainder are retained.
    pub fn rebase(&mut self, bios_tick: u32) {
        self.baseline = bios_tick;
    }

    /// Save the normal BIOS-path timing state while allowing further invocations.
    /// Every completed `advance` has equal sample and measurement baselines.
    pub fn checkpoint(&self) -> ClockCheckpoint {
        ClockCheckpoint {
            frames: self.frames,
            measured: self.measured,
            next_scale: self.next_scale,
            nominal_fraction: self.nominal_fraction,
            fine_fraction: self.fine_fraction,
        }
    }

    /// Restore saved counters and fractions, excluding time since the last sample.
    /// Menu updates retain their smoothing, remainder and warmup effects. The
    /// caller's last exposed `ClockStep` remains valid until the next `advance`.
    pub fn restore(&mut self, checkpoint: ClockCheckpoint, bios_tick: u32) {
        self.frames = checkpoint.frames;
        self.measured = checkpoint.measured;
        self.next_scale = checkpoint.next_scale;
        self.nominal_fraction = checkpoint.nominal_fraction;
        self.fine_fraction = checkpoint.fine_fraction;
        self.rebase(bios_tick);
    }

    /// Restart world-entry warmup without resetting accumulated timing state.
    pub fn restart_warmup(&mut self) {
        self.warmup = 12;
    }

    /// Advance once at a caller-established game-update boundary.
    ///
    /// The input uses the original BIOS tick units. A backwards clock sample
    /// contributes a signed delta; a negative accumulated remainder clamps to zero.
    /// This method neither establishes update eligibility nor schedules actors.
    pub fn advance(&mut self, bios_tick: u32) -> ClockStep {
        self.frames = self.frames.wrapping_add(1);
        self.remainder = self
            .remainder
            .wrapping_add(bios_tick.wrapping_sub(self.baseline) as i32)
            .max(0);
        self.baseline = bios_tick;
        let windows = self.remainder as u32 / 18;
        self.measurement_intervals = windows;
        if let Some(measured) = self.frames.checked_div(windows) {
            self.measured = measured;
            self.frames = 0;
            self.remainder %= 18;
        }
        let difference = self.measured.abs_diff(self.smoothed);
        let adjustment = if difference >= 5 {
            difference / 2 + 1
        } else {
            1
        };
        if self.measured > self.smoothed {
            self.smoothed += adjustment;
        } else if self.measured < self.smoothed {
            self.smoothed -= adjustment;
        }
        let warmup = self.warmup != 0;
        let fps = if self.settings.use_smooth_fps && !warmup {
            self.smoothed
        } else {
            self.measured
        }
        .clamp(self.settings.min_frame_rate, self.settings.max_frame_rate);
        let target = self.settings.normal_frame_rate * 256 / fps;
        if self.settings.use_smooth_divisor && !warmup {
            if self.next_scale < target {
                self.next_scale += 1;
            } else if self.next_scale > target {
                self.next_scale -= 1;
            }
            if self.next_scale > target {
                self.next_scale = target;
            } else if self.next_scale < target {
                self.next_scale += match target - self.next_scale {
                    1..=5 => 1,
                    6..=10 => 5,
                    11..=20 => 10,
                    _ => 20,
                };
            }
        } else {
            self.next_scale = target;
        }
        let nominal = self.nominal_fraction + self.next_scale;
        let fine = self.fine_fraction + self.next_scale;
        self.nominal_fraction = nominal & 255;
        self.fine_fraction = fine & 31;
        self.step = if warmup {
            self.warmup -= 1;
            ClockStep {
                frame_scale: 256,
                nominal_ticks: 1,
                fine_ticks: 8,
            }
        } else {
            ClockStep {
                frame_scale: self.next_scale,
                nominal_ticks: nominal >> 8,
                fine_ticks: fine >> 5,
            }
        };
        self.step
    }
}

pub(crate) fn enter_world(
    state: Option<Res<State<crate::AppState>>>,
    actors: Res<crate::script::ScriptActors>,
    clock: Option<ResMut<SimulationClock>>,
    mut entered: Local<Option<u64>>,
) {
    if !state.is_some_and(|state| *state.get() == crate::AppState::InGame) {
        return;
    }
    let Some(world) = actors.current.as_ref() else {
        return;
    };
    if *entered == Some(world.generation()) {
        return;
    }
    if let Some(mut clock) = clock {
        if actors.restored_generation != Some(world.generation()) {
            clock.restart_warmup();
        }
        *entered = Some(world.generation());
    }
}
