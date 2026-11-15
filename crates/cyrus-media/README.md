# cyrus-media

Portable Smacker movie decoding and PCM helpers on top of the `smk` codec. It
has no Bevy or install dependency: callers supply the compressed bytes and the
expected format. Installed asset discovery, subtitles, skip rules, network
transport and audio backends stay in the consumers (`cyrus-assets`,
`cyrus-startup`, `cyrus-viewer`).

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `Movie` (whole-file decoder, one current picture), `audio_tracks`, `pcm_wav` (PCM to WAV), `peaks` (waveform envelopes) |
| [packets](src/packets.rs) | `Packets`: reads a `Read` source one compressed packet at a time |
| [stream](src/stream.rs) | `MovieStream`: decodes chunks pushed in order, validated against a `MovieFormat` |

## Notes

- `MovieFormat` carries the expected frame count, frame time and viewport;
  `validate` rejects a codec header that does not match.
- `MovieStream` consumes chunks strictly in order and decodes skipped frames
  too, so delta frames stay correct across forward skips.
- The enable mask is shared: bits 0..=6 select audio tracks, bit 7 indexed video.

## Testing

```sh
cargo test -p cyrus-media
```

Installed streaming contracts live in
[`cyrus-assets/tests/movie_stream.rs`](../cyrus-assets/tests/movie_stream.rs).

For a bounded picture oracle from a local movie file, decode an inclusive frame
window (1-based) to raw RGBA plus metadata in a new output directory; the input
is never modified:

```sh
cargo run -p cyrus-media --example frame_window -- MOVIE OUTPUT FIRST LAST
```
