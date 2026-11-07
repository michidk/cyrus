//! `BounceObject`: the launch task that throws an actor up and forward and hands the descent to the
//! ordinary falling controller. The rules follow the original task's observable state machine;
//! the actor's live state belongs to the caller's [`BouncePort`].
//!
//! The call takes a power (how quickly the lift decays), a forward-speed scalar and a steering
//! flag. Installed content calls it once, on the selected player, as `BounceObject(160, 40, 1)`.
use crate::script::JumpMotion;
use crate::script::LivingState;
use anyhow::{Context, Result};
use cyrus_assets::surface::Surface;
use cyrus_soup::AnimationGroup;
use cyrus_world::heading::wrap;

/// The animation groups the entry request waits for: weapon, torch, plain.
const ENTRY_GROUPS: [AnimationGroup; 3] = [
    AnimationGroup::FIGHT_LAND,
    AnimationGroup::LAND_TORCH,
    AnimationGroup::LAND,
];

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
    fn living_state(&self) -> LivingState;
    fn set_living_state(&mut self, state: LivingState);
    /// The actor's surface value; 2 and 4 are deep water.
    fn surface(&self) -> Surface;
    /// The ordinary holding-weapon predicate.
    fn holding_weapon(&mut self) -> Result<bool>;
    /// The combat record's stance bit (`0x04`), which a drawn weapon raises.
    fn combat_mode(&self) -> bool;
    fn torch_burning(&self) -> bool;
    fn jump_height(&self) -> u8;
    /// Clear the falling, queued and sliding bytes and the first two motion-vector words.
    fn clear_motion(&mut self);
    fn falling_state(&self) -> JumpMotion;
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
    fn current_group(&self) -> Option<AnimationGroup>;
    fn request_group(&mut self, group: AnimationGroup, force: bool) -> Result<()>;
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
            || living == LivingState::BOUNCE_ENTRY
            || living == LivingState::BOUNCE_RISING
            || living.is_ledge()
            || port.surface().is_deep_water()
            || arguments.power == 0
        {
            return Ok(true);
        }
        port.clear_motion();
        port.set_living_state(LivingState::BOUNCE_ENTRY);
        let group = if port.holding_weapon()? {
            ENTRY_GROUPS[0]
        } else if port.torch_burning() {
            ENTRY_GROUPS[1]
        } else {
            ENTRY_GROUPS[2]
        };
        port.request_group(group, false)?;
    }
    if port.living_state() == LivingState::BOUNCE_ENTRY {
        let current = port
            .current_group()
            .context("BounceObject requires the actor's animation")?;
        if !ENTRY_GROUPS.contains(&current) {
            port.clear_motion();
            return Ok(false);
        }
        port.set_living_state(LivingState::BOUNCE_RISING);
        // `attr_jump_height` is the jump height in map units.
        port.set_lift(i32::from(port.jump_height()) * 256);
    }
    if glide(
        port,
        scale,
        arguments.steer != 0,
        arguments.speed,
        arguments.power,
        (LivingState::BOUNCE_RISING, LivingState::BOUNCE_FALLING),
    )?
    .is_some()
    {
        return Ok(false);
    }
    if port.falling_state() == JumpMotion::NONE {
        port.set_living_state(LivingState::ORDINARY);
        return Ok(true);
    }
    Ok(false)
}

/// The steering, forward step and lift shared by `BounceObject` and the actor bounce. While the
/// actor is in the `rising` state the lift moves it up and either keeps it rising (`Some(true)`) or
/// starts the fall and enters the `falling` state (`Some(false)`); in any other state it returns
/// `None`.
fn glide<P: BouncePort + ?Sized>(
    port: &mut P,
    scale: u32,
    steer: bool,
    speed: i32,
    power: i32,
    (rising, falling): (LivingState, LivingState),
) -> Result<Option<bool>> {
    if port.is_selected_player() && steer {
        let heading = i64::from(port.heading()) - port.turn_delta(scale);
        port.set_heading(wrap(heading) as u16);
    }
    let mut position = port.position();
    position = cyrus_world::transform::translate_forward(
        position,
        port.heading(),
        forward_step(speed, scale),
    )?;
    port.set_position(position)?;
    let decay = (256i32.wrapping_sub(power)) << 4;
    if port.living_state() != rising {
        return Ok(None);
    }
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
            if port.current_group() != Some(AnimationGroup::FIGHT_FALL_ATTACK) {
                port.request_group(AnimationGroup::FIGHT_FALL, true)?;
            }
        } else {
            port.request_group(
                if port.torch_burning() {
                    AnimationGroup::FALL_TORCH
                } else {
                    AnimationGroup::FALL
                },
                true,
            )?;
        }
        port.set_living_state(falling);
        return Ok(Some(false));
    }
    port.request_group(
        if weapon {
            AnimationGroup::FIGHT_JUMP
        } else if port.torch_burning() {
            AnimationGroup::JUMP_TORCH
        } else {
            AnimationGroup::JUMP
        },
        false,
    )?;
    port.clear_motion();
    Ok(Some(true))
}

