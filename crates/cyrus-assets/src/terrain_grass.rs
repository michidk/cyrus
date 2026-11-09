//! Where the terrain is grass, read from the terrain's own textures.
//!
//! The original has no grass class: `SURFACE.INI` names sand, rock, wood and water but no turf,
//! and terrain tiles carry only an index into the 64 images of the terrain texture bank. Which
//! pixels show grass is therefore read from the images: a pixel is grass when green is its
//! strongest channel. Sand, dirt, rock and water never are, so an image's share of grass pixels is
//! zero or the greenery it draws, and blended tiles, with grass on one side and sand or dirt on
//! the other, fall between. Modern only: nothing here feeds the original's rendering.

/// Whether an RGBA pixel is grass: visible, with green its strongest channel.
pub fn is_grass([red, green, blue, alpha]: [u8; 4]) -> bool {
    alpha != 0 && green > red && green > blue
}

/// The share of `rgba` pixels that are grass, `0.0..=1.0`; zero for an empty image.
pub fn grass_share(rgba: &[u8]) -> f32 {
    let (pixels, _) = rgba.as_chunks::<4>();
    if pixels.is_empty() {
        return 0.0;
    }
    pixels.iter().filter(|pixel| is_grass(**pixel)).count() as f32 / pixels.len() as f32
}

/// The grass share from which a tile image counts as showing grass. In the installed terrain
/// bank the water and gravel images hold a few stray green pixels (at most 0.3 % of the image)
/// and the faintest grass edge is 3 %.
pub const GRASS_FROM: f32 = 0.01;

/// Whether a tile image with this grass share shows grass.
pub fn has_grass(share: f32) -> bool {
    share >= GRASS_FROM
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREEN: [u8; 4] = [40, 120, 50, 255];
    const SAND: [u8; 4] = [200, 160, 110, 255];
    const WATER: [u8; 4] = [20, 90, 200, 255];
    const ROCK: [u8; 4] = [110, 110, 110, 255];

    fn image(pixels: &[[u8; 4]]) -> Vec<u8> {
        pixels.concat()
    }

    #[test]
    fn only_green_pixels_are_grass() {
        assert_eq!(grass_share(&image(&[GREEN, GREEN])), 1.0);
        assert_eq!(grass_share(&image(&[SAND, WATER, ROCK])), 0.0);
        assert_eq!(grass_share(&image(&[GREEN, SAND])), 0.5);
        assert_eq!(grass_share(&image(&[[40, 120, 50, 0]])), 0.0, "clear");
        assert_eq!(grass_share(&[]), 0.0);
    }

    #[test]
    fn stray_green_pixels_do_not_make_a_tile_grass() {
        assert!(!has_grass(0.0));
        assert!(!has_grass(0.003));
        assert!(has_grass(0.03));
        assert!(has_grass(0.52));
    }
}
