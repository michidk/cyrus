//! Decode a bounded picture oracle from an explicitly supplied local movie payload.
use anyhow::{Context, ensure};
use std::{fs::File, io::BufReader, path::PathBuf};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 4,
        "usage: frame_window MOVIE OUTPUT FIRST LAST"
    );
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let first: u32 = args[2]
        .to_str()
        .context("FIRST must be an integer")?
        .parse()?;
    let last: u32 = args[3]
        .to_str()
        .context("LAST must be an integer")?
        .parse()?;
    let mut packets = cyrus_media::Packets::new(BufReader::new(File::open(&input)?), 0x80)?;
    let info = packets.codec.info();
    ensure!(
        first > 0 && first <= last && last <= info.frame_count,
        "invalid frame window"
    );
    let video = packets.codec.info_video();
    ensure!(
        video.width == 640 && video.height == 240,
        "expected installed cinematic viewport"
    );
    let micros = info.microseconds_per_frame;
    let count = info.frame_count;
    // A new directory prevents partial or stale oracles being silently reused.
    std::fs::create_dir(&output)?;
    let mut frames = Vec::new();
    for number in 1..=last {
        ensure!(
            packets.advance()?,
            "movie ended before requested frame {number}"
        );
        if number < first {
            continue;
        }
        let mut rgba = Vec::with_capacity(640 * 240 * 4);
        for index in packets.codec.video_data() {
            rgba.extend_from_slice(&packets.codec.palette()[usize::from(*index)]);
            rgba.push(255);
        }
        std::fs::write(output.join(format!("{number}.rgba")), rgba)?;
        frames.push(format!(
            r#"{{"number":{number},"picture":"{number}.rgba"}}"#
        ));
    }
    std::fs::write(
        output.join("window.json"),
        format!(
            r#"{{"metadata":{{"frames":{count},"frame_micros":{micros}}},"frames":[{}]}}"#,
            frames.join(",")
        ),
    )?;
    Ok(())
}