/// What the ground probe's bounce handler reads from the support actor of an actor in the
/// actor-bounce states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorBounceSupport {
    /// The support's bounce power (attribute 98): how quickly the lift decays.
    pub power: i32,
    /// The support's bounce move (attribute 99): the forward speed scalar.
    pub speed: i32,
    /// The support's bounce turn (attribute 100) is nonzero: the selected player steers.
    pub steer: bool,
    /// The support still shows the first half of its landing group, which holds state 27.
    pub waiting: bool,
}

/// Whether the support still holds an actor in state 27: its current group is the landing group
/// (`MISC1`, group 10), the animation's exit marker (`anim.stop_flag`) is clear, and the group's
/// command cursor is below half of its command count (signed, truncating toward zero).
pub fn actor_bounce_waiting(
    current_group: Option<AnimationGroup>,
    exit_reached: bool,
    cursor: Option<i32>,
    command_count: Option<usize>,
) -> bool {
    current_group == Some(AnimationGroup::MISC1)
        && !exit_reached
        && matches!(
            (cursor, command_count),
            (Some(cursor), Some(count)) if cursor < (count as i32) / 2
        )
}

/// One update of an actor in the actor-bounce states (27 to 29) whose support is valid. True when
/// the update is handled and the ground probe ends; false lets the probe continue. `clear_support`
/// empties the actor's support reference when the actor has landed.
pub fn actor_bounce_step<P: BouncePort + ?Sized>(
    port: &mut P,
    support: ActorBounceSupport,
    clear_support: impl FnOnce(&mut P),
) -> Result<bool> {
    let scale = port.frame_scale();
    if port.living_state() == LivingState::LANDED_ON_ACTOR {
        if support.waiting {
            port.clear_motion();
            return Ok(true);
        }
        port.set_living_state(LivingState::BOUNCING_FROM_ACTOR);
        // `attr_jump_height` times 16 times the first vertical factor, 16: the jump height in map
        // units as a Q8 lift.
        port.set_lift(i32::from(port.jump_height()) * 256);
    }
    if glide(
        port,
        scale,
        support.steer,
        support.speed,
        support.power,
        (
            LivingState::BOUNCING_FROM_ACTOR,
            LivingState::FALLING_FROM_ACTOR,
        ),
    )? == Some(true)
    {
        return Ok(true);
    }
    if port.falling_state() == JumpMotion::NONE {
        port.set_living_state(LivingState::ORDINARY);
        clear_support(port);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Actor {
        item_ready: bool,
        living: LivingState,
        surface: cyrus_assets::surface::Surface,
        holding: bool,
        drawn: bool,
        torch: bool,
        jump_height: u8,
        falling: JumpMotion,
        lift: i32,
        position: [i32; 3],
        heading: u16,
        selected: bool,
        turn: i64,
        group: Option<AnimationGroup>,
        requests: Vec<(AnimationGroup, bool)>,
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
        fn living_state(&self) -> LivingState {
            self.living
        }
        fn set_living_state(&mut self, state: LivingState) {
            self.living = state;
        }
        fn surface(&self) -> Surface {
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
        fn jump_height(&self) -> u8 {
            self.jump_height
        }
        fn clear_motion(&mut self) {
            self.cleared += 1;
            self.falling = JumpMotion::NONE;
        }
        fn falling_state(&self) -> JumpMotion {
            self.falling
        }
        fn begin_fall(&mut self) {
            self.fall_started = true;
            self.falling = JumpMotion::STANDING;
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
        fn current_group(&self) -> Option<AnimationGroup> {
            self.group
        }
        fn request_group(&mut self, group: AnimationGroup, force: bool) -> Result<()> {
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
                living: LivingState::BOUNCE_ENTRY,
                ..ready()
            },
            Actor {
                living: LivingState::BOUNCE_RISING,
                ..ready()
            },
            Actor {
                living: LivingState::TURNING_TO_LOW_LEDGE,
                ..ready()
            },
            Actor {
                surface: Surface::SCAPEDEEPWATER,
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
        for (holding, torch, group) in [
            (true, false, AnimationGroup::FIGHT_LAND),
            (false, true, AnimationGroup::LAND_TORCH),
            (false, false, AnimationGroup::LAND),
        ] {
            let mut actor = Actor {
                holding,
                torch,
                group: Some(AnimationGroup::PANIC),
                ..ready()
            };
            let mut state = BounceState::default();
            assert!(!step(&mut actor, &mut state, ARGS).unwrap());
            assert_eq!(actor.requests, [(group, false)]);
            assert_eq!(actor.living, LivingState::BOUNCE_ENTRY);
        }
    }

    #[test]
    fn rising_waits_for_the_entry_group_then_lifts_and_decays() {
        let mut actor = Actor {
            group: Some(AnimationGroup::PANIC),
            ..ready()
        };
        let mut state = BounceState::default();
        assert!(!step(&mut actor, &mut state, ARGS).unwrap());
        assert_eq!(actor.living, LivingState::BOUNCE_ENTRY);
        assert_eq!(actor.position, [0; 3]);
        actor.group = Some(AnimationGroup::FIRST_LAND_GROUP);
        assert!(!step(&mut actor, &mut state, ARGS).unwrap());
        assert_eq!(actor.living, LivingState::BOUNCE_RISING);
        // Lift 80 * 256, minus the decay (256 - 160) * 16 after the first rise.
        assert_eq!(actor.position[1], -80 * 256);
        assert_eq!(actor.lift, 80 * 256 - 1536);
        assert_eq!(actor.requests.last(), Some(&(AnimationGroup::JUMP, false)));
        // The horizontal step is (128 - 40) * 256 / 3 along the heading.
        assert_eq!(forward_step(40, 256), 7509);
    }

    #[test]
    fn the_apex_starts_the_ordinary_fall_and_landing_ends_the_task() {
        let mut actor = Actor {
            living: LivingState::BOUNCE_RISING,
            lift: 1000,
            group: Some(AnimationGroup::FIRST_LAND_GROUP),
            falling: JumpMotion::NONE,
            ..ready()
        };
        let mut state = BounceState { started: true };
        assert!(!step(&mut actor, &mut state, ARGS).unwrap());
        assert!(actor.fall_started);
        assert_eq!(actor.living, LivingState::BOUNCE_FALLING);
        assert_eq!(actor.requests, [(AnimationGroup::FALL, true)]);
        // Falling continues until the ordinary landing clears the falling byte.
        assert!(!step(&mut actor, &mut state, ARGS).unwrap());
        actor.falling = JumpMotion::NONE;
        assert!(step(&mut actor, &mut state, ARGS).unwrap());
        assert_eq!(actor.living, LivingState::ORDINARY);
    }

    #[test]
    fn a_weapon_keeps_its_descent_group_and_the_torch_chooses_the_other_group() {
        let mut kept = Actor {
            living: LivingState::BOUNCE_RISING,
            lift: 10,
            holding: true,
            group: Some(AnimationGroup::FIGHT_FALL_ATTACK),
            ..ready()
        };
        step(&mut kept, &mut BounceState { started: true }, ARGS).unwrap();
        assert!(kept.requests.is_empty());
        let mut weapon = Actor {
            living: LivingState::BOUNCE_RISING,
            lift: 10,
            drawn: true,
            group: Some(AnimationGroup::IDLE1),
            ..ready()
        };
        step(&mut weapon, &mut BounceState { started: true }, ARGS).unwrap();
        assert_eq!(weapon.requests, [(AnimationGroup::FIGHT_FALL, true)]);
        let mut torch = Actor {
            living: LivingState::BOUNCE_RISING,
            lift: 10,
            torch: true,
            group: Some(AnimationGroup::IDLE1),
            ..ready()
        };
        step(&mut torch, &mut BounceState { started: true }, ARGS).unwrap();
        assert_eq!(torch.requests, [(AnimationGroup::FALL_TORCH, true)]);
    }

    #[test]
    fn only_the_selected_player_steers() {
        let mut steered = Actor {
            living: LivingState::BOUNCE_FALLING,
            falling: JumpMotion::STANDING,
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
    const SUPPORT: ActorBounceSupport = ActorBounceSupport {
        power: 160,
        speed: 40,
        steer: true,
        waiting: false,
    };

    #[test]
    fn a_landed_actor_waits_for_the_support_animation_and_reports_handled() {
        let mut actor = Actor {
            living: LivingState::LANDED_ON_ACTOR,
            falling: JumpMotion::STANDING,
            ..ready()
        };
        let waiting = ActorBounceSupport {
            waiting: true,
            ..SUPPORT
        };
        assert!(actor_bounce_step(&mut actor, waiting, |_| ()).unwrap());
        assert_eq!(actor.living, LivingState::LANDED_ON_ACTOR);
        assert_eq!((actor.cleared, actor.falling), (1, JumpMotion::NONE));
        assert_eq!(actor.position, [0; 3]);
    }

    #[test]
    fn the_landed_actor_lifts_after_the_wait_and_reports_handled_while_rising() {
        let mut actor = Actor {
            living: LivingState::LANDED_ON_ACTOR,
            heading: 100,
            selected: true,
            turn: 30,
            ..ready()
        };
        assert!(actor_bounce_step(&mut actor, SUPPORT, |_| ()).unwrap());
        assert_eq!(actor.living, LivingState::BOUNCING_FROM_ACTOR);
        // Lift 80 x 256, less one decay step of (256 - 160) x 16; the Y move uses the old lift.
        assert_eq!(actor.lift, 80 * 256 - 96 * 16);
        assert_eq!(actor.position[1], -(80 * 256));
        assert_eq!(actor.heading, 70);
        assert_eq!(actor.requests, [(AnimationGroup::JUMP, false)]);
        assert_eq!(actor.cleared, 1);
    }

    #[test]
    fn a_spent_lift_starts_the_fall_unhandled_and_keeps_the_support() {
        for (holding, torch, group) in [
            (false, false, AnimationGroup::FALL),
            (false, true, AnimationGroup::FALL_TORCH),
            (true, false, AnimationGroup::FIGHT_FALL),
        ] {
            let mut actor = Actor {
                living: LivingState::BOUNCING_FROM_ACTOR,
                lift: 10,
                holding,
                torch,
                ..ready()
            };
            let mut cleared = false;
            assert!(!actor_bounce_step(&mut actor, SUPPORT, |_| cleared = true).unwrap());
            assert!(actor.fall_started);
            assert_eq!(actor.living, LivingState::FALLING_FROM_ACTOR);
            assert_eq!(actor.requests, [(group, true)]);
            assert!(!cleared);
        }
    }

    #[test]
    fn a_landed_fall_clears_the_state_and_the_support_reference() {
        let mut actor = Actor {
            living: LivingState::FALLING_FROM_ACTOR,
            ..ready()
        };
        let mut cleared = false;
        assert!(!actor_bounce_step(&mut actor, SUPPORT, |_| cleared = true).unwrap());
        assert_eq!(actor.living, LivingState::ORDINARY);
        assert!(cleared);
        // Still falling: the state and the reference stay.
        let mut falling = Actor {
            living: LivingState::FALLING_FROM_ACTOR,
            falling: JumpMotion::STANDING,
            ..ready()
        };
        let mut cleared = false;
        assert!(!actor_bounce_step(&mut falling, SUPPORT, |_| cleared = true).unwrap());
        assert_eq!(falling.living, LivingState::FALLING_FROM_ACTOR);
        assert!(!cleared);
    }

    #[test]
    fn the_actor_bounce_steers_only_a_selected_player_with_a_turning_support() {
        let actor = |selected| Actor {
            living: LivingState::FALLING_FROM_ACTOR,
            falling: JumpMotion::STANDING,
            heading: 100,
            turn: 30,
            selected,
            ..ready()
        };
        for (selected, steer, heading) in [(true, true, 70), (true, false, 100), (false, true, 100)]
        {
            let mut actor = actor(selected);
            actor_bounce_step(&mut actor, ActorBounceSupport { steer, ..SUPPORT }, |_| ()).unwrap();
            assert_eq!(actor.heading, heading, "{selected} {steer}");
        }
    }
    #[test]
    fn the_wait_holds_below_half_the_command_count_in_the_landing_group() {
        let waits = |group, exit, cursor, count| actor_bounce_waiting(group, exit, cursor, count);
        let misc = Some(AnimationGroup::MISC1);
        // Ten commands: half is 5; seven: half is 3 (truncated); one: half is 0.
        assert!(waits(misc, false, Some(4), Some(10)));
        assert!(!waits(misc, false, Some(5), Some(10)));
        assert!(waits(misc, false, Some(2), Some(7)));
        assert!(!waits(misc, false, Some(3), Some(7)));
        assert!(!waits(misc, false, Some(0), Some(1)));
        // The cursor is signed.
        assert!(waits(misc, false, Some(-1), Some(1)));
        // The exit marker, another group or missing playback data end the wait.
        assert!(!waits(misc, true, Some(0), Some(10)));
        assert!(!waits(
            Some(AnimationGroup::IDLE1),
            false,
            Some(0),
            Some(10)
        ));
        assert!(!waits(None, false, Some(0), Some(10)));
        assert!(!waits(misc, false, None, Some(10)));
        assert!(!waits(misc, false, Some(0), None));
    }
}
