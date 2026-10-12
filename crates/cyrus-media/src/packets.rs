//! Sequential packets retain only the current compressed block.
use anyhow::{Context, ensure};
/// Reads only the header and current packet from a sequential compressed source.
pub struct Packets<R> {
    reader: R,
    packet: Vec<u8>,
    pub codec: smk::Smk,
}
impl<R: std::io::Read> Packets<R> {
    /// Mask bits 0..=6 enable audio tracks; bit 7 enables indexed video.
    pub fn new(mut reader: R, mask: u8) -> anyhow::Result<Self> {
        let mut header = vec![0; 104];
        reader.read_exact(&mut header)?;
        let size = smk::Smk::header_size(&header)?.context("incomplete movie header")?;
        ensure!(
            size <= 8 * 1024 * 1024,
            "movie header exceeds streaming capacity"
        );
        header.resize(size, 0);
        reader.read_exact(&mut header[104..])?;
        let mut codec = smk::Smk::open_stream(&header)?;
        codec.enable_all(mask);
        Ok(Self {
            reader,
            packet: Vec::new(),
            codec,
        })
    }
    /// Decode one complete packet; false means the codec reached its authored end.
    pub fn advance(&mut self) -> anyhow::Result<bool> {
        let Some(size) = self.codec.packet_size() else {
            return Ok(false);
        };
        ensure!(
            size <= 8 * 1024 * 1024,
            "movie packet exceeds streaming capacity"
        );
        self.packet.resize(size, 0);
        self.reader
            .read_exact(&mut self.packet)
            .context("truncated movie packet")?;
        self.codec.decode_packet(&self.packet)?;
        Ok(true)
    }
}
