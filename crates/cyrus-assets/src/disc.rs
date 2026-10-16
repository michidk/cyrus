//! CD cue-sheet interpretation shared by both runtime targets.
use anyhow::Result;
/// Red Book raw sector size in bytes; audio is 44.1 kHz 16-bit stereo, 75 sectors per second.
pub const SECTOR_BYTES: u64 = 2352;
/// Byte range of `track` in the single `BINARY` file a cue sheet names:
/// (file name, start, end). `audio` states whether the track must be audio or data.
/// `end` is `None` for the last track, which runs to the end of the file.
pub fn cue_track(cue: &str, track: u32, audio: bool) -> Result<(String, u64, Option<u64>), String> {
    let mut file = None;
    // (number, is audio, first index sector, INDEX 01 sector)
    let mut tracks: Vec<(u32, bool, Option<u64>, Option<u64>)> = Vec::new();
    for line in cue.lines().map(str::trim) {
        let mut words = line.split_whitespace();
        match words.next().map(str::to_ascii_uppercase).as_deref() {
            Some("FILE") => {
                let rest = line[4..].trim();
                let (name, kind) = rest
                    .strip_prefix('"')
                    .and_then(|r| r.split_once('"'))
                    .map(|(n, k)| (n, k.trim()))
                    .ok_or_else(|| format!("cue FILE line without quoted name: {line}"))?;
                if !kind.eq_ignore_ascii_case("BINARY") || file.replace(name.to_string()).is_some()
                {
                    return Err("only cue sheets with one BINARY file are supported".into());
                }
            }
            Some("TRACK") => {
                let number = words.next().and_then(|n| n.parse().ok());
                let kind = words.next().unwrap_or_default();
                let number = number.ok_or_else(|| format!("bad cue TRACK line: {line}"))?;
                tracks.push((number, kind.eq_ignore_ascii_case("AUDIO"), None, None));
            }
            Some("INDEX") => {
                let (index, time) = (words.next(), words.next());
                let sector = time.and_then(msf_sector);
                let (Some(index), Some(sector), Some(current)) = (index, sector, tracks.last_mut())
                else {
                    return Err(format!("bad cue INDEX line: {line}"));
                };
                current.2.get_or_insert(sector);
                if index.parse::<u32>() == Ok(1) {
                    current.3 = Some(sector);
                }
            }
            _ => {}
        }
    }
    let file = file.ok_or("cue sheet names no file")?;
    let position = tracks
        .iter()
        .position(|t| t.0 == track)
        .ok_or_else(|| format!("CD has no track {track}"))?;
    let (_, is_audio, _, start) = tracks[position];
    if is_audio != audio {
        let kind = if audio { "audio" } else { "data" };
        return Err(format!("CD track {track} is not {kind}"));
    }
    let start = start.ok_or_else(|| format!("CD track {track} has no INDEX 01"))?;
    let end = tracks.get(position + 1).and_then(|next| next.2);
    Ok((file, start * SECTOR_BYTES, end.map(|e| e * SECTOR_BYTES)))
}

/// `mm:ss:ff` (75 frames per second) to a sector number.
fn msf_sector(msf: &str) -> Option<u64> {
    let mut parts = msf.split(':').map(|p| p.parse::<u64>().ok());
    let (m, s, f) = (parts.next()??, parts.next()??, parts.next()??);
    (s < 60 && f < 75 && parts.next().is_none()).then_some((m * 60 + s) * 75 + f)
}

/// The 12-byte sync pattern that begins every CD-ROM data sector; scrambling leaves it intact.
const SYNC: [u8; 12] = [
    0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0,
];

/// Silences CD-ROM data-sector bytes at the start of an audio track's PCM and
/// returns how many bytes were zeroed. The GOG image's track 2 begins with the
/// overlapping tail of the data track (preservation `docs/engine/music.md`).
/// Muting starts at the track start, runs through the last of a contiguous
/// chain of sync-marked sectors, and keeps the track length and timing.
pub fn mute_leading_data_sectors(pcm: &mut [u8]) -> usize {
    let sector = SECTOR_BYTES as usize;
    let mut end = 0;
    // The first sector may start mid-sector, so the first search spans a whole sector.
    while let Some(offset) = pcm
        .get(end..(end + sector).min(pcm.len()))
        .and_then(|window| window.windows(SYNC.len()).position(|bytes| bytes == SYNC))
    {
        end = (end + offset + sector).min(pcm.len());
    }
    // Whole stereo 16-bit frames only.
    let end = end.next_multiple_of(4).min(pcm.len());
    pcm[..end].fill(0);
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leading_data_sectors_are_muted_and_audio_is_kept() {
        let sector = SECTOR_BYTES as usize;
        // GOG track 2 layout: sync-marked sectors at bytes 24 and 2376, then audio.
        let mut pcm = vec![7u8; 6 * sector];
        for start in [24, 24 + sector] {
            pcm[start..start + SYNC.len()].copy_from_slice(&SYNC);
        }
        assert_eq!(mute_leading_data_sectors(&mut pcm), 4728);
        assert!(pcm[..4728].iter().all(|byte| *byte == 0));
        assert!(pcm[4728..].iter().all(|byte| *byte == 7));
        let mut audio = vec![7u8; 3 * sector];
        assert_eq!(mute_leading_data_sectors(&mut audio), 0);
        assert!(audio.iter().all(|byte| *byte == 7));
        // A sync mark beyond the first sector is not part of a leading chain.
        audio[sector + 100..sector + 100 + SYNC.len()].copy_from_slice(&SYNC);
        assert_eq!(mute_leading_data_sectors(&mut audio), 0);
    }
    #[test]
    fn cue_track_ranges_end_at_the_next_track() {
        let cue = "FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 00 00:10:00\n    INDEX 01 00:12:00\n  TRACK 03 AUDIO\n    INDEX 01 01:00:74\n";
        assert_eq!(
            cue_track(cue, 2, true).unwrap(),
            (
                "disc.bin".into(),
                12 * 75 * SECTOR_BYTES,
                Some((60 * 75 + 74) * SECTOR_BYTES)
            )
        );
        assert_eq!(
            cue_track(cue, 3, true).unwrap(),
            ("disc.bin".into(), (60 * 75 + 74) * SECTOR_BYTES, None)
        );
        assert!(cue_track(cue, 1, true).is_err(), "data track");
        assert_eq!(cue_track(cue, 1, false).unwrap().1, 0);
        assert!(cue_track(cue, 4, true).is_err(), "missing track");
        assert!(cue_track(&format!("{cue}FILE \"b.bin\" BINARY\n"), 2, true).is_err());
        assert_eq!(msf_sector("00:60:00"), None);
    }
}
