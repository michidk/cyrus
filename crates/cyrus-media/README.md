# cyrus-media

Portable Smacker decoding through the existing codec, sequential bounded packets,
indexed movie frames, PCM-to-WAV conversion and waveform envelopes.

`MovieFormat` carries expected codec timing and dimensions. Callers supply these
values; installed asset discovery, subtitle fonts, skip rules, network transport
and audio backends remain in consumer crates. `Packets` accepts a `Read` source
and retains one compressed packet. `MovieStream` consumes downloaded chunks in
order so delta frames remain correct during forward skips.

Run `cargo test -p cyrus-media`. Installed streaming contracts live in
`cyrus-assets/tests/movie_stream.rs`; native playback contracts live in render.
