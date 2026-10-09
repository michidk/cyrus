use bevy::prelude::*;
use clap::Parser;
use cyrus_game::{GamePlugin, control::ControlAddr, install::GameInstall, settings::Settings};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "Redguard in Bevy, from your own GOG install.")]
struct Args {
    /// Path to the GOG install's Redguard directory (overrides $REDGUARD_DIR and ./redguard).
    #[arg(long)]
    install: Option<PathBuf>,
    /// Scene stem to load (overrides settings.toml).
    #[arg(long)]
    scene: Option<String>,
    /// No window, no renderer: preload, scene export and the control server only.
    #[arg(long)]
    headless: bool,
    /// Control server bind address.
    #[arg(long, env = "CYRUS_CONTROL_ADDR")]
    control: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut settings = Settings::load()?;
    if let Some(scene) = args.scene {
        settings.scene = scene;
    }
    let install = GameInstall::locate(args.install.as_deref())?;
    let control = ControlAddr(
        args.control
            .unwrap_or_else(|| settings.control_addr.clone()),
    );

    let mut app = App::new();
    cyrus_game::scene::register_cache_source(&mut app);
    if args.headless {
        app.add_plugins((
            MinimalPlugins,
            bevy::log::LogPlugin::default(),
            bevy::state::app::StatesPlugin,
            bevy::input::InputPlugin,
            bevy::asset::AssetPlugin::default(),
        ));
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "cyrus".into(),
                ..default()
            }),
            ..default()
        }));
    }
    app.insert_resource(control)
        .add_plugins(GamePlugin { install, settings });
    if args.headless {
        app.insert_resource(cyrus_game::scene::Headless);
    }
    app.run();
    Ok(())
}
