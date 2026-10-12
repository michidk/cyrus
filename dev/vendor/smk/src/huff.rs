use crate::bitstream::BitStream;
use crate::error::{Result, SmkError};

// ---------------------------------------------------------------------------
// 8-bit Huffman tree
// ---------------------------------------------------------------------------

const HUFF8_BRANCH: u16 = 0x8000;
const HUFF8_LEAF_MASK: u16 = 0x7FFF;
/// Maximum tree entries: 256 leaves + 255 branches.
const HUFF8_MAX_SIZE: usize = 511;

/// An 8-bit Huffman tree stored as a flat array.
///
/// Branch entries have `HUFF8_BRANCH` set, with the lower bits holding the
/// index of the right child. The left child is always the next entry.
/// Leaf entries store the decoded byte value directly.
pub(crate) struct Huff8 {
    tree: Vec<u16>,
}

impl Huff8 {
    /// Build a Huff8 tree from the bitstream.
    ///
    /// Format: a leading `true` bit means a tree follows, `false` means an
    /// empty tree (single zero-value leaf). The tree data is terminated by a
    /// `false` bit.
    pub fn build(bs: &mut BitStream) -> Result<Self> {
        let mut h = Huff8 { tree: Vec::new() };

        if bs.read_bit()? {
            h.build_rec(bs)?;
        } else {
            // No tree present — single leaf with value 0.
            h.tree.push(0);
        }

        // Trees are terminated by a false bit.
        if bs.read_bit()? {
            return Err(SmkError::TreeBuildFailed("expected trailing 0 bit"));
        }

        Ok(h)
    }

    /// Recursively build the tree from the bitstream.
    fn build_rec(&mut self, bs: &mut BitStream) -> Result<()> {
        if self.tree.len() >= HUFF8_MAX_SIZE {
            return Err(SmkError::TreeBuildFailed("huff8 tree size exceeded"));
        }

        if bs.read_bit()? {
            // Branch node: reserve a slot, build left subtree, record right
            // child index, then build right subtree.
            let slot = self.tree.len();
            self.tree.push(0); // placeholder

            self.build_rec(bs)?;

            // The right child starts at the current length.
            self.tree[slot] = HUFF8_BRANCH | self.tree.len() as u16;

            self.build_rec(bs)?;
        } else {
            // Leaf node: read the 8-bit value.
            let value = bs.read_byte()?;
            self.tree.push(u16::from(value));
        }

        Ok(())
    }

    /// Look up the next value by traversing the tree according to the
    /// bitstream. Returns the decoded byte.
    pub fn lookup(&self, bs: &mut BitStream) -> Result<u8> {
        let mut index = 0usize;

        while self.tree[index] & HUFF8_BRANCH != 0 {
            if bs.read_bit()? {
                // Right branch: jump to the stored index.
                index = (self.tree[index] & HUFF8_LEAF_MASK) as usize;
            } else {
                // Left branch: next entry.
                index += 1;
            }
        }

        Ok(self.tree[index] as u8)
    }
}

// ---------------------------------------------------------------------------
// 16-bit Huffman tree
// ---------------------------------------------------------------------------

const HUFF16_BRANCH: u32 = 0x8000_0000;
const HUFF16_CACHE: u32 = 0x4000_0000;
const HUFF16_LEAF_MASK: u32 = 0x3FFF_FFFF;

/// A 16-bit Huffman tree built from two Huff8 sub-trees (lo/hi byte),
/// stored as a flat array with a 3-entry MRU cache for escape codes.
///
/// Leaf values that match a cache entry at build time are replaced with
/// `HUFF16_CACHE | index` so that lookup can substitute the current cache
/// value at decode time (the cache evolves as values are looked up).
pub(crate) struct Huff16 {
    tree: Vec<u32>,
    cache: [u16; 3],
}

impl Default for Huff16 {
    fn default() -> Self {
        Huff16 {
            tree: vec![0],
            cache: [0; 3],
        }
    }
}

