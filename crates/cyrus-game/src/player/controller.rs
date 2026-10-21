//! Pure ordinary sheathed-ground controller dispatch.
//! Rules follow preservation's
//! [ordinary controller rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ordinary-controller-dispatch).
use super::input::PlayerInput;

/// Established globals at the selected player's ordinary native hook.
/// The caller supplies retained low words and the independent walk-mode toggle.
/// Missing state must not be replaced with zero or a default toggle.
#[derive(Debug, Clone, Copy)]
pub struct SheathedControllerState {
    pub lockout: u16,
    pub dialogue_state: u16,
    pub menu_state: u16,
    pub walk_mode: bool,
}

/// An early return from keyboard dispatch, not from the outer actor update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerReturn {
    Lockout,
    Dialogue,
    TopicMenu,
    View,
}

/// A controller selection, before the selected movement helper's admission tests.
/// Every variant still requires the outer update's continuation and ground stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheathedAction {
    Returned(ControllerReturn),
    Idle,
    WalkForward,
    RunForward,
    /// Backward input alone: the walk helper `WalkBackward` when the walk mode and key give a
    /// walk, otherwise the run helper `MoveBackward`, each with duration 1.
    WalkBackward,
    RunBackward,
    /// Left or right alone with the walk key held: the scripted `MoveLeft` or `MoveRight`
    /// stepper with duration 1, which a left press takes over a right press.
    Sidestep(crate::script::locomotion::Strafe),
}

/// Select an ordinary sheathed-ground action without changing actor state.
///
/// The caller preserves native housekeeping and idle-request order: lockout precedes
/// idle requests, while dialogue and menu returns follow them. This function only
/// selects the dispatch outcome. Preliminary countdown and override handling
/// must finish before invoking it. The caller must establish the selected ordinary
/// grounded, noncombat, nonassist, nonrope,
/// nonwater actor state and exclude special living states. This function does not
/// establish that eligibility or admit the selected movement helper.
///
/// `input` must contain Faithful held-key samples: `walk` is installed key_a,
/// not a Modern walking request or the retained Tab toggle. Left and right apply
/// the caller's [ground turn](super::turn); the held walk key with only left or
/// right selects a sidestep. Backward-only movement picks the walk or run helper by the same
/// walk-mode rule as forward movement (verified against the original controller in an
/// emulator). Jumping requires a separate implementation; activation is admitted here and
/// handled by the ordinary script hook, which owns receiver prompts and held-key state.
/// A return or idle selection does not suppress later animation continuation.
pub fn select_sheathed_action(
    input: &PlayerInput,
    state: SheathedControllerState,
) -> anyhow::Result<SheathedAction> {
    let returned = if state.lockout != 0 {
        Some(ControllerReturn::Lockout)
    } else if state.dialogue_state == 1 {
        Some(ControllerReturn::Dialogue)
    } else if state.menu_state == 1 {
        Some(ControllerReturn::TopicMenu)
    } else if input.look {
        Some(ControllerReturn::View)
    } else {
        None
    };
    if let Some(reason) = returned {
        return Ok(SheathedAction::Returned(reason));
    }
    anyhow::ensure!(
        !input.jump,
        "sheathed player controller: jumping unsupported"
    );
    if (input.left || input.right) && input.walk && !input.forward && !input.backward {
        use crate::script::locomotion::Strafe;
        return Ok(SheathedAction::Sidestep(if input.left {
            Strafe::Left
        } else {
            Strafe::Right
        }));
    }
    if input.forward {
        Ok(if state.walk_mode ^ input.walk {
            SheathedAction::WalkForward
        } else {
            SheathedAction::RunForward
        })
    } else if input.backward {
        Ok(if state.walk_mode ^ input.walk {
            SheathedAction::WalkBackward
        } else {
            SheathedAction::RunBackward
        })
    } else {
        Ok(SheathedAction::Idle)
    }
}
