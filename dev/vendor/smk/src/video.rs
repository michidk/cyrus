use crate::bitstream::BitStream;
use crate::error::{Result, SmkError};
use crate::huff::Huff16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum YScaleMode {
    #[default]
    None,
    Interlace,
    Double,
}

pub(crate) struct Video {
    pub enable: bool,
    pub w: u32,
    pub h: u32,
    pub y_scale_mode: YScaleMode,
    pub version: u8, // b'2' or b'4'
    pub tree: [Huff16; 4],
    pub palette: [[u8; 3]; 256],
    pub frame: Vec<u8>,
}

/// Smacker 6-bit to 8-bit palette expansion table.
const PALMAP: [u8; 64] = [
    0x00, 0x04, 0x08, 0x0C, 0x10, 0x14, 0x18, 0x1C, 0x20, 0x24, 0x28, 0x2C, 0x30, 0x34, 0x38, 0x3C,
    0x41, 0x45, 0x49, 0x4D, 0x51, 0x55, 0x59, 0x5D, 0x61, 0x65, 0x69, 0x6D, 0x71, 0x75, 0x79, 0x7D,
    0x82, 0x86, 0x8A, 0x8E, 0x92, 0x96, 0x9A, 0x9E, 0xA2, 0xA6, 0xAA, 0xAE, 0xB2, 0xB6, 0xBA, 0xBE,
    0xC3, 0xC7, 0xCB, 0xCF, 0xD3, 0xD7, 0xDB, 0xDF, 0xE3, 0xE7, 0xEB, 0xEF, 0xF3, 0xF7, 0xFB, 0xFF,
];

impl Video {
    /// Decode a palette chunk, updating `self.palette` in place.
    ///
    /// The palette format uses delta-encoding against the previous palette:
    /// - `0x80` prefix: skip (preserve) C+1 entries from the old palette
    /// - `0x40` prefix: copy C+1 entries from old palette starting at offset S
    /// - Otherwise: direct-set 3 bytes (6-bit values expanded via PALMAP)
    pub fn render_palette(&mut self, data: &[u8]) -> Result<()> {
        let old_palette = self.palette;
        let mut i: usize = 0;
        let mut pos: usize = 0;

        while i < 256 && pos < data.len() {
            let b = data[pos];

            if b & 0x80 != 0 {
                // Skip block: preserve (count) entries from old palette.
                let count = (b & 0x7F) as usize + 1;
                pos += 1;

                if i + count > 256 {
                    return Err(SmkError::InvalidData("palette skip overflow"));
                }
                // Entries already match old_palette since we copied it above,
                // but we need to restore them in case earlier ops modified them.
                self.palette[i..i + count].copy_from_slice(&old_palette[i..i + count]);
                i += count;
            } else if b & 0x40 != 0 {
                // Color-shift block: copy (count) entries from old palette at (src).
                let count = (b & 0x3F) as usize + 1;
                pos += 1;

                if pos >= data.len() {
                    return Err(SmkError::InvalidData("palette copy: missing src byte"));
                }
                let src = data[pos] as usize;
                pos += 1;

                if i + count > 256 || src + count > 256 {
                    return Err(SmkError::InvalidData("palette copy overflow"));
                }
                if src < i && src + count > i {
                    return Err(SmkError::InvalidData("palette copy overlaps destination"));
                }

                self.palette[i..i + count].copy_from_slice(&old_palette[src..src + count]);
                i += count;
            } else {
                // Set Color block: read 3 bytes (6-bit each), expand to 8-bit.
                if pos + 3 > data.len() {
                    return Err(SmkError::InvalidData("palette set: not enough bytes"));
                }

                for c in 0..3 {
                    let val = data[pos] as usize;
                    if val > 0x3F {
                        return Err(SmkError::InvalidData("palette index exceeds 0x3F"));
                    }
                    self.palette[i][c] = PALMAP[val];
                    pos += 1;
                }
                i += 1;
            }
        }

        if i < 256 {
            return Err(SmkError::InvalidData("palette incomplete"));
        }

        Ok(())
    }

