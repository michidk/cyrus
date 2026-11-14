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
    acquisition: Option<Acquisition>,
    first: Option<u64>,
    previous: Option<u64>,
    held: BTreeSet<KeyCode>,
    observations: Vec<serde_json::Value>,
    deadline: std::time::Instant,
}

struct Acquisition {
    endpoints: BTreeSet<u64>,
    key: String,
    code: KeyCode,
    max_updates: u64,
    normal_frame_rate: u32,
    origin: Option<(u64, u32)>,
    previous_scale: Option<u64>,
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
            acquisition: None,
            first: None,
            previous: None,
            held,
            observations: Vec::new(),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(35),
        });
        Ok(())
    }

    pub(super) fn arm_clock(
        &mut self,
        id: u64,
        reply: Sender<Response>,
        generation: Option<u64>,
        request: cyrus_control::ClockInput,
        keys: &ButtonInput<KeyCode>,
    ) -> Result<(), String> {
        if request.normal_frame_rate == 0
            || request.endpoints.is_empty()
            || request.endpoints.len() > 256
            || request.endpoints[0] == 0
            || request.endpoints.windows(2).any(|pair| pair[0] >= pair[1])
            || !(1..=255).contains(&request.max_updates)
        {
            return Err(
                "clock acquisition needs ordered positive endpoints and 1..255 updates".into(),
            );
        }
        let code = parse_key(&request.key).ok_or_else(|| format!("unknown key {}", request.key))?;
        self.arm(
            id,
            reply,
            generation,
            vec![
                InputBoundary {
                    after_updates: 0,
                    key: Some(request.key.clone()),
                    action: Some(InputAction::Press),
                },
                InputBoundary {
                    after_updates: request.max_updates,
                    key: Some(request.key.clone()),
                    action: Some(InputAction::Release),
                },
            ],
            keys,
        )?;
        if let Some(attempt) = self.active.as_mut() {
            attempt.acquisition = Some(Acquisition {
                endpoints: request.endpoints.into_iter().collect(),
                key: request.key,
                code,
                max_updates: request.max_updates,
                normal_frame_rate: request.normal_frame_rate,
                origin: None,
                previous_scale: None,
            });
        }
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
            let mut acquired = false;
            let mut consumed_scale = None;
            if let Some(acquisition) = attempt.acquisition.as_mut() {
                let (origin, rate) = *acquisition
                    .origin
                    .get_or_insert((boundary.elapsed_scale, boundary.normal_frame_rate));
                if rate != acquisition.normal_frame_rate
                    || rate != boundary.normal_frame_rate
                    || acquisition
                        .previous_scale
                        .is_some_and(|previous| boundary.elapsed_scale <= previous)
                {
                    return Err("clock acquisition scale did not advance or rate changed".into());
                }
                acquisition.previous_scale = Some(boundary.elapsed_scale);
                let consumed = boundary
                    .elapsed_scale
                    .checked_sub(origin)
                    .ok_or("clock acquisition restarted")?;
                consumed_scale = Some(consumed);
                acquired = acquisition.endpoints.contains(&consumed);
                if !acquired
                    && (offset >= acquisition.max_updates
                        || acquisition
                            .endpoints
                            .last()
                            .is_some_and(|last| consumed > *last))
                {
                    return Err(
                        "observed counterpart endpoints unavailable within acquisition bound"
                            .into(),
                    );
                }
                // Input and observation belong to this actual boundary, not a predicted update count.
                attempt.events.clear();
                attempt.events.push_back((
                    InputBoundary {
                        after_updates: offset,
                        key: (offset == 0 || acquired).then(|| acquisition.key.clone()),
                        action: if offset == 0 {
                            Some(InputAction::Press)
                        } else if acquired {
                            Some(InputAction::Release)
                        } else {
                            None
                        },
                    },
                    (offset == 0 || acquired).then_some(acquisition.code),
                ));
            }
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
                    "consumed_scale": consumed_scale,
                }));
                attempt.events.pop_front();
            }
            Ok(if attempt.acquisition.is_some() {
                acquired
            } else {
                attempt.events.is_empty()
            })
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
    fn clock_acquisition_adapts_to_cadence_after_admission() {
        for (scales, expected) in [
            (vec![1000, 1512, 1950, 2334], Some(1334)),
            (vec![1000, 1512, 1950, 2400], None),
        ] {
            let (reply, responses) = std::sync::mpsc::channel();
            let mut scheduler = InputScheduler::default();
            let mut keys = ButtonInput::default();
            scheduler
                .arm_clock(
                    81,
                    reply,
                    Some(8),
                    cyrus_control::ClockInput {
                        key: "ArrowUp".into(),
                        endpoints: vec![1024, 1334],
                        normal_frame_rate: 12,
                        max_updates: 8,
                    },
                    &keys,
                )
                .unwrap();
            for (index, scale) in scales.into_iter().enumerate() {
                scheduler.advance(
                    Some(Boundary {
                        generation: 8,
                        update: 20 + index as u64,
                        game_seconds: scale as f64 / 3072.,
                        elapsed_scale: scale,
                        normal_frame_rate: 12,
                        player: json!({}),
                    }),
                    &mut keys,
                );
                assert_eq!(keys.pressed(KeyCode::ArrowUp), index < 3);
            }
            let response = responses.try_recv().unwrap();
            assert_eq!(response.ok, expected.is_some());
            if let Some(expected) = expected {
                let result = response.result.unwrap();
                assert_eq!(result["boundaries"].as_array().unwrap().len(), 4);
                assert_eq!(result["boundaries"][3]["consumed_scale"], expected);
                assert_eq!(result["boundaries"][3]["action"], "release");
                assert_eq!(result["boundaries"][3]["after_updates"], 3);
            }
        }
    }

    #[test]
    fn clock_acquisition_rejects_invalid_requests_without_taking_input() {
        for (endpoints, max_updates, rate) in [
            (vec![], 8, 12),
            (vec![0], 8, 12),
            (vec![512, 512], 8, 12),
            (vec![1024, 512], 8, 12),
            (vec![512], 0, 12),
            (vec![512], 256, 12),
            (vec![512], 8, 0),
        ] {
            let (reply, _) = std::sync::mpsc::channel();
            let mut scheduler = InputScheduler::default();
            let keys = ButtonInput::default();
            assert!(
                scheduler
                    .arm_clock(
                        82,
                        reply,
                        Some(8),
                        cyrus_control::ClockInput {
                            key: "ArrowUp".into(),
                            endpoints,
                            normal_frame_rate: rate,
                            max_updates,
                        },
                        &keys
                    )
                    .is_err()
            );
            assert!(!scheduler.pending());
            assert!(!keys.pressed(KeyCode::ArrowUp));
        }
    }

    #[test]
    fn clock_acquisition_failure_releases_only_owned_input() {
        for (update, scale, rate) in [(21, 1000, 12), (22, 1512, 12), (21, 1512, 24)] {
            let (reply, responses) = std::sync::mpsc::channel();
            let mut scheduler = InputScheduler::default();
            let mut keys = ButtonInput::default();
            keys.press(KeyCode::KeyW);
            scheduler
                .arm_clock(
                    83,
                    reply,
                    Some(8),
                    cyrus_control::ClockInput {
                        key: "ArrowUp".into(),
                        endpoints: vec![1334],
                        normal_frame_rate: 12,
                        max_updates: 8,
                    },
                    &keys,
                )
                .unwrap();
            for (update, scale, rate) in [(20, 1000, 12), (update, scale, rate)] {
                scheduler.advance(
                    Some(Boundary {
                        generation: 8,
                        update,
                        game_seconds: scale as f64 / 3072.,
                        elapsed_scale: scale,
                        normal_frame_rate: rate,
                        player: json!({}),
                    }),
                    &mut keys,
                );
            }
            assert!(!responses.try_recv().unwrap().ok);
            assert!(!keys.pressed(KeyCode::ArrowUp));
            assert!(keys.pressed(KeyCode::KeyW));
            assert!(!scheduler.pending());
        }
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
