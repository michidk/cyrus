//! Shared launch option names and validation for CLI flags and browser URL parameters.
use clap::Parser;

/// Installation source. Native applications discover an install when omitted.
#[derive(clap::Args, Debug, Default)]
pub struct InstallArgs {
    /// Original Redguard directory, or its HTTP URL in a browser. Read only.
    #[arg(long, env = "REDGUARD_DIR")]
    pub install: Option<std::path::PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
impl InstallArgs {
    pub fn locate(&self) -> anyhow::Result<cyrus_assets::install::GameInstall> {
        cyrus_assets::install::GameInstall::locate(self.install.as_deref())
    }
}

/// Game options have identical names and values in CLI flags and URL parameters.
#[derive(Parser, Debug, Default)]
pub struct GameOptions {
    /// WORLD.INI world ID or map stem to load directly.
    #[arg(long, value_parser = scene)]
    pub scene: Option<String>,
    /// Bypass the splash while preparing the book menu behind the title.
    #[arg(long)]
    pub skip_startup: bool,
    /// Water presentation mode.
    #[arg(long, value_parser = ["faithful", "modern"])]
    pub water: Option<String>,
    /// Fog presentation mode.
    #[arg(long, value_parser = ["faithful", "modern"])]
    pub fog: Option<String>,
    /// Sky presentation mode.
    #[arg(long, value_parser = ["faithful", "modern"])]
    pub sky: Option<String>,
}

/// Viewer selection options also accept existing browser fragment links.
#[derive(Parser, Debug, Default, bevy::prelude::Resource)]
pub struct ViewerOptions {
    /// Asset ID to select after the catalog loads.
    #[arg(long)]
    pub asset: Option<String>,
    /// Source path identifying the selected asset's archive.
    #[arg(long, requires = "asset")]
    pub file: Option<String>,
    /// Palette name for the selected model.
    #[arg(long, requires = "asset")]
    pub palette: Option<String>,
}

fn scene(value: &str) -> Result<String, String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err("scene must be a WORLD.INI ID or an alphanumeric map stem".into());
    }
    Ok(value.to_ascii_uppercase())
}

/// Parse URL values through the same Clap command as native launch options.
#[cfg(target_arch = "wasm32")]
pub fn from_query<T: Parser>() -> anyhow::Result<T> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no browser window"))?;
    let params = web_sys::UrlSearchParams::new_with_str(
        &window
            .location()
            .search()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?,
    )
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut args = vec!["browser".to_owned()];
    for arg in T::command().get_arguments() {
        let Some(name) = arg.get_long() else { continue };
        if let Some(value) = params.get(name) {
            if matches!(arg.get_action(), clap::ArgAction::SetTrue) {
                match value.as_str() {
                    "" | "true" | "1" => args.push(format!("--{name}")),
                    "false" | "0" => {}
                    _ => anyhow::bail!("{name}: expected true or false"),
                }
            } else {
                args.push(format!("--{name}={value}"));
            }
        }
    }
    T::try_parse_from(args).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_values_and_invalid_combinations() {
        let game = GameOptions::try_parse_from([
            "game",
            "--scene=start",
            "--water=modern",
            "--skip-startup",
        ])
        .unwrap();
        assert_eq!(game.scene.as_deref(), Some("START"));
        assert_eq!(game.water.as_deref(), Some("modern"));
        assert!(game.skip_startup);
        assert!(GameOptions::try_parse_from(["game", "--scene=../START"]).is_err());
        assert!(GameOptions::try_parse_from(["game", "--fog=other"]).is_err());
        let viewer = ViewerOptions::try_parse_from([
            "viewer",
            "--asset=abc",
            "--file=fxart/A.ROB",
            "--palette=night",
        ])
        .unwrap();
        assert_eq!(viewer.file.as_deref(), Some("fxart/A.ROB"));
        assert_eq!(viewer.palette.as_deref(), Some("night"));
        assert!(ViewerOptions::try_parse_from(["viewer", "--file=A.ROB"]).is_err());
    }
}
