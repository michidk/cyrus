//! Independent native and browser entry points for the shared asset viewer.
#[cfg(target_arch = "wasm32")]
mod browser;
mod loading;
mod preview;
mod ui;

use bevy::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    use clap::Parser;
    #[derive(Parser)]
    #[command(
        version,
        about = "Explore your original Redguard assets. The install is read only."
    )]
    struct Args {
        #[command(flatten)]
        source: cyrus_app::launch::InstallArgs,
        #[command(flatten)]
        selection: cyrus_app::launch::ViewerOptions,
    }
    let args = Args::parse();
    let Some(install) = args.source.locate_or_prompt("cyrus-viewer")? else {
        return Ok(());
    };
    install.cache_path(std::path::Path::new(cyrus_assets::CACHE_DIR))?;
    let mut app = application()?;
    app.insert_resource(loading::NativeSource(install))
        .insert_resource(args.selection)
        .add_systems(Update, loading::initial_selection.after(loading::poll));
    app.run();
    Ok(())
}
#[cfg(target_arch = "wasm32")]
fn main() {
    cyrus_app::launch::from_query::<cyrus_app::launch::ViewerOptions>()
        .expect("validated viewer URL options");
    application().expect("viewer setup").run();
}

fn application() -> anyhow::Result<App> {
    let mut app = App::new();
    #[cfg(not(target_arch = "wasm32"))]
    cyrus_app::register_cache(&mut app)?;
    #[cfg(target_arch = "wasm32")]
    browser::install(&mut app)?;
    app.add_plugins(
        DefaultPlugins
            .set(cyrus_app::window("cyrus · asset viewer"))
            .set(AssetPlugin {
                meta_check: bevy::asset::AssetMetaCheck::Never,
                ..default()
            }),
    )
    .insert_resource(ClearColor(Color::srgb(0.055, 0.073, 0.095)))
    .add_plugins((cyrus_app::EguiPlugin, cyrus_app::LoadingOverlayPlugin))
    .insert_resource(cyrus_app::bevy_egui::EguiGlobalSettings {
        auto_create_primary_context: false,
        ..default()
    })
    .init_resource::<ui::Viewer>()
    .init_asset::<loading::Blob>()
    .init_asset_loader::<loading::BlobLoader>()
    .add_systems(Startup, (preview::setup, loading::start))
    .add_systems(
        Update,
        (
            loading::poll,
            preview::animate,
            preview::sync,
            preview::frame_camera,
            preview::movie,
            preview::edges,
        )
            .chain(),
    )
    .add_systems(cyrus_app::bevy_egui::EguiPrimaryContextPass, ui::draw);
    Ok(app)
}
