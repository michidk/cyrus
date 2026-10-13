//! Cheat assignment and reports; cyrus-console owns input and numeric parsing.
use crate::cheats::{CheatEffect, Cheats};
use anyhow::{Context, Result};
use cyrus_console::number;

/// Execute normalized cheat arguments; the shared console owns command dispatch.
pub(crate) fn execute(
    cheats: &mut Cheats,
    name: &str,
    args: &str,
    symbol: impl FnOnce(&str) -> Result<Option<u32>>,
) -> Result<Vec<String>> {
    let mut diagnostics = Vec::new();
    let value = if args.is_empty() {
        None
    } else {
        let value = args
            .strip_prefix('=')
            .context("usage: name [= value]")?
            .trim();
        if matches!(name, "oracle" | "savecheats") {
            None
        } else {
            Some(number(value, |name| {
                Ok(symbol(name)?.unwrap_or_else(|| {
                    diagnostics.push(format!("Unknown console value {name:?}; using 1."));
                    1
                }))
            })?)
        }
    };
    let effect = cheats.console(name, value)?;
    let mut output = match effect {
        CheatEffect::List => cheats
            .state()?
            .entries()
            .iter()
            .filter_map(|entry| {
                entry
                    .name
                    .as_ref()
                    .map(|name| format!("{name} {}", entry.value))
            })
            .collect(),
        CheatEffect::Save => vec!["Cheat states saved.".into()],
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
            output
        }
    };
    diagnostics.append(&mut output);
    Ok(diagnostics)
}
