//! Shared engine random arithmetic, following preservation's SOUP random functions.
use anyhow::Result;
use bevy::prelude::Resource;

/// One stream for engine consumers, retained independently of world and actor lifetimes.
/// The stored initial state is established; complete session consumption order is not.
#[derive(Debug, Resource)]
pub struct EngineRandom {
    state: u32,
}

impl Default for EngineRandom {
    fn default() -> Self {
        Self::from_state(1)
    }
}

impl EngineRandom {
    /// Resume the exact unsigned generator state, without consuming a draw.
    pub fn from_state(state: u32) -> Self {
        Self { state }
    }

    pub fn state(&self) -> u32 {
        self.state
    }

    /// Consume one draw in the inclusive range 0 through 32767.
    #[track_caller]
    pub fn draw(&mut self) -> i32 {
        let previous = self.state;
        self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
        let caller = std::panic::Location::caller();
        bevy::log::trace!(
            target: "cyrus_game::random",
            previous,
            next = self.state,
            file = caller.file(),
            line = caller.line(),
            "engine random draw"
        );
        ((self.state >> 16) & 32767) as i32
    }

    /// Consume one draw and apply a full-width bit mask, including zero or negative masks.
    #[track_caller]
    pub fn quick_rnd(&mut self, mask: i32) -> i32 {
        self.draw() & mask
    }

    /// Mask then reduce a draw to an exclusive positive bound; this is not unbiased sampling.
    /// Invalid bounds fail before consuming state instead of entering the original's loop.
    #[track_caller]
    pub fn rnd(&mut self, bound: i32) -> Result<i32> {
        anyhow::ensure!(bound > 0, "Rnd requires a positive bound, got {bound}");
        let bound = bound as u32;
        let mask = bound.next_power_of_two() - 1;
        Ok(((self.draw() as u32 & mask) % bound) as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_trace_identifies_consumers_through_bounded_and_masked_calls() {
        use bevy::log::tracing::{Event, Subscriber, field::Visit, subscriber::with_default};
        use bevy::log::tracing_subscriber::{Layer, Registry, layer::SubscriberExt};
        use std::collections::BTreeMap;
        use std::sync::{Arc, Mutex};

        #[derive(Default)]
        struct Fields(BTreeMap<String, String>);
        impl Visit for Fields {
            fn record_debug(
                &mut self,
                field: &bevy::log::tracing::field::Field,
                value: &dyn std::fmt::Debug,
            ) {
                self.0.insert(field.name().into(), format!("{value:?}"));
            }
        }
        struct Draws(Arc<Mutex<Vec<Fields>>>);
        impl<S: Subscriber> Layer<S> for Draws {
            fn on_event(
                &self,
                event: &Event<'_>,
                _: bevy::log::tracing_subscriber::layer::Context<'_, S>,
            ) {
                if event.metadata().target() == "cyrus_game::random" {
                    let mut fields = Fields::default();
                    event.record(&mut fields);
                    self.0.lock().unwrap().push(fields);
                }
            }
        }
        let draws = Arc::new(Mutex::new(Vec::new()));
        let locations = with_default(Registry::default().with(Draws(draws.clone())), || {
            let mut random = EngineRandom::default();
            let direct = line!() + 1;
            random.draw();
            let bounded = line!() + 1;
            random.rnd(3).unwrap();
            let masked = line!() + 1;
            random.quick_rnd(7);
            assert!(random.rnd(0).is_err());
            assert_eq!(random.state(), 662824084);
            [direct, bounded, masked]
        });
        let draws = draws.lock().unwrap();
        assert_eq!(
            draws.len(),
            3,
            "invalid bounds must not draw or emit a draw"
        );
        for ((draw, line), (previous, next)) in draws.iter().zip(locations).zip([
            (1u32, 1103527590u32),
            (1103527590, 2524885223),
            (2524885223, 662824084),
        ]) {
            assert_eq!(draw.0["file"], format!("{:?}", file!()));
            assert_eq!(draw.0["line"], line.to_string());
            assert_eq!(draw.0["previous"], previous.to_string());
            assert_eq!(draw.0["next"], next.to_string());
        }
    }

    #[test]
    fn sequential_calls_match_original_results_and_state() {
        let mut random = EngineRandom::default();
        for (quick, operand, value, state) in [
            (true, 127, 70, 1103527590),
            (false, 3, 2, 2524885223),
            (false, 5, 1, 662824084),
            (true, -1, 17515, 3295386429),
            (false, 32769, 31051, 4182499122),
            (false, 1, 0, 2516284547),
            (false, 16384, 6626, 3655513600),
            (true, 0, 0, 2633739833),
            (true, -1, 16212, 3210001534),
        ] {
            let actual = if quick {
                random.quick_rnd(operand)
            } else {
                random.rnd(operand).unwrap()
            };
            assert_eq!((actual, random.state()), (value, state));
        }
    }

    #[test]
    fn invalid_bounds_preserve_state_and_largest_positive_bound_is_supported() {
        let mut random = EngineRandom::default();
        for bound in [0, -1, i32::MIN] {
            assert!(random.rnd(bound).is_err());
            assert_eq!(random.state(), 1);
        }
        assert_eq!(random.rnd(i32::MAX).unwrap(), 16838);
        let mut restored = EngineRandom::from_state(random.state());
        assert_eq!(restored.draw(), 5758);
    }
}
