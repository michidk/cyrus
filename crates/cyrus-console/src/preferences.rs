//! Console preferences and host persistence; CPF encoding belongs to preservation.
use crate::{Console, Invocation};
use anyhow::{Context, Result, bail, ensure};
use bevy::prelude::{In, World};
use serde::Serialize;

/// Original settings, including raw GUI indices and a screen limit in 1/256 units.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Preferences {
    pub font: u32,
    pub shadow: u32,
    pub colors: [u32; 4],
    pub hex_lines: i32,
    pub paper_disabled: u32,
    pub background: u8,
    pub screen_limit: i32,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            font: 0,
            shadow: 0,
            colors: [0, 13, 9, 0],
            hex_lines: 16,
            paper_disabled: 0,
            background: 0,
            screen_limit: 0,
        }
    }
}

/// Hosts read local state before installed defaults and never write into the install.
pub trait Store: Send + Sync + 'static {
    fn load(&self) -> Result<Option<Preferences>>;
    fn save(&self, preferences: &Preferences) -> Result<()>;
    fn write_log(&self, text: &str) -> Result<()>;
}

impl Preferences {
    /// Decode raw fields without applying the original loader's runtime normalization.
    #[cfg(feature = "cpf")]
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let file = rgpre::import::cpf::parse_cpf_file(bytes)?;
        Ok(Self {
            font: file.font,
            shadow: file.shadow,
            colors: file.colors,
            hex_lines: file.hex_lines,
            paper_disabled: file.paper_disabled,
            background: file.background,
            screen_limit: file.screen_limit,
        })
    }

    /// Encode through rgpre; no file layout knowledge lives in the console.
    #[cfg(feature = "cpf")]
    pub fn encode(&self) -> Vec<u8> {
        rgpre::import::cpf::encode_cpf_file(&rgpre::import::cpf::CpfFile {
            font: self.font,
            shadow: self.shadow,
            colors: self.colors,
            hex_lines: self.hex_lines,
            paper_disabled: self.paper_disabled,
            background: self.background,
            screen_limit: self.screen_limit,
        })
        .to_vec()
    }
}

pub(crate) fn command(In(command): In<Invocation>, world: &mut World) -> Result<Vec<String>> {
    let (option, argument) = command.args.split_once(',').unwrap_or((&command.args, ""));
    let option = option.trim();
    let mut output = Vec::new();
    let numeric = if option == "limit" && argument.trim().is_empty() {
        Some(0)
    } else if matches!(option, "hex" | "limit") {
        ensure!(
            !argument.trim().is_empty(),
            "usage: console {option},number"
        );
        Some(crate::value(world, argument.trim(), &mut output)? as i32)
    } else {
        None
    };
    let mut console = world.resource_mut::<Console>();
    if option == "clear" {
        console.clear();
        return Ok(Vec::new());
    }
    if option == "write" {
        let text = console
            .lines
            .iter()
            .map(|line| format!("{}{}\n", if line.report { " " } else { "> " }, line.text))
            .collect::<String>();
        console
            .store
            .as_ref()
            .context("console storage is unavailable")?
            .write_log(&text)?;
        return Ok(vec!["CONSOLE.LOG written.".into()]);
    }
    if option == "show" {
        let p = &console.preferences;
        let mut lines = vec![
            format!(
                "font={} shadow={} paper_disabled={}",
                p.font, p.shadow, p.paper_disabled
            ),
            format!("colors={:?} background={}", p.colors, p.background),
            format!("hex={} limit={}", p.hex_lines, p.screen_limit / 256),
        ];
        if let Some(error) = &console.preference_error {
            lines.push(error.clone());
        }
        return Ok(lines);
    }
    if let Some(error) = &console.preference_error {
        bail!("{error}");
    }
    let mut next = console.preferences.clone();
    match option {
        "save" => {
            console
                .store
                .as_ref()
                .context("console storage is unavailable")?
                .save(&next)?;
            return Ok(vec!["REDGUARD.CPF saved.".into()]);
        }
        "font" => next.font = next.font.wrapping_add(1) & 1,
        "shadow" => next.shadow = u32::from(next.shadow == 0),
        "paper" => next.paper_disabled = u32::from(next.paper_disabled == 0),
        "col0" | "col1" | "col2" | "col3" => {
            let index = usize::from(option.as_bytes()[3] - b'0');
            ensure!(
                index != 3 || next.paper_disabled != 0,
                "console col3 requires paper off"
            );
            next.colors[index] = next.colors[index].wrapping_add(1) & 15;
            if index == 3 {
                next.background = console
                    .palette
                    .context("console col3 requires the current world palette")?
                    [next.colors[3] as usize];
            }
        }
        "hex" | "limit" => {
            let value = numeric.expect("numeric option parsed before mutation");
            if option == "hex" {
                next.hex_lines = value;
            } else {
                ensure!(
                    (0..=300).contains(&value),
                    "console limit must be between 0 and 300"
                );
                next.screen_limit = value * 256;
            }
        }
        _ => bail!(
            "unknown console option {option:?}; use console show, clear, write, save, font, shadow, paper, col0..3, hex,number or limit,number"
        ),
    }
    console.preferences = next;
    Ok(output)
}
