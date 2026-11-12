//! Diagnostic input delivery at admitted ordinary simulation boundaries.
use super::*;
use cyrus_control::InputBoundary;
use std::collections::BTreeSet;
use std::sync::mpsc::Sender;

#[derive(Resource, Default)]
pub(super) struct InputScheduler {
    active: Option<Attempt>,
}

struct Attempt {
    id: u64,
    reply: Sender<Response>,
    generation: u64,
    events: std::collections::VecDeque<(InputBoundary, Option<KeyCode>)>,
    first: Option<u64>,
    previous: Option<u64>,
    held: BTreeSet<KeyCode>,
    observations: Vec<serde_json::Value>,
    deadline: std::time::Instant,
}

struct Boundary {
    generation: u64,
    update: u64,
    game_seconds: f64,
    elapsed_scale: u64,
    normal_frame_rate: u32,
    player: serde_json::Value,
}

impl InputScheduler {
    pub(super) fn arm(
        &mut self,
        id: u64,
        reply: Sender<Response>,
        generation: Option<u64>,
        events: Vec<InputBoundary>,
        keys: &ButtonInput<KeyCode>,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err("an input schedule is already pending".into());
        }
        let generation = generation.ok_or("ordinary input boundary unavailable: no world")?;
        if events.is_empty() || events.len() > 256 || events[0].after_updates != 0 {
            return Err("input schedule needs 1..256 events starting at offset zero".into());
        }
        let mut parsed = std::collections::VecDeque::new();
        let mut held = BTreeSet::new();
        let mut previous = 0;
        for event in events {
            let key = match (&event.key, event.action) {
                (Some(key), Some(_)) => {
                    Some(parse_key(key).ok_or_else(|| format!("unknown key {key}"))?)
                }
                (None, None) => None,
                _ => return Err("boundary requires both key and action, or neither".into()),
            };
            if event.after_updates < previous || key.is_some_and(|key| keys.pressed(key)) {
                return Err(
                    "input boundaries must be ordered and scheduled keys initially released".into(),
                );
            }
            previous = event.after_updates;
            match (key, event.action) {
                (None, None) => {}
                (Some(key), Some(InputAction::Press)) if held.insert(key) => {}
                (Some(key), Some(InputAction::Release)) if held.remove(&key) => {}
                _ => {
                    return Err(
                        "scheduled input needs balanced explicit press/release events".into(),
                    );
                }
            }
            parsed.push_back((event, key));
        }
        if !held.is_empty() {
            return Err("input schedule must release every pressed key".into());
        }
        self.active = Some(Attempt {
            id,
            reply,
            generation,
            events: parsed,
            first: None,
            previous: None,
            held,
            observations: Vec::new(),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(35),
        });
        Ok(())
    }

    pub(super) fn pending(&self) -> bool {
        self.active.is_some()
    }

    fn advance(&mut self, boundary: Option<Boundary>, keys: &mut ButtonInput<KeyCode>) {
        let Some(attempt) = self.active.as_mut() else {
            return;
        };
        let result = (|| -> Result<bool, String> {
            let boundary = boundary.ok_or("ordinary input boundary unavailable")?;
            if boundary.generation != attempt.generation
                || std::time::Instant::now() > attempt.deadline
            {
                return Err(
                    "ordinary input boundary generation changed or deadline expired".into(),
                );
            }
            if attempt
                .previous
                .is_some_and(|previous| previous.checked_add(1) != Some(boundary.update))
            {
                return Err("requested ordinary input boundary was skipped or repeated".into());
            }
            attempt.previous = Some(boundary.update);
            let first = *attempt.first.get_or_insert(boundary.update);
            let offset = boundary
                .update
                .checked_sub(first)
                .ok_or("ordinary update counter restarted")?;
            while let Some((event, key)) = attempt.events.front() {
                if event.after_updates > offset {
                    break;
                }
                if event.after_updates < offset {
                    return Err("requested ordinary input boundary was skipped".into());
                }
                if let Some(key) = key {
                    if event.action == Some(InputAction::Press) {
                        keys.press(*key);
                        attempt.held.insert(*key);
                    } else {
                        keys.release(*key);
                        attempt.held.remove(key);
                    }
                }
                attempt.observations.push(json!({
                    "key": event.key, "action": event.action, "after_updates": offset,
                    "update": boundary.update, "generation": boundary.generation,
                    "game_seconds": boundary.game_seconds, "player": boundary.player,
                    "elapsed_scale": boundary.elapsed_scale, "normal_frame_rate": boundary.normal_frame_rate,
                }));
                attempt.events.pop_front();
            }
            Ok(attempt.events.is_empty())
        })();
        if matches!(result, Ok(false)) {
            return;
        }
        let Some(attempt) = self.active.take() else {
            return;
        };
        for key in attempt.held {
            keys.release(key);
        }
        let response = match result {
            Ok(_) => Response::ok(attempt.id, json!({"boundaries":attempt.observations})),
            Err(error) => Response::err(
                attempt.id,
                format!(
                    "{error}; observed boundaries: {}",
                    json!(attempt.observations)
                ),
            ),
        };
        let _ = attempt.reply.send(response);
    }
}

