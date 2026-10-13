//! Shared nominal clock arithmetic; callers supply original-clock samples and invocation boundaries.
use bevy::prelude::*;
use cyrus_assets::script::ClockSettings;

/// Convert a host monotonic duration to the standard PC BIOS timer units.
/// Uses the same timer frequency as preservation's `docs/engine/water.md`.
pub fn bios_tick(elapsed: std::time::Duration) -> u32 {
    (elapsed.as_nanos() * 1_193_182 / (65_536 * 1_000_000_000u128)) as u32
}

/// Timing consumed by one complete actor update, including zero-tick updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockStep {
    /// Movement scale in units of 1/256 nominal tick.
    pub frame_scale: u32,
    /// Whole animation and script ticks for this invocation.
    pub nominal_ticks: u32,
    /// Whole increments of the separate 1/32 nominal-tick counter.
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
    /// Installed inputs remain fixed while one game's timing state is retained.
    pub fn settings(&self) -> ClockSettings {
        self.settings
    }

    /// Last exposed timing, retained across checkpoint restoration until advance.
    pub fn step(&self) -> ClockStep {
        self.step
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
        clock.restart_warmup();
        *entered = Some(world.generation());
    }
}
