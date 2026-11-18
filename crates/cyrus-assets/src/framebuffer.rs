//! Encoded-channel RGB565 storage shared by artwork and the logbook reader.

/// Store an encoded RGB colour at top-left framebuffer pixel `(x, y)`.
/// Uses preservation `engine/fog.md` ordered 4×4 thresholds and replicated display bits.
pub fn store(colour: [u8; 3], x: u32, y: u32) -> [u8; 3] {
    const THRESHOLDS: [u32; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
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
