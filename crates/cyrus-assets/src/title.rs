//! Portable title lettering; the installed low font and palette determine every label pixel.
use bevy::prelude::{Asset, TypePath};
use serde::{Deserialize, Serialize};

/// Installed font and palette-modulated tints, separate from the single title background.
#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct TitleLabels {
    /// SYSTEM.INI gui_low_font, decoded through preservation.
    pub font: cyrus_text::Font,
    /// Encoded RGB after low-font texture modulation of the version tint.
    pub version_colour: [u8; 3],
    /// Encoded RGB after low-font texture modulation of the status tint.
    pub status_colour: [u8; 3],
}

impl TitleLabels {
    /// Paint labels into artwork pixels using the preserved startup title-label layout.
    /// The 640×480 source frame is centered at the artwork's height, before window fitting.
    pub fn draw(&self, status: &str, size: [u32; 2], mut plot: impl FnMut(u32, u32, [u8; 4])) {
        let scale = size[1] as f32 / 480.0;
        let left = (size[0] as f32 - 640.0 * scale) / 2.0;
        let edge = |cell: i32, origin: f32| (origin + cell as f32 * scale).round() as i32;
        for (text, x, color) in [
            ("Version 1.0.1.0", 10, self.version_colour),
            (status, 630 - self.font.width(status), self.status_colour),
        ] {
            self.font.draw(text, [x, 467], |x, y, _| {
                if !(0..640).contains(&x) || !(0..480).contains(&y) {
                    return;
                }
                for py in edge(y, 0.0)..edge(y + 1, 0.0) {
                    for px in edge(x, left)..edge(x + 1, left) {
                        if (0..size[0] as i32).contains(&px) && (0..size[1] as i32).contains(&py) {
                            plot(px as u32, py as u32, [color[0], color[1], color[2], 255]);
                        }
                    }
                }
            });
        }
    }
}
