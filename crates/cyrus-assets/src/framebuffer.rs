//! Encoded-channel RGB565 storage shared by artwork and the logbook reader.

const THRESHOLDS: [u32; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// Store an encoded RGB colour at top-left framebuffer pixel `(x, y)`.
/// Uses preservation `engine/fog.md` ordered 4×4 thresholds and replicated display bits.
pub fn store(colour: [u8; 3], x: u32, y: u32) -> [u8; 3] {
    let threshold = THRESHOLDS[((y % 4) * 4 + x % 4) as usize];
    std::array::from_fn(|channel| {
        let c = u32::from(colour[channel]);
        if channel == 1 {
            let stored = ((4 * c - c / 16 + c / 64 + threshold) / 16).min(63);
            ((stored << 2) | (stored >> 4)) as u8
        } else {
            let stored = ((2 * c - c / 16 + c / 128 + threshold) / 16).min(31);
            ((stored << 3) | (stored >> 2)) as u8
        }
    })
}

/// Blend encoded source channels against a stored RGB565 destination, then store again.
/// `alpha` is in 0..=256. Zero preserves the destination; 256 replaces it.
/// Destination reconstruction and integer source/destination terms follow preservation's
/// `engine/sky.md` framebuffer blending contract, also used by loading fades.
pub fn blend(destination: [u8; 3], source: [u8; 3], alpha: u16, x: u32, y: u32) -> [u8; 3] {
    if alpha == 0 {
        return destination;
    }
    if alpha >= 256 {
        return store(source, x, y);
    }
    let inverse = 15 - THRESHOLDS[((y % 4) * 4 + x % 4) as usize];
    let colour = std::array::from_fn(|channel| {
        let (mask, scale) = if channel == 1 { (252, 4) } else { (248, 2) };
        let destination = (u32::from(destination[channel] & mask) * scale + inverse) / scale;
        ((u32::from(source[channel]) * (u32::from(alpha) + 1) / 256)
            + destination * (256 - u32::from(alpha)) / 256)
            .min(255) as u8
    });
    store(colour, x, y)
}

#[cfg(test)]
mod tests {
    #[test]
    fn quantity_shadow_reconstructs_destination_before_integer_blending() {
        assert_eq!(
            super::blend([165, 117, 82], [0; 3], 199, 298, 117),
            [41, 28, 16]
        );
        assert_eq!(
            super::blend([132, 73, 57], [0; 3], 199, 391, 119),
            [24, 16, 8]
        );
        assert_eq!(
            super::blend([165, 117, 82], [0; 3], 0, 298, 117),
            [165, 117, 82]
        );
        assert_eq!(super::blend([165, 117, 82], [0; 3], 256, 298, 117), [0; 3]);
    }
}
