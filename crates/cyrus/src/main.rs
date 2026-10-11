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
    /// WORLD.INI world ID or map stem to load (overrides settings.toml).
    #[arg(long)]
    scene: Option<String>,
    /// No window, no renderer: preload, scene export and the control server only.
    #[arg(long)]
    headless: bool,
    /// Skip splash and title and open the book menu. --scene loads a world directly.
    #[arg(long)]
    skip_startup: bool,
    /// Export every WORLD.INI world to the browser cache and exit.
    #[arg(long)]
    export_all_worlds: bool,
    /// Export splash, title, book-menu, cinematics and HUD assets to the browser cache and exit.
    #[arg(long)]
    export_startup: bool,
    /// Control server bind address.
    #[arg(long, env = "CYRUS_CONTROL_ADDR")]
    control: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let startup = args.scene.is_none() && !args.headless;
    let mut settings = Settings::load()?;
    if let Some(scene) = args.scene {
        settings.scene = scene;
    }
    let install = GameInstall::locate(args.install.as_deref())?;
    if args.export_startup {
        cyrus_game::startup::export_movies(&install, &Settings::cache_dir())?;
        cyrus_game::startup::export_menu(&install, &Settings::cache_dir())?;
        cyrus_game::export_hud(&install, &Settings::cache_dir())?;
        return cyrus_game::startup::export_startup(&install, &Settings::cache_dir());
    }
    if args.export_all_worlds {
        return cyrus_game::scene::export_all_worlds(&install, &Settings::cache_dir());
    }
    if !args.headless {
        cyrus_game::export_hud(&install, &Settings::cache_dir())?;
    }
    if startup {
        cyrus_game::startup::export_menu(&install, &Settings::cache_dir())?;
    }
    if startup && !args.skip_startup {
        cyrus_game::startup::export_startup(&install, &Settings::cache_dir())?;
    }
    let control = ControlAddr(
        args.control
            .unwrap_or_else(|| settings.control_addr.clone()),
    );

    let mut app = App::new();
    cyrus_game::scene::register_cache_source(&mut app)?;
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
    app.add_plugins(cyrus_game::animation::AnimationPlugin);
    if !args.headless {
        app.add_plugins((
            cyrus_game::environment::EnvironmentPlugin,
            cyrus_game::hud::HudPlugin,
        ));
        if startup {
            app.add_plugins(cyrus_game::startup::StartupPlugin);
            if args.skip_startup {
                app.insert_state(cyrus_game::AppState::Menu);
            }
        } else {
            app.add_plugins(cyrus_debug::DebugMenuPlugin);
        }
    }
    app.insert_resource(control)
        .add_plugins(GamePlugin { install, settings });
    if args.headless {
        app.insert_resource(cyrus_game::scene::Headless);
    }
    app.run();
    Ok(())
}
