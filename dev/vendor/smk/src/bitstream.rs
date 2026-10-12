use crate::error::{Result, SmkError};

/// Wraps a byte slice for reading individual bits (LSB-first) or full bytes.
pub(crate) struct BitStream<'a> {
    buffer: &'a [u8],
    pos: usize,
    bit_num: u8,
}

impl<'a> BitStream<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitStream {
            buffer: data,
            pos: 0,
            bit_num: 0,
        }
    }

    /// Read a single bit.
    pub fn read_bit(&mut self) -> Result<bool> {
        if self.pos >= self.buffer.len() {
            return Err(SmkError::BitstreamExhausted);
        }

        let ret = (self.buffer[self.pos] >> self.bit_num) & 1 != 0;

        if self.bit_num >= 7 {
            self.pos += 1;
            self.bit_num = 0;
        } else {
            self.bit_num += 1;
        }

        Ok(ret)
    }

    /// Read 8 bits as a byte, handling unaligned reads.
    pub fn read_byte(&mut self) -> Result<u8> {
        if self.pos + usize::from(self.bit_num > 0) >= self.buffer.len() {
            return Err(SmkError::BitstreamExhausted);
        }

        if self.bit_num > 0 {
            let ret = self.buffer[self.pos] >> self.bit_num;
            self.pos += 1;
            Ok(ret | (self.buffer[self.pos] << (8 - self.bit_num)))
        } else {
            let ret = self.buffer[self.pos];
            self.pos += 1;
            Ok(ret)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_bit_sequential() {
        // 0b10110010 = 0xB2
        let data = [0xB2];
        let mut bs = BitStream::new(&data);
        // LSB first: bits are 0,1,0,0,1,1,0,1
        assert!(!bs.read_bit().unwrap());
        assert!(bs.read_bit().unwrap());
        assert!(!bs.read_bit().unwrap());
        assert!(!bs.read_bit().unwrap());
        assert!(bs.read_bit().unwrap());
        assert!(bs.read_bit().unwrap());
        assert!(!bs.read_bit().unwrap());
        assert!(bs.read_bit().unwrap());
        assert!(bs.read_bit().is_err());
    }

    #[test]
    fn read_byte_aligned() {
        let data = [0xAB, 0xCD];
        let mut bs = BitStream::new(&data);
        assert_eq!(bs.read_byte().unwrap(), 0xAB);
        assert_eq!(bs.read_byte().unwrap(), 0xCD);
        assert!(bs.read_byte().is_err());
    }

    #[test]
    fn read_byte_unaligned() {
        let data = [0xFF, 0x00, 0xFF];
        let mut bs = BitStream::new(&data);
        // Read 1 bit first to misalign
        assert!(bs.read_bit().unwrap());
        // Now read_byte should combine across byte boundary
        let val = bs.read_byte().unwrap();
        // bits 1..8 of byte 0 (all 1s = 0x7F) | bits 0..0 of byte 1 (0) << 7
        assert_eq!(val, 0x7F);
    }

    #[test]
    fn exhausted() {
        let data = [];
        let mut bs = BitStream::new(&data);
        assert!(bs.read_bit().is_err());
        assert!(bs.read_byte().is_err());
    }
}
