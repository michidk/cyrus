//! CD cue-sheet interpretation shared by both runtime targets.
use anyhow::Result;
const SECTOR_BYTES: u64 = 2352;
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
