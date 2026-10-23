#![cfg_attr(windows, windows_subsystem = "windows")]

//! Native install, persistence and control adapters with game/presentation assembly.
mod audit;
mod control;
mod scene;
mod session;
mod settings;
mod subtitles;
use crate::control::ControlAddr;
use crate::settings::Settings;
mod bindings;
mod cheats;
mod console;
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
    #[command(flatten)]
    log: cyrus_app::logging::LogArgs,
    /// No window, no renderer: preload, scene export and the control server only.
    #[arg(long)]
    headless: bool,
    /// Audit every installed WORLD.INI entry without a window; reports go under magic/artifacts/world-audit.
    #[arg(long)]
    audit_worlds: bool,
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
    /// Restore the newest save in this slot of cyrus-config/slots through the book. Implies
    /// --skip-startup.
    #[arg(
        long,
        value_name = "SLOT",
        conflicts_with_all = ["scene", "new_game", "movie", "headless"]
    )]
    load_slot: Option<u16>,
    /// Print the saved slots as JSON and exit.
    #[arg(long)]
    list_slots: bool,
    /// Control server bind address.
    #[arg(long, env = "CYRUS_CONTROL_ADDR")]
    control: Option<String>,
}

fn main() -> anyhow::Result<()> {
    // Bevy's task-pool threads take the default 2 MiB stack, which a debug build overflows while
    // loading ISLAND (an IO pool thread, minutes after load). Set before any thread exists.
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        // SAFETY: nothing else is running yet; the process is still single-threaded.
        unsafe { std::env::set_var("RUST_MIN_STACK", "33554432") };
    }
    cyrus_app::logging::attach_parent_console();
    let args = Args::parse();
    let mut app = App::new();
    // First, so the export and audit modes log too, and a failure reaches the log file.
    let logging = args.log.plugin(&mut app, "cyrus");
    app.add_plugins(logging);
    cyrus_app::logging::log_panics();
    args.log.announce("cyrus");
    run(args, app).inspect_err(|error| error!("fatal: {error:#}"))
}

fn run(args: Args, mut app: App) -> anyhow::Result<()> {
    if args.list_slots {
        println!(
            "{}",
            serde_json::to_string(&session::slots::SlotStore::default().list()?)?
        );
        return Ok(());
    }
    anyhow::ensure!(
        !args.headless || !(args.launch.new_game || args.launch.movie.is_some()),
        "--new-game and --movie drive the book menu, which --headless does not open"
    );
    if let Some(slot) = args.load_slot {
        // Fail before a window opens when the slot is absent or unreadable.
        session::slots::SlotStore::default().metadata(slot, None)?;
        app.insert_resource(session::LaunchSlot(slot));
    }
    let install = if args.audit_worlds
        || args.headless
        || args.list_worlds
        || args.export_scene
        || args.export_all_worlds
        || args.export_startup
    {
        Some(args.source.locate()?)
    } else {
        None
    };
    if args.audit_worlds {
        return audit::run(install.as_ref().unwrap());
    }
    if args.list_worlds {
        println!(
            "{}",
            serde_json::to_string(&cyrus_assets::scene::world_names(
                install.as_ref().unwrap()
            )?)?
        );
        return Ok(());
    }
    let startup = args.launch.scene.is_none() && !args.headless;
    let mut settings = Settings::load()?;
    if let Some(scene) = args.launch.scene {
        settings.scene = scene;
    }
    if let Some(install) = &install {
        install.cache_path(cyrus_config::cache_dir())?;
    }
    if args.export_startup || args.export_all_worlds {
        cyrus_assets::music::export(install.as_ref().unwrap(), cyrus_config::cache_dir())?;
    }
    for (area, value) in [
        (cyrus_game::fidelity::Area::Water, args.launch.water),
        (cyrus_game::fidelity::Area::Fog, args.launch.fog),
        (cyrus_game::fidelity::Area::Sky, args.launch.sky),
        (cyrus_game::fidelity::Area::Music, args.launch.music),
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
            install.as_ref().unwrap(),
            &settings.scene,
            &cyrus_config::cache_dir().join(format!("{}.glb", settings.scene)),
        );
    }
    if args.export_startup {
        let install = install.as_ref().unwrap();
        cyrus_assets::startup::export_movies(install, cyrus_config::cache_dir())?;
        cyrus_assets::startup::export_menu(install, cyrus_config::cache_dir())?;
        cyrus_assets::hud_export::export_hud(install, cyrus_config::cache_dir())?;
        let menu: cyrus_assets::menu::MenuAsset = serde_json::from_slice(&std::fs::read(
            cyrus_config::cache_dir().join("menu/book.menu"),
        )?)?;
        let name = menu.new_game.world.to_string();
        cyrus_assets::scene::export_scene(
            install,
            &name,
            &cyrus_config::cache_dir().join(format!("{name}.glb")),
        )?;
        return cyrus_assets::startup::export_startup(install, cyrus_config::cache_dir());
    }
    if args.export_all_worlds {
        return cyrus_assets::scene::export_all_worlds(
            install.as_ref().unwrap(),
            cyrus_config::cache_dir(),
        );
    }
    let control = ControlAddr(
        args.control
            .unwrap_or_else(|| settings.control_addr.clone()),
    );

    if !args.headless
        && let Some(view) = args.launch.view
    {
        app.insert_resource(cyrus_render::camera::InitialView(view));
    }
    app.insert_resource(cyrus_game::preferences::Preferences::new(
        preferences::LocalPreferences,
    ));
    crate::scene::register_cache_source(&mut app)?;
    if args.headless {
        app.add_plugins((
            MinimalPlugins,
            bevy::state::app::StatesPlugin,
            bevy::input::InputPlugin,
            bevy::asset::AssetPlugin::default(),
        ));
    } else {
        app.add_plugins(
            DefaultPlugins
                .set(cyrus_app::window("cyrus"))
                .disable::<bevy::log::LogPlugin>(),
        );
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
        app.add_systems(
            Update,
            cyrus_audio::follow_listener::<cyrus_render::camera::ViewCamera>
                .before(cyrus_audio::ApplyPreferences),
        );
        debug::register(&mut app);
        if startup {
            app.add_plugins(cyrus_startup::StartupPlugin);
            let direct =
                args.launch.new_game || args.launch.movie.is_some() || args.load_slot.is_some();
            app.insert_resource(cyrus_startup::LaunchRequest::new(
                args.launch.new_game,
                args.launch.movie,
                args.launch.skip_movies,
            ));
            let mut presentation = if args.launch.skip_startup || direct {
                cyrus_startup::StartupPresentation::without_splash()
            } else {
                cyrus_startup::StartupPresentation::default()
            };
            presentation.splash = cyrus_startup::Splash::from_option(args.launch.splash.as_deref());
            app.insert_resource(presentation);
        } else {
            app.add_plugins((cyrus_startup::LogbookPlugin, cyrus_startup::OverlayPlugin));
        }
    }
    if let Some(install) = install {
        app.insert_resource(install)
            .add_systems(Startup, (bindings::load, cheats::load));
    } else {
        args.source.select(&mut app);
        app.insert_state(cyrus_game::AppState::SelectingInstall)
            .insert_resource(cyrus_audio::AudioReadiness { prepare: false })
            .add_systems(
                Update,
                prepare_install.run_if(in_state(cyrus_game::AppState::SelectingInstall)),
            )
            .add_systems(
                OnExit(cyrus_game::AppState::SelectingInstall),
                (bindings::load, cheats::load),
            );
    }
    app.init_resource::<cyrus_game::script::ScriptHud>()
        .insert_resource(control)
        .insert_resource(settings.fidelity.clone())
        .insert_resource(cyrus_world::lifecycle::SceneRequest(settings.scene.clone()))
        .add_plugins((
            GamePlugin,
            cyrus_session::SessionBoundaryPlugin,
            cyrus_startup::LoadingPlugin,
            cyrus_app::loading::LoadingOverlayPlugin,
            scene::ScenePlugin,
            control::ControlPlugin,
            cyrus_audio::sound::SoundPlugin,
            cyrus_audio::music::MusicPlugin,
            cyrus_render::subtitles::SubtitlePlugin,
        ));
    subtitles::register(&mut app);
    app.run();
    Ok(())
}

