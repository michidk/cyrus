//! Export a bounded installed cinematic window for the browser clock acceptance.
use anyhow::Context;
use std::{io::Read, path::PathBuf};
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let install =
        PathBuf::from(args.next().context(
            "usage: movie_window INSTALL OUTPUT [MOVIE_INDEX] [FRAME_COUNT] [FIRST_FRAME]",
        )?);
    let output =
        PathBuf::from(args.next().context(
            "usage: movie_window INSTALL OUTPUT [MOVIE_INDEX] [FRAME_COUNT] [FIRST_FRAME]",
        )?);
    let index = args
        .next()
        .map(|v| v.to_string_lossy().parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    let count = args
        .next()
        .map(|v| v.to_string_lossy().parse::<u32>())
        .transpose()?
        .unwrap_or(60);
    let first = args
        .next()
        .map(|v| v.to_string_lossy().parse::<u32>())
        .transpose()?
        .unwrap_or(1);
    let movie = cyrus_assets::startup::movie_native::prepare(&install, index)?;
    anyhow::ensure!(
        first > 0 && first <= count && count <= movie.metadata.frames,
        "invalid movie window range {first}..={count}"
    );
    anyhow::ensure!(
        !output.exists(),
        "output already exists: {}",
        output.display()
    );
    let parent = output
        .parent()
        .context("movie window output has no parent")?
        .canonicalize()
        .context("movie window output parent must exist")?;
    let destination = parent.join(
        output
            .file_name()
            .context("movie window output has no name")?,
    );
    anyhow::ensure!(
        !destination.starts_with(install.canonicalize()?),
        "movie window output must be outside the install"
    );
    std::fs::create_dir(&output)?;
    let mut input = movie.source.open()?;
    let mut stream = cyrus_media::MovieStream::new(movie.metadata.codec_format(), 0x81);
    let mut records = vec![];
    while stream.decoded_frames() < count {
        if let Some(frame) = stream.next_frame()? {
            // Decode the authored prefix, retaining only the requested window.
            if frame.number < first {
                continue;
            }
            let mut rgba = Vec::with_capacity(frame.indices.len() * 4);
            for index in frame.indices {
                rgba.extend_from_slice(&frame.palette[usize::from(*index)]);
                rgba.push(255);
            }
            let picture = format!("{}.rgba", frame.number);
            std::fs::write(output.join(&picture), rgba)?;
            let audio = if let Some(audio) = frame.audio {
                let pcm = format!("{}.pcm", frame.number);
                std::fs::write(output.join(&pcm), audio.pcm)?;
                Some(
                    serde_json::json!({"file":pcm,"rate":audio.rate,"channels":audio.channels,"bits":audio.bits,"start_sample":audio.start_sample}),
                )
            } else {
                None
            };
            records
                .push(serde_json::json!({"number":frame.number,"picture":picture,"audio":audio}));
        } else {
            let mut chunk = vec![0; 65536];
            let length = input.read(&mut chunk)?;
            anyhow::ensure!(length > 0, "movie truncated");
            stream.push(&chunk[..length]);
        }
    }
    std::fs::write(
        output.join("window.json"),
        serde_json::to_vec(
            &serde_json::json!({"metadata":{"frames":movie.metadata.frames,"frame_micros":movie.metadata.frame_micros},"frames":records}),
        )?,
    )?;
    Ok(())
}
