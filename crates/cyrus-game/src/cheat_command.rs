//! Console cheat syntax and numeric literals from preservation's console contract.
use crate::cheats::{CheatEffect, Cheats};
use anyhow::{Context, Result, bail, ensure};

/// Dispatch one command. A semicolon terminates input, rather than chaining commands.
/// Symbolic equates and SOUP flag expressions remain explicitly unsupported.
pub fn execute(cheats: &mut Cheats, line: &str) -> Result<Vec<String>> {
    let line = line
        .split([';', '\n', '\r'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if line.is_empty() {
        return Ok(Vec::new());
    }
    let (name, value) = if let Some((name, value)) = line.split_once('=') {
        (
            name.trim(),
            if matches!(name.trim(), "oracle" | "savecheats") {
                None
            } else {
                Some(number(value.trim())?)
            },
        )
    } else {
        (line.as_str(), None)
    };
    let effect = cheats.console(name, value)?;
    match effect {
        CheatEffect::List => Ok(cheats
            .state()?
            .entries()
            .iter()
            .filter_map(|entry| {
                entry
                    .name
                    .as_ref()
                    .map(|name| format!("{name} {}", entry.value))
            })
            .collect()),
        CheatEffect::Save => Ok(vec!["Cheat states saved.".into()]),
        _ => {
            let raw = cheats.state()?.value(name).context("cheat state missing")?;
            let mut output = if value.is_none() {
                vec![format!(
                    "Cheat {name} turned {}",
                    if raw == 0 { "off" } else { "on" }
                )]
            } else {
                Vec::new()
            };
            if effect == CheatEffect::Unsupported {
                output.push(format!(
                    "{name}: state retained; gameplay effect unavailable in cyrus."
                ));
            }
            Ok(output)
        }
    }
}

fn number(value: &str) -> Result<u32> {
    ensure!(!value.is_empty(), "missing cheat value");
    if let Some(hex) = value.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).context("invalid hexadecimal cheat value");
    }
    if value.starts_with('-') {
        return value
            .parse::<i32>()
            .map(|v| v as u32)
            .context("invalid signed cheat value");
    }
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse().context("cheat value exceeds 32 bits");
    }
    bail!("unsupported cheat value {value:?}; use a decimal or hexadecimal integer")
}