impl Huff16 {
    /// Build a Huff16 tree from the bitstream.
    ///
    /// `alloc_size` is the byte-size field from the SMK header for this tree.
    /// The C code uses `(alloc_size - 12) / 4` as the expected entry count.
    pub fn build(bs: &mut BitStream, alloc_size: u32) -> Result<Self> {
        let h;

        if bs.read_bit()? {
            // Build the two 8-bit sub-trees used for leaf values.
            let low8 = Huff8::build(bs)?;
            let hi8 = Huff8::build(bs)?;

            // Read the 3-entry escape-code cache (lo byte, hi byte each).
            let mut cache = [0u16; 3];
            for entry in &mut cache {
                let lo = bs.read_byte()?;
                let hi = bs.read_byte()?;
                *entry = u16::from(lo) | (u16::from(hi) << 8);
            }

            // Validate and compute expected tree size.
            if alloc_size < 12 || alloc_size % 4 != 0 {
                return Err(SmkError::TreeBuildFailed("illegal alloc_size for huff16"));
            }
            let limit = ((alloc_size - 12) / 4) as usize;

            h = Huff16 {
                tree: Vec::with_capacity(limit),
                cache,
            };

            let mut h = h;
            h.build_rec(bs, &low8, &hi8, limit)?;

            if h.tree.len() != limit {
                return Err(SmkError::TreeBuildFailed(
                    "huff16 tree size does not match expected",
                ));
            }

            // Trees are terminated by a false bit.
            if bs.read_bit()? {
                return Err(SmkError::TreeBuildFailed("expected trailing 0 bit"));
            }

            Ok(h)
        } else {
            // No tree — single zero-value leaf.
            h = Huff16 {
                tree: vec![0],
                cache: [0; 3],
            };

            // Trees are terminated by a false bit.
            if bs.read_bit()? {
                return Err(SmkError::TreeBuildFailed("expected trailing 0 bit"));
            }

            Ok(h)
        }
    }

    /// Recursively build the 16-bit tree.
    fn build_rec(
        &mut self,
        bs: &mut BitStream,
        low8: &Huff8,
        hi8: &Huff8,
        limit: usize,
    ) -> Result<()> {
        if self.tree.len() >= limit {
            return Err(SmkError::TreeBuildFailed("huff16 tree size exceeded"));
        }

        if bs.read_bit()? {
            // Branch: reserve slot, build left, fill jump, build right.
            let slot = self.tree.len();
            self.tree.push(0); // placeholder

            self.build_rec(bs, low8, hi8, limit)?;

            self.tree[slot] = HUFF16_BRANCH | self.tree.len() as u32;

            self.build_rec(bs, low8, hi8, limit)?;
        } else {
            // Leaf: look up lo and hi bytes from the 8-bit sub-trees.
            let lo = low8.lookup(bs)?;
            let hi = hi8.lookup(bs)?;
            let value = u16::from(lo) | (u16::from(hi) << 8);

            // Replace values matching cache entries with escape codes.
            let entry = if value == self.cache[0] {
                HUFF16_CACHE
            } else if value == self.cache[1] {
                HUFF16_CACHE | 1
            } else if value == self.cache[2] {
                HUFF16_CACHE | 2
            } else {
                u32::from(value)
            };

            self.tree.push(entry);
        }

        Ok(())
    }

    /// Reset the MRU cache to zeros (must be called before each video frame).
    pub fn reset_cache(&mut self) {
        self.cache = [0; 3];
    }

