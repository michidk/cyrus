//! Button labels the original launcher has no bitmap for, assembled from the letters of the bitmaps it
//! does have, so they look exactly like the originals.
//!
//! The lettering of the original buttons is an italic face drawn with a dark shadow over the parchment,
//! all baked into each button's bitmap, and neighbouring letters overlap. So a letter is taken as a
//! strip of columns of the bitmap (letter, shadow and the parchment behind it), cut where the red of
//! one letter ends and the next begins, and a label is strips set side by side.
use crate::artwork::{Artwork, Button, Image};

/// Columns `from..to` of the original button bitmap `button`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strip {
    pub button: usize,
    pub from: u32,
    pub to: u32,
}

const fn strip(button: usize, from: u32, to: u32) -> Strip {
    Strip { button, from, to }
}

/// The size of the original bitmaps the strips below were measured on: PLAYREDGUARD and GAMEOPTIONS.
/// Bitmaps of any other size (another edition of the launcher) are not cut.
const SIZES: [(usize, u32, u32); 2] = [(0, 395, 54), (1, 290, 43)];

/// "MODS": the M and the O and the S of GAME OPTIONS, and the last D of PLAY REDGUARD.
pub const MODS: [Strip; 4] = [
    strip(1, 67, 98),
    strip(1, 130, 156),
    strip(0, 352, 389),
    strip(1, 261, 290),
];

/// The strip as an image, brought to `height` rows when it comes from a bitmap of another height.
fn cut(art: &Artwork, state: fn(&Button) -> &Image, strip: Strip, height: u32) -> Option<Image> {
    let source = state(art.buttons.get(strip.button)?);
    let known = SIZES
        .iter()
        .any(|&(button, w, h)| button == strip.button && (source.width, source.height) == (w, h));
    if !known || strip.from >= strip.to || strip.to > source.width {
        return None;
    }
    let width = strip.to - strip.from;
    let mut rgba = Vec::with_capacity((width * source.height * 4) as usize);
    for y in 0..source.height {
        let row = ((y * source.width + strip.from) * 4) as usize;
        rgba.extend_from_slice(&source.rgba[row..row + (width * 4) as usize]);
    }
    Some(resized(
        &Image {
            width,
            height: source.height,
            rgba,
        },
        height,
    ))
}

