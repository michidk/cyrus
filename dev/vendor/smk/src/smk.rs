use std::borrow::Cow;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::audio::{AudioCompress, AudioInfo, AudioTrack};
use crate::bitstream::BitStream;
use crate::error::{Result, SmkError};
use crate::huff::Huff16;
use crate::video::{Video, YScaleMode};

/// Frame-advance return status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameStatus {
    Done,
    More,
    Last,
}

/// General information about an SMK file.
#[derive(Debug, Clone)]
pub struct SmkInfo {
    pub current_frame: u32,
    pub frame_count: u32,
    pub microseconds_per_frame: f64,
}

/// Video dimensions and scaling mode.
#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub y_scale: YScaleMode,
}

// --- Source: file-backed or memory-backed ---

pub(crate) enum Source {
    Memory {
        chunk_data: Vec<Vec<u8>>,
    },
    Packet {
        data: Vec<u8>,
        next: usize,
    },
    Disk {
        reader: Option<BufReader<File>>,
        chunk_offset: Vec<u64>,
    },
}

// --- Helper: read little-endian values ---

fn read_le_u32(r: &mut impl Read) -> Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_le_u8(r: &mut impl Read) -> Result<u8> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

// --- Main Smacker handle ---

pub struct Smk {
    /// Microseconds per frame
    pub(crate) usf: f64,
    /// Total frame count
    pub(crate) frame_count: u32,
    /// Does the file loop?
    pub(crate) ring_frame: bool,
    /// Current frame index
    pub(crate) cur_frame: u32,

    pub(crate) source: Source,
    pub(crate) chunk_size: Vec<u32>,
    pub(crate) keyframe: Vec<bool>,
    pub(crate) frame_type: Vec<u8>,

    pub(crate) video: Video,
    pub(crate) audio: [AudioTrack; 7],
}

impl Smk {
    /// Open an SMK file from disk.
    ///
    /// If `memory_mode` is true, all frame data is loaded into memory and the
    /// file handle is closed. Otherwise the file is kept open for streaming.
    pub fn open_file(path: impl AsRef<Path>, memory_mode: bool) -> Result<Smk> {
        let file = File::open(path.as_ref())?;
        let mut reader = BufReader::new(file);

        let mut s = Self::open_generic(&mut reader, memory_mode)?;

        if !memory_mode {
            if let Source::Disk {
                reader: ref mut r, ..
            } = s.source
            {
                *r = Some(reader);
            }
        }

        Ok(s)
    }

    /// Open an SMK from an in-memory buffer.
    pub fn open_memory(data: &[u8]) -> Result<Smk> {
        let mut cursor = Cursor::new(data);
        Self::open_generic(&mut cursor, true)
    }

    /// Bytes required for the header, frame tables and Huffman trees.
    /// Returns `None` until the fixed 104-byte header is available.
    pub fn header_size(data: &[u8]) -> Result<Option<usize>> {
        if data.len() < 104 {
            return Ok(None);
        }
        if !matches!(&data[..4], b"SMK2" | b"SMK4") {
            return Err(SmkError::InvalidSignature);
        }
        let field = |offset| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        let frames = field(12)
            .checked_add(field(20) & 1)
            .ok_or(SmkError::InvalidData("frame count overflow"))? as usize;
        let size = frames
            .checked_mul(5)
            .and_then(|n| n.checked_add(104))
            .and_then(|n| n.checked_add(field(52) as usize))
            .ok_or(SmkError::InvalidData("header size overflow"))?;
        Ok(Some(size))
    }

