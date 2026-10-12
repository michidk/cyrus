use crate::bitstream::BitStream;
use crate::error::{Result, SmkError};
use crate::huff::Huff8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AudioCompress {
    #[default]
    Raw,
    Dpcm,
    Bink,
}

#[derive(Debug, Default)]
pub(crate) struct AudioTrack {
    pub exists: bool,
    pub enable: bool,
    pub channels: u8,
    pub bitdepth: u8,
    pub rate: u32,
    pub max_buffer: u32,
    pub compress: AudioCompress,
    pub buffer: Vec<u8>,
    pub buffer_size: u32,
}

impl AudioTrack {
    /// Decode an audio chunk into `self.buffer`.
    pub fn render(&mut self, data: &[u8]) -> Result<()> {
        match self.compress {
            AudioCompress::Raw => self.render_raw(data),
            AudioCompress::Dpcm => self.render_dpcm(data),
            AudioCompress::Bink => Err(SmkError::InvalidData(
                "Bink audio compression is unsupported",
            )),
        }
    }

    /// Raw PCM: just copy the data.
    fn render_raw(&mut self, data: &[u8]) -> Result<()> {
        let len = data.len().min(self.buffer.len());
        self.buffer[..len].copy_from_slice(&data[..len]);
        self.buffer_size = len as u32;
        Ok(())
    }

    /// DPCM: Huffman-compressed differential PCM.
    fn render_dpcm(&mut self, data: &[u8]) -> Result<()> {
        if data.len() < 4 {
            return Err(SmkError::InvalidData(
                "DPCM: need 4 bytes for unpacked size",
            ));
        }

        let unpack_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        self.buffer_size = unpack_size;

        let mut bs = BitStream::new(&data[4..]);

        // Initial marker bit (must be set).
        if !bs.read_bit()? {
            return Err(SmkError::InvalidData("DPCM: initial bit must be 1"));
        }

        // Verify stereo/mono and bitdepth flags.
        let is_stereo = bs.read_bit()?;
        if (is_stereo && self.channels != 2) || (!is_stereo && self.channels != 1) {
            log::warn!("audio mono/stereo mismatch in DPCM stream");
        }

        let is_16bit = bs.read_bit()?;
        if (is_16bit && self.bitdepth != 16) || (!is_16bit && self.bitdepth != 8) {
            log::warn!("audio 8/16-bit mismatch in DPCM stream");
        }

        // Build Huffman trees.
        // tree[0]: left channel (or mono) low byte
        // tree[1]: left channel high byte (16-bit only)
        // tree[2]: right channel low byte (stereo only)
        // tree[3]: right channel high byte (stereo + 16-bit)
        let tree0 = Huff8::build(&mut bs)?;
        let tree1 = if is_16bit {
            Some(Huff8::build(&mut bs)?)
        } else {
            None
        };
        let tree2 = if is_stereo {
            Some(Huff8::build(&mut bs)?)
        } else {
            None
        };
        let tree3 = if is_stereo && is_16bit {
            Some(Huff8::build(&mut bs)?)
        } else {
            None
        };

        let buf = &mut self.buffer;
        let buf_size = unpack_size as usize;

        if is_16bit {
            // 16-bit DPCM: work with i16 samples via the byte buffer.
            // j indexes samples (not bytes), k tracks bytes written.
            let mut j: usize; // sample index
            let mut k: usize; // byte count

            // Read initial sample(s).
            // C code reads: first_byte => high byte, second_byte => low byte.
            // i.e. sample = (first_read << 8) | second_read
            if is_stereo {
                // Right channel initial sample (stored first in bitstream).
                let hi = bs.read_byte()?;
                let lo = bs.read_byte()?;
                buf[2..4].copy_from_slice(&i16::from_le_bytes([lo, hi]).to_le_bytes());
                j = 2;
                k = 4;
            } else {
                j = 1;
                k = 2;
            }

            // Left/mono initial sample.
            let hi = bs.read_byte()?;
            let lo = bs.read_byte()?;
            buf[0..2].copy_from_slice(&i16::from_le_bytes([lo, hi]).to_le_bytes());

            // Decode loop.
            while k < buf_size {
                // Left/mono channel.
                let delta_lo = tree0.lookup(&mut bs)?;
                let delta_hi = tree1.as_ref().unwrap().lookup(&mut bs)?;
                let delta = i16::from_le_bytes([delta_lo, delta_hi]);
                let prev_off = (j - self.channels as usize) * 2;
                let prev = i16::from_le_bytes([buf[prev_off], buf[prev_off + 1]]);
                buf[j * 2..j * 2 + 2].copy_from_slice(&prev.wrapping_add(delta).to_le_bytes());
                j += 1;
                k += 2;

                // Right channel.
                if is_stereo && k < buf_size {
                    let delta_lo = tree2.as_ref().unwrap().lookup(&mut bs)?;
                    let delta_hi = tree3.as_ref().unwrap().lookup(&mut bs)?;
                    let delta = i16::from_le_bytes([delta_lo, delta_hi]);
                    let prev_off = (j - 2) * 2;
                    let prev = i16::from_le_bytes([buf[prev_off], buf[prev_off + 1]]);
                    buf[j * 2..j * 2 + 2].copy_from_slice(&prev.wrapping_add(delta).to_le_bytes());
                    j += 1;
                    k += 2;
                }
            }
        } else {
            // 8-bit DPCM: work directly with bytes.
            let mut j: usize; // byte index into output
            let mut k: usize; // byte count (same as j for 8-bit)

            if is_stereo {
                // Right channel initial value.
                buf[1] = bs.read_byte()?;
                j = 2;
                k = 2;
            } else {
                j = 1;
                k = 1;
            }

            // Left/mono initial value.
            buf[0] = bs.read_byte()?;

            while k < buf_size {
                // Left/mono channel.
                let delta = tree0.lookup(&mut bs)? as i8;
                buf[j] = (buf[j - self.channels as usize] as i8).wrapping_add(delta) as u8;
                j += 1;
                k += 1;

                // Right channel.
                if is_stereo && k < buf_size {
                    let delta = tree2.as_ref().unwrap().lookup(&mut bs)? as i8;
                    buf[j] = (buf[j - 2] as i8).wrapping_add(delta) as u8;
                    j += 1;
                    k += 1;
                }
            }
        }

        Ok(())
    }
}