/// `image` with `height` rows, keeping its proportions; the nearest pixels are taken.
fn resized(image: &Image, height: u32) -> Image {
    if image.height == height {
        return image.clone();
    }
    let width = ((image.width * height) as f32 / image.height as f32)
        .round()
        .max(1.0) as u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let sy = (y * image.height / height).min(image.height - 1);
        for x in 0..width {
            let sx = (x * image.width / width).min(image.width - 1);
            let at = ((sy * image.width + sx) * 4) as usize;
            rgba.extend_from_slice(&image.rgba[at..at + 4]);
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

/// Images of equal height side by side.
fn join(parts: &[Image]) -> Option<Image> {
    let height = parts.first()?.height;
    let width: u32 = parts.iter().map(|part| part.width).sum();
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let mut left = 0u32;
    for part in parts {
        let row_bytes = (part.width * 4) as usize;
        for y in 0..height {
            let from = (y * part.width * 4) as usize;
            let to = ((y * width + left) * 4) as usize;
            rgba[to..to + row_bytes].copy_from_slice(&part.rgba[from..from + row_bytes]);
        }
        left += part.width;
    }
    Some(Image {
        width,
        height,
        rgba,
    })
}

/// `image` with its parchment made transparent, leaving the lettering and its shadow. The parchment
/// baked into a bitmap is a different piece of the map than the one behind the button, so left opaque
/// it shows as a patch with seams. Lettering is red to yellow (saturated) or the dark shadow.
pub fn cut_out(image: &Image) -> Image {
    let mut image = image.clone();
    for pixel in image.rgba.as_chunks_mut::<4>().0 {
        let (high, low) = (
            pixel[..3].iter().max().copied().unwrap_or(0) as f32,
            pixel[..3].iter().min().copied().unwrap_or(0) as f32,
        );
        let lettering = high < LETTERING_DARK || (high - low) / high > LETTERING_SATURATION;
        if !lettering {
            pixel[3] = 0;
        }
    }
    image
}

/// A pixel is lettering when its brightest channel is below this (the shadow)...
const LETTERING_DARK: f32 = 110.0;
/// ...or when its saturation is above this (the red, orange and yellow letters; parchment is paler).
const LETTERING_SATURATION: f32 = 0.55;

/// A label made of `strips`, in the three states of an original button, at the height of the Game
/// Options bitmap. `None` when a strip's bitmap is not the one the strips were measured on.
pub fn label(art: &Artwork, strips: &[Strip]) -> Option<Button> {
    let height = art.buttons.get(1)?.up.height;
    let state = |pick: fn(&Button) -> &Image| {
        let parts: Option<Vec<Image>> = strips
            .iter()
            .map(|strip| cut(art, pick, *strip, height))
            .collect();
        join(&parts?)
    };
    Some(Button {
        up: state(|b| &b.up)?,
        focus: state(|b| &b.focus)?,
        down: state(|b| &b.down)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Image {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    fn art(buttons: Vec<(u32, u32)>) -> Artwork {
        let button = |(w, h)| Button {
            up: gradient(w, h),
            focus: gradient(w, h),
            down: gradient(w, h),
        };
        Artwork {
            background: gradient(640, 480),
            buttons: buttons.into_iter().map(button).collect(),
            sounds: Default::default(),
        }
    }

    #[test]
    fn only_the_parchment_becomes_transparent() {
        let image = Image {
            width: 4,
            height: 1,
            rgba: vec![
                230, 200, 130, 255, // parchment
                220, 30, 20, 255, // red letter
                255, 220, 0, 255, // yellow letter
                40, 20, 10, 255, // shadow
            ],
        };
        let alphas: Vec<u8> = cut_out(&image).rgba.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alphas, [0, 255, 255, 255]);
    }

    #[test]
    fn resizing_keeps_the_proportions() {
        let image = gradient(4, 8);
        let small = resized(&image, 4);
        assert_eq!((small.width, small.height), (2, 4));
        assert_eq!(resized(&image, 8), image);
    }

    #[test]
    fn joining_sets_images_side_by_side() {
        let joined = join(&[gradient(2, 2), gradient(3, 2)]).unwrap();
        assert_eq!((joined.width, joined.height), (5, 2));
        // The second image starts at column 2 with its own column 0.
        assert_eq!(&joined.rgba[8..12], &[0, 0, 0, 255]);
        assert_eq!(&joined.rgba[12..16], &[1, 0, 0, 255]);
        // Its second row starts at column 2 of the joined second row.
        let second_row = (5 * 4 + 8) as usize;
        assert_eq!(&joined.rgba[second_row..second_row + 4], &[0, 1, 0, 255]);
        assert!(join(&[]).is_none());
    }

    #[test]
    fn a_strip_outside_its_bitmap_or_from_an_unknown_size_is_refused() {
        let known = art(vec![(395, 54), (290, 43)]);
        assert!(cut(&known, |b| &b.up, strip(1, 67, 98), 43).is_some());
        assert!(cut(&known, |b| &b.up, strip(1, 67, 400), 43).is_none());
        assert!(cut(&known, |b| &b.up, strip(1, 98, 67), 43).is_none());
        let other = art(vec![(300, 54), (290, 43)]);
        assert!(cut(&other, |b| &b.up, strip(0, 352, 389), 43).is_none());
    }

    #[test]
    fn a_label_is_as_tall_as_the_game_options_bitmap() {
        let label = label(&art(vec![(395, 54), (290, 43)]), &MODS).unwrap();
        assert_eq!(label.up.height, 43);
        // 31 + 26 + (37 columns scaled from 54 to 43 rows) + 29.
        assert_eq!(label.up.width, 31 + 26 + 29 + 29);
    }
}