    /// Decode a video frame from the bitstream into `self.frame`.
    ///
    /// The frame is processed as a grid of 4x4 pixel blocks, left-to-right,
    /// top-to-bottom. Each block's type is determined by looking up the TYPE
    /// tree, which yields the block type (2 bits), a repeat count via the
    /// size table (6 bits), and per-type data (8 bits).
    pub fn render_video(&mut self, data: &[u8]) -> Result<()> {
        // Size table: entries 0-58 are literal (n+1), last 5 are powers of 2.
        const SIZETABLE: [u16; 64] = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
            47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 128, 256, 512, 1024, 2048,
        ];

        // Tree indices.
        const TREE_MMAP: usize = 0;
        const TREE_MCLR: usize = 1;
        const TREE_FULL: usize = 2;
        const TREE_TYPE: usize = 3;

        let w = self.w as usize;
        let h = self.h as usize;

        let mut bs = BitStream::new(data);

        // Reset the MRU cache on all 4 Huff16 trees before each frame.
        for tree in &mut self.tree {
            tree.reset_cache();
        }

        let mut row: usize = 0;
        let mut col: usize = 0;

        while row < h {
            let type_val = self.tree[TREE_TYPE].lookup(&mut bs)?;

            let mut block_type = (type_val & 0x0003) as u8;
            let blocklen = ((type_val & 0x00FC) >> 2) as usize;
            let typedata = ((type_val & 0xFF00) >> 8) as u8;

            // SMK v4 extends type 1 (full block) with two sub-types.
            if block_type == 1 && self.version == b'4' {
                if bs.read_bit()? {
                    block_type = 4; // v4 double block
                } else if bs.read_bit()? {
                    block_type = 5; // v4 half block
                }
            }

            let repeat = SIZETABLE[blocklen] as usize;

            for _ in 0..repeat {
                if row >= h {
                    break;
                }

                let mut skip = row * w + col;

                match block_type {
                    0 => {
                        // MONO BLOCK: 2-color pattern via MCLR + MMAP trees.
                        let clr = self.tree[TREE_MCLR].lookup(&mut bs)?;
                        let s1 = ((clr >> 8) & 0xFF) as u8;
                        let s2 = (clr & 0xFF) as u8;

                        let map = self.tree[TREE_MMAP].lookup(&mut bs)?;
                        let mut mask = 0x0001u16;

                        for _ in 0..4 {
                            for i in 0..4 {
                                self.frame[skip + i] = if map & mask != 0 { s1 } else { s2 };
                                mask <<= 1;
                            }
                            skip += w;
                        }
                    }

                    1 => {
                        // FULL BLOCK (v2): each row is two 16-bit lookups,
                        // pixels stored in reverse order within each pair.
                        for _ in 0..4 {
                            let val = self.tree[TREE_FULL].lookup(&mut bs)?;
                            self.frame[skip + 3] = ((val >> 8) & 0xFF) as u8;
                            self.frame[skip + 2] = (val & 0xFF) as u8;

                            let val = self.tree[TREE_FULL].lookup(&mut bs)?;
                            self.frame[skip + 1] = ((val >> 8) & 0xFF) as u8;
                            self.frame[skip] = (val & 0xFF) as u8;

                            skip += w;
                        }
                    }

                    2 => {
                        // VOID BLOCK: no change (preserve previous frame data).
                    }

                    3 => {
                        // SOLID BLOCK: fill 4x4 with typedata color.
                        for _ in 0..4 {
                            self.frame[skip..skip + 4].fill(typedata);
                            skip += w;
                        }
                    }

                    4 => {
                        // V4 DOUBLE BLOCK: 2x2 pixel sub-blocks, each row
                        // pair gets the same colors.
                        for _ in 0..2 {
                            let val = self.tree[TREE_FULL].lookup(&mut bs)?;
                            let hi = ((val >> 8) & 0xFF) as u8;
                            let lo = (val & 0xFF) as u8;

                            for _ in 0..2 {
                                self.frame[skip + 2] = hi;
                                self.frame[skip + 3] = hi;
                                self.frame[skip] = lo;
                                self.frame[skip + 1] = lo;
                                skip += w;
                            }
                        }
                    }

                    5 => {
                        // V4 HALF BLOCK: each 2x2 quadrant gets its own pair
                        // of pixels, duplicated across 2 rows.
                        for _ in 0..2 {
                            let val = self.tree[TREE_FULL].lookup(&mut bs)?;
                            let hi = ((val >> 8) & 0xFF) as u8;
                            let lo = (val & 0xFF) as u8;
                            self.frame[skip + 3] = hi;
                            self.frame[skip + 2] = lo;
                            self.frame[skip + w + 3] = hi;
                            self.frame[skip + w + 2] = lo;

                            let val = self.tree[TREE_FULL].lookup(&mut bs)?;
                            let hi = ((val >> 8) & 0xFF) as u8;
                            let lo = (val & 0xFF) as u8;
                            self.frame[skip + 1] = hi;
                            self.frame[skip] = lo;
                            self.frame[skip + w + 1] = hi;
                            self.frame[skip + w] = lo;

                            skip += w * 2;
                        }
                    }

                    _ => unreachable!(),
                }

                col += 4;
                if col >= w {
                    col = 0;
                    row += 4;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_video() -> Video {
        Video {
            enable: true,
            w: 8,
            h: 8,
            y_scale_mode: YScaleMode::None,
            version: b'4',
            tree: Default::default(),
            palette: [[0; 3]; 256],
            frame: vec![0; 64],
        }
    }

    #[test]
    fn palette_direct_set_all() {
        let mut v = make_video();

        // Set all 256 entries directly: 3 bytes each, all value 0x3F => maps to 0xFF
        let mut data = Vec::new();
        for _ in 0..256 {
            data.extend_from_slice(&[0x3F, 0x3F, 0x3F]);
        }

        v.render_palette(&data).unwrap();
        for i in 0..256 {
            assert_eq!(v.palette[i], [0xFF, 0xFF, 0xFF]);
        }
    }

    #[test]
    fn palette_skip_all() {
        let mut v = make_video();
        // Pre-fill with a known palette
        for i in 0..256 {
            v.palette[i] = [i as u8, 0, 0];
        }

        // Skip all 256 entries: 0x80 | (256-1) = 0xFF, but max is 0x7F+1=128.
        // So we need two skip blocks of 128 each.
        let data = [0xFF, 0xFF]; // two blocks of 128

        v.render_palette(&data).unwrap();
        // All entries should be preserved.
        for i in 0..256 {
            assert_eq!(v.palette[i], [i as u8, 0, 0]);
        }
    }

    #[test]
    fn palette_copy_block() {
        let mut v = make_video();
        // Set first 4 entries to known colors
        v.palette[0] = [10, 20, 30];
        v.palette[1] = [40, 50, 60];
        v.palette[2] = [70, 80, 90];
        v.palette[3] = [100, 110, 120];

        // Skip first 4, then copy 4 entries from src=0 to entries 4-7,
        // then set remaining 248 entries directly.
        let mut data = Vec::new();
        // Skip 4: 0x80 | 3 = 0x83
        data.push(0x83);
        // Copy 4 from src=0: 0x40 | 3 = 0x43, src=0
        data.push(0x43);
        data.push(0x00);
        // Set remaining 248 entries to (0, 0, 0)
        for _ in 0..248 {
            data.extend_from_slice(&[0x00, 0x00, 0x00]);
        }

        v.render_palette(&data).unwrap();
        assert_eq!(v.palette[4], [10, 20, 30]);
        assert_eq!(v.palette[5], [40, 50, 60]);
        assert_eq!(v.palette[6], [70, 80, 90]);
        assert_eq!(v.palette[7], [100, 110, 120]);
    }

    #[test]
    fn palette_value_exceeds_6bit() {
        let mut v = make_video();
        // 0x40 is > 0x3F, should error
        let _data = [0x40, 0x00, 0x00];
        // But 0x40 is actually the copy-block prefix, not a direct-set.
        // For a direct-set error, the first byte must be < 0x40 (the flag byte)
        // and one of the color bytes must be > 0x3F.
        // First byte 0x00 means direct-set. Then bytes [0x40, 0x00, 0x00] for colors.
        // Wait — first byte IS 0x40, which triggers the copy path. Let me fix the test.
        // To trigger the > 0x3F check, we need first byte < 0x40, then a color byte > 0x3F.

        // entry: first byte 0x00 (< 0x40, < 0x80) = direct set,
        // then R=0x00, G=0x00, B=0x40 (> 0x3F)
        let data2 = [0x00, 0x00, 0x40];
        let err = v.render_palette(&data2);
        assert!(err.is_err());
    }

    #[test]
    fn palette_incomplete() {
        let mut v = make_video();
        // Only set 1 entry, palette should be incomplete.
        let data = [0x00, 0x00, 0x00];
        let err = v.render_palette(&data);
        assert!(err.is_err());
    }
}
