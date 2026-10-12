//! Immediate attribute services; model-dependent writes remain explicit failures.
use anyhow::{Context, Result};
use cyrus_script::Argument;

pub(super) fn call(
    name: &str,
    arguments: &[Argument],
    attributes: &mut [u8],
) -> Result<Option<i32>> {
    let get = name.eq_ignore_ascii_case("GetAttribute");
    let get_my = name.eq_ignore_ascii_case("GetMyAttr");
    let set = name.eq_ignore_ascii_case("SetAttribute");
    let set_my = name.eq_ignore_ascii_case("SetMyAttr");
    if !get && !get_my && !set && !set_my {
        return Ok(None);
    }
    anyhow::ensure!(
        arguments.len() == if get || get_my { 1 } else { 2 },
        "{name}: wrong argument count"
    );
    let index = usize::try_from(arguments[0].integer()?)
        .ok()
        .filter(|index| *index < attributes.len());
    if index.is_none() && (get_my || set_my) {
        return Ok(Some(0));
    }
    let index =
        index.with_context(|| format!("{name}: attribute index outside 0..255 unsupported"))?;
    if get || get_my {
        return Ok(Some(i32::from(attributes[index])));
    }
    anyhow::ensure!(
        index != 44,
        "{name}: attribute 44 model/default-group coupling unsupported"
    );
    let value = arguments[1].integer()? as u8;
    attributes[index] = value;
    if set {
        match index {
            1 => attributes[119] = value,
            6 => attributes[115] = value,
            _ => {}
        }
    }
    Ok(Some(0))
}
