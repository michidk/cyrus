//! Native install, persistence and control adapters with game/presentation assembly.
mod control;
mod scene;
mod settings;
use crate::control::ControlAddr;
use crate::settings::Settings;
mod bindings;
mod debug;
mod preferences;
use bevy::prelude::*;
use clap::Parser;
use cyrus_game::GamePlugin;

#[derive(Parser, Debug)]
#[command(version, about = "Redguard in Bevy, from your own GOG install.")]
struct Args {
    #[command(flatten)]
    source: cyrus_app::launch::InstallArgs,
    #[command(flatten)]
    launch: cyrus_app::launch::GameOptions,
    /// No window, no renderer: preload, scene export and the control server only.
    #[arg(long)]
    headless: bool,
    /// Export the selected scene and exit.
    #[arg(long, requires = "scene")]
    export_scene: bool,
    /// Print accepted world names as JSON and exit.
    #[arg(long)]
    list_worlds: bool,
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
    let install = if args.headless
        || args.list_worlds
        || args.export_scene
        || args.export_all_worlds
        || args.export_startup
    {
        args.source.locate()?
    } else {
        let Some(install) = args.source.locate_or_prompt("cyrus")? else {
            return Ok(());
        };
        install
    };
    if args.list_worlds {
        println!(
            "{}",
            serde_json::to_string(&cyrus_assets::scene::world_names(&install)?)?
        );
        return Ok(());
    }
    let startup = args.launch.scene.is_none() && !args.headless;
    let mut settings = Settings::load()?;
    if let Some(scene) = args.launch.scene {
        settings.scene = scene;
    }
    install.cache_path(&Settings::cache_dir())?;
    if args.export_startup || args.export_all_worlds {
        cyrus_assets::music::export(&install, &Settings::cache_dir())?;
    }
    for (area, value) in [
        (cyrus_game::fidelity::Area::Water, args.launch.water),
        (cyrus_game::fidelity::Area::Fog, args.launch.fog),
        (cyrus_game::fidelity::Area::Sky, args.launch.sky),
    ] {
        if let Some(value) = value {
            settings.fidelity.set(
                area,
                if value == "modern" {
                    cyrus_game::fidelity::Mode::Modern
                } else {
                    cyrus_game::fidelity::Mode::Faithful
                },
            );
        }
    }
    if args.export_scene {
        return cyrus_assets::scene::export_scene(
            &install,
            &settings.scene,
            &Settings::cache_dir().join(format!("{}.glb", settings.scene)),
        );
    }
    if args.export_startup {
        cyrus_assets::startup::export_movies(&install, &Settings::cache_dir())?;
        cyrus_assets::startup::export_menu(&install, &Settings::cache_dir())?;
        cyrus_assets::hud_export::export_hud(&install, &Settings::cache_dir())?;
        let menu: cyrus_assets::menu::MenuAsset = serde_json::from_slice(&std::fs::read(
            Settings::cache_dir().join("menu/book.menu"),
        )?)?;
        let name = menu.new_game.world.to_string();
        cyrus_assets::scene::export_scene(
            &install,
            &name,
            &Settings::cache_dir().join(format!("{name}.glb")),
        )?;
        return cyrus_assets::startup::export_startup(&install, &Settings::cache_dir());
    }
    if args.export_all_worlds {
        return cyrus_assets::scene::export_all_worlds(&install, &Settings::cache_dir());
    }
    if !args.headless && !startup {
        cyrus_assets::hud_export::export_hud(&install, &Settings::cache_dir())?;
    }
    if startup {
        cyrus_assets::startup::export_startup(&install, &Settings::cache_dir())?;
    }
    let control = ControlAddr(
        args.control
            .unwrap_or_else(|| settings.control_addr.clone()),
    );

    let mut app = App::new();
    app.insert_resource(cyrus_game::preferences::Preferences::new(
        preferences::LocalPreferences,
    ));
    crate::scene::register_cache_source(&mut app)?;
    if args.headless {
        app.add_plugins((
            MinimalPlugins,
            bevy::log::LogPlugin::default(),
            bevy::state::app::StatesPlugin,
            bevy::input::InputPlugin,
            bevy::asset::AssetPlugin::default(),
        ));
    } else {
        app.add_plugins(DefaultPlugins.set(cyrus_app::window("cyrus")));
    }
    app.add_plugins((
        cyrus_world::animation::AnimationPlugin,
        cyrus_world::lifecycle::WorldPlugin,
    ));
    if !args.headless {
        app.add_plugins((
            cyrus_render::WorldRenderPlugin,
            cyrus_render::hud::HudPlugin,
            cyrus_debug::DebugMenuPlugin,
        ));
        debug::register(&mut app);
        if startup {
            app.add_plugins(cyrus_startup::StartupPlugin);
            if args.launch.skip_startup {
                app.insert_resource(cyrus_startup::StartupPresentation::without_splash());
            }
        }
    }
    app.add_systems(Startup, bindings::load);
    app.insert_resource(control)
        .insert_resource(install)
        .insert_resource(settings.fidelity.clone())
        .insert_resource(cyrus_world::lifecycle::SceneRequest(settings.scene.clone()))
        .add_plugins((
            GamePlugin,
            cyrus_startup::LoadingPlugin,
            cyrus_app::LoadingOverlayPlugin,
            scene::ScenePlugin,
            control::ControlPlugin,
            cyrus_audio::sound::SoundPlugin,
            cyrus_audio::music::MusicPlugin,
            cyrus_render::subtitles::SubtitlePlugin,
        ));
    app.run();
    Ok(())
}
