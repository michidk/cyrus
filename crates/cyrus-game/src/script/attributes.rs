//! Immediate attribute services. Runtime writes retain the actor byte; family/model
//! ownership is established when spatial state is prepared and is not rebound by a write.
use anyhow::{Context, Result};
use cyrus_script::Argument;

pub(super) fn call(
    name: &str,
    arguments: &[Argument],
    attributes: &mut [u8],
    control: Option<&mut super::ActorControlState>,
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
    let value = arguments[1].integer()? as u8;
    if index == 44 {
        let control =
            control.with_context(|| format!("{name}: attribute 44 actor lifecycle unavailable"))?;
        if !set
            && control.object_update_excluded
            && let Some(default) = &mut control.combat_death_default
        {
            *default = i32::from(value);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_and_attachment_attributes_are_plain_runtime_byte_writes() {
        let mut attributes = vec![0; 256];
        for index in [25usize, 26, 27, 28, 33] {
            let result = call(
                "SetAttribute",
                &[
                    Argument::Integer(index as i32),
                    Argument::Integer(0x123),
                ],
                &mut attributes,
                None,
            )
            .unwrap();
            assert_eq!(result, Some(0));
            assert_eq!(attributes[index], 0x23);
        }
    }
}
