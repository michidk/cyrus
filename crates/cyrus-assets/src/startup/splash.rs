//! Splash animation frames shared by startup and asset inspection.
use anyhow::{Context, Result};
use image::RgbaImage;
use rgpre::import::{
    glide_splash::parse_glide_splash,
    glide_splash_dll::{WindowsGlideSplash, parse_windows_glide_splash},
};

pub(super) fn windows_frames(
    splash: &WindowsGlideSplash,
    mut emit: impl FnMut(usize, RgbaImage) -> Result<()>,
) -> Result<()> {
    let final_image = super::raster::render(splash, &splash.final_draws, 1.0);
    let mut fade_images = [final_image.clone(), final_image];
    for frame in 0..crate::SPLASH_FRAMES {
        let image = if frame < splash.frames.len() {
            let visibility = if frame < 24 {
                0.0
            } else {
                ((frame - 24) as f32 / 8.0).min(1.0)
            };
            super::raster::render(splash, &splash.frames[frame], visibility)
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
        emit(frame, image)?;
    }
    Ok(())
}

pub(crate) fn preview(
    bytes: &[u8],
    windows: bool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<crate::Picture>> {
    let count = if windows {
        crate::SPLASH_FRAMES
    } else {
        crate::DOS_SPLASH_FRAMES
    };
    let mut frames = Vec::with_capacity(count);
    let mut emit = |frame: usize, image: RgbaImage| {
        frames.push(crate::Picture {
            label: format!("Frame {frame}"),
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
            indices: None,
            embedded_palette: false,
        });
        progress(frame + 1, count);
        Ok(())
    };
    if windows {
        let splash = parse_windows_glide_splash(bytes).context("Windows splash")?;
        windows_frames(&splash, &mut emit)?;
    } else {
        let splash = parse_glide_splash(bytes).context("DOS splash")?;
        for frame in 0..crate::DOS_SPLASH_FRAMES {
            emit(
                frame,
                super::dos_raster::render(&splash, frame + super::dos_raster::FIRST_FRAME),
            )?;
        }
    }
    Ok(frames)
}
