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
    /// Resolves CLI/environment, remembered location, then conventional folders.
    /// Invalid supplied or remembered locations remain errors for noninteractive callers.
    pub fn locate(&self) -> anyhow::Result<cyrus_assets::install::GameInstall> {
        let path = self.install.clone().or_else(|| {
            std::env::var_os(cyrus_assets::install::ENV_VAR).map(std::path::PathBuf::from)
        });
        crate::native_install::locate(path.as_deref(), &crate::native_install::location())
            .inspect(|install| bevy::log::info!("game files: {}", install.root.display()))
            .inspect_err(|error| bevy::log::warn!("game files not found: {error:#}"))
    }
    /// Inserts the resolved install, or shows the picker in the application's egui context.
    /// Call after installing the shared EguiPlugin. Cancel exits this application.
    pub fn select(&self, app: &mut bevy::prelude::App) {
        match self.locate() {
            Ok(install) => {
                app.insert_resource(install);
            }
            Err(error) => {
                crate::native_install::prompt(app, self.install.as_deref(), error);
            }
        }
    }
}

/// Game options have identical names and values in CLI flags and URL parameters.
#[derive(Parser, Debug, Default)]
pub struct GameOptions {
    /// WORLD.INI world ID or map stem to load directly.
    #[arg(long, value_parser = scene)]
    pub scene: Option<String>,
    /// First-world inspection camera: scene-space x,y,z and unit quaternion qx,qy,qz,qw.
    #[arg(long, value_parser = view)]
    pub view: Option<bevy::prelude::Transform>,
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
    /// CD music mode; modern mutes CD-ROM data bytes inside audio tracks.
    #[arg(long, value_parser = ["faithful", "modern"])]
    pub music: Option<String>,
    /// Installed 3dfx splash: the GOG Windows launch's `3DfxSpl2.dll` or the DOS Glide overlay.
    #[arg(long, value_parser = ["windows", "dos"])]
    pub splash: Option<String>,
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

fn view(value: &str) -> Result<bevy::prelude::Transform, String> {
    use bevy::prelude::*;
    let values = value
        .split(',')
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "view requires seven finite numbers: x,y,z,qx,qy,qz,qw")?;
    let [x, y, z, qx, qy, qz, qw]: [f32; 7] = values
        .try_into()
        .map_err(|_| "view requires seven finite numbers: x,y,z,qx,qy,qz,qw")?;
    let position = Vec3::new(x, y, z);
    let rotation = Quat::from_xyzw(qx, qy, qz, qw);
    if !position.is_finite() || !rotation.is_finite() || !rotation.is_normalized() {
        return Err("view requires a finite position and a unit quaternion".into());
    }
    Ok(Transform::from_translation(position).with_rotation(rotation.normalize()))
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
    fn inspection_view_accepts_menu_entry_and_requires_a_valid_pose() {
        let options =
            GameOptions::try_parse_from(["game", "--scene=START", "--view=1,-2,3,0,1,0,0"])
                .unwrap();
        let pose = options.view.unwrap();
        assert_eq!(pose.translation.to_array(), [1.0, -2.0, 3.0]);
        assert_eq!(pose.rotation.to_array(), [0.0, 1.0, 0.0, 0.0]);
        let menu = GameOptions::try_parse_from(["game", "--view=1,2,3,0,0,0,1"]).unwrap();
        assert!(menu.scene.is_none());
        assert!(!menu.skip_startup);
        assert_eq!(menu.view.unwrap().translation.to_array(), [1.0, 2.0, 3.0]);
        for invalid in ["1,2,3", "NaN,0,0,0,0,0,1", "0,0,0,0,0,0,0", "0,0,0,0,0,0,2"] {
            assert!(
                GameOptions::try_parse_from([
                    "game",
                    "--scene=START",
                    &format!("--view={invalid}")
                ])
                .is_err()
            );
        }
    }
    #[test]
    fn launch_values_and_invalid_combinations() {
        let game = GameOptions::try_parse_from([
            "game",
            "--scene=start",
            "--water=modern",
            "--skip-startup",
            "--music=modern",
            "--splash=dos",
        ])
        .unwrap();
        assert_eq!(game.music.as_deref(), Some("modern"));
        assert_eq!(game.splash.as_deref(), Some("dos"));
        assert!(GameOptions::try_parse_from(["game", "--splash=other"]).is_err());
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