    /// Open only the header, tables and trees for sequential packet decoding.
    /// Call `packet_size` and `decode_packet`; file navigation is unavailable.
    pub fn open_stream(header: &[u8]) -> Result<Smk> {
        let size = Self::header_size(header)?.ok_or(SmkError::InvalidData("incomplete header"))?;
        if header.len() != size {
            return Err(SmkError::InvalidData("incomplete or oversized header"));
        }
        let field = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
        let buffers = u64::from(field(4)) * u64::from(field(8));
        if buffers > 64 * 1024 * 1024 {
            return Err(SmkError::InvalidData(
                "streaming allocation budget exceeded",
            ));
        }
        let buffers = buffers
            + (24..52)
                .step_by(4)
                .map(|i| u64::from(field(i)))
                .sum::<u64>()
            + (56..72)
                .step_by(4)
                .map(|i| u64::from(field(i)))
                .sum::<u64>();
        if field(4) == 0 || field(8) == 0 || field(12) == 0 || buffers > 64 * 1024 * 1024 {
            return Err(SmkError::InvalidData(
                "invalid dimensions or streaming allocation budget exceeded",
            ));
        }
        let mut decoder = Self::open_generic(&mut Cursor::new(header), false)?;
        decoder.source = Source::Packet {
            data: Vec::new(),
            next: 0,
        };
        Ok(decoder)
    }

    /// Required byte length of the next packet, or `None` at the end.
    pub fn packet_size(&self) -> Option<usize> {
        match &self.source {
            Source::Packet { next, .. } => self.chunk_size.get(*next).map(|n| *n as usize),
            _ => None,
        }
    }

    /// Decode exactly one complete packet, preserving palette and frame deltas.
    /// Packet errors are terminal; discard the decoder after an error.
    pub fn decode_packet(&mut self, packet: &[u8]) -> Result<FrameStatus> {
        let Source::Packet { data, next } = &mut self.source else {
            return Err(SmkError::InvalidData("not a packet decoder"));
        };
        let expected = self
            .chunk_size
            .get(*next)
            .ok_or(SmkError::InvalidData("packet after end of stream"))?;
        if packet.len() != *expected as usize {
            return Err(SmkError::InvalidData("incorrect packet length"));
        }
        data.clear();
        data.extend_from_slice(packet);
        self.cur_frame = *next as u32;
        *next += 1;
        self.render_frame()?;
        Ok(if self.packet_size().is_some() {
            FrameStatus::More
        } else {
            FrameStatus::Last
        })
    }

    pub fn info(&self) -> SmkInfo {
        SmkInfo {
            current_frame: self.cur_frame,
            frame_count: self.frame_count,
            microseconds_per_frame: self.usf,
        }
    }

    pub fn info_video(&self) -> VideoInfo {
        VideoInfo {
            width: self.video.w,
            height: self.video.h,
            y_scale: self.video.y_scale_mode,
        }
    }

    pub fn info_audio(&self) -> AudioInfo {
        let mut info = AudioInfo {
            track_mask: 0,
            channels: [0; 7],
            bitdepth: [0; 7],
            rate: [0; 7],
        };
        for (i, track) in self.audio.iter().enumerate() {
            if track.exists {
                info.track_mask |= 1 << i;
                info.channels[i] = track.channels;
                info.bitdepth[i] = track.bitdepth;
                info.rate[i] = track.rate;
            }
        }
        info
    }

    pub fn enable_all(&mut self, mask: u8) {
        self.video.enable = mask & 0x80 != 0;
        for i in 0..7 {
            self.audio[i].enable = mask & (1 << i) != 0;
        }
    }

    pub fn enable_video(&mut self, enable: bool) {
        self.video.enable = enable;
    }

    pub fn enable_audio(&mut self, track: u8, enable: bool) {
        if (track as usize) < 7 {
            self.audio[track as usize].enable = enable;
        }
    }

    pub fn palette(&self) -> &[[u8; 3]; 256] {
        &self.video.palette
    }

    pub fn video_data(&self) -> &[u8] {
        &self.video.frame
    }

    pub fn audio_data(&self, track: u8) -> Option<&[u8]> {
        self.audio
            .get(track as usize)
            .map(|t| &t.buffer[..t.buffer_size as usize])
    }

