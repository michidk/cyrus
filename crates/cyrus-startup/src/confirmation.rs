//! Mini-confirmation input and pixels, following preservation's book-menu rules.
use cyrus_assets::menu::ConfirmationArtwork;
use image::RgbaImage;
use serde::Serialize;

/// Logical actions sampled through installed bindings; Enter is always a direct key.
#[derive(Clone, Copy, Default)]
pub struct ConfirmationInput {
    pub up: bool,
    pub down: bool,
    pub enter: bool,
    pub action_d: bool,
}

impl ConfirmationInput {
    fn confirming(self) -> bool {
        self.enter || self.action_d
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationChoice {
    Yes,
    No,
}

#[derive(Clone, Copy)]
enum Phase {
    Delay(u32),
    DrainEntry,
    Choosing,
    DrainReturn(ConfirmationChoice),
    Returned(ConfirmationChoice),
}

/// One last-game dialog. A context-three delay starts at its first display tick,
/// rather than at the earlier world-exit request or while assets are loading.
pub struct LastGameConfirmation {
    choice: ConfirmationChoice,
    phase: Phase,
}

impl LastGameConfirmation {
    pub fn displayed(retained_book_context: u8, bios_tick: u32) -> Self {
        Self {
            choice: ConfirmationChoice::Yes,
            phase: if retained_book_context == 3 {
                Phase::Delay(bios_tick)
            } else {
                Phase::Choosing
            },
        }
    }

    pub fn accepting_input(&self) -> bool {
        matches!(self.phase, Phase::Choosing)
    }

    pub fn choice(&self) -> ConfirmationChoice {
        self.choice
    }

    /// Returns once confirmation keys have both released. Repeated calls retain
    /// that result; the caller owns its one-time dispatch and dialog removal.
    pub fn update(
        &mut self,
        bios_tick: u32,
        input: ConfirmationInput,
    ) -> Option<ConfirmationChoice> {
        if let Phase::Delay(start) = self.phase {
            if bios_tick.wrapping_sub(start) < 72 {
                return None;
            }
            self.phase = Phase::DrainEntry;
        }
        if matches!(self.phase, Phase::DrainEntry) {
            if input.confirming() {
                return None;
            }
            self.phase = Phase::Choosing;
        }
        match self.phase {
            Phase::Choosing => {
                if input.confirming() {
                    self.phase = Phase::DrainReturn(self.choice);
                } else if input.up {
                    self.choice = ConfirmationChoice::Yes;
                } else if input.down {
                    self.choice = ConfirmationChoice::No;
                }
            }
            Phase::DrainReturn(choice) if !input.confirming() => {
                self.phase = Phase::Returned(choice);
                return Some(choice);
            }
            Phase::Returned(choice) => return Some(choice),
            _ => {}
        }
        None
    }

    /// Paints over an immutable canonical 640×480 capture. Every redraw starts
    /// from it, so translucent panel and glyph pixels never accumulate.
    pub fn paint(
        &self,
        artwork: &ConfirmationArtwork,
        background: &RgbaImage,
        prompt: &str,
    ) -> anyhow::Result<RgbaImage> {
        anyhow::ensure!(
            background.dimensions() == (640, 480),
            "last-game confirmation: expected 640x480 captured background"
        );
        let mut image = background.clone();
        let picture = &artwork.picture;
        for y in 140..=(140 + u32::from(picture.height)).min(479) {
            for x in 175..=(175 + u32::from(picture.width)).min(639) {
                blend(&mut image, x as i32, y as i32, [10; 3], 120);
            }
        }
        for y in 0..u32::from(picture.height) {
            for x in 0..u32::from(picture.width) {
                let at = ((y * u32::from(picture.width) + x) * 4) as usize;
                let pixel = &picture.rgba[at..at + 4];
                blend(
                    &mut image,
                    x as i32 + 175,
                    y as i32 + 140,
                    [pixel[0], pixel[1], pixel[2]],
                    pixel[3],
                );
            }
        }
        let font = &artwork.font;
        let line_height = i32::from(font.line_height);
        let mut text = |line: &str, y, selected| {
            let [scale, alpha] = artwork.styles[usize::from(selected)];
            let x = 320 + (0 - font.width(line)) / 2;
            let mut pen = x;
            for ch in line.bytes() {
                for (offset, shadow) in [(2, true), (0, false)] {
                    font.draw_char(ch, [pen + offset, y + offset], |x, y, index| {
                        let colour = if shadow {
                            [0; 3]
                        } else {
                            font.colour(index)
                                .map(|c| ((u32::from(c) * u32::from(scale) + 127) / 255) as u8)
                        };
                        let alpha = if shadow {
                            (u16::from(alpha) * 200 / 256) as u8
                        } else {
                            alpha
                        };
                        blend(&mut image, x, y, colour, alpha);
                    });
                }
                pen += font.advance(ch);
            }
        };
        for (line, value) in prompt.split('\n').enumerate() {
            text(value, 160 + line as i32 * line_height, true);
        }
        text(
            "Yes",
            160 + 4 * line_height,
            self.choice == ConfirmationChoice::Yes,
        );
        text(
            "No",
            160 + 5 * line_height,
            self.choice == ConfirmationChoice::No,
        );
        Ok(image)
    }
}

fn blend(image: &mut RgbaImage, x: i32, y: i32, colour: [u8; 3], alpha: u8) {
    if !(0..640).contains(&x) || !(0..480).contains(&y) {
        return;
    }
    let pixel = image.get_pixel_mut(x as u32, y as u32);
    let source_alpha = u32::from(alpha);
    let destination_alpha = u32::from(pixel[3]);
    let combined = source_alpha * 255 + destination_alpha * (255 - source_alpha);
    if combined == 0 {
        return;
    }
    for c in 0..3 {
        pixel[c] = ((u32::from(colour[c]) * source_alpha * 255
            + u32::from(pixel[c]) * destination_alpha * (255 - source_alpha)
            + combined / 2)
            / combined) as u8;
    }
    pixel[3] = ((combined + 127) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirms_previous_selection_and_waits_for_both_keys_to_release() {
        let mut dialog = LastGameConfirmation::displayed(1, 0);
        assert_eq!(dialog.choice(), ConfirmationChoice::Yes);
        assert_eq!(
            dialog.update(
                0,
                ConfirmationInput {
                    down: true,
                    enter: true,
                    action_d: true,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(dialog.choice(), ConfirmationChoice::Yes);
        assert_eq!(
            dialog.update(
                1,
                ConfirmationInput {
                    action_d: true,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(
            dialog.update(2, ConfirmationInput::default()),
            Some(ConfirmationChoice::Yes)
        );
    }

    #[test]
    fn up_wins_and_direction_can_select_no_before_confirmation() {
        let mut dialog = LastGameConfirmation::displayed(1, 0);
        dialog.update(
            0,
            ConfirmationInput {
                down: true,
                ..Default::default()
            },
        );
        assert_eq!(dialog.choice(), ConfirmationChoice::No);
        dialog.update(
            1,
            ConfirmationInput {
                up: true,
                down: true,
                ..Default::default()
            },
        );
        assert_eq!(dialog.choice(), ConfirmationChoice::Yes);
        dialog.update(
            2,
            ConfirmationInput {
                down: true,
                ..Default::default()
            },
        );
        dialog.update(
            3,
            ConfirmationInput {
                enter: true,
                ..Default::default()
            },
        );
        assert_eq!(
            dialog.update(4, ConfirmationInput::default()),
            Some(ConfirmationChoice::No)
        );
    }

    #[test]
    fn context_three_delays_72_wrapping_ticks_then_drains_held_input_once() {
        let start = u32::MAX - 30;
        let mut dialog = LastGameConfirmation::displayed(3, start);
        let down = ConfirmationInput {
            down: true,
            ..Default::default()
        };
        dialog.update(start.wrapping_add(71), down);
        assert_eq!(dialog.choice(), ConfirmationChoice::Yes);
        dialog.update(
            start.wrapping_add(72),
            ConfirmationInput {
                enter: true,
                ..down
            },
        );
        dialog.update(start.wrapping_add(73), down);
        assert_eq!(dialog.choice(), ConfirmationChoice::No);
        dialog.update(
            start.wrapping_add(75),
            ConfirmationInput {
                action_d: true,
                ..Default::default()
            },
        );
        assert_eq!(
            dialog.update(start.wrapping_add(76), ConfirmationInput::default()),
            Some(ConfirmationChoice::No)
        );
    }

    #[test]
    fn redraw_restores_capture_and_panel_includes_boundary_pixels() {
        let artwork = ConfirmationArtwork {
            picture: cyrus_assets::menu::MenuPicture {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
            font: cyrus_text::Font::default(),
            styles: [[255; 2]; 2],
        };
        let background = RgbaImage::from_pixel(640, 480, image::Rgba([100, 100, 100, 255]));
        let dialog = LastGameConfirmation::displayed(1, 0);
        let first = dialog.paint(&artwork, &background, "").unwrap();
        assert_eq!(first.get_pixel(175, 140).0, [58, 58, 58, 255]);
        assert_eq!(first.get_pixel(176, 141).0, [58, 58, 58, 255]);
        assert_eq!(first.get_pixel(177, 141).0, [100, 100, 100, 255]);
        assert_eq!(first, dialog.paint(&artwork, &background, "").unwrap());
        assert_eq!(background.get_pixel(175, 140).0, [100, 100, 100, 255]);
    }
    #[test]
    fn prompt_keeps_case_and_choices_keep_fixed_rows_and_normal_colour_factor() {
        let mut font = cyrus_text::Font {
            start: 0,
            line_height: 8,
            glyphs: vec![
                cyrus_text::Glyph {
                    enabled: true,
                    width: 1,
                    height: 1,
                    offset: [0, 0],
                    pixels: vec![1],
                };
                256
            ],
            palette: vec![[0; 3], [63; 3], [63, 0, 0]],
        };
        font.glyphs[usize::from(b'a')].pixels[0] = 2;
        let artwork = ConfirmationArtwork {
            picture: cyrus_assets::menu::MenuPicture {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
            font,
            styles: [[128, 255], [255, 255]],
        };
        let background = RgbaImage::from_pixel(640, 480, image::Rgba([0, 0, 0, 255]));
        let mut dialog = LastGameConfirmation::displayed(1, 0);
        let yes = dialog.paint(&artwork, &background, "a\nB\nC").unwrap();
        assert_eq!(yes.get_pixel(319, 160).0, [252, 0, 0, 255]);
        assert_eq!(yes.get_pixel(317, 192).0, [252, 252, 252, 255]);
        assert_eq!(yes.get_pixel(318, 200).0, [126, 126, 126, 255]);
        dialog.update(
            0,
            ConfirmationInput {
                down: true,
                ..Default::default()
            },
        );
        let no = dialog.paint(&artwork, &background, "a").unwrap();
        assert_eq!(no.get_pixel(317, 192).0, [126, 126, 126, 255]);
        assert_eq!(no.get_pixel(318, 200).0, [252, 252, 252, 255]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn installed_confirmation_prepares_portable_artwork_and_renders_both_choices() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = cyrus_assets::install::GameInstall::open(&root) else {
            eprintln!("skipped: no install (last-game confirmation)");
            return;
        };
        let cache = std::env::temp_dir().join(format!("cyrus-confirmation-{}", std::process::id()));
        cyrus_assets::startup::export_menu(&install, &cache).unwrap();
        let book: cyrus_assets::menu::MenuAsset =
            serde_json::from_slice(&std::fs::read(cache.join("menu/book.menu")).unwrap()).unwrap();
        book.validate().unwrap();
        assert!(book.confirmation.font.glyph(b'Y').is_some());
        assert!(book.confirmation.font.glyph(b'N').is_some());
        let background = RgbaImage::from_pixel(640, 480, image::Rgba([80, 90, 100, 255]));
        let mut dialog = LastGameConfirmation::displayed(1, 0);
        let first = dialog
            .paint(
                &book.confirmation,
                &background,
                "Example\nMixed case description",
            )
            .unwrap();
        dialog.update(
            0,
            ConfirmationInput {
                down: true,
                ..Default::default()
            },
        );
        let second = dialog
            .paint(
                &book.confirmation,
                &background,
                "Example\nMixed case description",
            )
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(first.get_pixel(174, 140), background.get_pixel(174, 140));
        assert_eq!(first.get_pixel(639, 479), background.get_pixel(639, 479));
        std::fs::remove_dir_all(cache).unwrap();
    }
}
