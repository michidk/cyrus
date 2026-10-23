//! `BounceObject`: the launch task that throws an actor up and forward and hands the descent to the
//! ordinary falling controller. The rules follow the original task's observable state machine;
//! the actor's live state belongs to the caller's [`BouncePort`].
//!
//! The call takes a power (how quickly the lift decays), a forward-speed scalar and a steering
//! flag. Installed content calls it once, on the selected player, as `BounceObject(160, 40, 1)`.
use anyhow::{Context, Result};

/// Lifecycle bytes the task owns: requested-entry, rising and falling.
const ENTRY: u8 = 30;
const RISING: u8 = 31;
const FALLING: u8 = 32;
/// The animation groups the entry request waits for: weapon, torch, plain.
const ENTRY_GROUPS: [u16; 3] = [114, 66, 65];
/// Descent group a held or drawn weapon uses, and the group that keeps it.
const WEAPON_DESCENT: u16 = 113;
const WEAPON_DESCENT_KEPT: u16 = 115;

/// Arguments of one call, in script order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BounceArguments {
    /// Zero refuses the call; otherwise `256 - power` (in sixteenths of a unit) is the lift
    /// decay per nominal tick.
    pub power: i32,
    /// Forward speed scalar: the horizontal step per nominal tick is `(128 - speed) * 256 / 3`
    /// in Q8 units.
    pub speed: i32,
    /// Nonzero lets the selected player steer with the ground-turn amounts.
    pub steer: i32,
}

/// Retained task state: whether the first invocation has run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BounceState {
    pub started: bool,
}

/// The acting actor's live state. Positions and lift are Q8; `scale` values have eight
/// fractional bits (256 is one nominal tick).
pub trait BouncePort {
    fn item_system_ready(&self) -> Result<bool>;
    /// The frame scale of the invocation.
    fn frame_scale(&self) -> u32;
    fn living_state(&self) -> u8;
    fn set_living_state(&mut self, state: u8);
    /// The actor's surface value; 2 and 4 are deep water.
    fn surface(&self) -> u8;
    /// The ordinary holding-weapon predicate.
    fn holding_weapon(&mut self) -> Result<bool>;
    /// The combat record's stance bit (`0x04`), which a drawn weapon raises.
    fn combat_mode(&self) -> bool;
    fn torch_burning(&self) -> bool;
    fn attribute_32(&self) -> u8;
    /// Clear the falling, queued and sliding bytes and the first two motion-vector words.
    fn clear_motion(&mut self);
    fn falling_state(&self) -> u8;
    /// Start an ordinary fall at the current height: origin, timer and falling state one.
    fn begin_fall(&mut self);
    /// The lift speed, held in the fall-origin word while the actor rises.
    fn lift(&self) -> i32;
    fn set_lift(&mut self, lift: i32);
    fn position(&self) -> [i32; 3];
    fn set_position(&mut self, position: [i32; 3]) -> Result<()>;
    fn heading(&self) -> u16;
    fn set_heading(&mut self, heading: u16);
    fn is_selected_player(&self) -> bool;
    /// Scaled right minus scaled left ground-turn amount.
    fn turn_delta(&self, scale: u32) -> i64;
    fn current_group(&self) -> Option<u16>;
    fn request_group(&mut self, group: u16, force: bool) -> Result<()>;
}

/// Horizontal step per call before the frame scale, from the speed scalar.
pub fn forward_step(speed: i32, scale: u32) -> i32 {
    let step = ((128i32.wrapping_sub(speed)) << 8) / 3;
    step.wrapping_mul(scale as i32) >> 8
}