    /// Rewind to the first frame and decode it.
    pub fn first_frame(&mut self) -> Result<FrameStatus> {
        if matches!(self.source, Source::Packet { .. }) {
            return Err(SmkError::InvalidData("use decode_packet for streaming"));
        }
        self.cur_frame = 0;
        self.render_frame()?;

        if self.frame_count == 1 {
            Ok(FrameStatus::Last)
        } else {
            Ok(FrameStatus::More)
        }
    }

    /// Advance to the next frame and decode it.
    pub fn next_frame(&mut self) -> Result<FrameStatus> {
        if matches!(self.source, Source::Packet { .. }) {
            return Err(SmkError::InvalidData("use decode_packet for streaming"));
        }
        let total = self.frame_count + u32::from(self.ring_frame);

        if self.cur_frame + 1 < total {
            self.cur_frame += 1;
            self.render_frame()?;

            if self.cur_frame + 1 == total {
                Ok(FrameStatus::Last)
            } else {
                Ok(FrameStatus::More)
            }
        } else if self.ring_frame {
            // Loop: jump back to frame 1 (frame 0 is the setup frame).
            self.cur_frame = 1;
            self.render_frame()?;

            if self.cur_frame + 1 == total {
                Ok(FrameStatus::Last)
            } else {
                Ok(FrameStatus::More)
            }
        } else {
            Ok(FrameStatus::Done)
        }
    }

    /// Seek to the keyframe at or before the given frame index, then decode it.
    pub fn seek_keyframe(&mut self, frame: u32) -> Result<()> {
        if matches!(self.source, Source::Packet { .. }) {
            return Err(SmkError::InvalidData("use decode_packet for streaming"));
        }
        self.cur_frame = frame;

        // Roll back to the nearest keyframe.
        while self.cur_frame > 0 && !self.keyframe[self.cur_frame as usize] {
            self.cur_frame -= 1;
        }

        self.render_frame()
    }