/// Prepare the first visible assets without blocking the window's event loop.
fn prepare_install(
    mut commands: Commands,
    install: Option<Res<cyrus_assets::install::GameInstall>>,
    startup: Option<Res<cyrus_startup::StartupPresentation>>,
    mut pending: Local<Option<bevy::tasks::Task<anyhow::Result<()>>>>,
    mut attempted: Local<bool>,
    mut progress: ResMut<cyrus_app::loading::LoadingProgress>,
    mut next: ResMut<NextState<cyrus_game::AppState>>,
) {
    if let Some(task) = pending.as_mut() {
        if let Some(result) = bevy::tasks::futures::check_ready(task) {
            *pending = None;
            match result {
                Ok(()) => {
                    progress.visible = false;
                    commands.insert_resource(cyrus_audio::AudioReadiness {
                        prepare: startup.is_none(),
                    });
                    next.set(cyrus_game::AppState::Loading);
                }
                Err(error) => {
                    progress.label = format!("Preparing game files: {error:#}");
                    progress.fraction = Some(0.);
                }
            }
        }
        return;
    }
    let Some(install) = install else {
        return;
    };
    if *attempted {
        return;
    }
    *attempted = true;
    progress.visible = true;
    progress.label = "Preparing game files…".into();
    progress.fraction = None;
    let install = install.clone();
    let startup = startup.is_some();
    *pending = Some(bevy::tasks::IoTaskPool::get().spawn(async move {
        let cache = cyrus_config::cache_dir();
        install.cache_path(cache)?;
        if startup {
            cyrus_assets::startup::export_startup(&install, cache)
        } else {
            cyrus_assets::hud_export::export_hud(&install, cache)
        }
    }));
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn preparation_waits_for_selection_and_keeps_failures_in_the_same_window() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .insert_state(cyrus_game::AppState::SelectingInstall)
            .init_resource::<cyrus_app::loading::LoadingProgress>()
            .add_systems(Update, prepare_install);
        let window = app.world_mut().spawn(Window::default()).id();
        for _ in 0..3 {
            app.update();
        }
        assert!(
            !app.world()
                .resource::<cyrus_app::loading::LoadingProgress>()
                .visible
        );
        let root =
            std::env::temp_dir().join(format!("cyrus-missing-install-{}", std::process::id()));
        assert!(!root.exists());
        app.insert_resource(cyrus_assets::install::GameInstall { root });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            app.update();
            let progress = app
                .world()
                .resource::<cyrus_app::loading::LoadingProgress>();
            if progress.fraction == Some(0.) {
                assert!(
                    progress.label.contains("cyrus-missing-install"),
                    "{}",
                    progress.label
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "preparation did not report its failure"
            );
            std::thread::yield_now();
        }
        assert_eq!(
            *app.world().resource::<State<cyrus_game::AppState>>().get(),
            cyrus_game::AppState::SelectingInstall
        );
        assert!(app.world().get::<Window>(window).is_some());
        assert_eq!(
            app.world_mut().query::<&Window>().iter(app.world()).count(),
            1
        );
    }
}
