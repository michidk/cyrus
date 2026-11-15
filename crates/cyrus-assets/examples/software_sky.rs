//! Render one software sky through the portable preparation and presentation interface.
use cyrus_assets::{
    install::GameInstall,
    native::Files,
    software_sky::{SoftwareSky, View},
};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.len() == 7,
        "usage: software_sky INSTALL WORLD DRAW HEADING PITCH SCREEN_SCALE OUTPUT.png"
    );
    let install = GameInstall {
        root: std::fs::canonicalize(&args[0])?,
    };
    let files = Files::open(&install)?;
    let sky = SoftwareSky::prepare(&files, args[1].parse()?)?;
    let pixels = sky.rgba(View {
        draw: args[2].parse()?,
        angles: [args[3].parse()?, args[4].parse()?],
        screen_scale: args[5].parse()?,
    })?;
    image::save_buffer(&args[6], &pixels, 640, 480, image::ColorType::Rgba8)?;
    Ok(())
}
