use anyhow::Context;
use rgpre::import::{
    fnt::parse_fnt, glide_splash_dll::parse_windows_glide_splash, gxa::parse_gxa_file,
};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

/// The title artwork cyrus ships: a 16:9 redraw of the installed 640×480 title, without labels.
const TITLE_ARTWORK: &[u8] = include_bytes!("../../assets/title.png");

/// Generates local presentation images from the player's own splash DLL, GXA and label font.
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
    (10u32, &dll_bytes, &title_bytes, &font_bytes, TITLE_ARTWORK).hash(&mut fingerprint);
    let fingerprint = fingerprint.finish().to_string();
    let directory = Path::new("startup");
    let stamp = directory.join("source.txt");
    if output
        .read(&stamp)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .as_deref()
        == Some(&fingerprint)
        && output.exists(&directory.join("splash.splash"))
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
    let asset = super::splash_data::export(&splash);
    anyhow::ensure!(
        splash.frames.len() + 2 * splash.frames_per_second as usize == crate::SPLASH_FRAMES,
        "startup: {dll}: the splash runs {} frames, expected {}",
        splash.frames.len() + 2 * splash.frames_per_second as usize,
        crate::SPLASH_FRAMES
    );
    let path = directory.join("splash.splash");
    output
        .publish(&path, &serde_json::to_vec(&asset)?)
        .with_context(|| format!("startup: writing {}", path.display()))?;
    let mut background = image::load_from_memory(TITLE_ARTWORK)
        .context("startup: decoding the shipped title artwork")?
        .to_rgba8();
    let palette = rgpre::import::palette::Palette {
        colors: title_frame
            .palette
            .map(|rgb| rgb.map(|component| component >> 2)),
    };
    let version_colour = title_colour(&title_frame.palette, palette.match_vga([32; 3]));
    let status_colour = title_colour(&title_frame.palette, palette.match_vga([50, 0, 0]));
    draw_text(
        &mut background,
        &font,
        "Version 1.0.1.0",
        10,
        version_colour,
    );
    for (name, text) in [
        ("title.png", "Loading..."),
        ("title-preparing.png", "Preparing menu and HUD..."),
        ("title-menu.png", "Loading menu..."),
        ("title-sound.png", "Loading sound..."),
    ] {
        let mut image = background.clone();
        draw_text(
            &mut image,
            &font,
            text,
            630 - font.width(text),
            status_colour,
        );
        let path = directory.join(name);
        output
            .image(&path, &image)
            .with_context(|| format!("startup: writing {}", path.display()))?;
    }
    output.publish(&stamp, fingerprint.as_bytes())?;
    Ok(())
}

/// The low font's indexed texture and palette-selected tint multiply in Glide.
/// See preservation's startup title-label rules.
fn title_colour(palette: &[[u8; 3]; 256], tint: u8) -> [u8; 3] {
    std::array::from_fn(|channel| {
        (u16::from(palette[161][channel]) * u16::from(palette[usize::from(tint)][channel]) / 255)
            as u8
    })
}

/// Draws one flat-colour line with its pen at (`x`, 467) of the original 640×480 title frame.
/// The artwork is shown filling the window, so that frame is its centred 4:3 region at the
/// artwork's height; each font pixel becomes the block of artwork pixels it covers there.
fn draw_text(
    image: &mut image::RgbaImage,
    font: &cyrus_text::Font,
    text: &str,
    x: i32,
    color: [u8; 3],
) {
    let scale = image.height() as f32 / 480.0;
    let left = (image.width() as f32 - 640.0 * scale) / 2.0;
    let edge = |cell: i32, origin: f32| (origin + cell as f32 * scale).round() as i32;
    font.draw(text, [x, 467], |x, y, _| {
        if !(0..640).contains(&x) || !(0..480).contains(&y) {
            return;
        }
        for py in edge(y, 0.0)..edge(y + 1, 0.0) {
            for px in edge(x, left)..edge(x + 1, left) {
                if (0..image.width() as i32).contains(&px)
                    && (0..image.height() as i32).contains(&py)
                {
                    image.put_pixel(
                        px as u32,
                        py as u32,
                        image::Rgba([color[0], color[1], color[2], 255]),
                    );
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn title_tint_is_modulated_by_the_low_font_texture_colour() {
        let palette = [[255; 3]; 256];
        let mut palette = palette;
        palette[7] = [128, 64, 32];
        palette[161] = [200, 100, 50];
        assert_eq!(super::title_colour(&palette, 7), [100, 25, 6]);
    }
}