/// Public return type for audio track information.
pub struct AudioInfo {
    pub track_mask: u8,
    pub channels: [u8; 7],
    pub bitdepth: [u8; 7],
    pub rate: [u32; 7],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_track(channels: u8, bitdepth: u8) -> AudioTrack {
        AudioTrack {
            exists: true,
            enable: true,
            channels,
            bitdepth,
            rate: 22050,
            max_buffer: 4096,
            compress: AudioCompress::Raw,
            buffer: vec![0u8; 4096],
            buffer_size: 0,
        }
    }

    #[test]
    fn raw_copy() {
        let mut t = make_track(1, 8);
        let data = [0x10, 0x20, 0x30, 0x40];
        t.render(&data).unwrap();
        assert_eq!(t.buffer_size, 4);
        assert_eq!(&t.buffer[..4], &[0x10, 0x20, 0x30, 0x40]);
    }

    #[test]
    fn dpcm_mono_8bit() {
        // Build a minimal DPCM chunk for mono 8-bit audio.
        let mut t = make_track(1, 8);
        t.compress = AudioCompress::Dpcm;

        let mut bits: Vec<u8> = Vec::new();

        // Unpacked size: 3 bytes (header is separate, not in bitstream)
        let unpack_size = 3u32;

        // Bitstream:
        // bit=1 (marker)
        bits.push(1);
        // bit=0 (mono)
        bits.push(0);
        // bit=0 (8-bit)
        bits.push(0);

        // Huff8 tree for tree0: empty tree (always returns 0 = delta of 0)
        // bit=0 (no tree), bit=0 (term)
        bits.push(0);
        bits.push(0);

        // Initial value: 0x80 (8 bits LSB first)
        for i in 0..8 {
            bits.push((0x80u8 >> i) & 1);
        }

        // Now decode 2 more samples (k starts at 1, buffer_size=3).
        // Each lookup on empty tree returns 0, delta=0.
        // So samples are: [0x80, 0x80, 0x80]

        let bs_bytes = bits_to_bytes(&bits);

        // Build the full data: 4-byte LE unpack size + bitstream bytes
        let mut data = Vec::new();
        data.extend_from_slice(&unpack_size.to_le_bytes());
        data.extend_from_slice(&bs_bytes);

        t.render(&data).unwrap();
        assert_eq!(t.buffer_size, 3);
        assert_eq!(&t.buffer[..3], &[0x80, 0x80, 0x80]);
    }

    #[test]
    fn bink_unsupported() {
        let mut t = make_track(1, 8);
        t.compress = AudioCompress::Bink;
        assert!(t.render(&[0]).is_err());
    }

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
