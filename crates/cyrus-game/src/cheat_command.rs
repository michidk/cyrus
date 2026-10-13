//! Cheat assignment and reports; cyrus-console owns input and numeric parsing.
use crate::cheats::{CheatEffect, Cheats};
use anyhow::{Context, Result};
use cyrus_console::number;

/// Execute normalized cheat arguments; the shared console owns command dispatch.
/// Symbolic equates and SOUP flag expressions remain explicitly unsupported.
pub(crate) fn execute(cheats: &mut Cheats, name: &str, args: &str) -> Result<Vec<String>> {
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
            Some(number(value)?)
        }
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