// The clock has sampled this invocation, but its input and simulation have not run.
fn boundary_scale(clock: &cyrus_script::clock::SimulationClock) -> Option<u64> {
    clock
        .elapsed_frame_scale()
        .checked_sub(u64::from(clock.step().frame_scale))
}

fn observe(
    mut context: cyrus_script::script::OrdinaryContext,
    clock: Option<Res<cyrus_script::clock::SimulationClock>>,
) -> Option<Boundary> {
    if !context.models_prepared() {
        return None;
    }
    let clock = clock.as_deref()?;
    let elapsed_scale = boundary_scale(clock)?;
    let normal_frame_rate = clock.settings().normal_frame_rate;
    let seconds = elapsed_scale as f64 / 256. / f64::from(normal_frame_rate);
    context.with_world_services(|world, _| {
        let player = world.selected_player()?.index;
        Some(Boundary {
            generation: world.generation(),
            update: world.ordinary_updates().checked_add(1)?,
            game_seconds: seconds,
            elapsed_scale,
            normal_frame_rate,
            player: world.inspect(player, 1),
        })
    })?
}

pub(super) fn apply(world: &mut World) {
    if world.resource::<InputScheduler>().active.is_none() {
        return;
    }
    let boundary = world.run_system_cached(observe).ok().flatten();
    world.resource_scope(|world, mut scheduler: Mut<InputScheduler>| {
        scheduler.advance(boundary, &mut world.resource_mut::<ButtonInput<KeyCode>>());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events() -> Vec<InputBoundary> {
        serde_json::from_value(json!([
            {"after_updates":0,"key":"ArrowUp","action":"press"},
            {"after_updates":2,"key":"ArrowUp","action":"release"}
        ]))
        .unwrap()
    }

    #[test]
    fn boundary_clock_excludes_the_step_whose_input_has_not_been_sampled() {
        let mut clock =
            cyrus_script::clock::SimulationClock::new(cyrus_assets::script::ClockSettings {
                normal_frame_rate: 12,
                min_frame_rate: 6,
                max_frame_rate: 300,
                use_smooth_fps: false,
                use_smooth_divisor: false,
            })
            .unwrap();
        let mut scales = BTreeSet::new();
        let mut tick = 0;
        for update in 0..40 {
            let before = clock.elapsed_frame_scale();
            tick += if update % 2 == 0 { 1 } else { 36 };
            scales.insert(clock.advance(tick).frame_scale);
            assert_eq!(boundary_scale(&clock), Some(before));
        }
        assert!(scales.len() > 1, "exercise changing update durations");
    }

    #[test]
    fn public_input_contract_samples_press_and_release_at_exact_updates() {
        let (reply, responses) = std::sync::mpsc::channel();
        let mut scheduler = InputScheduler::default();
        let mut keys = ButtonInput::default();
        scheduler.arm(71, reply, Some(8), events(), &keys).unwrap();
        use cyrus_script::player::input::{Action, Input, PlayerBindings};
        let bindings: PlayerBindings = [(Action::Forward, Input::try_from(72).unwrap())]
            .into_iter()
            .collect();
        for update in 20..=22 {
            scheduler.advance(
                Some(Boundary {
                    generation: 8,
                    update,
                    game_seconds: update as f64 / 12.0,
                    elapsed_scale: update * 256,
                    normal_frame_rate: 12,
                    player: json!({}),
                }),
                &mut keys,
            );
            assert_eq!(bindings.sample(&keys).forward, update < 22);
        }
        let response = responses.try_recv().unwrap();
        assert!(response.ok);
        assert_eq!(response.id, 71);
        let rows = response.result.unwrap()["boundaries"].clone();
        assert_eq!(rows[0]["update"], 20);
        assert_eq!(rows[1]["update"], 22);
    }

    #[test]
    fn observation_only_boundaries_preserve_input_and_report_clock() {
        let (reply, responses) = std::sync::mpsc::channel();
        let mut scheduler = InputScheduler::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::ArrowUp);
        let events = serde_json::from_value(json!([
            {"after_updates":0}, {"after_updates":1}
        ]))
        .unwrap();
        scheduler.arm(73, reply, Some(8), events, &keys).unwrap();
        for update in 20..=21 {
            scheduler.advance(
                Some(Boundary {
                    generation: 8,
                    update,
                    game_seconds: update as f64 / 12.,
                    elapsed_scale: update * 256,
                    normal_frame_rate: 12,
                    player: json!({}),
                }),
                &mut keys,
            );
            assert!(keys.pressed(KeyCode::ArrowUp));
        }
        let rows = responses.try_recv().unwrap().result.unwrap();
        assert_eq!(rows["boundaries"][1]["game_seconds"], 21. / 12.);
        assert_eq!(rows["boundaries"][1]["elapsed_scale"], 21 * 256);
        assert_eq!(rows["boundaries"][1]["normal_frame_rate"], 12);
    }

    #[test]
    fn invalid_schedule_never_takes_input_ownership() {
        for value in [
            json!([]),
            json!([{"after_updates":1}]),
            json!([{"after_updates":0,"key":"ArrowUp","action":"tap"}]),
            json!([{"after_updates":0,"key":"ArrowUp","action":"press"}]),
            json!([{"after_updates":0,"key":"ArrowUp"}]),
        ] {
            let (reply, _) = std::sync::mpsc::channel();
            let mut scheduler = InputScheduler::default();
            assert!(
                scheduler
                    .arm(
                        74,
                        reply,
                        Some(8),
                        serde_json::from_value(value).unwrap(),
                        &ButtonInput::default()
                    )
                    .is_err()
            );
            assert!(!scheduler.pending());
        }
    }

    #[test]
    fn unavailable_or_skipped_boundary_releases_only_owned_keys_and_rejects() {
        for boundary in [
            None,
            Some(Boundary {
                generation: 9,
                update: 21,
                game_seconds: 2.,
                elapsed_scale: 6144,
                normal_frame_rate: 12,
                player: json!({}),
            }),
            Some(Boundary {
                generation: 8,
                update: 20,
                game_seconds: 2.,
                elapsed_scale: 6144,
                normal_frame_rate: 12,
                player: json!({}),
            }),
            Some(Boundary {
                generation: 8,
                update: 19,
                game_seconds: 2.,
                elapsed_scale: 6144,
                normal_frame_rate: 12,
                player: json!({}),
            }),
            Some(Boundary {
                generation: 8,
                update: 24,
                game_seconds: 2.,
                elapsed_scale: 6144,
                normal_frame_rate: 12,
                player: json!({}),
            }),
        ] {
            let (reply, responses) = std::sync::mpsc::channel();
            let mut scheduler = InputScheduler::default();
            let mut keys = ButtonInput::default();
            keys.press(KeyCode::KeyW);
            scheduler.arm(72, reply, Some(8), events(), &keys).unwrap();
            scheduler.advance(
                Some(Boundary {
                    generation: 8,
                    update: 20,
                    game_seconds: 1.,
                    elapsed_scale: 3072,
                    normal_frame_rate: 12,
                    player: json!({}),
                }),
                &mut keys,
            );
            scheduler.advance(boundary, &mut keys);
            assert!(!responses.try_recv().unwrap().ok);
            assert!(!keys.pressed(KeyCode::ArrowUp));
            assert!(keys.pressed(KeyCode::KeyW));
        }
    }
}
