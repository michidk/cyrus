//! Where the terrain is grass, read from the terrain's own textures.
//!
//! The original has no grass class: `SURFACE.INI` names sand, rock, wood and water but no turf,
//! and terrain tiles carry only an index into the 64 images of the terrain texture bank. Which
//! images show grass is therefore read from their pixels: a pixel is grass when green is its
//! strongest channel. Sand, dirt, rock and water never are, so each image's share of grass pixels
//! is zero or the greenery it draws, and blended tiles, with grass on one side and sand or dirt on
//! the other, fall between. Coverage is that share relative to the greenest image of the bank, so
//! the bank's solid grass tiles are 1 and its blended edges a fraction. Modern only: nothing here
//! feeds the original's rendering.
use rgpre::gltf::TextureCache;

/// The terrain tile texture indices, `0..64`, all in the one terrain texture bank.
pub const TERRAIN_TILES: u8 = 64;

/// Whether an RGBA pixel is grass: visible, with green its strongest channel.
fn is_grass([red, green, blue, alpha]: [u8; 4]) -> bool {
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

/// Coverage below this counts as none: the water and gravel images hold a few stray green pixels
/// (under half a percent of the greenest image), while the faintest real grass edge is over six.
const NOISE_FLOOR: f32 = 0.02;

/// Each share relative to the largest one, so the greenest image is 1, with stray pixels dropped;
/// all zero when none is green.
pub fn coverage_of(shares: &[f32]) -> Vec<f32> {
    let greenest = shares.iter().copied().fold(0.0, f32::max);
    shares
        .iter()
        .map(|share| {
            let coverage = if greenest > 0.0 {
                share / greenest
            } else {
                0.0
            };
            if coverage < NOISE_FLOOR {
                0.0
            } else {
                coverage
            }
        })
        .collect()
}

/// The grass coverage of each terrain tile image of `terrain_bank`, indexed by tile texture index.
/// An image the bank does not hold has no grass.
pub fn terrain_grass(textures: &mut TextureCache, terrain_bank: u16) -> Vec<f32> {
    let shares: Vec<f32> = (0..TERRAIN_TILES)
        .map(|image| {
            textures
                .get_image_rgba(terrain_bank, image)
                .map_or(0.0, |(rgba, _, _)| grass_share(&rgba))
        })
        .collect();
    coverage_of(&shares)
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
    fn coverage_is_relative_to_the_greenest_image() {
        assert_eq!(coverage_of(&[0.0, 0.26, 0.52]), [0.0, 0.5, 1.0]);
        assert_eq!(coverage_of(&[0.001, 0.52]), [0.0, 1.0], "stray pixels");
        assert_eq!(coverage_of(&[0.0, 0.0]), [0.0, 0.0], "no grass anywhere");
        assert!(coverage_of(&[]).is_empty());
    }
}