    /// Core open logic shared by file and memory paths.
    fn open_generic(r: &mut (impl Read + Seek), memory_mode: bool) -> Result<Smk> {
        // --- Signature: "SMK" ---
        let mut sig = [0u8; 3];
        r.read_exact(&mut sig)?;
        if &sig != b"SMK" {
            return Err(SmkError::InvalidSignature);
        }

        // --- Version: '2' or '4' ---
        let mut version = read_le_u8(r)?;
        if version != b'2' && version != b'4' {
            log::warn!(
                "invalid SMK version '{}', guessing based on value",
                version as char
            );
            version = if version < b'4' { b'2' } else { b'4' };
        }

        // --- Dimensions ---
        let w = read_le_u32(r)?;
        let h = read_le_u32(r)?;

        // --- Frame count ---
        let frame_count = read_le_u32(r)?;

        // --- Frame rate ---
        let raw_rate = read_le_u32(r)?;
        let temp_l = raw_rate as i32;
        let usf = if temp_l > 0 {
            f64::from(temp_l) * 1000.0
        } else if temp_l < 0 {
            f64::from(temp_l) * -10.0
        } else {
            100_000.0
        };

        // --- Video flags ---
        let flags = read_le_u32(r)?;
        let ring_frame = flags & 0x01 != 0;

        let y_scale_mode = if flags & 0x04 != 0 {
            YScaleMode::Interlace
        } else if flags & 0x02 != 0 {
            YScaleMode::Double
        } else {
            YScaleMode::None
        };

        // --- Audio max buffer sizes (7 tracks) ---
        let mut audio: [AudioTrack; 7] = Default::default();
        for track in &mut audio {
            track.max_buffer = read_le_u32(r)?;
        }

        // --- Huffman tree chunk size ---
        let tree_chunk_size = read_le_u32(r)?;

        // --- Unpacked sizes for 4 huffman trees ---
        let mut tree_size = [0u32; 4];
        for ts in &mut tree_size {
            *ts = read_le_u32(r)?;
        }

        // --- Audio rate data (7 tracks) ---
        for (i, track) in audio.iter_mut().enumerate() {
            let temp_u = read_le_u32(r)?;

            if temp_u & 0x4000_0000 != 0 {
                track.exists = true;
                track.buffer = vec![0u8; track.max_buffer as usize];

                track.compress = if temp_u & 0x0C00_0000 != 0 {
                    log::warn!("audio track {} uses Bink compression (unsupported)", i);
                    AudioCompress::Bink
                } else if temp_u & 0x8000_0000 != 0 {
                    AudioCompress::Dpcm
                } else {
                    AudioCompress::Raw
                };

                track.bitdepth = if temp_u & 0x2000_0000 != 0 { 16 } else { 8 };
                track.channels = if temp_u & 0x1000_0000 != 0 { 2 } else { 1 };
                track.rate = temp_u & 0x00FF_FFFF;
            }
        }

        // --- Dummy field ---
        let _ = read_le_u32(r)?;

        // --- Frame sizes + keyframes ---
        let total_frames = frame_count + u32::from(ring_frame);
        let mut chunk_size = vec![0u32; total_frames as usize];
        let mut keyframe = vec![false; total_frames as usize];

        for i in 0..total_frames as usize {
            let raw = read_le_u32(r)?;
            keyframe[i] = raw & 0x01 != 0;
            // Bits 0-1 are flags; actual size has those cleared.
            chunk_size[i] = raw & 0xFFFF_FFFC;
        }

        // --- Frame types ---
        let mut frame_type = vec![0u8; total_frames as usize];
        for ft in &mut frame_type {
            *ft = read_le_u8(r)?;
        }

        // --- Huffman trees ---
        let mut hufftree_chunk = vec![0u8; tree_chunk_size as usize];
        r.read_exact(&mut hufftree_chunk)?;

        let mut bs = BitStream::new(&hufftree_chunk);
        let mut trees: [Huff16; 4] = Default::default();
        for (i, tree) in trees.iter_mut().enumerate() {
            *tree = Huff16::build(&mut bs, tree_size[i])?;
        }

        // --- Allocate video frame buffer ---
        let frame_buf = vec![0u8; (w as usize) * (h as usize)];

        // --- Read or index frame data ---
        let source = if memory_mode {
            let mut chunk_data = Vec::with_capacity(total_frames as usize);
            for &size in chunk_size.iter().take(total_frames as usize) {
                let mut data = vec![0u8; size as usize];
                r.read_exact(&mut data)?;
                chunk_data.push(data);
            }
            Source::Memory { chunk_data }
        } else {
            let mut chunk_offset = vec![0u64; total_frames as usize];
            for (offset, &size) in chunk_offset
                .iter_mut()
                .zip(chunk_size.iter().take(total_frames as usize))
            {
                *offset = r.stream_position()?;
                r.seek(SeekFrom::Current(size as i64))?;
            }
            Source::Disk {
                // Placeholder — caller fills in the real reader after return.
                reader: None,
                chunk_offset,
            }
        };

        let video = Video {
            enable: true,
            w,
            h,
            y_scale_mode,
            version,
            tree: trees,
            palette: [[0u8; 3]; 256],
            frame: frame_buf,
        };

        Ok(Smk {
            usf,
            frame_count,
            ring_frame,
            cur_frame: 0,
            source,
            chunk_size,
            keyframe,
            frame_type,
            video,
            audio,
        })
    }

