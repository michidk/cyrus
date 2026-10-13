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
    pub fn draw(&mut self) -> i32 {
        self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
        ((self.state >> 16) & 32767) as i32
    }

    /// Consume one draw and apply a full-width bit mask, including zero or negative masks.
    pub fn quick_rnd(&mut self, mask: i32) -> i32 {
        self.draw() & mask
    }

    /// Mask then reduce a draw to an exclusive positive bound; this is not unbiased sampling.
    /// Invalid bounds fail before consuming state instead of entering the original's loop.
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

    #[test]
    fn game_plugin_initializes_once_and_preserves_a_supplied_state() {
        use bevy::prelude::*;
        for supplied in [None, Some(42)] {
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin);
            if let Some(state) = supplied {
                app.insert_resource(EngineRandom::from_state(state));
            }
            app.add_plugins(crate::GamePlugin);
            assert_eq!(
                app.world().resource::<EngineRandom>().state(),
                supplied.unwrap_or(1)
            );
        }
    }
}