    /// Look up the next 16-bit value, updating the MRU cache.
    pub fn lookup(&mut self, bs: &mut BitStream) -> Result<u16> {
        let mut index = 0usize;

        while self.tree[index] & HUFF16_BRANCH != 0 {
            if bs.read_bit()? {
                index = (self.tree[index] & HUFF16_LEAF_MASK) as usize;
            } else {
                index += 1;
            }
        }

        let raw = self.tree[index];

        let value = if raw & HUFF16_CACHE != 0 {
            // Escape code — substitute from cache.
            let idx = (raw & HUFF16_LEAF_MASK) as usize;
            if idx >= self.cache.len() {
                return Err(SmkError::InvalidData("huff16 cache index out of range"));
            }
            self.cache[idx]
        } else {
            raw as u16
        };

        // Update MRU cache: push value to front if not already there.
        if self.cache[0] != value {
            self.cache[2] = self.cache[1];
            self.cache[1] = self.cache[0];
            self.cache[0] = value;
        }

        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a Huff8 tree from raw bytes interpreted as a bitstream.
    fn build_huff8(data: &[u8]) -> Result<Huff8> {
        let mut bs = BitStream::new(data);
        Huff8::build(&mut bs)
    }

    #[test]
    fn empty_tree() {
        // Bit 0 = no tree, then bit 0 = terminator.
        // Two zero bits = byte 0x00 is enough.
        let data = [0x00];
        let h = build_huff8(&data).unwrap();
        assert_eq!(h.tree.len(), 1);
        assert_eq!(h.tree[0], 0);
    }

    #[test]
    fn single_leaf_tree() {
        // Build a tree with one leaf: bit=1 (tree present), bit=0 (leaf),
        // 8-bit value, bit=0 (terminator).
        //
        // Bits (LSB first): 1, 0, [8 bits of value 0x42], 0
        //   bit0=1, bit1=0, bit2..9 = 0x42 LSB first: 0,1,0,0,0,0,1,0
        //   bit10=0 (terminator)
        //
        // Byte 0 (bits 0-7): 1,0,0,1,0,0,0,0 = 0x09
        // Byte 1 (bits 8-10): 1,0,0 = 0x01
        let data = [0x09, 0x01];
        let h = build_huff8(&data).unwrap();
        assert_eq!(h.tree.len(), 1);
        // Lookup should always return 0x42.
        let mut bs = BitStream::new(&[0x00]); // bits don't matter for single leaf
        assert_eq!(h.lookup(&mut bs).unwrap(), 0x42);
    }

    #[test]
    fn two_leaf_tree() {
        // Tree: branch -> left=leaf(0xAA), right=leaf(0xBB)
        // Bits:
        //   1       (tree present)
        //   1       (branch)
        //   0       (left leaf)
        //   [0xAA = 0b10101010, LSB first: 0,1,0,1,0,1,0,1]
        //   0       (right leaf)
        //   [0xBB = 0b10111011, LSB first: 1,1,0,1,1,1,0,1]
        //   0       (terminator)
        let data = [0x53, 0xB5, 0x0B];
        let h = build_huff8(&data).unwrap();
        assert_eq!(h.tree.len(), 3); // 1 branch + 2 leaves

        // Lookup: bit=0 -> left -> 0xAA
        let mut bs = BitStream::new(&[0x00]);
        assert_eq!(h.lookup(&mut bs).unwrap(), 0xAA);

        // Lookup: bit=1 -> right -> 0xBB
        let mut bs = BitStream::new(&[0x01]);
        assert_eq!(h.lookup(&mut bs).unwrap(), 0xBB);
    }

    #[test]
    fn trailing_one_is_error() {
        // Empty tree (bit=0) followed by bit=1 instead of terminator 0.
        // bits: 0, 1 => byte 0x02
        let data = [0x02];
        assert!(build_huff8(&data).is_err());
    }

    // --- Huff16 tests ---

    #[test]
    fn huff16_empty_tree() {
        // bit=0 (no tree), bit=0 (terminator) => 0x00
        let data = [0x00];
        let mut bs = BitStream::new(&data);
        let mut h = Huff16::build(&mut bs, 16).unwrap();
        assert_eq!(h.tree.len(), 1);
        assert_eq!(h.tree[0], 0);

        // Lookup always returns 0.
        let mut lbs = BitStream::new(&[0x00]);
        assert_eq!(h.lookup(&mut lbs).unwrap(), 0);
    }

    #[test]
    fn huff16_single_leaf() {
        // Build bitstream programmatically
        let mut bits: Vec<u8> = Vec::new();

        // bit=1: tree present
        bits.push(1);
        // low8: bit=0 (no tree), bit=0 (term)
        bits.push(0);
        bits.push(0);
        // hi8: bit=0 (no tree), bit=0 (term)
        bits.push(0);
        bits.push(0);
        // cache[0]: lo=0x01 (8 bits LSB first), hi=0x00 (8 bits)
        for b in lsb_bits(0x01) {
            bits.push(b);
        }
        for b in lsb_bits(0x00) {
            bits.push(b);
        }
        // cache[1]: lo=0x02, hi=0x00
        for b in lsb_bits(0x02) {
            bits.push(b);
        }
        for b in lsb_bits(0x00) {
            bits.push(b);
        }
        // cache[2]: lo=0x03, hi=0x00
        for b in lsb_bits(0x03) {
            bits.push(b);
        }
        for b in lsb_bits(0x00) {
            bits.push(b);
        }
        // huff16 recursive: bit=0 (leaf)
        // low8 lookup on empty tree => 0, no bits consumed
        // hi8 lookup on empty tree => 0, no bits consumed
        bits.push(0);
        // terminator
        bits.push(0);

        let bytes = bits_to_bytes(&bits);
        let mut bs = BitStream::new(&bytes);
        let mut h = Huff16::build(&mut bs, 16).unwrap();
        assert_eq!(h.tree.len(), 1);

        // Lookup: single leaf = 0x0000
        let mut lbs = BitStream::new(&[0x00]);
        assert_eq!(h.lookup(&mut lbs).unwrap(), 0x0000);
    }

    #[test]
    fn huff16_cache_substitution() {
        // Build a tree where the leaf value matches cache[0].
        // low8 returns 0, hi8 returns 0 => value = 0x0000.
        // Set cache[0] = 0x0000 so the leaf becomes a cache escape.
        // Then at lookup time, mutate cache[0] and verify the escape resolves.

        let mut bits: Vec<u8> = Vec::new();

        bits.push(1); // tree present
        // low8 empty, hi8 empty
        bits.push(0);
        bits.push(0);
        bits.push(0);
        bits.push(0);
        // cache[0] = 0x0000
        for b in lsb_bits(0x00) {
            bits.push(b);
        }
        for b in lsb_bits(0x00) {
            bits.push(b);
        }
        // cache[1] = 0x0001
        for b in lsb_bits(0x01) {
            bits.push(b);
        }
        for b in lsb_bits(0x00) {
            bits.push(b);
        }
        // cache[2] = 0x0002
        for b in lsb_bits(0x02) {
            bits.push(b);
        }
        for b in lsb_bits(0x00) {
            bits.push(b);
        }
        // leaf (value 0x0000 matches cache[0])
        bits.push(0);
        // terminator
        bits.push(0);

        let bytes = bits_to_bytes(&bits);
        let mut bs = BitStream::new(&bytes);
        let mut h = Huff16::build(&mut bs, 16).unwrap();

        // The leaf should be HUFF16_CACHE | 0
        assert_eq!(h.tree[0], HUFF16_CACHE);

        // Lookup returns cache[0] = 0x0000
        let mut lbs = BitStream::new(&[0x00]);
        assert_eq!(h.lookup(&mut lbs).unwrap(), 0x0000);

        // Now mutate cache[0] and verify escape resolves to new value
        h.cache[0] = 0xBEEF;
        let mut lbs = BitStream::new(&[0x00]);
        assert_eq!(h.lookup(&mut lbs).unwrap(), 0xBEEF);
    }

    #[test]
    fn huff16_mru_cache_update() {
        // Build an empty huff16 tree (single leaf = 0).
        let data = [0x00]; // bit=0 (no tree), bit=0 (term)
        let mut bs = BitStream::new(&data);
        let mut h = Huff16::build(&mut bs, 16).unwrap();

        h.cache = [0x0A, 0x0B, 0x0C];

        // Lookup returns 0. 0 != cache[0]=0x0A, so cache shifts.
        let mut lbs = BitStream::new(&[0x00]);
        assert_eq!(h.lookup(&mut lbs).unwrap(), 0);
        assert_eq!(h.cache, [0x00, 0x0A, 0x0B]);

        // Lookup returns 0 again. 0 == cache[0], no shift.
        let mut lbs = BitStream::new(&[0x00]);
        assert_eq!(h.lookup(&mut lbs).unwrap(), 0);
        assert_eq!(h.cache, [0x00, 0x0A, 0x0B]);
    }

    // --- Test helpers ---

    /// Convert a byte value to 8 bits in LSB-first order.
    fn lsb_bits(byte: u8) -> [u8; 8] {
        let mut out = [0u8; 8];
        for i in 0..8 {
            out[i] = (byte >> i) & 1;
        }
        out
    }

    /// Pack a slice of individual bits (0 or 1) into bytes (LSB-first).
    fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for chunk in bits.chunks(8) {
            let mut byte = 0u8;
            for (i, &b) in chunk.iter().enumerate() {
                byte |= b << i;
            }
            bytes.push(byte);
        }
        bytes
    }
}
