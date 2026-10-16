//! Black overlay fades around new-game world entry, from preservation's
//! `docs/engine/startup.md` ("World loading screen"): each whole nominal tick
//! changes the overlay by 12 out of 256. The world keeps updating under the
//! final fade-in; the overlay only covers presentation.
use bevy::prelude::*;

/// One running fade. Fades in remove themselves once clear.
#[derive(Component)]
pub(crate) struct Fade {
    elapsed: f32,
    /// Installed rates: whole nominal ticks per second, and the slowest measured
    /// frame rate, which caps one update at `normal / min` ticks.
    clock: cyrus_assets::script::ClockSettings,
    fading_in: bool,
}

/// Overlay opacity after `elapsed` seconds. The first update already applies one step.
pub(crate) fn opacity(elapsed: f32, rate: u32, fading_in: bool) -> f32 {
    let ticks = (elapsed * rate as f32) as u32 + 1;
    let step = (12 * ticks).min(256);
    (if fading_in { 256 - step } else { step }) as f32 / 256.0
}

pub(crate) fn spawn(
    commands: &mut Commands,
    clock: cyrus_assets::script::ClockSettings,
    fading_in: bool,
) -> Entity {
    let rate = clock.normal_frame_rate;
    commands
        .spawn((
            Fade {
                elapsed: 0.0,
                clock,
                fading_in,
            },
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::BLACK.with_alpha(opacity(0.0, rate, fading_in))),
            GlobalZIndex(i32::MAX),
        ))
        .id()
}

impl Fade {
    /// True once a fade-out is fully black.
    pub(crate) fn black(&self) -> bool {
        !self.fading_in && opacity(self.elapsed, self.clock.normal_frame_rate, false) >= 1.0
    }

    /// Turns a finished fade-out into the following fade-in, keeping the same overlay node.
    pub(crate) fn reverse(&mut self) {
        self.elapsed = 0.0;
        self.fading_in = true;
    }
}

pub(crate) fn advance(
    mut commands: Commands,
    time: Res<Time>,
    mut fades: Query<(Entity, &mut Fade, &mut BackgroundColor)>,
) {
    for (entity, mut fade, mut color) in &mut fades {
        // A long frame advances at most what the original's slowest measured rate allows.
        fade.elapsed += time
            .delta_secs()
            .min(1.0 / fade.clock.min_frame_rate as f32);
        let opacity = opacity(fade.elapsed, fade.clock.normal_frame_rate, fade.fading_in);
        color.0 = Color::BLACK.with_alpha(opacity);
        if fade.fading_in && opacity == 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_step_twelve_per_nominal_tick_over_twenty_two_ticks() {
        assert_eq!(opacity(0.0, 12, true), 244.0 / 256.0);
        assert_eq!(opacity(0.0, 12, false), 12.0 / 256.0);
        assert_eq!(opacity(1.0, 12, false), 156.0 / 256.0);
        // 22 ticks at the shipped 12 per second: about 1.8 seconds.
        assert!(opacity(20.9 / 12.0, 12, false) < 1.0);
        assert_eq!(opacity(21.0 / 12.0, 12, false), 1.0);
        assert_eq!(opacity(21.0 / 12.0, 12, true), 0.0);
    }
}
