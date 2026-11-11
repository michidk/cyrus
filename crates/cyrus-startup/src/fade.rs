//! Black overlay fades around new-game world entry, from preservation's
//! `docs/engine/startup.md` ("World loading screen"): each whole nominal tick
//! changes the overlay by 12 out of 256. The world keeps updating under the
//! final fade-in; the overlay only covers presentation.
use bevy::prelude::*;

/// Installed loading or world-entry black overlay. Query its `BackgroundColor` for opacity.
/// The transition removes itself when a fade-in becomes clear.
#[derive(Component)]
pub struct Fade {
    elapsed: f32,
    /// Installed rates: whole nominal ticks per second, and the slowest measured
    /// frame rate, which caps one update at `normal / min` ticks.
    clock: cyrus_assets::script::ClockSettings,
    fading_in: bool,
    world_update: Option<u64>,
    world_ticks: u32,
}

/// Overlay opacity after `elapsed` seconds. The first update already applies one step.
pub(crate) fn opacity(elapsed: f32, rate: u32, fading_in: bool) -> f32 {
    let ticks = (elapsed * rate as f32) as u32 + 1;
    let step = (12 * ticks).min(256);
    (if fading_in { 256 - step } else { step }) as f32 / 256.0
}

fn world_opacity(ticks: u32) -> f32 {
    (256 - ticks.saturating_mul(12).min(256)) as f32 / 256.0
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
                world_update: None,
                world_ticks: 0,
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
        self.world_update = Some(0);
        self.world_ticks = 0;
    }
}

pub(crate) fn advance(
    mut commands: Commands,
    time: Res<Time>,
    state: Option<Res<State<cyrus_game::AppState>>>,
    actors: Option<Res<cyrus_script::script::ScriptActors>>,
    camera: Option<Res<cyrus_script::script::CameraFrameState>>,
    sample: Option<Res<cyrus_script::script::WorldClockSample>>,
    mut fades: Query<(Entity, &mut Fade, &mut BackgroundColor)>,
) {
    for (entity, mut fade, mut color) in &mut fades {
        if fade.world_update.is_some()
            && state
                .as_ref()
                .is_none_or(|state| *state.get() != cyrus_game::AppState::InGame)
        {
            commands.entity(entity).despawn();
            continue;
        }
        let delta = if state
            .as_ref()
            .is_some_and(|state| *state.get() == cyrus_game::AppState::InGame)
            && fade.world_update.is_some()
        {
            // Asset preparation can take host updates after Loading ends. Only a completed
            // world update with a published camera can expose the first gameplay view.
            let world = actors.as_ref().and_then(|actors| actors.current.as_ref());
            let step = world.and_then(|world| {
                let update = world.ordinary_updates();
                if fade.world_update == Some(update)
                    || camera
                        .as_ref()
                        .is_none_or(|camera| camera.published.is_none())
                {
                    return None;
                }
                let step = sample.as_ref()?.presentation_step(world.generation())?;
                fade.world_update = Some(update);
                Some(step)
            });
            let Some(step) = step else { continue };
            fade.world_ticks = fade.world_ticks.saturating_add(step.nominal_ticks);
            0.0
        } else {
            time.delta_secs()
                .min(1.0 / fade.clock.min_frame_rate as f32)
        };
        fade.elapsed += delta;
        let opacity = if fade.world_update.is_some() {
            world_opacity(fade.world_ticks)
        } else {
            opacity(fade.elapsed, fade.clock.normal_frame_rate, fade.fading_in)
        };
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
    fn first_world_fade_waits_for_gameplay_publication() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .insert_state(cyrus_game::AppState::InGame)
            .init_resource::<cyrus_script::script::CameraFrameState>()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs(1),
            ))
            .add_systems(Update, advance);
        let overlay = app
            .world_mut()
            .spawn((
                Fade {
                    elapsed: 0.0,
                    clock: cyrus_assets::script::ClockSettings {
                        normal_frame_rate: 12,
                        min_frame_rate: 6,
                        max_frame_rate: 60,
                        use_smooth_fps: false,
                        use_smooth_divisor: false,
                    },
                    fading_in: true,
                    world_update: Some(0),
                    world_ticks: 0,
                },
                BackgroundColor(Color::BLACK),
            ))
            .id();
        for _ in 0..30 {
            app.update();
        }
        assert!(
            app.world().get_entity(overlay).is_ok(),
            "world entry cleared while gameplay camera was unavailable"
        );
        assert_eq!(
            app.world()
                .get::<BackgroundColor>(overlay)
                .unwrap()
                .0
                .alpha(),
            1.0,
            "unpublished world must remain covered"
        );
    }

    #[test]
    fn world_fade_starts_black_and_consumes_whole_game_ticks() {
        assert_eq!(world_opacity(0), 1.0);
        assert_eq!(world_opacity(1), 244.0 / 256.0);
        assert_eq!(world_opacity(4), 208.0 / 256.0);
        assert_eq!(world_opacity(22), 0.0);
    }

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
