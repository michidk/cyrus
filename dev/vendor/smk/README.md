# Vendored smk

Vendored from the published `smk` 0.1.0 package by Alessandro Rosetti
([upstream](https://github.com/roarc0/smk)), a Rust port of libsmacker 1.2.0,
under LGPL-2.1-or-later ([LGPL-2.1.txt](LGPL-2.1.txt),
[GPL-2.0.txt](GPL-2.0.txt)). The file and memory APIs are retained. No media
is included. The workspace depends on this copy by path.

## Changes

- **Sequential packet input**: `header_size`, `open_stream`, `packet_size` and
  `decode_packet` let the browser feed complete compressed packets as they
  arrive, keeping palette and delta-frame state without loading the whole
  movie. A packet decoder cannot seek; callers decode sequentially and discard
  frames to skip forward.
- Header dimensions and allocation sizes are checked before streaming buffers
  are created.

## Testing

```sh
cargo test --manifest-path dev/vendor/smk/Cargo.toml
```

The installed frame and palette contract lives in `cyrus-media`'s `stream`
module.