    /// Decode the current frame: palette, audio tracks, and video.
    pub(crate) fn render_frame(&mut self) -> Result<()> {
        let idx = self.cur_frame as usize;
        let chunk_sz = self.chunk_size[idx] as usize;

        if chunk_sz == 0 {
            return Ok(());
        }

        // Get the frame data — borrow from memory or read from disk.
        let buf: Cow<'_, [u8]> = match &mut self.source {
            Source::Memory { chunk_data } => Cow::Borrowed(&chunk_data[idx]),
            Source::Packet { data, .. } => Cow::Borrowed(data),
            Source::Disk {
                reader,
                chunk_offset,
            } => {
                let r = reader
                    .as_mut()
                    .ok_or(SmkError::InvalidData("no file reader"))?;
                let offset = chunk_offset[idx];
                r.seek(SeekFrom::Start(offset))?;
                let mut data = vec![0u8; chunk_sz];
                r.read_exact(&mut data)?;
                Cow::Owned(data)
            }
        };

        let mut pos = 0;
        let mut remaining = chunk_sz;
        let ftype = self.frame_type[idx];

        // --- Palette record ---
        if ftype & 0x01 != 0 {
            if remaining == 0 {
                return Err(SmkError::InvalidData("no data for palette record"));
            }
            // First byte * 4 = size of palette sub-chunk.
            let pal_size = buf[pos] as usize * 4;
            if pal_size > remaining {
                return Err(SmkError::InvalidData("palette size exceeds chunk"));
            }

            if self.video.enable && pal_size > 1 {
                self.video.render_palette(&buf[pos + 1..pos + pal_size])?;
            }

            pos += pal_size;
            remaining -= pal_size;
        }

        // --- Audio tracks ---
        for track in 0u8..7 {
            if ftype & (0x02 << track) != 0 {
                if remaining < 4 {
                    return Err(SmkError::InvalidData("no data for audio record"));
                }
                // First 4 bytes = sub-chunk size (LE u32).
                let audio_size =
                    u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
                        as usize;
                if audio_size > remaining {
                    return Err(SmkError::InvalidData("audio size exceeds chunk"));
                }

                let t = &mut self.audio[track as usize];
                if t.enable && audio_size > 4 {
                    t.render(&buf[pos + 4..pos + audio_size])?;
                }

                pos += audio_size;
                remaining -= audio_size;
            } else {
                self.audio[track as usize].buffer_size = 0;
            }
        }

