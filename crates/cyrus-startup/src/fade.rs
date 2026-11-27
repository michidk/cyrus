//! Black overlay fades around new-game world entry, from preservation's
//! `docs/engine/startup.md` ("World loading screen"). World-entry steps use the
//! measured nominal carry before warmup overrides gameplay timing, and restart
//! warmup after drawing. The world keeps updating under the final fade-in.
use bevy::prelude::*;

/// Installed loading or world-entry black overlay. Query its `BackgroundColor` for opacity.
/// The transition removes itself when a fade-in becomes clear.
#[derive(Component)]
pub struct Fade {
    elapsed: f64,
    // The spawn already publishes step one; its preceding host interval is not fade time.
    first_update: bool,
    /// Installed rates: whole nominal ticks per second, and the slowest measured
    /// frame rate, which caps one update at `normal / min` ticks.
    clock: cyrus_assets::script::ClockSettings,
    fading_in: bool,
    world_update: Option<u64>,
    world_ticks: u32,
}

/// Overlay opacity after `elapsed` seconds. Creation already applies one step.
pub(crate) fn opacity(elapsed: f64, rate: u32, fading_in: bool) -> f32 {
    let ticks = (elapsed * rate as f64) as u32 + 1;
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
                first_update: true,
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
            Visibility::Inherited,
            GlobalZIndex(i32::MAX),
        ))
        .id()
}

impl Fade {
    fn advance_world(
        &mut self,
        update: u64,
        world_loop_frame: u32,
        measured_nominal_ticks: u32,
        clock: &mut cyrus_script::clock::SimulationClock,
    ) {
        if self.world_update == Some(update) {
            return;
        }
        self.world_update = Some(update);
        if world_loop_frame as i32 <= 1 {
            return;
        }
        self.world_ticks = self.world_ticks.saturating_add(measured_nominal_ticks | 1);
        clock.restart_warmup();
    }

    /// True while loading artwork is being uncovered.
    pub(crate) fn fading_in(&self) -> bool {
        self.fading_in
    }

    /// True once a fade-out is fully black.
    pub(crate) fn black(&self) -> bool {
        !self.fading_in && opacity(self.elapsed, self.clock.normal_frame_rate, false) >= 1.0
    }

    /// Turns a finished fade-out into the following fade-in, keeping the same overlay node.
    pub(crate) fn reverse(&mut self) {
        self.elapsed = 0.0;
        self.first_update = false;
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
    (sample, mut clock): (
        Option<Res<cyrus_script::script::WorldClockSample>>,
        Option<ResMut<cyrus_script::clock::SimulationClock>>,
    ),
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
        if fade.first_update {
            fade.first_update = false;
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
                sample.as_ref()?.presentation_step(world.generation())?;
                Some((update, world.world_loop_frame()))
            });
            let (Some((update, frame)), Some(clock)) = (step, clock.as_deref_mut()) else {
                continue;
            };
            let Some(ticks) = clock.measured_nominal_ticks() else {
                continue;
            };
            fade.advance_world(update, frame, ticks, clock);
            0.0
        } else {
            time.delta_secs_f64()
                .min(1.0 / fade.clock.min_frame_rate as f64)
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
    fn world_fade_restarts_warmup_through_the_clearing_update() {
        use cyrus_script::clock::SimulationClock;
        let settings = cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 60,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        };
        let mut clock = SimulationClock::new(settings).unwrap();
        let mut fade = Fade {
            elapsed: 0.0,
            first_update: false,
            clock: settings,
            fading_in: true,
            world_update: Some(0),
            world_ticks: 0,
        };
        clock.advance(1);
        fade.advance_world(1, 1, 1, &mut clock);
        assert_eq!(fade.world_ticks, 0, "first ordinary body remains black");
        assert_eq!(
            serde_json::to_value(clock.snapshot()).unwrap()["warmup"],
            11
        );
        for update in 2..=23 {
            clock.advance(update as u32);
            let mut expected = serde_json::to_value(clock.snapshot()).unwrap();
            expected["warmup"] = 12.into();
            fade.advance_world(update, update as u32, 1, &mut clock);
            assert_eq!(
                serde_json::to_value(clock.snapshot()).unwrap(),
                expected,
                "fade resets only warmup, including its clearing update"
            );
            assert_eq!(
                world_opacity(fade.world_ticks),
                world_opacity((update - 1) as u32)
            );
        }
        assert_eq!(world_opacity(fade.world_ticks), 0.0);
    }

    #[test]
    fn restored_world_fade_uses_the_retained_loop_counter() {
        let settings = cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 60,
            use_smooth_fps: false,
            use_smooth_divisor: false,
        };
        let mut clock = cyrus_script::clock::SimulationClock::new(settings).unwrap();
        clock.advance(1);
        let mut fade = Fade {
            elapsed: 0.0,
            first_update: false,
            clock: settings,
            fading_in: true,
            world_update: Some(0),
            world_ticks: 0,
        };
        // Restoring resets the diagnostic count, not the game's retained loop counter.
        fade.advance_world(1, 784, 1, &mut clock);
        assert_eq!(world_opacity(fade.world_ticks), 244.0 / 256.0);
        assert_eq!(
            serde_json::to_value(clock.snapshot()).unwrap()["warmup"],
            12
        );
    }

    #[test]
    fn world_fade_uses_odd_nominal_ticks_and_ignores_duplicate_updates() {
        let settings = cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 60,
            use_smooth_fps: false,
            use_smooth_divisor: false,
        };
        let mut fade = Fade {
            elapsed: 0.0,
            first_update: false,
            clock: settings,
            fading_in: true,
            world_update: Some(1),
            world_ticks: 0,
        };
        let mut clock = cyrus_script::clock::SimulationClock::new(settings).unwrap();
        for (update, measured_ticks, expected) in [(2, 0, 1), (2, 0, 1), (3, 2, 4), (4, 3, 7)] {
            fade.advance_world(update, update as u32, measured_ticks, &mut clock);
            assert_eq!(fade.world_ticks, expected);
        }
    }

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
        let mut clock =
            cyrus_script::clock::SimulationClock::new(cyrus_assets::script::ClockSettings {
                normal_frame_rate: 12,
                min_frame_rate: 6,
                max_frame_rate: 60,
                use_smooth_fps: false,
                use_smooth_divisor: false,
            })
            .unwrap();
        clock.advance(1);
        let before = serde_json::to_value(clock.snapshot()).unwrap();
        app.insert_resource(clock);
        let overlay = app
            .world_mut()
            .spawn((
                Fade {
                    elapsed: 0.0,
                    first_update: true,
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
        assert_eq!(
            serde_json::to_value(
                app.world()
                    .resource::<cyrus_script::clock::SimulationClock>()
                    .snapshot()
            )
            .unwrap(),
            before,
            "unpublished host updates must not restart clock warmup"
        );
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
