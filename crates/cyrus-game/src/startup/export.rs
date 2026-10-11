use crate::install::GameInstall;
use anyhow::Context;
use rgpre::import::{fnt::parse_fnt, glide_splash::parse_glide_splash, gxa::parse_gxa_file};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

/// Generates local presentation images from the player's own overlay and GXA.
/// The install is read-only; every output stays under the supplied cache path.
pub fn export_startup(install: &GameInstall, cache: &Path) -> anyhow::Result<()> {
    let overlay = install
        .root
        .parent()
        .context("install has no parent directory")?
        .join("DOSBOX/glide2x_emu.ovl");
    let overlay = if overlay.is_file() {
        overlay
    } else {
        install.root.join("glide2x_emu.ovl")
    };
    let title = install.root.join("system/STARTUP2.GXA");
    let read = |path: &Path| {
        std::fs::read(path).with_context(|| format!("startup: reading {}", path.display()))
    };
    let overlay_bytes = read(&overlay)?;
    let title_bytes = read(&title)?;
    let system = read(&install.root.join("SYSTEM.INI"))?;
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
    let mut font_path = install.root.clone();
    for part in font_name.split(['\\', '/']) {
        anyhow::ensure!(
            !part.is_empty() && part != ".." && part != ".",
            "startup: invalid gui_low_font path {font_name}"
        );
        font_path = crate::scene::find_file(&font_path, part)
            .with_context(|| format!("startup: gui_low_font file {font_name} is missing"))?;
    }
    let font_bytes = read(&font_path)?;
    let font = cyrus_text::Font::from(
        &parse_fnt(&font_bytes).with_context(|| format!("startup: {}", font_path.display()))?,
    );
    let mut fingerprint = DefaultHasher::new();
    (3u32, &overlay_bytes, &title_bytes, &font_bytes).hash(&mut fingerprint);
    let fingerprint = fingerprint.finish().to_string();
    let output = cache.join("startup");
    let stamp = output.join("source.txt");
    if std::fs::read_to_string(&stamp).ok().as_deref() == Some(&fingerprint)
        && (1..75).all(|frame| output.join(format!("splash-{frame:02}.png")).is_file())
        && output.join("title.png").is_file()
    {
        return Ok(());
    }
    let splash = parse_glide_splash(&overlay_bytes)
        .with_context(|| format!("startup: {}", overlay.display()))?;
    let decoded =
        parse_gxa_file(&title_bytes).with_context(|| format!("startup: {}", title.display()))?;
    let [title_frame] = decoded.frames.as_slice() else {
        anyhow::bail!("startup: {}: expected one title frame", title.display());
    };
    if (title_frame.width, title_frame.height) != (640, 480) {
        anyhow::bail!("startup: {}: unsupported title dimensions", title.display());
    }
    std::fs::create_dir_all(&output)
        .with_context(|| format!("startup: creating {}", output.display()))?;
    // An interrupted export must regenerate rather than accept a partial cache.
    if stamp.exists() {
        std::fs::remove_file(&stamp)?;
    }
    for frame in 1..75 {
        let path = output.join(format!("splash-{frame:02}.png"));
        super::raster::render(&splash, frame)
            .save(&path)
            .with_context(|| format!("startup: writing {}", path.display()))?;
    }
    let path = output.join("title.png");
    let mut image = image::RgbaImage::from_raw(640, 480, title_frame.rgba.clone())
        .context("startup: title pixel count is invalid")?;
    draw_text(&mut image, &font, "Version 1.0.1.0", 10, [32, 32, 32]);
    let text = "Loading...";
    draw_text(&mut image, &font, text, 630 - font.width(text), [50, 0, 0]);
    image
        .save(&path)
        .with_context(|| format!("startup: writing {}", path.display()))?;
    std::fs::write(&stamp, fingerprint)?;
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