        // --- Video ---
        if self.video.enable {
            self.video.render_video(&buf[pos..pos + remaining])?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_real_smk_file() {
        let path = std::path::Path::new("testdata/test.smk");
        if !path.exists() {
            eprintln!("testdata/test.smk not found, skipping");
            return;
        }
        let data = std::fs::read(path).unwrap();
        let s = Smk::open_memory(&data).unwrap();

        let info = s.info();
        eprintln!(
            "frames: {}, cur: {}, usf: {}",
            info.frame_count, info.current_frame, info.microseconds_per_frame
        );
        assert!(info.frame_count > 0);
        assert!(info.microseconds_per_frame > 0.0);

        let video = s.info_video();
        eprintln!(
            "video: {}x{}, yscale: {:?}",
            video.width, video.height, video.y_scale
        );
        assert!(video.width > 0);
        assert!(video.height > 0);
        assert_eq!(
            s.video.frame.len(),
            (video.width as usize) * (video.height as usize)
        );

        let audio = s.info_audio();
        eprintln!("audio track_mask: 0x{:02x}", audio.track_mask);
        for i in 0..7 {
            if audio.track_mask & (1 << i) != 0 {
                eprintln!(
                    "  track {i}: {}ch {}bit {}Hz",
                    audio.channels[i], audio.bitdepth[i], audio.rate[i]
                );
            }
        }
    }

    #[test]
    fn decode_all_frames_real_smk() {
        let path = std::path::Path::new("testdata/test.smk");
        if !path.exists() {
            eprintln!("testdata/test.smk not found, skipping");
            return;
        }
        let data = std::fs::read(path).unwrap();
        let mut s = Smk::open_memory(&data).unwrap();

        let info = s.info();
        eprintln!("decoding {} frames...", info.frame_count);

        let mut status = s.first_frame().unwrap();
        let mut decoded = 1u32;

        // Verify frames decode without error and contain real data.
        let mut total_nonzero = 0usize;

        loop {
            let frame = s.video_data();
            total_nonzero += frame.iter().filter(|&&b| b != 0).count();

            if status == FrameStatus::Done || status == FrameStatus::Last {
                break;
            }
            status = s.next_frame().unwrap();
            decoded += 1;
        }

        eprintln!("decoded {decoded} frames, total non-zero pixels: {total_nonzero}");
        assert_eq!(decoded, info.frame_count);
        // The video should have substantial content across all frames.
        assert!(total_nonzero > 0, "all frames were blank");
    }

    /// Build a minimal valid SMK file in memory and open it.
    #[test]
    fn open_memory_minimal() {
        let smk_data = build_minimal_smk(1, 8, 8, false);
        let s = Smk::open_memory(&smk_data).unwrap();
        assert_eq!(s.frame_count, 1);
        assert_eq!(s.video.w, 8);
        assert_eq!(s.video.h, 8);
        assert!(!s.ring_frame);
        assert_eq!(s.video.version, b'4');
        assert_eq!(s.video.frame.len(), 64);
    }

    #[test]
    fn open_memory_ring_frame() {
        let smk_data = build_minimal_smk(3, 16, 16, true);
        let s = Smk::open_memory(&smk_data).unwrap();
        assert_eq!(s.frame_count, 3);
        assert!(s.ring_frame);
        // total_frames = 3 + 1 = 4
        assert_eq!(s.chunk_size.len(), 4);
        assert_eq!(s.keyframe.len(), 4);
        assert_eq!(s.frame_type.len(), 4);
    }

    #[test]
    fn open_memory_bad_signature() {
        let mut data = build_minimal_smk(1, 8, 8, false);
        data[0] = b'X'; // corrupt signature
        assert!(Smk::open_memory(&data).is_err());
    }

    #[test]
    fn info_accessors() {
        let smk_data = build_minimal_smk(5, 320, 200, false);
        let s = Smk::open_memory(&smk_data).unwrap();
        let info = s.info();
        assert_eq!(info.current_frame, 0);
        assert_eq!(info.frame_count, 5);
        assert!(info.microseconds_per_frame > 0.0);

        let video = s.info_video();
        assert_eq!(video.width, 320);
        assert_eq!(video.height, 200);
        assert_eq!(video.y_scale, YScaleMode::None);
    }

    #[test]
    fn first_single_frame() {
        let smk_data = build_minimal_smk(1, 8, 8, false);
        let mut s = Smk::open_memory(&smk_data).unwrap();
        assert_eq!(s.first_frame().unwrap(), FrameStatus::Last);
    }

    #[test]
    fn first_next_multi_frame() {
        let smk_data = build_minimal_smk(3, 8, 8, false);
        let mut s = Smk::open_memory(&smk_data).unwrap();
        assert_eq!(s.first_frame().unwrap(), FrameStatus::More);
        assert_eq!(s.cur_frame, 0);
        assert_eq!(s.next_frame().unwrap(), FrameStatus::More);
        assert_eq!(s.cur_frame, 1);
        assert_eq!(s.next_frame().unwrap(), FrameStatus::Last);
        assert_eq!(s.cur_frame, 2);
        assert_eq!(s.next_frame().unwrap(), FrameStatus::Done);
    }

    #[test]
    fn next_loops_with_ring_frame() {
        let smk_data = build_minimal_smk(2, 8, 8, true);
        let mut s = Smk::open_memory(&smk_data).unwrap();
        // total_frames = 3 (2 + ring)
        assert_eq!(s.first_frame().unwrap(), FrameStatus::More);
        assert_eq!(s.next_frame().unwrap(), FrameStatus::More);
        assert_eq!(s.next_frame().unwrap(), FrameStatus::Last);
        // Now next_frame() should loop back to frame 1.
        assert_eq!(s.next_frame().unwrap(), FrameStatus::More);
        assert_eq!(s.cur_frame, 1);
    }

    #[test]
    fn sequential_packets_require_complete_header_and_exact_lengths() {
        let data = build_minimal_smk(3, 8, 8, true);
        assert_eq!(Smk::header_size(&data[..103]).unwrap(), None);
        assert_eq!(Smk::header_size(&data).unwrap(), Some(data.len()));
        assert!(Smk::open_stream(&data[..data.len() - 1]).is_err());
        let mut stream = Smk::open_stream(&data).unwrap();
        assert_eq!(stream.packet_size(), Some(0));
        assert!(stream.decode_packet(&[1]).is_err());
        assert_eq!(stream.packet_size(), Some(0));
        assert!(stream.first_frame().is_err());
        assert!(stream.next_frame().is_err());
        assert!(stream.seek_keyframe(0).is_err());
        for frame in 0..4 {
            let expected = if frame == 3 {
                FrameStatus::Last
            } else {
                FrameStatus::More
            };
            assert_eq!(stream.decode_packet(&[]).unwrap(), expected);
            assert_eq!(stream.info().current_frame, frame);
            assert_eq!(stream.video_data(), &[0; 64]);
        }
        assert_eq!(stream.packet_size(), None);
        assert!(stream.decode_packet(&[]).is_err());
    }

    // -----------------------------------------------------------------------
    // Test helper: builds a minimal valid SMK binary blob
    // -----------------------------------------------------------------------
    fn build_minimal_smk(frames: u32, w: u32, h: u32, ring: bool) -> Vec<u8> {
        let mut out = Vec::new();

        // Signature + version
        out.extend_from_slice(b"SMK4");

        // Width, height, frame count
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&frames.to_le_bytes());

        // Frame rate: 33333 usf (~30 fps)
        let rate: i32 = -3333; // *-10 = 33330 usf
        out.extend_from_slice(&(rate as u32).to_le_bytes());

        // Flags: ring_frame bit
        let flags: u32 = if ring { 0x01 } else { 0x00 };
        out.extend_from_slice(&flags.to_le_bytes());

        // Audio max buffer sizes (7 tracks, all 0)
        for _ in 0..7 {
            out.extend_from_slice(&0u32.to_le_bytes());
        }

        // We need to build the hufftree chunk.
        // 4 empty trees: each is bit=0 (no tree) + bit=0 (terminator) = 2 bits.
        // 4 trees = 8 bits = 1 byte.
        let tree_chunk = [0x00u8]; // 8 zero bits = 4 empty trees
        let tree_chunk_size = tree_chunk.len() as u32;

        out.extend_from_slice(&tree_chunk_size.to_le_bytes());

        // Unpacked sizes for 4 trees.
        // Empty tree: alloc_size doesn't matter since bit=0 path doesn't use it.
        // But we still need valid values. The C code does malloc(sizeof(uint))
        // for empty trees and ignores alloc_size. Our Rust code skips validation
        // for the bit=0 path, so any value works.
        for _ in 0..4 {
            out.extend_from_slice(&16u32.to_le_bytes());
        }

        // Audio rate data (7 tracks, all 0 = no track)
        for _ in 0..7 {
            out.extend_from_slice(&0u32.to_le_bytes());
        }

        // Dummy field
        out.extend_from_slice(&0u32.to_le_bytes());

        // Frame sizes + keyframes
        let total_frames = frames + u32::from(ring);
        let chunk_data_size = 0u32; // empty frames
        for _ in 0..total_frames {
            out.extend_from_slice(&chunk_data_size.to_le_bytes());
        }

        // Frame types
        for _ in 0..total_frames {
            out.push(0u8);
        }

        // Huffman tree chunk
        out.extend_from_slice(&tree_chunk);

        // Frame data (all empty, size=0 each — nothing to write)

        out
    }
}