/// One invocation. True ends the task; false keeps it running.
pub fn step<P: BouncePort + ?Sized>(
    port: &mut P,
    state: &mut BounceState,
    arguments: BounceArguments,
) -> Result<bool> {
    let scale = port.frame_scale();
    if !state.started {
        state.started = true;
        let living = port.living_state();
        if !port.item_system_ready()?
            || living == ENTRY
            || living == RISING
            || (10..=19).contains(&living)
            || matches!(port.surface(), 2 | 4)
            || arguments.power == 0
        {
            return Ok(true);
        }
        port.clear_motion();
        port.set_living_state(ENTRY);
        let group = if port.holding_weapon()? {
            ENTRY_GROUPS[0]
        } else if port.torch_burning() {
            ENTRY_GROUPS[1]
        } else {
            ENTRY_GROUPS[2]
        };
        port.request_group(group, false)?;
    }
    if port.living_state() == ENTRY {
        let current = port
            .current_group()
            .context("BounceObject requires the actor's animation")?;
        if !ENTRY_GROUPS.contains(&current) {
            port.clear_motion();
            return Ok(false);
        }
        port.set_living_state(RISING);
        // Attribute 32 is the jump height in map units.
        port.set_lift(i32::from(port.attribute_32()) * 256);
    }
    if port.is_selected_player() && arguments.steer != 0 {
        let heading = i64::from(port.heading()) - port.turn_delta(scale);
        port.set_heading(heading.rem_euclid(2048) as u16);
    }
    let mut position = port.position();
    position = cyrus_world::transform::translate_forward(
        position,
        port.heading(),
        forward_step(arguments.speed, scale),
    )?;
    port.set_position(position)?;
    let decay = (256i32.wrapping_sub(arguments.power)) << 4;
    if port.living_state() == RISING {
        let mut position = port.position();
        position[1] = position[1].wrapping_sub(port.lift().wrapping_mul(scale as i32) >> 8);
        port.set_position(position)?;
        port.set_lift(
            port.lift()
                .wrapping_sub(decay.wrapping_mul(scale as i32) >> 8),
        );
        let weapon = port.holding_weapon()? || port.combat_mode();
        if port.lift() <= 0 {
            port.begin_fall();
            if weapon {
                if port.current_group() != Some(WEAPON_DESCENT_KEPT) {
                    port.request_group(WEAPON_DESCENT, true)?;
                }
            } else {
                port.request_group(if port.torch_burning() { 57 } else { 56 }, true)?;
            }
            port.set_living_state(FALLING);
        } else {
            port.request_group(
                if weapon {
                    112
                } else if port.torch_burning() {
                    38
                } else {
                    39
                },
                false,
            )?;
            port.clear_motion();
        }
        return Ok(false);
    }
    if port.falling_state() == 0 {
        port.set_living_state(0);
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Actor {
        item_ready: bool,
        living: u8,
        surface: u8,
        holding: bool,
        drawn: bool,
        torch: bool,
        jump_height: u8,
        falling: u8,
        lift: i32,
        position: [i32; 3],
        heading: u16,
        selected: bool,
        turn: i64,
        group: Option<u16>,
        requests: Vec<(u16, bool)>,
        cleared: usize,
        fall_started: bool,
    }

    impl BouncePort for Actor {
        fn item_system_ready(&self) -> Result<bool> {
            Ok(self.item_ready)
        }
        fn frame_scale(&self) -> u32 {
            256
        }
        fn living_state(&self) -> u8 {
            self.living
        }
        fn set_living_state(&mut self, state: u8) {
            self.living = state;
        }
        fn surface(&self) -> u8 {
            self.surface
        }
        fn holding_weapon(&mut self) -> Result<bool> {
            Ok(self.holding)
        }
        fn combat_mode(&self) -> bool {
            self.drawn
        }
        fn torch_burning(&self) -> bool {
            self.torch
        }
        fn attribute_32(&self) -> u8 {
            self.jump_height
        }
        fn clear_motion(&mut self) {
            self.cleared += 1;
            self.falling = 0;
        }
        fn falling_state(&self) -> u8 {
            self.falling
        }
        fn begin_fall(&mut self) {
            self.fall_started = true;
            self.falling = 1;
        }
        fn lift(&self) -> i32 {
            self.lift
        }
        fn set_lift(&mut self, lift: i32) {
            self.lift = lift;
        }
        fn position(&self) -> [i32; 3] {
            self.position
        }
        fn set_position(&mut self, position: [i32; 3]) -> Result<()> {
            self.position = position;
            Ok(())
        }
        fn heading(&self) -> u16 {
            self.heading
        }
        fn set_heading(&mut self, heading: u16) {
            self.heading = heading;
        }
        fn is_selected_player(&self) -> bool {
            self.selected
        }
        fn turn_delta(&self, _: u32) -> i64 {
            self.turn
        }
        fn current_group(&self) -> Option<u16> {
            self.group
        }
        fn request_group(&mut self, group: u16, force: bool) -> Result<()> {
            self.requests.push((group, force));
            Ok(())
        }
    }

    const ARGS: BounceArguments = BounceArguments {
        power: 160,
        speed: 40,
        steer: 1,
    };

    fn ready() -> Actor {
        Actor {
            item_ready: true,
            jump_height: 80,
            ..Default::default()
        }
    }

    #[test]
    fn bounce_object_refused_calls_end_without_effects() {
        for actor in [
            Actor {
                item_ready: false,
                ..ready()
            },
            Actor {
                living: 30,
                ..ready()
            },
            Actor {
                living: 31,
                ..ready()
            },
            Actor {
                living: 12,
                ..ready()
            },
            Actor {
                surface: 4,
                ..ready()
            },
        ] {
            let mut actor = actor;
            let living = actor.living;
            assert!(step(&mut actor, &mut BounceState::default(), ARGS).unwrap());
            assert!(actor.requests.is_empty());
            assert_eq!(actor.living, living);
        }
        let mut actor = ready();
        let no_power = BounceArguments { power: 0, ..ARGS };
        assert!(step(&mut actor, &mut BounceState::default(), no_power).unwrap());
        assert!(actor.requests.is_empty());
    }

    #[test]
    fn bounce_object_entry_request_picks_weapon_torch_or_plain_group() {
        for (holding, torch, group) in [(true, false, 114), (false, true, 66), (false, false, 65)] {
            let mut actor = Actor {
                holding,
                torch,
                group: Some(0),
                ..ready()
            };
            let mut state = BounceState::default();
            assert!(!step(&mut actor, &mut state, ARGS).unwrap());
            assert_eq!(actor.requests, [(group, false)]);
            assert_eq!(actor.living, ENTRY);
        }
    }

    #[test]
    fn rising_waits_for_the_entry_group_then_lifts_and_decays() {
        let mut actor = Actor {
            group: Some(0),
            ..ready()
        };
        let mut state = BounceState::default();
        assert!(!step(&mut actor, &mut state, ARGS).unwrap());
        assert_eq!(actor.living, ENTRY);
        assert_eq!(actor.position, [0; 3]);
        actor.group = Some(65);
        assert!(!step(&mut actor, &mut state, ARGS).unwrap());
        assert_eq!(actor.living, RISING);
        // Lift 80 * 256, minus the decay (256 - 160) * 16 after the first rise.
        assert_eq!(actor.position[1], -80 * 256);
        assert_eq!(actor.lift, 80 * 256 - 1536);
        assert_eq!(actor.requests.last(), Some(&(39, false)));
        // The horizontal step is (128 - 40) * 256 / 3 along the heading.
        assert_eq!(forward_step(40, 256), 7509);
    }

    #[test]
    fn the_apex_starts_the_ordinary_fall_and_landing_ends_the_task() {
        let mut actor = Actor {
            living: RISING,
            lift: 1000,
            group: Some(65),
            falling: 0,
            ..ready()
        };
        let mut state = BounceState { started: true };
        assert!(!step(&mut actor, &mut state, ARGS).unwrap());
        assert!(actor.fall_started);
        assert_eq!(actor.living, FALLING);
        assert_eq!(actor.requests, [(56, true)]);
        // Falling continues until the ordinary landing clears the falling byte.
        assert!(!step(&mut actor, &mut state, ARGS).unwrap());
        actor.falling = 0;
        assert!(step(&mut actor, &mut state, ARGS).unwrap());
        assert_eq!(actor.living, 0);
    }

    #[test]
    fn a_weapon_keeps_its_descent_group_and_the_torch_chooses_the_other_group() {
        let mut kept = Actor {
            living: RISING,
            lift: 10,
            holding: true,
            group: Some(115),
            ..ready()
        };
        step(&mut kept, &mut BounceState { started: true }, ARGS).unwrap();
        assert!(kept.requests.is_empty());
        let mut weapon = Actor {
            living: RISING,
            lift: 10,
            drawn: true,
            group: Some(1),
            ..ready()
        };
        step(&mut weapon, &mut BounceState { started: true }, ARGS).unwrap();
        assert_eq!(weapon.requests, [(113, true)]);
        let mut torch = Actor {
            living: RISING,
            lift: 10,
            torch: true,
            group: Some(1),
            ..ready()
        };
        step(&mut torch, &mut BounceState { started: true }, ARGS).unwrap();
        assert_eq!(torch.requests, [(57, true)]);
    }

    #[test]
    fn only_the_selected_player_steers() {
        let mut steered = Actor {
            living: FALLING,
            falling: 1,
            heading: 100,
            selected: true,
            turn: 30,
            ..ready()
        };
        step(&mut steered, &mut BounceState { started: true }, ARGS).unwrap();
        assert_eq!(steered.heading, 70);
        let mut other = Actor {
            selected: false,
            ..steered
        };
        other.heading = 100;
        step(&mut other, &mut BounceState { started: true }, ARGS).unwrap();
        assert_eq!(other.heading, 100);
    }
}
