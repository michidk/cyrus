//! CD track extraction shared with native game playback.
pub use crate::disc::cue_track;
use anyhow::{Context, Result};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub fn read_pcm(cue_path: &Path, track: u32) -> Result<Vec<u8>> {
    let cue = std::fs::read_to_string(cue_path)?;
    let (name, start, end) = cue_track(&cue, track, true).map_err(anyhow::Error::msg)?;
    crate::validate_path(&name)?;
    let mut file = std::fs::File::open(cue_path.with_file_name(name))?;
    let end = end.unwrap_or(file.metadata()?.len());
    anyhow::ensure!(
        end <= file.metadata()?.len(),
        "track {track}: end beyond image"
    );
    let len = end.checked_sub(start).context("track ends before start")?;
    let mut pcm = vec![0; usize::try_from(len)?];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut pcm)?;
    Ok(pcm)
}
