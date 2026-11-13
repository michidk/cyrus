//! Cinematic texture sampling and framebuffer composition, following preservation's
//! engine/movies.md. Shared by native decoding and browser packet playback.

const WIDTH: usize = 640;
const HEIGHT: usize = 480;
const SOURCE_HEIGHT: usize = 240;
const TILE: usize = 256;
const DITHER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Expand an installed indexed movie frame with the original tiled sampling grid.
/// The decoder validates the 640×240 input. Palette index zero fills unused
/// texture backing; filtering wraps within that backing rather than the image.
pub(super) fn expand(indices: &[u8], palette: &[[u8; 3]; 256]) -> Vec<u8> {
    let mut output = vec![255; WIDTH * HEIGHT * 4];
    for left in (0..WIDTH).step_by(TILE) {
        let width = TILE.min(WIDTH - left);
        let extent = (width + 1).min(WIDTH - left);
        // Clipping adjusts the source endpoint before single-precision triangle
        // setup. Normalize edge deltas by area before multiplying source deltas.
        let source_right = (width as f64 * extent as f64 / (width + 1) as f64) as f32;
        let source_bottom = (SOURCE_HEIGHT as f64 * HEIGHT as f64 / (HEIGHT + 1) as f64) as f32;
        let inverse_area = 1.0_f32 / (extent * HEIGHT) as f32;
        let dx = fixed(source_right * (HEIGHT as f32 * inverse_area));
        let dy = fixed(source_bottom * (extent as f32 * inverse_area));
        let start = fixed(source_right);
        let texel = |x: i64, y: i64| {
            let (x, y) = ((x & 255) as usize, (y & 255) as usize);
            let index = if x < width && y < SOURCE_HEIGHT {
                indices[y * WIDTH + left + x]
            } else {
                0
            };
            palette[usize::from(index)].map(i32::from)
        };
        for y in 0..HEIGHT {
            let t = ((y as i64 * dy + (dy >> 1)) >> 24) - 128;
            let (ty, fy) = (t >> 8, (t & 240) as i32);
            for x in 0..extent {
                let s = ((start + (x as i64 - extent as i64) * dx + (dx >> 1)) >> 24) - 128;
                let (tx, fx) = (s >> 8, (s & 240) as i32);
                let a = texel(tx, ty);
                let b = texel(tx + 1, ty);
                let c = texel(tx, ty + 1);
                let d = texel(tx + 1, ty + 1);
                let colour = std::array::from_fn(|channel| {
                    let upper = a[channel] + (((b[channel] - a[channel]) * fx) >> 8);
                    let lower = c[channel] + (((d[channel] - c[channel]) * fx) >> 8);
                    upper + (((lower - upper) * fy) >> 8)
                });
                let at = (y * WIDTH + left + x) * 4;
                output[at..at + 3].copy_from_slice(&store(colour, left + x, y));
            }
        }
    }
    output
}

fn fixed(value: f32) -> i64 {
    (f64::from(value) * 4_294_967_296.0) as i64
}

fn store(colour: [i32; 3], x: usize, y: usize) -> [u8; 3] {
    let dither = DITHER[y & 3][x & 3];
    std::array::from_fn(|channel| {
        let value = colour[channel];
        let (stored, bits) = if channel == 1 {
            (
                ((4 * value - (value >> 4) + (value >> 6) + dither) >> 2) >> 2,
                6,
            )
        } else {
            (
                ((2 * value - (value >> 4) + (value >> 7) + dither) >> 1) >> 3,
                5,
            )
        };
        ((stored << (8 - bits)) | (stored >> (2 * bits - 8))) as u8
    })
}

/// Blend a subtitle pass over an opaque movie framebuffer. Source colours are
/// eight-bit entries from the selected upload palette, not the font's BPAL.
pub(super) fn blend(pixel: &mut [u8], colour: [u8; 3], alpha: u8, x: usize, y: usize) {
    let dither = DITHER[y & 3][x & 3];
    let colour = std::array::from_fn(|channel| {
        let (shift, scale) = if channel == 1 { (2, 2) } else { (3, 1) };
        let stored = (i32::from(pixel[channel]) >> shift) << shift;
        let destination = ((stored << scale) + 15 - dither) >> scale;
        ((i32::from(colour[channel]) * (i32::from(alpha) + 1)) >> 8)
            + ((destination * (256 - i32::from(alpha))) >> 8)
    });
    pixel[..3].copy_from_slice(&store(colour, x, y));
    pixel[3] = 255;
}
