use anyhow::Context;
use rgpre::import::{
    fnt::parse_fnt, glide_splash_dll::parse_windows_glide_splash, gxa::parse_gxa_file,
};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

/// Generates local presentation images from the player's own splash DLL and GXA.
/// The install is read-only; every output stays under the supplied cache path.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export_startup(install: &crate::install::GameInstall, cache: &Path) -> anyhow::Result<()> {
    let source = crate::native::Files::scan(install, false)
        .context("startup: locating installed 3DfxSpl2.dll")?;
    prepare_startup(&source, &mut crate::preparation::DiskOutput(cache))
}

pub(crate) fn prepare_startup(
    source: &dyn crate::source::ReadAssets,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    let dll = crate::runtime::geometry::find_name(source, "3DfxSpl2.dll")
        .context("startup: installed 3DfxSpl2.dll is missing")?;
    let title = "system/STARTUP2.GXA";
    let dll_bytes = source.read(&dll)?;
    let title_bytes = crate::preparation::read(source, title)?;
    let system = crate::preparation::read(source, "SYSTEM.INI")?;
    let system_text = std::str::from_utf8(&system).context("startup: SYSTEM.INI is not UTF-8")?;
    let mut section = "";
    let mut font_name = None;
    for line in system_text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = name;
        }
        if section.eq_ignore_ascii_case("system")
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("gui_low_font")
        {
            font_name = Some(value.trim());
        }
    }
    let font_name = font_name.context("startup: SYSTEM.INI [system] gui_low_font is missing")?;
    let font_path = crate::preparation::path(source, font_name)?;
    let font_bytes = source.read(&font_path)?;
    let font = cyrus_text::Font::from(
        &parse_fnt(&font_bytes).with_context(|| format!("startup: {}", font_path))?,
    );
    let mut fingerprint = DefaultHasher::new();
    (5u32, &dll_bytes, &title_bytes, &font_bytes).hash(&mut fingerprint);
    let fingerprint = fingerprint.finish().to_string();
    let directory = Path::new("startup");
    let stamp = directory.join("source.txt");
    if output
        .read(&stamp)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .as_deref()
        == Some(&fingerprint)
        && (0..crate::SPLASH_FRAMES)
            .all(|frame| output.exists(&directory.join(format!("splash-{frame:02}.png"))))
        && [
            "title.png",
            "title-preparing.png",
            "title-menu.png",
            "title-sound.png",
        ]
        .iter()
        .all(|name| output.exists(&directory.join(name)))
    {
        return Ok(());
    }
    let splash =
        parse_windows_glide_splash(&dll_bytes).with_context(|| format!("startup: {}", dll))?;
    let decoded = parse_gxa_file(&title_bytes).with_context(|| format!("startup: {}", title))?;
    let [title_frame] = decoded.frames.as_slice() else {
        anyhow::bail!("startup: {}: expected one title frame", title);
    };
    if (title_frame.width, title_frame.height) != (640, 480) {
        anyhow::bail!("startup: {}: unsupported title dimensions", title);
    }
    output.remove(&stamp)?;
    let final_image = super::raster::render(&splash, &splash.final_draws, 1.0);
    let mut fade_images = [final_image.clone(), final_image];
    for frame in 0..crate::SPLASH_FRAMES {
        let image = if frame < splash.frames.len() {
            let visibility = if frame < 24 {
                0.0
            } else {
                ((frame - 24) as f32 / 8.0).min(1.0)
            };
            super::raster::render(&splash, &splash.frames[frame], visibility)
        } else {
            let step = frame - splash.frames.len();
            let alpha = (102 * step.saturating_sub(1) / (2 * splash.frames_per_second as usize))
                as f32
                / 255.0;
            let image = &mut fade_images[step % 2];
            for pixel in image.pixels_mut() {
                for channel in &mut pixel.0[..3] {
                    *channel = (f32::from(*channel) * (1.0 - alpha)) as u8;
                }
            }
            image.clone()
        };
        let path = directory.join(format!("splash-{frame:02}.png"));
        output
            .image(&path, &image)
            .with_context(|| format!("startup: writing {}", path.display()))?;
    }
    let mut background = image::RgbaImage::from_raw(640, 480, title_frame.rgba.clone())
        .context("startup: title pixel count is invalid")?;
    draw_text(&mut background, &font, "Version 1.0.1.0", 10, [32, 32, 32]);
    for (name, text) in [
        ("title.png", "Loading..."),
        ("title-preparing.png", "Preparing menu and HUD..."),
        ("title-menu.png", "Loading menu..."),
        ("title-sound.png", "Loading sound..."),
    ] {
        let mut image = background.clone();
        draw_text(&mut image, &font, text, 630 - font.width(text), [50, 0, 0]);
        let path = directory.join(name);
        output
            .image(&path, &image)
            .with_context(|| format!("startup: writing {}", path.display()))?;
    }
    output.publish(&stamp, fingerprint.as_bytes())?;
    Ok(())
}

/// Draws one flat-colour line with its pen at (`x`, 467).
fn draw_text(
    image: &mut image::RgbaImage,
    font: &cyrus_text::Font,
    text: &str,
    x: i32,
    color: [u8; 3],
) {
    font.draw(text, [x, 467], |x, y, _| {
        if (0..640).contains(&x) && (0..480).contains(&y) {
            image.put_pixel(
                x as u32,
                y as u32,
                image::Rgba([color[0], color[1], color[2], 255]),
            );
        }
    });
}
