//! Immediate script sound commands follow preservation's channel ownership rules.
use anyhow::Result;
use cyrus_script::Argument;
pub use cyrus_world::sound::ScriptSound;

pub(super) fn command(name: &str, arguments: &[Argument]) -> Result<Option<ScriptSound>> {
    let count = if name.eq_ignore_ascii_case("FlatSound") {
        3
    } else if name.eq_ignore_ascii_case("EndSound") || name.eq_ignore_ascii_case("EndMySound") {
        1
    } else if name.eq_ignore_ascii_case("EndMySounds") {
        0
    } else {
        return Ok(None);
    };
    anyhow::ensure!(arguments.len() == count, "{name}: wrong argument count");
    let command = if count == 3 {
        ScriptSound::Flat {
            effect: arguments[0].integer()?,
            volume: arguments[1].integer()?,
            rate_offset_hz: arguments[2].integer()?,
        }
    } else if count == 1 {
        ScriptSound::StopEffect {
            effect: arguments[0].integer()?,
            owned: name.eq_ignore_ascii_case("EndMySound"),
        }
    } else {
        ScriptSound::StopOwned
    };
    Ok(Some(command))
}
